//! Signal pipeline & output compression for Rupoo.
//!
//! Three responsibilities:
//! 1. **Output compression** — truncate tool results intelligently so LLM
//!    gets the most informative bytes within a tight budget.
//! 2. **Environment signals** — auto-inject PWD / git / project state into
//!    the system prompt so the LLM can "see" the user's context without
//!    asking.
//! 3. **Intent state** — track the user's intent across turns so old
//!    conversation history can be replaced by a compact summary.
//!
//! Design principle: signals are *compression*. Every environment signal
//! replaces a round of "what's my project?" dialogue; every compressed
//! output replaces raw bytes that the LLM would have to skim past; every
//! intent state update replaces the full replay of prior turns.

use std::path::Path;

use serde::{Serialize, Deserialize};

// ---------------------------------------------------------------------------
// Output compression
// ---------------------------------------------------------------------------

/// Maximum characters to include from a tool output.
/// Target: ~1500 tokens for a typical output, enough for the LLM to
/// understand the result without needing the full content.
const MAX_TOOL_OUTPUT_CHARS: usize = 4000;

/// Maximum characters for a file read without a specific target (function name, line range).
const MAX_FILE_CHARS_DEFAULT: usize = 3000;

/// Maximum characters for a file read when a target is specified (smaller window around the target).
const MAX_FILE_CHARS_TARGETED: usize = 2000;

/// Compress a raw tool output string into a token-efficient representation.
///
/// Strategies (applied in order):
/// 1. If under budget → return as-is.
/// 2. If over budget → show head + tail + line count summary.
pub fn compress_output(raw: &str, budget: Option<usize>) -> String {
    let limit = budget.unwrap_or(MAX_TOOL_OUTPUT_CHARS);

    if raw.len() <= limit {
        return raw.to_string();
    }

    let lines: Vec<&str> = raw.lines().collect();
    let total_lines = lines.len();

    if total_lines <= 20 {
        // Few lines but long lines — truncate by chars, show everything we can
        let end = raw.floor_char_boundary(limit);
        let mut out = raw[..end].to_string();
        out.push_str(&format!(
            "\n...[truncated at {end} chars, {} total lines]",
            total_lines
        ));
        return out;
    }

    // Many lines — show head + tail + summary
    let head_lines = (limit / 4).max(10).min(30);
    let tail_lines = (limit / 4).max(5).min(15);

    // Collect head lines within char budget
    let mut head_buf = String::new();
    let mut head_count = 0;
    for line in lines.iter().take(head_lines) {
        if head_buf.len() + line.len() + 1 > limit / 2 {
            break;
        }
        head_buf.push_str(line);
        head_buf.push('\n');
        head_count += 1;
    }

    // Collect tail lines within char budget
    let mut tail_buf = String::new();
    let mut tail_count = 0;
    for line in lines.iter().rev().take(tail_lines) {
        if tail_buf.len() + line.len() + 1 > limit / 3 {
            break;
        }
        tail_buf.insert_str(0, "\n");
        tail_buf.insert_str(0, line);
        tail_count += 1;
    }

    let omitted = total_lines - head_count - tail_count;

    format!(
        "{}\n...[{} lines omitted]...\n{}\n[total: {} lines, {} chars — use file_read with offset/limit for details]",
        head_buf.trim_end(), omitted, tail_buf.trim_start(), total_lines, raw.len()
    )
}

