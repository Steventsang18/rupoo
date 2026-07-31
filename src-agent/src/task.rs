//! Core data types for Rupoo's plan-execution system.
//!
//! # Key Types
//!
//! - [`Step`] — tagged enum of 7 step kinds: Think, ToolCall, WaitForInput,
//!   Finish, Exec, HttpRequest, BrowserAction
//! - [`Plan`] — ordered sequence of steps with execution state tracking
//! - [`Checkpoint`] — per-step execution record for crash recovery
//! - [`StepStatus`] / [`PlanStatus`] — lifecycle state machines
//! - [`McpToolResult`] — MCP tool call response wrapper
//! - [`MemoryEntry`] — long-term memory record
//!
//! # Convenience Constructors
//!
//! Seven step factory functions are provided:
//! [`think_step()`], [`tool_call_step()`], [`wait_for_input_step()`],
//! [`exec_step()`], [`http_request_step()`], [`browser_action_step()`],
//! [`finish_step()`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Browser action type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowserActionType {
    Navigate,
    Screenshot,
    Click,
    GetText,
    ExtractLinks,
    JavaScript,
}

// ---------------------------------------------------------------------------
// HTTP method
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HttpMethod {
    GET,
    POST,
}

// ---------------------------------------------------------------------------
// Step enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    WaitingForInput,
}

/// Serializable step in an agent plan.
///
/// Seven variants cover the full range of agent actions:
/// - `Think` — LLM reasoning step
/// - `ToolCall` — call a registered tool with JSON params
/// - `WaitForInput` — pause for user input
/// - `Finish` — terminal step marking plan completion
/// - `Exec` — shell command execution
/// - `HttpRequest` — HTTP API call
/// - `BrowserAction` — browser automation (navigate, click, screenshot, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Step {
    Think {
        id: String,
        instruction: String,
        status: StepStatus,
        output: Option<String>,
    },
    ToolCall {
        id: String,
        tool_name: String,
        params: serde_json::Value,
        status: StepStatus,
        result: Option<serde_json::Value>,
    },
    WaitForInput {
        id: String,
        prompt: String,
        status: StepStatus,
        response: Option<String>,
    },
    Finish {
        id: String,
        summary: String,
        status: StepStatus,
    },
    /// Execute an external command with optional timeout.
    Exec {
        id: String,
        command: String,
        args: Vec<String>,
        timeout_secs: Option<u64>,
        status: StepStatus,
        output: Option<String>,
    },
    /// Perform an HTTP request.
    HttpRequest {
        id: String,
        url: String,
        method: HttpMethod,
        body: Option<String>,
        headers: Option<std::collections::HashMap<String, String>>,
        status: StepStatus,
        response: Option<String>,
    },
    /// Control a browser (navigate, screenshot, click, get text).
    BrowserAction {
        id: String,
        action: BrowserActionType,
        url: Option<String>,
        selector: Option<String>,
        timeout_secs: Option<u64>,
        status: StepStatus,
        output: Option<String>,
    },
}

impl Step {
    pub fn id(&self) -> &str {
        match self {
            Step::Think { id, .. }
            | Step::ToolCall { id, .. }
            | Step::WaitForInput { id, .. }
            | Step::Finish { id, .. }
            | Step::Exec { id, .. }
            | Step::HttpRequest { id, .. }
            | Step::BrowserAction { id, .. } => id,
        }
    }

    /// Human-readable label for this step. Short, max ~40 chars.
    pub fn label(&self) -> String {
        match self {
            Step::Think { instruction, .. } => instruction.chars().take(40).collect(),
            Step::ToolCall { tool_name, .. } => format!("调用 {}", tool_name),
            Step::WaitForInput { prompt, .. } => prompt.chars().take(40).collect(),
            Step::Finish { summary, .. } => summary.chars().take(40).collect(),
            Step::Exec { command, args, .. } => {
                if args.is_empty() {
                    command.chars().take(40).collect()
                } else {
                    format!("{} {}", command, args.join(" "))
                        .chars()
                        .take(40)
                        .collect()
                }
            }
            Step::HttpRequest { method, url, .. } => {
                let m = match method {
                    HttpMethod::GET => "GET",
                    HttpMethod::POST => "POST",
                };
                format!("{} {}", m, url).chars().take(40).collect()
            }
            Step::BrowserAction { action, .. } => format!("{:?}", action),
        }
    }

