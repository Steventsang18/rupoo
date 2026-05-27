//! Chat Mode — multi-turn agent conversation with streaming

use tracing::warn;

use super::{AgentToTui, ChatMessage, ToolPhase};
use super::bridge::AgentUiBridge;

impl AgentUiBridge {
    /// Handle Chat Mode: multi-turn agent conversation with streaming.
    pub(super) async fn handle_chat_message(&mut self, user_message: &str) {
        // Check if LLM is configured
        if !self.agent.has_llm() {
            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::error("LLM not configured. Please set up your API key first.".to_string()),
            ));
            let _ = self.ui_tx.send(AgentToTui::Idle);
            return;
        }

        // Send thinking state
        let _ = self.ui_tx.send(AgentToTui::Thinking);

        // Create a callback closure to send events to TUI
        let ui_tx = self.ui_tx.clone();
        let mut full_response = String::new();

        let on_event = |event: rupoo::llm::AgentEvent| {
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
                    // Show compact tool call status
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
                    // Show compact tool result
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

        // Determine safe_mode based on user preferences
        let safe_mode = true; // Default to safe mode in Chat Mode

        // Run the agent chat
        match self.agent.agent_chat(
            user_message,
            &self.conversation_history,
            10, // max_turns
            safe_mode,
            on_event,
        ).await {
            Ok((response, usage)) => {
                // Update conversation history
                self.conversation_history.push_user(user_message);
                self.conversation_history.push_assistant(&response);

                // Persist history to DB
                if let Err(e) = self.repo.save_conversation_history(&self.session_id, &self.conversation_history).await {
                    warn!(error = %e, "failed to persist conversation history");
                }

                // Send token update
                let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                    in_count: usage.prompt_tokens as u64,
                    out_count: usage.completion_tokens as u64,
                });

                // Flush any remaining stream content as a final message
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

                // Still add user message to history for context
                self.conversation_history.push_user(user_message);
                self.conversation_history.push_assistant(&format!("Error: {}", e));

                let _ = self.ui_tx.send(AgentToTui::Idle);
            }
        }
    }
}
