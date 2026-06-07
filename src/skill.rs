use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{AgentError, AgentResult};
use crate::task::{finish_step, think_step, tool_call_step, Plan, Step};

/// A reusable skill definition stored as a JSON file in `.skills/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub description: String,
    pub version: String,
    /// Schema version for format compatibility (default "2.0").
    /// Version "2.0" adds Exec, HttpRequest, BrowserAction step types.
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    /// Trigger keywords that suggest this skill (optional).
    #[serde(default)]
    pub trigger: Vec<String>,
    /// Ordered list of step definitions.
    pub steps: Vec<SkillStep>,
}

fn default_schema_version() -> String {
    "2.0".to_string()
}

/// A step definition within a skill, convertible to a Plan Step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SkillStep {
    Think {
        instruction: String,
    },
    ToolCall {
        tool_name: String,
        params: serde_json::Value,
    },
    /// Execute an external command with optional timeout.
    Exec {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Perform an HTTP request.
    HttpRequest {
        url: String,
        #[serde(default)]
        method: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        headers: Option<std::collections::HashMap<String, String>>,
    },
    /// Control a browser (navigate, screenshot, get text, extract links, etc.).
    BrowserAction {
        action: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    WaitForInput {
        prompt: String,
    },
    Finish {
        summary: String,
    },
}

/// Manages discovery, loading, and conversion of skills.
pub struct SkillManager {
    /// Directory where skill files are stored.
    skills_dir: PathBuf,
}

impl SkillManager {
    /// Create a manager that looks for skills in the given directory.
    pub fn new(skills_dir: PathBuf) -> Self {
        Self { skills_dir }
    }

    /// Return the default skills directory.
    ///
    /// Priority: `RUPOO_SKILLS_DIR` env var > `$HOME/.skills` > `./.skills`
    pub fn default_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("RUPOO_SKILLS_DIR") {
            PathBuf::from(dir)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".skills")
        } else {
            PathBuf::from(".skills")
        }
    }

    /// Ensure the skills directory exists.
    pub fn ensure_dir(&self) -> AgentResult<()> {
        if !self.skills_dir.exists() {
            std::fs::create_dir_all(&self.skills_dir)?;
            info!(
                dir = %self.skills_dir.display(),
                "created skills directory"
            );
        }
        Ok(())
    }

    /// List all available skill names.
    pub fn list_skills(&self) -> AgentResult<Vec<String>> {
        let mut skills = Vec::new();
        if !self.skills_dir.exists() {
            return Ok(skills);
        }

        for entry in std::fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Some(stem) = path.file_stem() {
                    skills.push(stem.to_string_lossy().to_string());
                }
            }
        }
        skills.sort();
        Ok(skills)
    }

    /// Load a skill definition by name (without the .json extension).
    /// Handles backward compatibility with schema_version "1.0" (no schema_version field).
    pub fn load_skill(&self, name: &str) -> AgentResult<SkillDef> {
        let path = self.skills_dir.join(format!("{name}.json"));
        if !path.exists() {
            return Err(AgentError::Skill(format!(
                "skill not found: '{name}' (looked at {})",
                path.display()
            )));
        }
        let content = std::fs::read_to_string(&path)?;

        // Try parsing as v2.0 first (has schema_version field)
        if let Ok(skill) = serde_json::from_str::<SkillDef>(&content) {
            return Ok(skill);
        }

        // Fallback: parse as v1.0 and add schema_version
        #[derive(Deserialize)]
        struct SkillDefV1 {
            pub name: String,
            pub description: String,
            pub version: String,
            #[serde(default)]
            pub trigger: Vec<String>,
            pub steps: Vec<serde_json::Value>,
        }

        let skill_v1: SkillDefV1 = serde_json::from_str(&content)?;

        // Convert v1 steps to v2 SkillStep enum
        let v2_steps: Vec<SkillStep> = skill_v1
            .steps
            .into_iter()
            .map(|s| {
                let map = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(s)
                    .unwrap_or_default();
                let type_str = map.get("type").and_then(|v| v.as_str()).unwrap_or("think");

                match type_str {
                    "think" => SkillStep::Think {
                        instruction: map
                            .get("instruction")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "toolCall" => SkillStep::ToolCall {
                        tool_name: map
                            .get("toolName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        params: map.get("params").cloned().unwrap_or(serde_json::json!({})),
                    },
                    "waitForInput" => SkillStep::WaitForInput {
                        prompt: map
                            .get("prompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    "finish" => SkillStep::Finish {
                        summary: map
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    },
                    _ => SkillStep::Think {
                        instruction: format!("Unknown step type: {}", type_str),
                    },
                }
            })
            .collect();

        // Build the v2 SkillDef
        Ok(SkillDef {
            name: skill_v1.name,
            description: skill_v1.description,
            version: skill_v1.version,
            schema_version: "1.0".to_string(),
            trigger: skill_v1.trigger,
            steps: v2_steps,
        })
    }

    /// Save a skill definition to a file.
    pub fn save_skill(&self, skill: &SkillDef) -> AgentResult<()> {
        self.ensure_dir()?;
        let path = self.skills_dir.join(format!("{}.json", skill.name));
        let content = serde_json::to_string_pretty(skill)?;
        std::fs::write(&path, content)?;
        info!(
            name = %skill.name,
            path = %path.display(),
            "skill saved"
        );
        Ok(())
    }

    /// Delete a skill file by name.
    pub fn delete_skill(&self, name: &str) -> AgentResult<()> {
        let path = self.skills_dir.join(format!("{name}.json"));
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!(name = %name, "skill deleted");
            Ok(())
        } else {
            Err(AgentError::Skill(format!("skill not found: '{name}'")))
        }
    }

    /// Convert a SkillDef into a Plan that can be executed by the Agent.
    /// Handles backward compatibility with schema_version "1.0".
    pub fn skill_to_plan(&self, skill: &SkillDef) -> Plan {
        let steps: Vec<Step> = skill
            .steps
            .iter()
            .map(|s| match s {
                SkillStep::Think { instruction } => think_step(instruction),
                SkillStep::ToolCall { tool_name, params } => {
                    tool_call_step(tool_name, params.clone())
                }
                SkillStep::Exec {
                    command,
                    args,
                    timeout_secs,
                } => crate::task::exec_step(command, args.clone(), *timeout_secs),
                SkillStep::HttpRequest {
                    url,
                    method,
                    body,
                    headers,
                } => {
                    let http_method = match method.to_uppercase().as_str() {
                        "POST" => crate::task::HttpMethod::POST,
                        _ => crate::task::HttpMethod::GET,
                    };
                    crate::task::http_request_step(url, http_method, body.clone(), headers.clone())
                }
                SkillStep::BrowserAction {
                    action,
                    url,
                    timeout_secs,
                } => {
                    let action_type = match action.to_lowercase().as_str() {
                        "navigate" => crate::task::BrowserActionType::Navigate,
                        "screenshot" => crate::task::BrowserActionType::Screenshot,
                        "gettext" | "get_text" => crate::task::BrowserActionType::GetText,
                        "click" => crate::task::BrowserActionType::Click,
                        "extractlinks" | "extract_links" => {
                            crate::task::BrowserActionType::ExtractLinks
                        }
                        "javascript" | "js" => crate::task::BrowserActionType::JavaScript,
                        _ => crate::task::BrowserActionType::Navigate,
                    };
                    crate::task::browser_action_step(action_type, url.clone(), None, *timeout_secs)
                }
                SkillStep::WaitForInput { prompt } => crate::task::wait_for_input_step(prompt),
                SkillStep::Finish { summary } => finish_step(summary),
            })
            .collect();

        Plan::new(&skill.name, steps)
    }

    /// Convert a completed Plan into a SkillDef (self-evolution).
    /// Steps that completed successfully become skill steps.
    /// Failed or pending steps are excluded.
    pub fn plan_to_skill(plan: &Plan, name: &str, description: &str) -> SkillDef {
        use crate::task::StepStatus;

        let steps: Vec<SkillStep> = plan
            .steps
            .iter()
            .filter(|s| *s.status() != StepStatus::Pending && *s.status() != StepStatus::Failed)
            .map(|s| match s {
                crate::task::Step::Think { instruction, .. } => SkillStep::Think {
                    instruction: instruction.clone(),
                },
                crate::task::Step::ToolCall {
                    tool_name, params, ..
                } => SkillStep::ToolCall {
                    tool_name: tool_name.clone(),
                    params: params.clone(),
                },
                crate::task::Step::Exec {
                    command,
                    args,
                    timeout_secs,
                    ..
                } => SkillStep::Exec {
                    command: command.clone(),
                    args: args.clone(),
                    timeout_secs: *timeout_secs,
                },
                crate::task::Step::HttpRequest {
                    url,
                    method,
                    body,
                    headers,
                    ..
                } => SkillStep::HttpRequest {
                    url: url.clone(),
                    method: match method {
                        crate::task::HttpMethod::GET => "GET".to_string(),
                        crate::task::HttpMethod::POST => "POST".to_string(),
                    },
                    body: body.clone(),
                    headers: headers.clone(),
                },
                crate::task::Step::BrowserAction {
                    action,
                    url,
                    timeout_secs,
                    ..
                } => {
                    let action_str = match action {
                        crate::task::BrowserActionType::Navigate => "navigate",
                        crate::task::BrowserActionType::Screenshot => "screenshot",
                        crate::task::BrowserActionType::GetText => "getText",
                        crate::task::BrowserActionType::Click => "click",
                        crate::task::BrowserActionType::ExtractLinks => "extractLinks",
                        crate::task::BrowserActionType::JavaScript => "javascript",
                    };
                    SkillStep::BrowserAction {
                        action: action_str.to_string(),
                        url: url.clone(),
                        timeout_secs: *timeout_secs,
                    }
                }
                crate::task::Step::WaitForInput { prompt, .. } => SkillStep::WaitForInput {
                    prompt: prompt.clone(),
                },
                crate::task::Step::Finish { summary, .. } => SkillStep::Finish {
                    summary: summary.clone(),
                },
            })
            .collect();

        SkillDef {
            name: name.to_string(),
            description: description.to_string(),
            version: "1.0".to_string(),
            schema_version: "2.0".to_string(),
            trigger: vec![],
            steps,
        }
    }

    /// Create the built-in skills in the default directory.
    pub fn install_builtin_skills() -> AgentResult<()> {
        let manager = Self::new(Self::default_dir());
        manager.ensure_dir()?;

        // 1. Code review skill
        let code_review = SkillDef {
            name: "code-review".into(),
            description: "Review code changes for bugs and best practices".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec!["review".into(), "code review".into(), "审查".into()],
            steps: vec![
                SkillStep::Think {
                    instruction: "Analyze the structure and intent of the code changes".into(),
                },
                SkillStep::ToolCall {
                    tool_name: "list_directory".into(),
                    params: serde_json::json!({ "path": "." }),
                },
                SkillStep::Think {
                    instruction: "Identify potential bugs, security issues, and style problems"
                        .into(),
                },
                SkillStep::Finish {
                    summary: "Code review analysis complete. See step outputs for details.".into(),
                },
            ],
        };
        manager.save_skill(&code_review)?;

        // 2. Generate README skill
        let readme_gen = SkillDef {
            name: "generate-readme".into(),
            description: "Automatically generate a README.md from project files".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec!["readme".into(), "generate readme".into(), "文档".into()],
            steps: vec![
                SkillStep::ToolCall {
                    tool_name: "file_read".into(),
                    params: serde_json::json!({ "path": "Cargo.toml" }),
                },
                SkillStep::Think {
                    instruction: "Generate a README.md based on the Cargo.toml content".into(),
                },
                SkillStep::ToolCall {
                    tool_name: "file_write".into(),
                    params: serde_json::json!({
                        "path": "README.md",
                        "content": "# Project Name\n\nGenerated by Plan Executor Agent.\n"
                    }),
                },
                SkillStep::Finish {
                    summary: "README.md has been generated.".into(),
                },
            ],
        };
        manager.save_skill(&readme_gen)?;

        info!("built-in skills installed");
        Ok(())
    }

    /// Match a user message against skill triggers.
    ///
    /// Scans all loaded skills for any trigger keyword present in the message.
    /// Returns the first matching skill, or `None` if no trigger matches.
    /// Supports both exact and fuzzy matching (Levenshtein distance ≤ 2).
    pub fn match_trigger(&self, message: &str) -> AgentResult<Option<SkillDef>> {
        let msg_lower = message.to_lowercase();
        let msg_words: Vec<&str> = msg_lower.split_whitespace().collect();
        let skills = self.list_skills()?;

        let mut best_match: Option<(SkillDef, usize)> = None;

        for name in &skills {
            let skill = match self.load_skill(name) {
                Ok(s) => s,
                Err(_) => continue,
            };

            for trigger in &skill.trigger {
                let trigger_lower = trigger.to_lowercase();

                // Exact substring match (highest priority)
                if msg_lower.contains(&trigger_lower) {
                    return Ok(Some(skill));
                }

                // Fuzzy match: check each word against trigger
                for word in &msg_words {
                    let dist = levenshtein_distance(word, &trigger_lower);
                    if dist <= 2 && dist > 0 {
                        match &best_match {
                            None => best_match = Some((skill.clone(), dist)),
                            Some((_, best_dist)) if dist < *best_dist => {
                                best_match = Some((skill.clone(), dist));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(best_match.map(|(skill, _)| skill))
    }

    /// Load a SKILL.md file (YAML frontmatter + Markdown body).
    ///
    /// SKILL.md format:
    /// ```markdown
    /// ---
    /// name: my-skill
    /// description: Does something useful
    /// triggers:
    ///   - "help me with X"
    ///   - "do X"
    /// ---
    /// # Instructions
    /// Step-by-step instructions for the skill...
    /// ```
    pub fn load_skill_md(&self, name: &str) -> AgentResult<SkillDef> {
        let path = self.skills_dir.join(format!("{name}.md"));
        if !path.exists() {
            return Err(AgentError::Skill(format!(
                "SKILL.md not found: '{name}' (looked at {})",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(&path)?;
        Self::parse_skill_md(&content, name)
    }

    /// Parse a SKILL.md file content into a SkillDef.
    fn parse_skill_md(content: &str, fallback_name: &str) -> AgentResult<SkillDef> {
        // Extract YAML frontmatter
        let (frontmatter, body) = if let Some(stripped) = content.strip_prefix("---") {
            let end = stripped.find("---").map(|i| i + 3);
            match end {
                Some(end_idx) => {
                    let fm = &content[3..end_idx];
                    let body = content[end_idx + 3..].trim();
                    (fm.to_string(), body.to_string())
                }
                None => (String::new(), content.to_string()),
            }
        } else {
            (String::new(), content.to_string())
        };

        // Parse frontmatter
        let fm: SkillFrontmatter = if frontmatter.is_empty() {
            SkillFrontmatter::default()
        } else {
            serde_yaml::from_str(&frontmatter)
                .map_err(|e| AgentError::Skill(format!("parse SKILL.md frontmatter: {e}")))?
        };

        // Convert body to steps — each section heading becomes a Think step,
        // code blocks become Exec steps, tool references become ToolCall steps.
        let steps = Self::body_to_steps(&body);

        Ok(SkillDef {
            name: fm.name.unwrap_or_else(|| fallback_name.to_string()),
            description: fm.description.unwrap_or_default(),
            version: fm.version.unwrap_or_else(|| "1.0".to_string()),
            schema_version: "2.0".to_string(),
            trigger: fm.triggers.unwrap_or_default(),
            steps,
        })
    }

    /// Convert markdown body to skill steps.
    fn body_to_steps(body: &str) -> Vec<SkillStep> {
        let mut steps = Vec::new();

        for line in body.lines() {
            let trimmed = line.trim();

            // Section headings → Think steps
            if let Some(title) = trimmed.strip_prefix("# ") {
                let instruction = title.trim().to_string();
                if !instruction.is_empty() {
                    steps.push(SkillStep::Think { instruction });
                }
            }
            // Tool references: `@tool_name(params)` → ToolCall steps
            else if trimmed.starts_with("@") {
                if let Some(tool_call) = Self::parse_tool_reference(trimmed) {
                    steps.push(tool_call);
                }
            }
            // Code blocks: ```bash ... ``` → Exec steps
            else if trimmed.starts_with("```") {
                // Simplified: just note it as a think step about running code
                // Full implementation would need multi-line code block parsing
                continue;
            }
            // Regular text → additional context for the last Think step
            else if !trimmed.is_empty() {
                if let Some(SkillStep::Think {
                    ref mut instruction,
                }) = steps.last_mut()
                {
                    instruction.push_str(&format!("\n{}", trimmed));
                } else {
                    steps.push(SkillStep::Think {
                        instruction: trimmed.to_string(),
                    });
                }
            }
        }

        // Add a Finish step if not present
        if !steps.iter().any(|s| matches!(s, SkillStep::Finish { .. })) {
            steps.push(SkillStep::Finish {
                summary: "Skill execution complete".to_string(),
            });
        }

        steps
    }

    /// Parse a tool reference like `@file_read(path="Cargo.toml")`.
    fn parse_tool_reference(line: &str) -> Option<SkillStep> {
        let line = line.strip_prefix('@')?;
        let paren_start = line.find('(')?;
        let paren_end = line.rfind(')')?;

        let tool_name = line[..paren_start].trim().to_string();
        let params_str = &line[paren_start + 1..paren_end];

        let params: serde_json::Value = if params_str.trim().is_empty() {
            serde_json::json!({})
        } else {
            // Try to parse as JSON first
            serde_json::from_str(params_str).ok().unwrap_or_else(|| {
                // Fallback: try key=value format
                let mut map = serde_json::Map::new();
                for pair in params_str.split(',') {
                    if let Some((key, value)) = pair.split_once('=') {
                        let key = key.trim().trim_matches('"');
                        let value = value.trim().trim_matches('"');
                        map.insert(
                            key.to_string(),
                            serde_json::Value::String(value.to_string()),
                        );
                    }
                }
                serde_json::Value::Object(map)
            })
        };

        Some(SkillStep::ToolCall { tool_name, params })
    }
}

/// YAML frontmatter structure for SKILL.md files.
#[derive(Debug, Default, serde::Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    triggers: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Levenshtein distance (shared with commands.rs)
// ---------------------------------------------------------------------------

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate() {
        *cell = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, SkillManager) {
        let dir = TempDir::new().unwrap();
        let manager = SkillManager::new(dir.path().join(".skills"));
        (dir, manager)
    }

    #[test]
    fn test_save_and_load_skill() {
        let (_tmp, manager) = setup();
        let skill = SkillDef {
            name: "test-skill".into(),
            description: "A test skill".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec!["test".into()],
            steps: vec![
                SkillStep::Think {
                    instruction: "analyze".into(),
                },
                SkillStep::Finish {
                    summary: "done".into(),
                },
            ],
        };

        manager.save_skill(&skill).unwrap();
        let loaded = manager.load_skill("test-skill").unwrap();
        assert_eq!(loaded.name, "test-skill");
        assert_eq!(loaded.steps.len(), 2);
    }

    #[test]
    fn test_list_skills() {
        let (_tmp, manager) = setup();
        let skill = SkillDef {
            name: "skill-a".into(),
            description: "A".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec![],
            steps: vec![],
        };
        manager.save_skill(&skill).unwrap();

        let list = manager.list_skills().unwrap();
        assert_eq!(list, vec!["skill-a"]);
    }

    #[test]
    fn test_skill_to_plan() {
        let (_tmp, manager) = setup();
        let skill = SkillDef {
            name: "plan-test".into(),
            description: "test".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec![],
            steps: vec![
                SkillStep::Think {
                    instruction: "step1".into(),
                },
                SkillStep::Finish {
                    summary: "done".into(),
                },
            ],
        };

        let plan = manager.skill_to_plan(&skill);
        assert_eq!(plan.name, "plan-test");
        assert_eq!(plan.steps.len(), 2);
        assert!(matches!(plan.steps[0], Step::Think { .. }));
        assert!(matches!(plan.steps[1], Step::Finish { .. }));
    }

    #[test]
    fn test_load_nonexistent_skill_errors() {
        let (_tmp, manager) = setup();
        assert!(manager.load_skill("nonexistent").is_err());
    }

    #[test]
    fn test_plan_to_skill() {
        use crate::task::StepStatus;

        let steps = vec![
            crate::task::think_step("analyze"),
            crate::task::tool_call_step("file_read", serde_json::json!({"path": "test.txt"})),
            crate::task::finish_step("done"),
        ];
        let mut plan = crate::task::Plan::new("test-plan", steps);
        // Mark steps as completed
        if let Some(s) = plan.steps.get_mut(0) {
            s.set_status(StepStatus::Completed);
        }
        if let Some(s) = plan.steps.get_mut(1) {
            s.set_status(StepStatus::Completed);
        }
        if let Some(s) = plan.steps.get_mut(2) {
            s.set_status(StepStatus::Completed);
        }

        let skill = SkillManager::plan_to_skill(&plan, "learned-skill", "Learned from test");
        assert_eq!(skill.name, "learned-skill");
        assert_eq!(skill.steps.len(), 3);
        assert!(matches!(skill.steps[0], SkillStep::Think { .. }));
        assert!(matches!(skill.steps[1], SkillStep::ToolCall { .. }));
        assert!(matches!(skill.steps[2], SkillStep::Finish { .. }));
    }

    #[test]
    fn test_delete_skill() {
        let (_tmp, manager) = setup();
        let skill = SkillDef {
            name: "delete-me".into(),
            description: "to be deleted".into(),
            version: "1.0".into(),
            schema_version: "2.0".into(),
            trigger: vec![],
            steps: vec![],
        };
        manager.save_skill(&skill).unwrap();
        manager.delete_skill("delete-me").unwrap();
        assert!(manager.load_skill("delete-me").is_err());
    }
}