/// Compress a file's content with awareness of line numbers and optional target.
///
/// This is smarter than raw truncation:
/// - Returns line-numbered output so the LLM can reference specific lines.
/// - If a `target` substring is provided, centers the window around the first match.
/// - Always includes total line count so the LLM knows the file's scope.
pub fn compress_file_content(content: &str, path: &str, target: Option<&str>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Small file → return fully line-numbered
    // Truly tiny file (1-2 lines, <100 chars) → skip line number overhead
    if total_lines <= 80 && content.len() <= MAX_FILE_CHARS_DEFAULT {
        if total_lines <= 2 && content.len() <= 100 {
            return format!("[file_read: {}]\n{}", path, content);
        }
        return format_line_numbered(path, &lines, 0, total_lines, total_lines);
    }

    let budget = if target.is_some() {
        MAX_FILE_CHARS_TARGETED
    } else {
        MAX_FILE_CHARS_DEFAULT
    };

    // Targeted: center window around the first match
    if let Some(target_str) = target {
        if let Some(hit_line) = lines.iter().position(|l| l.contains(target_str)) {
            let window = 40; // 40 lines around the target
            let start = hit_line.saturating_sub(window / 2);
            let end = (start + window).min(total_lines);
            let shown = end - start;
            let mut out = format!(
                "[file_read: {} — {} lines total, showing lines {}-{} near \"{}\"]\n",
                path, total_lines, start + 1, end, target_str
            );
            out.push_str(&format_line_numbered(path, &lines, start, end, total_lines));
            out.push_str(&format!(
                "\n[{} lines omitted — use file_read with offset/limit for other sections]",
                total_lines - shown
            ));
            // If the output is still over budget, compress further
            if out.len() > budget * 2 {
                return compress_output(&out, Some(budget));
            }
            return out;
        }
    }

    // No target: show head (first 30 lines) + tail (last 10 lines) + summary
    let head_n = 30.min(total_lines);
    let tail_n = 15.min(total_lines.saturating_sub(head_n));

    let mut out = format!(
        "[file_read: {} — {} lines total, showing first {} and last {} lines]\n",
        path, total_lines, head_n, tail_n
    );

    out.push_str(&format_line_numbered(path, &lines, 0, head_n, total_lines));

    if tail_n > 0 {
        let tail_start = total_lines - tail_n;
        out.push_str(&format!(
            "\n...[{} lines omitted]...\n",
            tail_start - head_n
        ));
        out.push_str(&format_line_numbered(path, &lines, tail_start, total_lines, total_lines));
    }

    out.push_str(&format!(
        "\n[use file_read with offset/limit to read specific sections]"
    ));

    if out.len() > budget * 2 {
        return compress_output(&out, Some(budget));
    }

    out
}

/// Format lines with line numbers (1-indexed, right-aligned).
fn format_line_numbered(_path: &str, lines: &[&str], start: usize, end: usize, _total: usize) -> String {
    let width = format!("{}", end).len().max(3);
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>width$} | {}", start + i + 1, line, width = width))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Environment signals
// ---------------------------------------------------------------------------

/// Collected environment signals to inject into the system prompt.
#[derive(Debug, Default)]
pub struct EnvironmentSignals {
    pub pwd: String,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,     // e.g. "2 modified, 1 untracked"
    pub project_type: Option<String>,   // e.g. "Rust (Cargo.toml)"
    pub recent_files: Vec<String>,      // files modified in last 24h
    pub dir_summary: Option<String>,    // e.g. "src/ (12 files), tests/ (1 file)"
}

impl EnvironmentSignals {
    /// Collect environment signals from the current working directory.
    pub fn collect() -> Self {
        let mut signals = Self::default();

        // PWD
        if let Ok(pwd) = std::env::var("PWD").or_else(|_| std::env::var("HOME")) {
            signals.pwd = pwd.clone();

            // Project type detection
            let pwd_path = Path::new(&pwd);
            signals.project_type = detect_project_type(pwd_path);

            // Directory summary
            signals.dir_summary = build_dir_summary(pwd_path);

            // Recent files (modified in last 24h)
            signals.recent_files = find_recent_files(pwd_path, 24);
        }

        // Git info
        signals.git_branch = run_git_command(&["branch", "--show-current"]);
        signals.git_status = run_git_command(&["status", "--short"])
            .map(|s| summarize_git_status(&s));

        signals
    }

    /// Format signals as a block to inject into the system prompt.
    /// Designed to be compact (< 200 tokens typically).
    pub fn to_prompt_block(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("- PWD: {}", self.pwd));