    pub fn status(&self) -> &StepStatus {
        match self {
            Step::Think { status, .. }
            | Step::ToolCall { status, .. }
            | Step::WaitForInput { status, .. }
            | Step::Finish { status, .. }
            | Step::Exec { status, .. }
            | Step::HttpRequest { status, .. }
            | Step::BrowserAction { status, .. } => status,
        }
    }

    pub fn set_status(&mut self, new_status: StepStatus) {
        let status = match self {
            Step::Think { status, .. }
            | Step::ToolCall { status, .. }
            | Step::WaitForInput { status, .. }
            | Step::Finish { status, .. }
            | Step::Exec { status, .. }
            | Step::HttpRequest { status, .. }
            | Step::BrowserAction { status, .. } => status,
        };
        *status = new_status;
    }

    /// Set the output/result field from a string value, if the step variant supports it.
    /// Used by record_step_completion to persist step outputs into steps_json.
    pub fn set_output_from_string(&mut self, output: Option<String>) {
        match (self, output) {
            (
                Step::Think {
                    output: ref mut out,
                    ..
                },
                Some(val),
            ) => *out = Some(val),
            (
                Step::Exec {
                    output: ref mut out,
                    ..
                },
                Some(val),
            ) => *out = Some(val),
            (
                Step::BrowserAction {
                    output: ref mut out,
                    ..
                },
                Some(val),
            ) => *out = Some(val),
            (
                Step::ToolCall {
                    result: ref mut res,
                    ..
                },
                Some(val),
            ) => *res = serde_json::from_str(&val).ok(),
            (
                Step::HttpRequest {
                    response: ref mut resp,
                    ..
                },
                Some(val),
            ) => *resp = Some(val),
            (
                Step::WaitForInput {
                    response: ref mut resp,
                    ..
                },
                Some(val),
            ) => *resp = Some(val),
            _ => {} // Finish has no output field
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Step::Finish { .. })
    }

    pub fn is_waiting(&self) -> bool {
        matches!(self, Step::WaitForInput { .. })
    }
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Pending,
    Running,
    Completed,
    Failed,
    WaitingForInput,
}

