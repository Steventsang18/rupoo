//! AgentUiBridge — runs in a separate thread, bridges async agent to TUI

use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::{AgentToTui, TuiToAgent};
use super::approval::ApprovalExt;
use rupoo::agent::ToolExecutor;

// ═══════════════════════════════════════════════════════════════════════════
// AgentUiBridge — bridges async agent to TUI
// ═══════════════════════════════════════════════════════════════════════════

pub(super) struct AgentUiBridge {
    pub(super) agent: rupoo::agent::Agent,
    pub(super) repo: Arc<rupoo::db::TaskRepo>,
    pub(super) rx: Receiver<TuiToAgent>,
    pub(super) ui_tx: Sender<AgentToTui>,
    /// Current plan being executed (Shared with Mutex for interior mutability).
    pub(super) pending_plan: Mutex<Option<rupoo::task::Plan>>,
    /// Step index of the tool-call that is blocked on approval.
    pub(super) pending_step_index: Mutex<Option<usize>>,
    /// Direct reference to the tool executor for approval-time tool execution.
    pub(super) tool_executor: Arc<Box<dyn ToolExecutor>>,
    /// When true, automatically approve all future tool calls without user prompt.
    pub(super) approve_all: bool,
    /// Conversation history for multi-turn Chat Mode.
    pub(super) conversation_history: rupoo::llm::ConversationHistory,
}

impl AgentUiBridge {
    pub(super) async fn run(mut self) {
        loop {
            // Wait for a message from TUI or check for agent output
            match self.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(TuiToAgent::SubmitMessage(text)) => {
                    // Route to Chat Mode or Plan Mode based on prefix
                    if text.starts_with("/plan ") {
                        // Plan Mode: generate plan and execute
                        let task = text.trim_start_matches("/plan ").trim();
                        self.handle_plan_mode(task).await;
                    } else {
                        // Chat Mode: multi-turn agent chat
                        self.handle_chat_message(&text).await;
                    }
                }
                Ok(TuiToAgent::ApproveTool(_tool_name)) => {
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::ApproveAll) => {
                    self.approve_all = true;
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::DenyTool) => {
                    self.handle_denial().await;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message — continue loop
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // TUI closed — shutdown
                    break;
                }
            }
        }
    }
}