        if let Some(ref branch) = self.git_branch {
            parts.push(format!("- Git: branch={}", branch));
        }
        if let Some(ref status) = self.git_status {
            parts.push(format!("- Git status: {}", status));
        }
        if let Some(ref ptype) = self.project_type {
            parts.push(format!("- Project: {}", ptype));
        }
        if !self.recent_files.is_empty() {
            let files_str = self.recent_files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            parts.push(format!("- Recent files (24h): {}", files_str));
        }
        if let Some(ref summary) = self.dir_summary {
            parts.push(format!("- Directory: {}", summary));
        }

        if parts.len() > 1 {
            format!("## Current Environment\n{}", parts.join("\n"))
        } else {
            String::new()
        }
    }
}

/// Detect project type from marker files.
fn detect_project_type(dir: &Path) -> Option<String> {
    if dir.join("Cargo.toml").exists() {
        return Some("Rust (Cargo.toml)".into());
    }
    if dir.join("package.json").exists() {
        return Some("Node.js (package.json)".into());
    }
    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return Some("Python".into());
    }
    if dir.join("go.mod").exists() {
        return Some("Go (go.mod)".into());
    }
    None
}

/// Build a brief directory summary.
fn build_dir_summary(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    let mut file_count = 0;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push(name);
        } else {
            file_count += 1;
        }
    }

    if dirs.is_empty() && file_count == 0 {
        return None;
    }

    let mut parts = Vec::new();
    if !dirs.is_empty() {
        parts.push(format!("dirs: {}", dirs.join(", ")));
    }
    if file_count > 0 {
        parts.push(format!("{} files", file_count));
    }

    Some(parts.join(", "))
}

/// Find files modified in the last N hours (depth=2 max for performance).
fn find_recent_files(dir: &Path, hours: u64) -> Vec<String> {
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(hours * 3600);

    let mut recent = Vec::new();
    let _ = collect_recent_recursive(dir, &cutoff, 0, 2, &mut recent);
    recent.truncate(8); // Cap at 8 files
    recent
}

fn collect_recent_recursive(
    dir: &Path,
    cutoff: &std::time::SystemTime,
    depth: usize,
    max_depth: usize,
    results: &mut Vec<String>,
) {
    if depth > max_depth || results.len() >= 8 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            collect_recent_recursive(&path, cutoff, depth + 1, max_depth, results);
        } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
            if modified > *cutoff {
                results.push(path.to_string_lossy().to_string());
            }
        }
    }
}

/// Run a git command and return stdout if successful.
fn run_git_command(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Summarize `git status --short` output into a brief string.
fn summarize_git_status(status: &str) -> String {
    let lines: Vec<&str> = status.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return "clean".into();
    }

    let modified = lines.iter().filter(|l| l.starts_with(" M") || l.starts_with('M')).count();
    let untracked = lines.iter().filter(|l| l.starts_with("??")).count();
    let staged = lines.iter().filter(|l| l.starts_with('A') || (l.starts_with('M') && !l.starts_with(" M"))).count();

    let mut parts = Vec::new();
    if modified > 0 { parts.push(format!("{} modified", modified)); }
    if untracked > 0 { parts.push(format!("{} untracked", untracked)); }
    if staged > 0 { parts.push(format!("{} staged", staged)); }

    parts.join(", ")
}

// ---------------------------------------------------------------------------
// Intent state — track what the user wants across turns
// ---------------------------------------------------------------------------

/// How precisely we understand the user's intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentPrecision {
    /// Barely any signal — the user mentioned something vague
    Vague,
    /// We know the direction but not the specifics
    Directional,
    /// Key decisions are confirmed, some details pending
    Structured,
    /// Enough clarity to execute
    Actionable,
}

impl Default for IntentPrecision {
    fn default() -> Self {
        Self::Vague
    }
}

impl IntentPrecision {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Vague => "vague",
            Self::Directional => "directional",
            Self::Structured => "structured",
            Self::Actionable => "actionable",
        }
    }
}

