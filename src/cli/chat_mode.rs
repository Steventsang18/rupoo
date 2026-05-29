//! Chat Mode — multi-turn agent conversation with streaming
//!
//! Uses LlmRouter when available (intent-driven provider selection with circuit breaker),
//! falls back to direct agent.agent_chat() otherwise.

use tracing::warn;

use super::{AgentToTui, ChatMessage, ToolPhase};
use super::bridge::AgentUiBridge;

impl AgentUiBridge {
    /// Handle Chat Mode: multi-turn agent conversation with streaming.
    pub(super) async fn handle_chat_message(&mut self, user_message: &str) {
        // Check if LLM is configured (via router or direct gateway)
        let has_router = self.llm_router.is_some();
        if !has_router && !self.agent.has_llm() {
            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::error("LLM not configured. Please set up your API key first.".to_string()),
            ));
            let _ = self.ui_tx.send(AgentToTui::Idle);
            return;
        }

        // Enforce token budget before sending request
        let budget_status = self.conversation_history.enforce_budget();
        match &budget_status {
            rupoo::llm::history::BudgetStatus::OverBudget { suggestion, .. } => {
                let bar = self.conversation_history.budget_progress_bar();
                let _ = self.ui_tx.send(AgentToTui::Message(
                    ChatMessage::system(format!("⚠ Token budget exceeded!\n{}\n{}", bar, suggestion)),
                ));
            }
            rupoo::llm::history::BudgetStatus::Trimmed { .. } => {
                let bar = self.conversation_history.budget_progress_bar();
                let _ = self.ui_tx.send(AgentToTui::Message(
                    ChatMessage::system(format!("Trimmed old messages to fit budget\n{}", bar)),
                ));
            }
            _ => {}
        }

        // Send thinking state
        let _ = self.ui_tx.send(AgentToTui::Thinking);

        // Determine safe_mode based on user preferences
        let safe_mode = true; // Default to safe mode in Chat Mode

        // max_turns: tool call rounds per request. Configurable via `rupoo config set max_turns <N>`
        let max_turns: usize = self.repo.get_setting("max_turns").await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        if let Some(ref router) = self.llm_router {
            // ── Route through LlmRouter (intent-driven, circuit breaker, fallback) ──
            let ui_tx = self.ui_tx.clone();
            let cancelled = self.cancelled.clone();
            let mut full_response = String::new();

            let on_event = |event: rupoo::llm::AgentEvent| {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match event {
                    rupoo::llm::AgentEvent::TextDelta(text) => {
                        full_response.push_str(&text);
                        let _ = ui_tx.send(AgentToTui::StreamChunk { text });
                    }
                    rupoo::llm::AgentEvent::ToolCall { tool_name, args } => {
                        let _ = ui_tx.send(AgentToTui::ToolStatus {
                            tool_name: tool_name.clone(),
                            phase: ToolPhase::Calling,
                        });
                        let display_args = if args.len() > 60 {
                            format!("{}…", &args[..57])
                        } else {
                            args.clone()
                        };
                        let _ = ui_tx.send(AgentToTui::Message(
                            ChatMessage::system(format!("🔧 {}({})", tool_name, display_args)),
                        ));
                    }
                    rupoo::llm::AgentEvent::ToolResult { tool_name, result } => {
                        let _ = ui_tx.send(AgentToTui::ToolStatus {
                            tool_name: tool_name.clone(),
                            phase: ToolPhase::Completed,
                        });
                        let display_result = if result.len() > 120 {
                            format!("{}…", &result[..117])
                        } else {
                            result.clone()
                        };
                        let _ = ui_tx.send(AgentToTui::Message(
                            ChatMessage::system(format!("✅ {} → {}", tool_name, display_result)),
                        ));
                    }
                }
            };

            match router.chat_agent_loop(
                user_message,
                &self.conversation_history,
                max_turns,
                safe_mode,
                None, // memory_context
                on_event,
                None, // custom_preamble
                Some(&self.intent_state),
            ).await {
                Ok((response, usage)) => {
                    let (clean_response, new_intent) = rupoo::signal::IntentState::parse_from_response(
                        &response, &self.intent_state,
                    );
                    self.intent_state = new_intent;

                    self.conversation_history.push_user(user_message);
                    self.conversation_history.push_assistant(&clean_response);

                    if let Err(e) = self.repo.save_conversation_history(
                        &self.session_id, &self.conversation_history,
                    ).await {
                        warn!(error = %e, "failed to persist conversation history");
                    }

                    let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                        in_count: usage.prompt_tokens as u64,
                        out_count: usage.completion_tokens as u64,
                    });

                    if !full_response.is_empty() {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant(full_response),
                        ));
                    }

                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
                Err(e) => {
                    let err_msg = format!("Chat error: {}", e);
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::error(err_msg.clone()),
                    ));
                    self.conversation_history.push_user(user_message);
                    self.conversation_history.push_assistant(&format!("Error: {}", e));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
            }
        } else {
            // ── Fallback: direct agent.agent_chat (no router) ──
            let ui_tx = self.ui_tx.clone();
            let cancelled = self.cancelled.clone();
            let mut full_response = String::new();

            let on_event = |event: rupoo::llm::AgentEvent| {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match event {
                    rupoo::llm::AgentEvent::TextDelta(text) => {
                        full_response.push_str(&text);
                        let _ = ui_tx.send(AgentToTui::StreamChunk { text });
                    }
                    rupoo::llm::AgentEvent::ToolCall { tool_name, args } => {
                        let _ = ui_tx.send(AgentToTui::ToolStatus {
                            tool_name: tool_name.clone(),
                            phase: ToolPhase::Calling,
                        });
                        let display_args = if args.len() > 60 {
                            format!("{}…", &args[..57])
                        } else {
                            args.clone()
                        };
                        let _ = ui_tx.send(AgentToTui::Message(
                            ChatMessage::system(format!("🔧 {}({})", tool_name, display_args)),
                        ));
                    }
                    rupoo::llm::AgentEvent::ToolResult { tool_name, result } => {
                        let _ = ui_tx.send(AgentToTui::ToolStatus {
                            tool_name: tool_name.clone(),
                            phase: ToolPhase::Completed,
                        });
                        let display_result = if result.len() > 120 {
                            format!("{}…", &result[..117])
                        } else {
                            result.clone()
                        };
                        let _ = ui_tx.send(AgentToTui::Message(
                            ChatMessage::system(format!("✅ {} → {}", tool_name, display_result)),
                        ));
                    }
                }
            };

            match self.agent.agent_chat(
                user_message,
                &self.conversation_history,
                max_turns,
                safe_mode,
                on_event,
                Some(&self.intent_state),
            ).await {
                Ok((response, usage)) => {
                    let (clean_response, new_intent) = rupoo::signal::IntentState::parse_from_response(
                        &response, &self.intent_state,
                    );
                    self.intent_state = new_intent;

                    self.conversation_history.push_user(user_message);
                    self.conversation_history.push_assistant(&clean_response);

                    if let Err(e) = self.repo.save_conversation_history(
                        &self.session_id, &self.conversation_history,
                    ).await {
                        warn!(error = %e, "failed to persist conversation history");
                    }

                    let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                        in_count: usage.prompt_tokens as u64,
                        out_count: usage.completion_tokens as u64,
                    });

                    if !full_response.is_empty() {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant(full_response),
                        ));
                    }

                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
                Err(e) => {
                    let err_msg = format!("Chat error: {}", e);
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::error(err_msg.clone()),
                    ));
                    self.conversation_history.push_user(user_message);
                    self.conversation_history.push_assistant(&format!("Error: {}", e));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
            }
        }
    }
}