/// Execution plan — an ordered sequence of steps with cursor tracking.
///
/// # Fields
///
/// - `current_step_index` — index into `steps` that will execute next
/// - `status` — lifecycle state (`Pending` → `Running` → `Completed`/`Failed`)
///
/// Created via [`Plan::new()`], loaded from DB via [`crate::db::TaskRepo`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub steps: Vec<Step>,
    pub current_step_index: usize,
    pub status: PlanStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Plan {
    pub fn new(name: &str, steps: Vec<Step>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            steps,
            current_step_index: 0,
            status: PlanStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    pub fn current_step(&self) -> Option<&Step> {
        self.steps.get(self.current_step_index)
    }

    pub fn current_step_mut(&mut self) -> Option<&mut Step> {
        self.steps.get_mut(self.current_step_index)
    }

    pub fn is_complete(&self) -> bool {
        self.status == PlanStatus::Completed
    }
}

// ---------------------------------------------------------------------------
// Checkpoint
// ---------------------------------------------------------------------------

/// Per-step execution record for crash recovery.
///
/// Written after each step completes (or fails), allowing the agent to resume
/// from the last completed checkpoint after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub plan_id: String,
    pub step_index: usize,
    pub status: CheckpointStatus,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CheckpointStatus {
    Running,
    Completed,
    Failed,
}

impl Checkpoint {
    pub fn new(plan_id: &str, step_index: usize, status: CheckpointStatus) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            plan_id: plan_id.to_string(),
            step_index,
            status,
            output: None,
            created_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Step constructors (convenience)
// ---------------------------------------------------------------------------

pub fn think_step(instruction: &str) -> Step {
    Step::Think {
        id: Uuid::new_v4().to_string(),
        instruction: instruction.to_string(),
        status: StepStatus::Pending,
        output: None,
    }
}

pub fn tool_call_step(tool_name: &str, params: serde_json::Value) -> Step {
    Step::ToolCall {
        id: Uuid::new_v4().to_string(),
        tool_name: tool_name.to_string(),
        params,
        status: StepStatus::Pending,
        result: None,
    }
}

pub fn wait_for_input_step(prompt: &str) -> Step {
    Step::WaitForInput {
        id: Uuid::new_v4().to_string(),
        prompt: prompt.to_string(),
        status: StepStatus::Pending,
        response: None,
    }
}

pub fn exec_step(command: &str, args: Vec<String>, timeout_secs: Option<u64>) -> Step {
    Step::Exec {
        id: Uuid::new_v4().to_string(),
        command: command.to_string(),
        args,
        timeout_secs,
        status: StepStatus::Pending,
        output: None,
    }
}

pub fn http_request_step(
    url: &str,
    method: HttpMethod,
    body: Option<String>,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Step {
    Step::HttpRequest {
        id: Uuid::new_v4().to_string(),
        url: url.to_string(),
        method,
        body,
        headers,
        status: StepStatus::Pending,
        response: None,
    }
}

pub fn browser_action_step(
    action: BrowserActionType,
    url: Option<String>,
    selector: Option<String>,
    timeout_secs: Option<u64>,
) -> Step {
    Step::BrowserAction {
        id: Uuid::new_v4().to_string(),
        action,
        url,
        selector,
        timeout_secs,
        status: StepStatus::Pending,
        output: None,
    }
}

pub fn finish_step(summary: &str) -> Step {
    Step::Finish {
        id: Uuid::new_v4().to_string(),
        summary: summary.to_string(),
        status: StepStatus::Pending,
    }
}

// ---------------------------------------------------------------------------
// MCP tool result type (used for tool call responses)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpToolResult {
    Success { content: String },
    Error { message: String },
}

impl McpToolResult {
    /// Returns true if this is a success result.
    pub fn is_success(&self) -> bool {
        matches!(self, McpToolResult::Success { .. })
    }

    /// Get the content string. Returns content for Success, error message for Error.
    pub fn content(&self) -> &str {
        match self {
            McpToolResult::Success { content } => content,
            McpToolResult::Error { message } => message,
        }
    }

    /// Get the error message if this is an Error result.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            McpToolResult::Error { message } => Some(message),
            McpToolResult::Success { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Memory entry (long-term memory system)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Step constructors ─────────────────────────────────────────

    #[test]
    fn test_think_step() {
        let step = think_step("analyze the problem");
        assert_eq!(step.id().len(), 36); // UUID v4
        assert_eq!(step.label(), "analyze the problem");
        assert_eq!(step.status(), &StepStatus::Pending);
        assert!(!step.is_terminal());
        assert!(!step.is_waiting());
    }

    #[test]
    fn test_tool_call_step() {
        let step = tool_call_step("echo", serde_json::json!({"msg": "hello"}));
        assert!(step.label().contains("echo"));
        assert_eq!(step.status(), &StepStatus::Pending);
    }

    #[test]
    fn test_wait_for_input_step() {
        let step = wait_for_input_step("Enter name:");
        assert_eq!(step.label(), "Enter name:");
        assert!(step.is_waiting());
        assert!(!step.is_terminal());
    }

    #[test]
    fn test_exec_step() {
        let step = exec_step("ls", vec!["-la".into()], Some(10));
        assert!(step.label().contains("ls"));
        assert!(!step.is_terminal());
    }

    #[test]
    fn test_http_request_step() {
        let step = http_request_step("https://api.example.com", HttpMethod::GET, None, None);
        assert!(step.label().contains("GET"));
        assert!(step.label().contains("api.example.com"));
        assert!(!step.is_terminal());
    }

    #[test]
    fn test_browser_action_step() {
        let step = browser_action_step(
            BrowserActionType::Navigate,
            Some("https://example.com".into()),
            None,
            Some(30),
        );
        assert!(step.label().contains("Navigate"));
        assert!(!step.is_terminal());
    }

    #[test]
    fn test_finish_step() {
        let step = finish_step("task completed");
        assert_eq!(step.label(), "task completed");
        assert!(step.is_terminal());
        assert!(!step.is_waiting());
    }

    // ── Step::set_status ──────────────────────────────────────────

    #[test]
    fn test_step_set_status() {
        let mut step = think_step("test");
        assert_eq!(step.status(), &StepStatus::Pending);
        step.set_status(StepStatus::Running);
        assert_eq!(step.status(), &StepStatus::Running);
        step.set_status(StepStatus::Completed);
        assert_eq!(step.status(), &StepStatus::Completed);
    }

    // ── Step::set_output_from_string ──────────────────────────────

    #[test]
    fn test_set_output_think() {
        let mut step = think_step("test");
        step.set_output_from_string(Some("reasoning result".into()));
        assert!(matches!(&step, Step::Think { output: Some(o), .. } if o == "reasoning result"));
    }

    #[test]
    fn test_set_output_exec() {
        let mut step = exec_step("echo", vec!["hi".into()], None);
        step.set_output_from_string(Some("hi\n".into()));
        assert!(matches!(&step, Step::Exec { output: Some(o), .. } if o == "hi\n"));
    }

    #[test]
    fn test_set_output_tool_call() {
        let mut step = tool_call_step("echo", serde_json::json!({}));
        step.set_output_from_string(Some(r#"{"status":"ok"}"#.into()));
        assert!(matches!(
            &step,
            Step::ToolCall {
                result: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn test_set_output_http_request() {
        let mut step = http_request_step("https://example.com", HttpMethod::GET, None, None);
        step.set_output_from_string(Some("HTTP 200 OK".into()));
        assert!(matches!(&step, Step::HttpRequest { response: Some(r), .. } if r == "HTTP 200 OK"));
    }

    #[test]
    fn test_set_output_wait_for_input() {
        let mut step = wait_for_input_step("Enter:");
        step.set_output_from_string(Some("Alice".into()));
        assert!(matches!(&step, Step::WaitForInput { response: Some(r), .. } if r == "Alice"));
    }

    #[test]
    fn test_set_output_finish_ignored() {
        let mut step = finish_step("done");
        step.set_output_from_string(Some("ignored".into()));
        // Finish has no output field, should be no-op
        assert!(matches!(&step, Step::Finish { .. }));
    }

    #[test]
    fn test_set_output_none_is_noop() {
        let mut step = think_step("test");
        step.set_output_from_string(None);
        assert!(matches!(&step, Step::Think { output: None, .. }));
    }

    // ── Step label edge cases ─────────────────────────────────────

    #[test]
    fn test_label_long_text_truncated() {
        let long = "A".repeat(100);
        let step = think_step(&long);
        let label = step.label();
        assert!(label.len() <= 40);
    }

    #[test]
    fn test_label_exec_with_args() {
        let step = exec_step(
            "git",
            vec!["commit".into(), "-m".into(), "msg".into()],
            None,
        );
        let label = step.label();
        assert!(label.contains("git"));
        assert!(label.contains("commit"));
    }

    // ── Plan ──────────────────────────────────────────────────────

    #[test]
    fn test_plan_new() {
        let steps = vec![think_step("step1"), finish_step("done")];
        let plan = Plan::new("my-plan", steps);
        assert_eq!(plan.name, "my-plan");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.current_step_index, 0);
        assert_eq!(plan.status, PlanStatus::Pending);
        assert!(!plan.is_complete());
    }

    #[test]
    fn test_plan_current_step() {
        let steps = vec![think_step("first"), finish_step("last")];
        let plan = Plan::new("test", steps);
        let current = plan.current_step().unwrap();
        assert_eq!(current.label(), "first");
    }

    #[test]
    fn test_plan_current_step_out_of_bounds() {
        let plan = Plan::new("test", vec![]);
        assert!(plan.current_step().is_none());
    }

    #[test]
    fn test_plan_current_step_mut() {
        let steps = vec![think_step("first")];
        let mut plan = Plan::new("test", steps);
        let step = plan.current_step_mut().unwrap();
        step.set_status(StepStatus::Completed);
        assert_eq!(plan.steps[0].status(), &StepStatus::Completed);
    }

    // ── Checkpoint ────────────────────────────────────────────────

    #[test]
    fn test_checkpoint_new() {
        let ckpt = Checkpoint::new("plan-1", 2, CheckpointStatus::Running);
        assert_eq!(ckpt.plan_id, "plan-1");
        assert_eq!(ckpt.step_index, 2);
        assert_eq!(ckpt.status, CheckpointStatus::Running);
        assert!(ckpt.output.is_none());
    }

    // ── McpToolResult ─────────────────────────────────────────────

    #[test]
    fn test_mcp_tool_result_success() {
        let result = McpToolResult::Success {
            content: "operation ok".into(),
        };
        assert!(result.is_success());
        assert_eq!(result.content(), "operation ok");
        assert!(result.error_message().is_none());
    }

    #[test]
    fn test_mcp_tool_result_error() {
        let result = McpToolResult::Error {
            message: "timeout".into(),
        };
        assert!(!result.is_success());
        assert_eq!(result.content(), "timeout");
        assert_eq!(result.error_message(), Some("timeout"));
    }

    // ── Serialization round-trips ─────────────────────────────────

    #[test]
    fn test_step_serialization_roundtrip() {
        let steps = vec![
            think_step("analyze"),
            tool_call_step("echo", serde_json::json!({"msg": "hi"})),
            wait_for_input_step("Enter:"),
            exec_step("ls", vec!["-l".into()], Some(5)),
            http_request_step("https://example.com", HttpMethod::GET, None, None),
            browser_action_step(
                BrowserActionType::Screenshot,
                Some("https://ex.com".into()),
                None,
                None,
            ),
            finish_step("done"),
        ];

        for step in &steps {
            let json = serde_json::to_string(step).unwrap();
            let parsed: Step = serde_json::from_str(&json).unwrap();
            assert_eq!(step.id(), parsed.id());
            assert_eq!(step.status(), parsed.status());
        }
    }

    #[test]
    fn test_plan_serialization_roundtrip() {
        let steps = vec![think_step("s1"), finish_step("s2")];
        let plan = Plan::new("test-plan", steps);
        let json = serde_json::to_string(&plan).unwrap();
        let parsed: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan.name, parsed.name);
        assert_eq!(plan.steps.len(), parsed.steps.len());
        assert_eq!(plan.status, parsed.status);
    }

    #[test]
    fn test_browser_action_type_serialization() {
        let actions = vec![
            BrowserActionType::Navigate,
            BrowserActionType::Screenshot,
            BrowserActionType::Click,
            BrowserActionType::GetText,
            BrowserActionType::ExtractLinks,
            BrowserActionType::JavaScript,
        ];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let parsed: BrowserActionType = serde_json::from_str(&json).unwrap();
            assert_eq!(action, parsed);
        }
    }

    #[test]
    fn test_step_status_serialization() {
        let statuses = vec![
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Completed,
            StepStatus::Failed,
            StepStatus::WaitingForInput,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: StepStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }
}
