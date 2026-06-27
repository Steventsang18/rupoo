//! CognitiveEngine implementation — uses LLM to parse, decompose, and
//! validate user goals against safety constraints.
//!
//! Bridges the raw instruction → structured AgentGoal pipeline that the
//! orchestrator's layer 1 depends on.

use async_trait::async_trait;
use tracing::{info, warn};

use crate::cognitive::goal::{AgentGoal, AuthLevel, ConstraintSeverity, GoalConstraint};
use crate::cognitive::CognitiveEngine;
use crate::context::ConversationContext;
use crate::error::{AgentError, AgentResult};
use crate::llm::{LlmGateway, TokenUsage};
use crate::safety::SafetyContext;

/// Default LLM config key for cognitive engine prompts.
/// Uses a compact, focused preamble to minimize token cost.
const PARSE_PREAMBLE: &str = r#"You are a goal-parsing assistant. Given a user's raw instruction,
extract a structured goal definition.

Respond ONLY with a JSON object containing these fields:
- primary_objective: a concise description of the core goal (max 100 chars)
- success_criteria: array of strings describing what success looks like (2-4 items)
- constraints: array of objects with {field, description, severity} where severity is "Suggestion"|"Required"|"Security"
- required_auth_level: "FullAuto"|"RequiresReview"|"Forbidden"
- metadata: object with key-value pairs for any extra context mentioned

Rules:
- If the instruction involves system-level changes (sudo, rm -rf, /etc/), set required_auth_level to "RequiresReview" or "Forbidden"
- If the instruction involves destructive data operations (delete, drop, truncate), set required_auth_level to "RequiresReview"
- If the instruction mentions files outside the current project, add a constraint with severity "Required"
- If the instruction is purely informational (read, search, explain), prefer "FullAuto"
- Keep metadata sparse — only include non-obvious context"#;

const DECOMPOSE_PREAMBLE: &str = r#"You are a task decomposition assistant. Given a goal,
determine whether it should be broken down into independent sub-goals.

Respond with a JSON object:
{ "should_decompose": bool, "sub_goals": [ { "primary_objective": "...", "rationale": "..." } ] }

Only decompose when:
1. The goal has 3+ distinct phases that could be parallelized
2. Different steps require fundamentally different tools or permissions
3. A sub-step has independent success criteria

Otherwise return should_decompose: false and empty sub_goals."#;

/// Default maximum sub-goals to generate.
const MAX_SUB_GOALS: usize = 5;

/// CognitiveEngine implementation that uses LLM for goal parsing
/// and SafetyContext for boundary enforcement.
pub struct CognitiveEngineImpl {
    /// LLM gateway for structured goal parsing and decomposition.
    llm: LlmGateway,
    /// Safety context for boundary and permissions checking.
    safety: SafetyContext,
}

impl CognitiveEngineImpl {
    /// Create a new engine with the given LLM gateway and safety context.
    pub fn new(llm: LlmGateway, safety: SafetyContext) -> Self {
        Self { llm, safety }
    }

    /// Create from borrowed config (convenience for tests).
    #[cfg(test)]
    pub fn test_instance() -> Self {
        use crate::llm::LlmProvider;
        let config = crate::llm::LlmConfig::new(LlmProvider::Ollama, None);
        Self {
            llm: LlmGateway::new(config),
            safety: SafetyContext::default(),
        }
    }

