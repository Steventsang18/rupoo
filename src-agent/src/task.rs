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
            (Step::Think { output: ref mut out, .. }, Some(val)) => *out = Some(val),
            (Step::Exec { output: ref mut out, .. }, Some(val)) => *out = Some(val),
            (Step::BrowserAction { output: ref mut out, .. }, Some(val)) => *out = Some(val),
            (Step::ToolCall { result: ref mut res, .. }, Some(val)) => {
                *res = serde_json::from_str(&val).ok()
            }
            (Step::HttpRequest { response: ref mut resp, .. }, Some(val)) => *resp = Some(val),
            (Step::WaitForInput { response: ref mut resp, .. }, Some(val)) => *resp = Some(val),
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
