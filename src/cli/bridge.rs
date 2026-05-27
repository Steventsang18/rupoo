//! AgentUiBridge — runs in a separate thread, bridges async agent to TUI

use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::{AgentToTui, TuiToAgent};
use super::approval::ApprovalExt;
use super::ChatMessage;
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
    /// Session ID for persisting conversation history.
    pub(super) session_id: String,
}

impl AgentUiBridge {
    pub(super) async fn run(mut self) {
        loop {
            // Wait for a message from TUI or check for agent output
            match self.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(TuiToAgent::SubmitMessage(text)) => {
                    // Check skill triggers first — before Chat/Plan mode routing
                    let skill_manager = rupoo::skill::SkillManager::new(
                        rupoo::skill::SkillManager::default_dir(),
                    );
                    if let Ok(Some(skill)) = skill_manager.match_trigger(&text) {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::system(format!("Triggered skill: {}", skill.name)),
                        ));
                        let plan = skill_manager.skill_to_plan(&skill);
                        if let Err(e) = self.repo.save_plan(&plan).await {
                            let _ = self.ui_tx.send(AgentToTui::Message(
                                ChatMessage::error(format!("Failed to save skill plan: {}", e)),
                            ));
                        } else {
                            match self.agent.resume(&plan.id).await {
                                Ok(Some(mut plan)) => {
                                    self.run_plan(&mut plan).await;
                                }
                                Ok(None) => {
                                    let _ = self.ui_tx.send(AgentToTui::Message(
                                        ChatMessage::assistant("Skill plan already completed".to_string()),
                                    ));
                                    let _ = self.ui_tx.send(AgentToTui::Idle);
                                }
                                Err(e) => {
                                    let _ = self.ui_tx.send(AgentToTui::Message(
                                        ChatMessage::error(format!("Skill execution error: {}", e)),
                                    ));
                                    let _ = self.ui_tx.send(AgentToTui::Idle);
                                }
                            }
                        }
                    } else if text.starts_with("/plan ") {
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
                    // Persist approve_all setting
                    if let Err(e) = self.repo.set_setting("approve_all", "true").await {
                        tracing::warn!(error = %e, "failed to persist approve_all");
                    }
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