    /// Parse JSON response from the LLM into an AgentGoal.
    fn parse_goal_response(
        &self,
        raw_instruction: &str,
        response: &str,
        usage: Option<&TokenUsage>,
    ) -> AgentResult<AgentGoal> {
        let cleaned = crate::llm::gateway::extract_json_from_llm_response(response);

        #[derive(serde::Deserialize)]
        struct GoalResponse {
            primary_objective: String,
            #[serde(default)]
            success_criteria: Vec<String>,
            #[serde(default)]
            constraints: Vec<ConstraintResponse>,
            #[serde(default)]
            required_auth_level: String,
            #[serde(default)]
            metadata: std::collections::HashMap<String, String>,
        }

        #[derive(serde::Deserialize)]
        struct ConstraintResponse {
            field: String,
            description: String,
            severity: String,
        }

        let parsed: GoalResponse = serde_json::from_str(cleaned).map_err(|e| {
            AgentError::Other(format!(
                "Failed to parse goal from LLM response: {e}. Raw: {response}"
            ))
        })?;

        let auth_level = match parsed.required_auth_level.as_str() {
            "RequiresReview" => AuthLevel::RequiresReview,
            "Forbidden" => AuthLevel::Forbidden,
            _ => AuthLevel::FullAuto,
        };

        let constraints: Vec<GoalConstraint> = parsed
            .constraints
            .into_iter()
            .map(|c| {
                let severity = match c.severity.as_str() {
                    "Required" => ConstraintSeverity::Required,
                    "Security" => ConstraintSeverity::Security,
                    _ => ConstraintSeverity::Suggestion,
                };
                GoalConstraint {
                    field: c.field,
                    description: c.description,
                    severity,
                }
            })
            .collect();

        let mut goal =
            AgentGoal::new(raw_instruction, &parsed.primary_objective).with_auth(auth_level);

        for criterion in parsed.success_criteria {
            goal = goal.with_criterion(&criterion);
        }

        for constraint in constraints {
            goal = goal.with_constraint(
                &constraint.field,
                &constraint.description,
                constraint.severity,
            );
        }

        goal.metadata = parsed.metadata;

        if let Some(u) = usage {
            info!(
                prompt_tokens = u.prompt_tokens,
                completion_tokens = u.completion_tokens,
                "Goal parsed with LLM"
            );
        }

        Ok(goal)
    }
}

#[async_trait]
impl CognitiveEngine for CognitiveEngineImpl {
    async fn parse(&self, raw: &str, context: &ConversationContext) -> AgentResult<AgentGoal> {
        info!("[CognitiveEngine] Parsing instruction: {:.60}", raw);

        // Build a context-aware prompt
        let context_summary = format!(
            "## Context\n\
             - Current directory: {}\n\
             - Conversation turns: {}",
            context.environment.pwd, context.intent.turn,
        );

        let prompt = format!(
            "{}\n\n{}\n\n## User Instruction\n{}",
            PARSE_PREAMBLE, context_summary, raw
        );

        // Use chat (non-streaming) for structured parsing
        let message = crate::llm::history::LlmChatMessage::user(&prompt);
        let (response, usage) = self.llm.chat(&[message]).await?;

        let mut goal = self.parse_goal_response(raw, &response, Some(&usage))?;

        // Post-parse: enrich with concrete safety checks
        let safety_auth = self.safety_assessment(&goal);
        if safety_auth.clone() as u8 > goal.required_auth_level.clone() as u8 {
            info!(
                "Safety context overrides auth: {:?} → {:?}",
                goal.required_auth_level, safety_auth
            );
            goal = goal.with_auth(safety_auth);
        }

        Ok(goal)
    }