/// Compact representation of the user's intent, maintained across turns.
///
/// This is the "reins" — a lightweight structure that captures what the user
/// wants, what's confirmed, what's still open, and what artifacts exist.
/// It replaces the need to replay full conversation history.
///
/// The LLM updates this via a `[INTENT_UPDATE]` tag in its response.
/// rupoo parses, persists, and re-injects it next turn.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntentState {
    /// One-line summary of what the user wants
    pub summary: String,
    /// How precisely we understand the intent
    pub precision: IntentPrecision,
    /// Aspects that have been confirmed by the user
    pub confirmed: Vec<String>,
    /// Open questions or decisions pending
    pub pending: Vec<String>,
    /// Files or artifacts created so far
    pub artifacts: Vec<String>,
    /// Turn number (incremented each update)
    pub turn: usize,
}

impl IntentState {
    /// Create a new empty intent state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the intent is clear enough to start executing.
    pub fn is_actionable(&self) -> bool {
        self.precision == IntentPrecision::Actionable
    }

    /// Format as a compact prompt block (< 200 tokens typically).
    pub fn to_prompt_block(&self) -> String {
        if self.summary.is_empty() && self.turn == 0 {
            return String::new();
        }

        let mut parts = vec![format!(
            "## Intent (turn {}, precision: {})",
            self.turn,
            self.precision.as_str()
        )];

        if !self.summary.is_empty() {
            parts.push(format!("Summary: {}", self.summary));
        }

        if !self.confirmed.is_empty() {
            parts.push(format!("Confirmed: {}", self.confirmed.join(", ")));
        }

        if !self.pending.is_empty() {
            parts.push(format!("Pending: {}", self.pending.join(", ")));
        }

        if !self.artifacts.is_empty() {
            parts.push(format!("Artifacts: {}", self.artifacts.join(", ")));
        }

        parts.join("\n")
    }

    /// The instruction block to add to the system prompt so the LLM knows
    /// how to update intent state.
    pub fn system_instruction() -> String {
        "\
## Intent Tracking
You are tracking the user's intent across turns. After your response, update the intent state:
[INTENT_UPDATE]
summary: <one-line summary of what the user wants>
precision: <vague|directional|structured|actionable>
confirmed: <comma-separated confirmed aspects, or none>
pending: <comma-separated open questions, or none>
artifacts: <comma-separated files/paths created, or none>
[/INTENT_UPDATE]

Rules:
- precision=vague: barely any signal yet
- precision=directional: we know the general direction
- precision=structured: key decisions confirmed, some details open
- precision=actionable: clear enough to execute
- Only change precision when the user provides new confirming signal
- Keep summary under 20 words".to_string()
    }

