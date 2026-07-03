//! Chat Mode — multi-turn agent conversation with streaming

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::warn;

use super::bridge::AgentUiBridge;
use super::{AgentToTui, ChatMessage, ToolPhase};

// Magic number constants
/// Default max turns per chat request.
const DEFAULT_MAX_TURNS: usize = 50;

impl AgentUiBridge {
    /// Handle Chat Mode: multi-turn agent conversation with streaming.
    pub(super) async fn handle_chat_message(&mut self, user_message: &str) {
        // Check if LLM is configured
        if !self.agent.has_llm() {
            let _ = self.ui_tx.send(AgentToTui::Message(ChatMessage::error(
                "LLM not configured. Please set up your API key first.".to_string(),
            )));
            let _ = self.ui_tx.send(AgentToTui::Idle);
            return;
        }

        // Check if we need to start clarification for a development demand
        if self.intent_state.clarification_state == rupoo::signal::ClarificationState::NotStarted
            && rupoo::signal::IntentState::looks_like_development_demand(user_message)
        {
            // Start clarification process
            self.intent_state.start_clarification(user_message);
            let _ = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                "🔍 检测到开发需求，开始需求澄清流程...".to_string(),
            )));
        }

        // Send thinking state with execution tracking
        let _ = self.ui_tx.send(AgentToTui::Thinking);

        // Create execution tracker for progress display
        let _execution_start = std::time::Instant::now();
        let tool_call_count = Arc::new(AtomicBool::new(false));

        // Create a callback closure to send events to TUI
        let ui_tx = self.ui_tx.clone();
        let cancelled = self.cancelled.clone();
        let mut full_response = String::new();

        let on_event = |event: rupoo::llm::AgentEvent| {
            // If cancelled, stop processing events
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            match event {
                rupoo::llm::AgentEvent::TextDelta(text) => {
                    full_response.push_str(&text);
                    let _ = ui_tx.send(AgentToTui::StreamChunk { text });
                }
                rupoo::llm::AgentEvent::ToolCall { tool_name, args: _ } => {
                    // Mark that we're in a tool call
                    tool_call_count.store(true, Ordering::Relaxed);

                    let _ = ui_tx.send(AgentToTui::ToolStatus {
                        tool_name: tool_name.clone(),
                        phase: ToolPhase::Calling,
                    });

                    // Only show progress for meaningful tools — skip read-only noise
                    let is_readonly = matches!(
                        tool_name.as_str(),
                        "file_read" | "list_directory" | "list_dir" | "FileRead"
                    );
                    if !is_readonly {
                        let summary = format!("{} 中...", tool_name);
                        let _ = ui_tx.send(AgentToTui::ThinkingSummary { text: summary });
                    }
                }
                rupoo::llm::AgentEvent::ToolResult { tool_name, .. } => {
                    let _ = ui_tx.send(AgentToTui::ToolStatus {
                        tool_name: tool_name.clone(),
                        phase: ToolPhase::Completed,
                    });
                }
            }
        };

        // Determine safe_mode based on user preferences
        let safe_mode = true; // Default to safe mode in Chat Mode

        // Run the agent chat
        // max_turns: tool call rounds per request. Configurable via `rupoo config set max_turns <N>`
        let max_turns: usize = self
            .repo
            .get_setting("max_turns")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_TURNS);
        match self
            .agent
            .agent_chat(
                user_message,
                &self.conversation_history,
                max_turns,
                safe_mode,
                on_event,
                Some(&self.intent_state),
                None,
            )
            .await
        {
            Ok((response, usage)) => {
                // Parse intent update from LLM response
                let (clean_response, new_intent) =
                    rupoo::signal::IntentState::parse_from_response(&response, &self.intent_state);
                self.intent_state = new_intent;

                // Update conversation history
                self.conversation_history.push_user(user_message);
                self.conversation_history.push_assistant(&clean_response);

                // Persist history to DB
                if let Err(e) = self
                    .repo
                    .save_conversation_history(&self.session_id, &self.conversation_history)
                    .await
                {
                    warn!(error = %e, "failed to persist conversation history");
                }

                // Send token update
                let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                    in_count: usage.prompt_tokens as u64,
                    out_count: usage.completion_tokens as u64,
                });

                // Send the complete assistant response as a message (for history + rendering)
                if !full_response.is_empty() {
                    let _ = self
                        .ui_tx
                        .send(AgentToTui::Message(ChatMessage::assistant(full_response)));
                }

                let _ = self.ui_tx.send(AgentToTui::Idle);
            }
            Err(e) => {
                // Use user-friendly error message
                let user_msg = e.user_friendly_message();
                let causes = e.possible_causes();
                let solutions = e.solutions();

                // Build comprehensive error message
                let mut full_error = format!("❌ 错误: {}\n\n", user_msg);

                if !causes.is_empty() {
                    full_error.push_str("**可能原因:**\n");
                    for cause in &causes {
                        full_error.push_str(&format!("  • {}\n", cause));
                    }
                    full_error.push('\n');
                }

                if !solutions.is_empty() {
                    full_error.push_str("**建议解决方案:**\n");
                    for (i, solution) in solutions.iter().enumerate() {
                        full_error.push_str(&format!("  {}. {}\n", i + 1, solution));
                    }
                }

                // Show retry hint for retryable errors
                if e.is_retryable() {
                    full_error.push_str("\n💡 系统将自动重试或您可以稍后重试此操作");
                }

                let _ = self
                    .ui_tx
                    .send(AgentToTui::Message(ChatMessage::error(full_error)));

                // Still add user message to history for context
                self.conversation_history.push_user(user_message);
                self.conversation_history
                    .push_assistant(&format!("Error: {}", e));

                let _ = self.ui_tx.send(AgentToTui::Idle);
            }
        }
    }
}
