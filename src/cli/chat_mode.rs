//! Chat Mode — multi-turn agent conversation with streaming

use tracing::warn;

use super::{AgentToTui, ChatMessage};
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
                    let _ = ui_tx.send(AgentToTui::Message(
                        ChatMessage::system(format!("Calling tool: {} with args: {}", tool_name, args)),
                    ));
                }
                rupoo::llm::AgentEvent::ToolResult { tool_name, result } => {
                    let _ = ui_tx.send(AgentToTui::Message(
                        ChatMessage::system(format!("Tool {} returned: {}", tool_name, result)),
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
                let session_id = "default".to_string();
                if let Err(e) = self.repo.save_conversation_history(&session_id, &self.conversation_history).await {
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