    async fn decompose(&self, goal: &AgentGoal) -> AgentResult<Vec<AgentGoal>> {
        info!(
            "[CognitiveEngine] Decomposing goal: {:.60}",
            goal.primary_objective
        );

        let prompt = format!(
            "{}\n\n## Goal\nprimary_objective: {}\n\
             success_criteria: {:?}\nconstraints: {:?}",
            DECOMPOSE_PREAMBLE, goal.primary_objective, goal.success_criteria, goal.constraints
        );

        let message = crate::llm::history::LlmChatMessage::user(&prompt);
        let (response, _usage) = self.llm.chat(&[message]).await?;

        let cleaned = crate::llm::gateway::extract_json_from_llm_response(&response);

        #[derive(serde::Deserialize)]
        struct DecomposeResponse {
            #[serde(default)]
            should_decompose: bool,
            #[serde(default)]
            sub_goals: Vec<SubGoalResponse>,
        }

        #[derive(serde::Deserialize)]
        struct SubGoalResponse {
            primary_objective: String,
            #[serde(default)]
            rationale: String,
        }

        let parsed: DecomposeResponse = serde_json::from_str(cleaned).map_err(|e| {
            AgentError::Other(format!(
                "Failed to parse decomposition from LLM: {e}. Raw: {response}"
            ))
        })?;

        if !parsed.should_decompose || parsed.sub_goals.is_empty() {
            info!("[CognitiveEngine] Goal does not require decomposition");
            return Ok(vec![]);
        }

        let sub_goals: Vec<AgentGoal> = parsed
            .sub_goals
            .into_iter()
            .take(MAX_SUB_GOALS)
            .map(|sg| {
                let mut sub = AgentGoal::new(&goal.raw_instruction, &sg.primary_objective);
                sub.metadata.insert("rationale".to_string(), sg.rationale);
                sub.metadata
                    .insert("parent_goal_id".to_string(), goal.id.clone());
                sub
            })
            .collect();

        info!(
            "[CognitiveEngine] Decomposed into {} sub-goals",
            sub_goals.len()
        );

        Ok(sub_goals)
    }

    async fn check_boundary(&self, goal: &AgentGoal) -> AgentResult<AuthLevel> {
        // 1. Check if any constraint is Security-level → Forbidden
        for constraint in &goal.constraints {
            if constraint.severity == ConstraintSeverity::Security {
                warn!(
                    "[CognitiveEngine] Security constraint triggered: {} - {}",
                    constraint.field, constraint.description
                );
                return Ok(AuthLevel::Forbidden);
            }
        }

        // 2. Check safety context
        let safety_level = self.safety_assessment(goal);

        // 3. Take the more restrictive of goal's declared level and safety assessment
        let effective = match (&goal.required_auth_level, safety_level) {
            (AuthLevel::Forbidden, _) | (_, AuthLevel::Forbidden) => AuthLevel::Forbidden,
            (AuthLevel::RequiresReview, _) | (_, AuthLevel::RequiresReview) => {
                AuthLevel::RequiresReview
            }
            _ => AuthLevel::FullAuto,
        };

        Ok(effective)
    }
}

// -- Private helpers ---------------------------------------------------------

