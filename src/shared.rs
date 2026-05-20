//! Shared types between the agent core and the TUI presentation layer.
//!
//! This module is the single source of truth for message types that flow
//! between the agent engine and the terminal UI.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Message types (mirrors what was previously in cli/app.rs)
// ---------------------------------------------------------------------------

/// A single chat message in the TUI history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub is_command_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl ChatMessage {
    pub fn user(text: String) -> Self {
        Self { role: MessageRole::User, content: text, is_command_output: false }
    }
    pub fn assistant(text: String) -> Self {
        Self { role: MessageRole::Assistant, content: text, is_command_output: false }
    }
    pub fn system(text: String) -> Self {
        Self { role: MessageRole::System, content: text, is_command_output: false }
    }
    pub fn command_output(text: String) -> Self {
        Self { role: MessageRole::System, content: text, is_command_output: true }
    }
}

/// A tool call that is pending user approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTool {
    pub tool_name: String,
    pub args: String, // JSON representation of tool arguments
}

/// User's response to a pending tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalChoice {
    /// Approve this execution only.
    ApproveOnce,
    /// Approve this and all future tool calls in this plan.
    ApproveAll,
    /// Deny this execution only.
    Deny,
    /// Deny this and block all future tool calls in this plan.
    DenyBlock,
}

// ---------------------------------------------------------------------------
// Bidirectional channel types
// ---------------------------------------------------------------------------

/// Messages sent FROM the TUI TO the agent bridge thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TuiToAgent {
    /// The user submitted a chat message.
    SubmitMessage(String),
    /// User approved the currently pending tool call (once).
    ApproveTool(String),
    /// User approved the current and all future tool calls.
    ApproveAll,
    /// User denied the currently pending tool call.
    DenyTool,
}

/// Events sent FROM the agent bridge thread TO the TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentToTui {
    /// A new chat message to display.
    Message(ChatMessage),
    /// The agent has entered a thinking/streaming state.
    Thinking,
    /// The agent is idle (no pending work).
    Idle,
    /// Token usage counters updated.
    TokenUpdate { in_count: u64, out_count: u64 },
    /// The agent requires approval before executing a tool.
    RequestApproval(PendingTool),
}