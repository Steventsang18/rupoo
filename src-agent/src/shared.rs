//! Shared types between the agent core and the TUI presentation layer.
//!
//! This module is the single source of truth for message types that flow
//! between the agent engine and the terminal UI.

use serde::{Deserialize, Serialize};

use crate::task::StepStatus;

// ---------------------------------------------------------------------------
// Layout Mode — controls CLI rendering style
// ---------------------------------------------------------------------------

/// The current layout mode determines how the CLI renders events.
/// Auto-detected based on user intent, can be overridden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LayoutMode {
    /// Bubbles-only, like a casual chat. No tool panels.
    /// Used for: info queries, casual conversation, non-development asks.
    #[default]
    Chat,
    /// Left panel: progress + reasoning, Right panel: results.
    /// Used for: code generation, refactoring, multi-step development tasks.
    Work,
    /// Compact one-liner summary when a task completes.
    /// Auto-transitions to Chat after display.
    Summary,
}

/// What happened to a file during a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileAction {
    Modified,
    Created,
    Deleted,
}

/// Describes one file change for the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangeInfo {
    pub path: String,
    pub action: FileAction,
    pub lines_added: u32,
    pub lines_removed: u32,
}

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
    Error,
}

impl ChatMessage {
    pub fn user(text: String) -> Self {
        Self {
            role: MessageRole::User,
            content: text,
            is_command_output: false,
        }
    }
    pub fn assistant(text: String) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: text,
            is_command_output: false,
        }
    }
    pub fn system(text: String) -> Self {
        Self {
            role: MessageRole::System,
            content: text,
            is_command_output: false,
        }
    }
    pub fn command_output(text: String) -> Self {
        Self {
            role: MessageRole::System,
            content: text,
            is_command_output: true,
        }
    }
    pub fn error(text: String) -> Self {
        Self {
            role: MessageRole::Error,
            content: text,
            is_command_output: false,
        }
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
    /// User cancelled the current generation.
    Cancel,
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
    /// Streaming text chunk for incremental display.
    StreamChunk { text: String },
    /// LLM configuration status update.
    LlmStatus {
        configured: bool,
        provider: String,
        model_label: String,
    },
    /// Step progress update for Plan Mode.
    StepProgress {
        step_index: usize,
        total: usize,
        step_name: String,
    },
    /// Tool call status update for Chat Mode progress display.
    ToolStatus { tool_name: String, phase: ToolPhase },
    /// Plan task list for display in Plan Mode.
    PlanTaskList { tasks: Vec<(String, StepStatus)> },
    /// Hybrid search (deep search) status update.
    HybridSearchUpdate { enabled: bool },
    // ═══════════════════════════════════════════════════════════════════
    // 方案 C 新增事件类型 — 混合自适应布局
    // ═══════════════════════════════════════════════════════════════════
    /// LLM 推理摘要。轻量级说明当前在做什么思考。
    /// 例如："正在分析 src/error.rs 中的错误处理..."
    /// 渲染为左侧绿色斜体，不占用对话流空间。
    ThinkingSummary { text: String },

    /// 阶段级进度更新（Work 模式使用）。
    /// phase_name: "重构错误处理", percentage: 0-100
    PhaseProgress { phase_name: String, percentage: u8 },

    /// 布局模式自动切换提示。
    /// 由 bridge 层根据 IntentState 检测后发送。
    LayoutModeHint(LayoutMode),

    /// 文件变更事件。工具执行完成后发送一次。
    FileChanges { files: Vec<FileChangeInfo> },
}

/// Phase of a tool call for status display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolPhase {
    /// Tool is being called (e.g. "searching…")
    Calling,
    /// Tool has returned a result
    Completed,
}

impl AgentToTui {
    /// Check if this is a state event (triggers UI state change).
    pub fn is_event(&self) -> bool {
        matches!(
            self,
            Self::Message(_)
                | Self::Thinking
                | Self::Idle
                | Self::StreamChunk { .. }
                | Self::RequestApproval(_)
                | Self::StepProgress { .. }
                | Self::ToolStatus { .. }
                | Self::ThinkingSummary { .. }
                | Self::PhaseProgress { .. }
                | Self::LayoutModeHint(_)
                | Self::FileChanges { .. }
        )
    }

    /// Check if this is a data update (informational, non-UI-state).
    pub fn is_data(&self) -> bool {
        matches!(self, Self::TokenUpdate { .. } | Self::LlmStatus { .. })
    }
}