impl CognitiveEngineImpl {
    /// Check goal against SafetyContext rules.
    fn safety_assessment(&self, goal: &AgentGoal) -> AuthLevel {
        let raw = goal.raw_instruction.to_lowercase();

        // Check against forbidden commands
        for cmd in self.safety.forbidden_commands() {
            if raw.contains(&cmd.to_lowercase()) {
                warn!("[CognitiveEngine] Forbidden command detected: {}", cmd);
                return AuthLevel::Forbidden;
            }
        }

        // Built-in high-risk keywords that should require review
        let require_approval_keywords = [
            "delete",
            "drop ",
            "truncate",
            "overwrite",
            "remove",
            "http://",
            "curl ",
            "wget ",
            "post ",
            "put ",
            "config",
            "secret",
            "token",
            "password",
            "credential",
            "environment",
            "production",
            "deploy",
            "release",
        ];
        for keyword in &require_approval_keywords {
            if raw.contains(keyword) {
                info!("[CognitiveEngine] High-risk keyword detected: {}", keyword);
                return AuthLevel::RequiresReview;
            }
        }

        // Check for path safety violations
        if let Some(jail) = self.safety.jail_root() {
            let jail_str = jail.to_string_lossy().to_lowercase();
            for word in raw.split_whitespace() {
                if word.starts_with('/')
                    && !word.starts_with(&jail_str)
                    && word.len() > 1
                    && !word.starts_with("/.")
                {
                    info!("[CognitiveEngine] Path outside jail detected: {}", word);
                    return AuthLevel::RequiresReview;
                }
            }
        }

        AuthLevel::FullAuto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safety_assessment_forbidden_command() {
        let engine = CognitiveEngineImpl::test_instance();
        let goal = AgentGoal::new("sudo rm -rf /", "Delete everything");
        let result = engine.safety_assessment(&goal);
        assert_eq!(result, AuthLevel::Forbidden);
    }

    #[test]
    fn test_safety_assessment_require_approval() {
        let engine = CognitiveEngineImpl::test_instance();
        let goal = AgentGoal::new("drop table users", "Delete users table");
        let result = engine.safety_assessment(&goal);
        // "drop" might be in require_approval depending on SafetyContext defaults
        assert!(result == AuthLevel::RequiresReview || result == AuthLevel::Forbidden);
    }

    #[test]
    fn test_safety_assessment_read_only() {
        let engine = CognitiveEngineImpl::test_instance();
        let goal = AgentGoal::new("cat README.md", "Read readme");
        let result = engine.safety_assessment(&goal);
        assert_eq!(result, AuthLevel::FullAuto);
    }

    #[test]
    fn test_parse_goal_response() {
        let engine = CognitiveEngineImpl::test_instance();

        let response = r#"{
            "primary_objective": "Create a hello world Rust program",
            "success_criteria": ["Program compiles", "Outputs 'Hello, World!'"],
            "constraints": [{"field": "path", "description": "Must be in current directory", "severity": "Required"}],
            "required_auth_level": "FullAuto",
            "metadata": {"language": "rust"}
        }"#;

        let goal = engine
            .parse_goal_response("create hello world", response, None)
            .unwrap();

        assert_eq!(goal.primary_objective, "Create a hello world Rust program");
        assert_eq!(goal.success_criteria.len(), 2);
        assert_eq!(goal.constraints.len(), 1);
        assert_eq!(goal.required_auth_level, AuthLevel::FullAuto);
        assert_eq!(goal.metadata.get("language").unwrap(), "rust");
    }

    #[test]
    fn test_parse_goal_forbidden_level() {
        let engine = CognitiveEngineImpl::test_instance();

        let response = r#"{
            "primary_objective": "Remove system files",
            "success_criteria": [],
            "constraints": [],
            "required_auth_level": "Forbidden",
            "metadata": {}
        }"#;

        let goal = engine
            .parse_goal_response("rm -rf /", response, None)
            .unwrap();

        assert_eq!(goal.required_auth_level, AuthLevel::Forbidden);
    }

    #[test]
    fn test_parse_goal_empty_criteria() {
        let engine = CognitiveEngineImpl::test_instance();

        let response = r#"{
            "primary_objective": "Simple read operation",
            "success_criteria": [],
            "constraints": [],
            "required_auth_level": "FullAuto",
            "metadata": {}
        }"#;

        let goal = engine
            .parse_goal_response("read file", response, None)
            .unwrap();

        assert!(goal.success_criteria.is_empty());
    }

    #[tokio::test]
    async fn test_check_boundary_security_constraint() {
        let engine = CognitiveEngineImpl::test_instance();
        let mut goal = AgentGoal::new("do something", "Something");
        goal = goal.with_constraint(
            "network",
            "Must not access internal network",
            ConstraintSeverity::Security,
        );

        // Security constraint should result in Forbidden
        match engine.check_boundary(&goal).await {
            Ok(AuthLevel::Forbidden) => {} // expected
            other => panic!("Expected Forbidden, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_check_boundary_suggestion_constraint() {
        let engine = CognitiveEngineImpl::test_instance();
        let mut goal = AgentGoal::new("read file", "Read file");
        goal = goal.with_constraint("time", "Do it quickly", ConstraintSeverity::Suggestion);

        let result = engine.check_boundary(&goal).await.unwrap();
        assert_eq!(result, AuthLevel::FullAuto);
    }

    #[test]
    fn test_max_sub_goals_constant() {
        const _: () = assert!(MAX_SUB_GOALS <= 10, "MAX_SUB_GOALS should be reasonable");
    }
}