    /// Parse an INTENT_UPDATE block from an LLM response.
    /// Returns the response with the block stripped, and the parsed state.
    pub fn parse_from_response(response: &str, current: &IntentState) -> (String, IntentState) {
        let start_tag = "[INTENT_UPDATE]";
        let end_tag = "[/INTENT_UPDATE]";

        let Some(start) = response.find(start_tag) else {
            return (response.to_string(), current.clone());
        };

        let Some(end) = response.find(end_tag) else {
            return (response.to_string(), current.clone());
        };

        let block = &response[start + start_tag.len()..end];
        let cleaned_response = format!("{}{}", &response[..start], &response[end + end_tag.len()..]);

        let mut new_state = current.clone();
        new_state.turn += 1;

        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(val) = line.strip_prefix("summary:") {
                new_state.summary = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("precision:") {
                let p = val.trim();
                new_state.precision = match p {
                    "vague" => IntentPrecision::Vague,
                    "directional" => IntentPrecision::Directional,
                    "structured" => IntentPrecision::Structured,
                    "actionable" => IntentPrecision::Actionable,
                    _ => new_state.precision.clone(),
                };
            } else if let Some(val) = line.strip_prefix("confirmed:") {
                let v = val.trim();
                if v != "none" && !v.is_empty() {
                    new_state.confirmed = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            } else if let Some(val) = line.strip_prefix("pending:") {
                let v = val.trim();
                if v != "none" && !v.is_empty() {
                    new_state.pending = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            } else if let Some(val) = line.strip_prefix("artifacts:") {
                let v = val.trim();
                if v != "none" && !v.is_empty() {
                    new_state.artifacts = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            }
        }

        (cleaned_response.trim().to_string(), new_state)
    }
}

// ---------------------------------------------------------------------------
// History compression — replace old turns with intent summary
// ---------------------------------------------------------------------------

/// Number of recent turns to keep as full-text before compressing.
const RECENT_TURNS_TO_KEEP: usize = 3;

/// Compress conversation history: keep recent N turns as-is, replace
/// everything before with the intent state summary.
///
/// This turns a linearly-growing history into a constant-size prefix
/// + sliding window, drastically reducing token usage for long conversations.
pub fn compress_history_with_intent(
    messages: &[crate::llm::LlmChatMessage],
    intent: &IntentState,
) -> Vec<crate::llm::LlmChatMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let intent_block = intent.to_prompt_block();

    // Count user messages to identify turns
    let user_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == crate::llm::LlmChatRole::User)
        .map(|(i, _)| i)
        .collect();

    // If few enough turns, no compression needed
    if user_indices.len() <= RECENT_TURNS_TO_KEEP {
        return messages.to_vec();
    }

    // Find the cutoff: keep the last RECENT_TURNS_TO_KEEP user turns
    let cutoff_turn = user_indices.len() - RECENT_TURNS_TO_KEEP;
    let cutoff_index = user_indices[cutoff_turn];

    // Build compressed: intent summary as system message + recent turns
    let mut result = Vec::new();

    if !intent_block.is_empty() {
        result.push(crate::llm::LlmChatMessage {
            role: crate::llm::LlmChatRole::System,
            content: intent_block,
        });
    }

    result.extend(messages[cutoff_index..].to_vec());
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_output_short() {
        let input = "hello world";
        assert_eq!(compress_output(input, None), input);
    }

    #[test]
    fn test_compress_output_long_few_lines() {
        let long_line = "x".repeat(5000);
        let input = long_line.clone();
        let result = compress_output(&input, None);
        assert!(result.len() < input.len());
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_compress_output_many_lines() {
        let lines: Vec<String> = (0..200).map(|i| format!("line {} {}", i, "x".repeat(30))).collect();
        let input = lines.join("\n");
        let result = compress_output(&input, Some(1000));
        assert!(result.contains("lines omitted"));
        assert!(result.contains("line 0"));
        assert!(result.contains("line 199"));
    }

    #[test]
    fn test_compress_file_small() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let result = compress_file_content(content, "main.rs", None);
        assert!(result.contains("1 |"));
        assert!(!result.contains("omitted"));
    }

    #[test]
    fn test_compress_file_with_target() {
        let lines: Vec<String> = (0..100).map(|i| format!("fn func_{}() {{}}", i)).collect();
        let content = lines.join("\n");
        let result = compress_file_content(&content, "lib.rs", Some("func_50"));
        assert!(result.contains("func_50"));
        assert!(result.contains("omitted"));
    }

    #[test]
    fn test_detect_project_type_rust() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_project_type(tmp.path()), Some("Rust (Cargo.toml)".into()));
    }

    #[test]
    fn test_detect_project_type_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(detect_project_type(tmp.path()), None);
    }

    #[test]
    fn test_summarize_git_status() {
        let status = " M src/main.rs\n?? new_file.txt\nA src/lib.rs";
        let result = summarize_git_status(status);
        assert!(result.contains("modified"));
        assert!(result.contains("untracked"));
    }

    #[test]
    fn test_summarize_git_status_clean() {
        let result = summarize_git_status("");
        assert_eq!(result, "clean");
    }

    #[test]
    fn test_environment_signals_format() {
        let signals = EnvironmentSignals {
            pwd: "/projects/myapp".into(),
            git_branch: Some("main".into()),
            git_status: Some("2 modified".into()),
            project_type: Some("Rust (Cargo.toml)".into()),
            recent_files: vec!["src/main.rs".into()],
            dir_summary: Some("src/, 3 files".into()),
        };
        let block = signals.to_prompt_block();
        assert!(block.contains("PWD: /projects/myapp"));
        assert!(block.contains("branch=main"));
    }
}

    #[test]
    fn test_intent_state_parse_from_response() {
        let response = "I'll help you with that!\n[INTENT_UPDATE]\nsummary: Build a file organizer CLI tool\nprecision: structured\nconfirmed: CLI, Rust, by-type classification\npending: exact file types to handle\nartifacts: none\n[/INTENT_UPDATE]\nLet me start by creating the project structure.";
        let current = IntentState::default();
        let (cleaned, new_state) = IntentState::parse_from_response(response, &current);

        assert!(!cleaned.contains("[INTENT_UPDATE]"));
        assert!(!cleaned.contains("[/INTENT_UPDATE]"));
        assert_eq!(new_state.summary, "Build a file organizer CLI tool");
        assert_eq!(new_state.precision, IntentPrecision::Structured);
        assert!(new_state.confirmed.contains(&"CLI".to_string()));
        assert!(new_state.confirmed.contains(&"Rust".to_string()));
        assert!(new_state.pending.contains(&"exact file types to handle".to_string()));
        assert_eq!(new_state.turn, 1);
    }

    #[test]
    fn test_intent_state_no_update_block() {
        let response = "Just a normal response without any intent update.";
        let current = IntentState::default();
        let (cleaned, new_state) = IntentState::parse_from_response(response, &current);

        assert_eq!(cleaned, response);
        assert_eq!(new_state.turn, 0); // No increment without update
    }

    #[test]
    fn test_intent_state_precision_progression() {
        let mut state = IntentState::default();
        assert_eq!(state.precision, IntentPrecision::Vague);

        state.summary = "organize files".to_string();
        state.precision = IntentPrecision::Directional;
        assert!(!state.is_actionable());

        state.precision = IntentPrecision::Actionable;
        assert!(state.is_actionable());
    }

    #[test]
    fn test_intent_state_prompt_block() {
        let state = IntentState {
            summary: "Build CLI tool".to_string(),
            precision: IntentPrecision::Structured,
            confirmed: vec!["Rust".to_string(), "CLI".to_string()],
            pending: vec!["file types".to_string()],
            artifacts: vec!["src/main.rs".to_string()],
            turn: 3,
        };
        let block = state.to_prompt_block();
        assert!(block.contains("turn 3"));
        assert!(block.contains("structured"));
        assert!(block.contains("Rust, CLI"));
    }

    #[test]
    fn test_compress_history_few_turns() {
        let messages = vec![
            crate::llm::LlmChatMessage::user("hello"),
            crate::llm::LlmChatMessage::assistant("hi"),
        ];
        let intent = IntentState::default();
        let compressed = compress_history_with_intent(&messages, &intent);
        assert_eq!(compressed.len(), 2); // No compression for few turns
    }

    #[test]
    fn test_compress_history_many_turns() {
        let mut messages = Vec::new();
        for i in 0..10 {
            messages.push(crate::llm::LlmChatMessage::user(&format!("msg {}", i)));
            messages.push(crate::llm::LlmChatMessage::assistant(&format!("reply {}", i)));
        }
        let intent = IntentState {
            summary: "Building a tool".to_string(),
            precision: IntentPrecision::Actionable,
            confirmed: vec!["Rust".to_string()],
            pending: vec![],
            artifacts: vec![],
            turn: 10,
        };
        let compressed = compress_history_with_intent(&messages, &intent);
        // Should be: 1 system (intent) + last 3 turns (6 messages) = 7
        assert!(compressed.len() < messages.len());
        assert!(compressed[0].content.contains("Building a tool"));
    }
