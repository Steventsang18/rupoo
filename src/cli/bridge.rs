//! AgentUiBridge — runs in a separate thread, bridges async agent to TUI

use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;

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
    /// Intent state for token-efficient history compression.
    pub(super) intent_state: rupoo::signal::IntentState,
    /// Cancel flag — set by TUI when user interrupts generation.
    pub(super) cancelled: Arc<AtomicBool>,
}

impl AgentUiBridge {
    pub(super) async fn run(mut self) {
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(TuiToAgent::SubmitMessage(text)) => {
                    self.handle_submit(&text).await;
                }
                Ok(TuiToAgent::ApproveTool(_tool_name)) => {
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::ApproveAll) => {
                    self.approve_all = true;
                    if let Err(e) = self.repo.set_setting("approve_all", "true").await {
                        tracing::warn!(error = %e, "failed to persist approve_all");
                    }
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::DenyTool) => {
                    self.handle_denial().await;
                }
                Ok(TuiToAgent::Cancel) => {
                    // Cancel signal — the cancelled flag is already set by the TUI,
                    // just continue the loop; the chat callback will check the flag.
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

    /// Handle a submitted message from the user, dispatching to the appropriate handler.
    async fn handle_submit(&mut self, text: &str) {
        // Check skill triggers first — before Chat/Plan mode routing
        let skill_manager = rupoo::skill::SkillManager::new(
            rupoo::skill::SkillManager::default_dir(),
        );
        if let Ok(Some(skill)) = skill_manager.match_trigger(text) {
            self.handle_skill_trigger(skill).await;
        } else if text.starts_with("/plan ") {
            let task = text.trim_start_matches("/plan ").trim();
            self.handle_plan_mode(task).await;
        } else if text.starts_with("/model ") {
            self.handle_model_switch(text).await;
        } else if text == "/clear" {
            self.handle_clear().await;
        } else if text == "/status" {
            self.handle_status().await;
        } else if text == "/help" || text == "/?" {
            self.handle_help().await;
        } else {
            // Chat Mode: multi-turn agent chat
            self.handle_chat_message(text).await;
        }
    }

    /// Handle a skill trigger match.
    async fn handle_skill_trigger(&mut self, skill: rupoo::skill::SkillDef) {
        let _ = self.ui_tx.send(AgentToTui::Message(
            ChatMessage::system(format!("Triggered skill: {}", skill.name)),
        ));
        let skill_manager = rupoo::skill::SkillManager::new(
            rupoo::skill::SkillManager::default_dir(),
        );
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
    }

    /// Handle /model command to switch LLM provider/model.
    async fn handle_model_switch(&mut self, text: &str) {
        let args = text.trim_start_matches("/model ").trim();
        if args.is_empty() {
            let current = if self.agent.has_llm() {
                "configured".to_string()
            } else {
                "not configured".to_string()
            };
            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::system(format!("Current LLM: {}", current)),
            ));
        } else {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            let provider = parts[0].to_string();
            let model = parts.get(1).map(|s| s.to_string());
            match self.agent.switch_llm(&provider, model.as_deref(), &self.repo).await {
                Ok(label) => {
                    let _ = self.repo.set_setting("active_provider", &provider).await;
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::system(format!("Switched to {}", label)),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::LlmStatus {
                        configured: true,
                        provider: provider.clone(),
                        model_label: label.clone(),
                    });
                }
                Err(e) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::error(format!("Failed to switch: {}", e)),
                    ));
                }
            }
        }
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }

    /// Handle /clear command to reset conversation history.
    async fn handle_clear(&mut self) {
        self.conversation_history.clear();
        self.intent_state = rupoo::signal::IntentState::new();
        if let Err(e) = self.repo.save_conversation_history(&self.session_id, &self.conversation_history).await {
            tracing::warn!(error = %e, "failed to clear history in DB");
        }
        let _ = self.ui_tx.send(AgentToTui::Message(
            ChatMessage::system("Conversation history cleared".to_string()),
        ));
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }

    /// Handle /status command.
    async fn handle_status(&self) {
        let llm_status = if self.agent.has_llm() { "configured" } else { "not configured" };
        let status = format!(
            "Session: {}\nLLM: {}\nApprove all: {}\nHistory: {} turns",
            self.session_id,
            llm_status,
            self.approve_all,
            self.conversation_history.len(),
        );
        let _ = self.ui_tx.send(AgentToTui::Message(
            ChatMessage::system(status),
        ));
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }

    /// Handle /help command.
    async fn handle_help(&self) {
        let help = "\
Available commands:
  /plan <task>    — Generate and execute a step-by-step plan
  /model <prov> [model] — Switch LLM provider/model
  /clear          — Clear conversation history
  /status         — Show current session status
  /help           — Show this help message
  Ctrl+C         — Cancel current generation (press twice to quit)";
        let _ = self.ui_tx.send(AgentToTui::Message(
            ChatMessage::system(help.to_string()),
        ));
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }
}
