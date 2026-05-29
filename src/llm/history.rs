//! Conversation history and chat message types.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Chat message types for LLM communication
// ---------------------------------------------------------------------------

/// Chat message for LLM communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmChatMessage {
    pub role: LlmChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmChatRole {
    System,
    User,
    Assistant,
}

impl LlmChatMessage {
    pub fn system(content: &str) -> Self {
        Self { role: LlmChatRole::System, content: content.to_string() }
    }
    pub fn user(content: &str) -> Self {
        Self { role: LlmChatRole::User, content: content.to_string() }
    }
    pub fn assistant(content: &str) -> Self {
        Self { role: LlmChatRole::Assistant, content: content.to_string() }
    }
}

// ---------------------------------------------------------------------------
// ConversationHistory for multi-turn chat
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationHistory {
    messages: Vec<LlmChatMessage>,
    max_turns: usize,
    /// Maximum estimated token budget for history (0 = no limit)
    max_tokens: usize,
}

impl ConversationHistory {
    pub fn new(max_turns: usize) -> Self {
        Self { messages: Vec::new(), max_turns, max_tokens: 0 }
    }

    /// Set a token budget for conversation history. When exceeded, older messages are trimmed.
    /// Uses a rough estimate of ~2 chars per token.
    pub fn with_token_budget(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn push_user(&mut self, content: &str) {
        self.messages.push(LlmChatMessage::user(content));
        self.trim_to_limits();
    }

    pub fn push_assistant(&mut self, content: &str) {
        self.messages.push(LlmChatMessage::assistant(content));
        self.trim_to_limits();
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Convert to rig-core Message format for LLM consumption.
    pub fn to_rig_messages(&self) -> Vec<rig::message::Message> {
        self.messages
            .iter()
            .map(|m| {
                use rig::message::{Message, UserContent, AssistantContent, Text};
                use rig::OneOrMany;
                match m.role {
                    LlmChatRole::System | LlmChatRole::User => {
                        Message::User {
                            content: OneOrMany::one(UserContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                    LlmChatRole::Assistant => {
                        Message::Assistant {
                            id: None,
                            content: OneOrMany::one(AssistantContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                }
            })
            .collect()
    }

    fn trim_to_limits(&mut self) {
        // First trim by turn count
        self.trim_by_turns();
        // Then trim by token budget if set
        if self.max_tokens > 0 {
            self.trim_by_token_budget();
        }
    }

    fn trim_by_turns(&mut self) {
        // Keep system messages, trim user/assistant pairs from the front
        let systems: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == LlmChatRole::System)
            .cloned()
            .collect();

        let non_system: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role != LlmChatRole::System)
            .cloned()
            .collect();

        let to_remove = non_system.len().saturating_sub(self.max_turns * 2);
        let trimmed: Vec<_> = non_system.into_iter().skip(to_remove).collect();

        self.messages.clear();
        self.messages.extend(systems);
        self.messages.extend(trimmed);
    }

    /// Trim oldest non-system messages until estimated token count is within budget.
    /// Rough estimate: ~2 chars per token.
    fn trim_by_token_budget(&mut self) {
        let budget = self.max_tokens;
        // Calculate total estimated tokens
        let total_chars: usize = self.messages.iter().map(|m| m.content.len()).sum();
        let estimated_tokens = total_chars / 2;

        if estimated_tokens <= budget {
            return;
        }

        // Remove oldest non-system messages until within budget
        let systems: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == LlmChatRole::System)
            .cloned()
            .collect();

        let mut non_system: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role != LlmChatRole::System)
            .cloned()
            .collect();

        // Remove from front until budget is met
        let system_chars: usize = systems.iter().map(|m| m.content.len()).sum();
        let budget_chars = budget.saturating_mul(2).saturating_sub(system_chars);

        let mut current_chars: usize = non_system.iter().map(|m| m.content.len()).sum();
        while current_chars > budget_chars && non_system.len() > 2 {
            current_chars -= non_system.remove(0).content.len();
        }

        self.messages.clear();
        self.messages.extend(systems);
        self.messages.extend(non_system);
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Access the raw message list (for history compression).
    pub fn messages(&self) -> &[LlmChatMessage] {
        &self.messages
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the current token budget (0 = no limit)
    pub fn token_budget(&self) -> usize {
        self.max_tokens
    }

    /// Get estimated token count for current history
    pub fn estimated_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.content.len()).sum::<usize>() / 2
    }

    /// Enforce the token budget — if over limit, compress history.
    ///
    /// Strategy:
    /// 1. If within 80% of budget → no action
    /// 2. If between 80-100% → trim oldest non-system messages
    /// 3. If over 100% → compress older messages into a summary
    ///
    /// Returns a `BudgetStatus` indicating what action was taken.
    pub fn enforce_budget(&mut self) -> BudgetStatus {
        if self.max_tokens == 0 {
            return BudgetStatus::NoBudget;
        }

        let estimated = self.estimated_tokens();
        let ratio = estimated as f64 / self.max_tokens as f64;

        if ratio <= 0.8 {
            BudgetStatus::WithinBudget { used: estimated, limit: self.max_tokens }
        } else if ratio <= 1.0 {
            // Trim oldest messages to get under 80%
            self.trim_by_token_budget();
            BudgetStatus::Trimmed { used: self.estimated_tokens(), limit: self.max_tokens }
        } else {
            // Over budget — aggressive trim
            self.trim_by_token_budget();
            let remaining = self.estimated_tokens();
            if remaining > self.max_tokens {
                BudgetStatus::OverBudget {
                    used: remaining,
                    limit: self.max_tokens,
                    suggestion: "Use /compact to compress history or /new to start a fresh session".into(),
                }
            } else {
                BudgetStatus::Trimmed { used: remaining, limit: self.max_tokens }
            }
        }
    }

    /// Check if history is approaching the budget limit.
    /// Returns true when usage exceeds 80%.
    pub fn is_near_budget(&self) -> bool {
        if self.max_tokens == 0 {
            return false;
        }
        self.estimated_tokens() as f64 / self.max_tokens as f64 > 0.8
    }

    /// Format a progress bar showing token budget usage.
    /// Example: `[45k / 128k tokens] ████████░░░░ 35%`
    pub fn budget_progress_bar(&self) -> String {
        if self.max_tokens == 0 {
            return "[no budget limit]".into();
        }

        let used = self.estimated_tokens();
        let limit = self.max_tokens;
        let ratio = (used as f64 / limit as f64).min(1.0);

        let used_display = if used >= 1000 {
            format!("{}k", used / 1000)
        } else {
            format!("{}", used)
        };
        let limit_display = if limit >= 1000 {
            format!("{}k", limit / 1000)
        } else {
            format!("{}", limit)
        };

        let bar_width = 12;
        let filled = (ratio * bar_width as f64).round() as usize;
        let empty = bar_width - filled;
        let bar: String = "█".repeat(filled) + &"░".repeat(empty);

        format!("[{} / {} tokens] {} {:.0}%", used_display, limit_display, bar, ratio * 100.0)
    }

    /// Compact the history by keeping only the last N turns and a summary of earlier content.
    /// Returns a new ConversationHistory with compressed older messages.
    pub fn compact(&mut self, keep_recent_turns: usize) {
        if self.messages.len() <= keep_recent_turns * 2 {
            return; // Nothing to compact
        }

        // Split into system / old / recent
        let systems: Vec<_> = self.messages.iter()
            .filter(|m| m.role == LlmChatRole::System)
            .cloned()
            .collect();

        let non_system: Vec<_> = self.messages.iter()
            .filter(|m| m.role != LlmChatRole::System)
            .cloned()
            .collect();

        let split_point = non_system.len().saturating_sub(keep_recent_turns * 2);
        let (old, recent) = non_system.split_at(split_point);

        // Create a summary of old messages
        let old_summary = Self::summarize_old_messages(old);

        self.messages.clear();
        self.messages.extend(systems);

        // Add compressed summary as a system message
        if !old_summary.is_empty() {
            self.messages.push(LlmChatMessage::system(&format!(
                "[Earlier conversation summarized]\n{}", old_summary
            )));
        }

        // Keep recent messages
        self.messages.extend(recent.to_vec());
    }

    /// Create a brief summary of older messages for compression.
    fn summarize_old_messages(messages: &[LlmChatMessage]) -> String {
        if messages.is_empty() {
            return String::new();
        }

        let mut user_topics = Vec::new();
        let mut assistant_actions = Vec::new();

        for msg in messages {
            match msg.role {
                LlmChatRole::User => {
                    // Take first line as topic
                    let topic = msg.content.lines().next().unwrap_or("");
                    if !topic.is_empty() {
                        user_topics.push(topic.to_string());
                    }
                }
                LlmChatRole::Assistant => {
                    // Note if tool calls or file operations were done
                    let content = &msg.content;
                    if content.contains("file_read") || content.contains("file_write") {
                        assistant_actions.push("file operations".to_string());
                    } else if content.contains("shell_exec") {
                        assistant_actions.push("shell commands".to_string());
                    } else if content.contains("web_search") {
                        assistant_actions.push("web searches".to_string());
                    }
                }
                LlmChatRole::System => {}
            }
        }

        let mut parts = Vec::new();
        if !user_topics.is_empty() {
            let topics_str = user_topics.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
            parts.push(format!("Topics discussed: {}", topics_str));
        }
        if !assistant_actions.is_empty() {
            // Deduplicate
            let mut unique: Vec<String> = assistant_actions.into_iter().collect();
            unique.sort();
            unique.dedup();
            parts.push(format!("Actions taken: {}", unique.join(", ")));
        }

        parts.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Budget status
// ---------------------------------------------------------------------------

/// Result of enforcing the token budget.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetStatus {
    /// No budget limit configured.
    NoBudget,
    /// Within 80% of budget.
    WithinBudget { used: usize, limit: usize },
    /// Trimmed oldest messages to fit budget.
    Trimmed { used: usize, limit: usize },
    /// Over budget even after trimming.
    OverBudget { used: usize, limit: usize, suggestion: String },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_chat_message_serde() {
        let msg = LlmChatMessage::user("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("User"));
        assert!(json.contains("Hello"));

        let deserialized: LlmChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, LlmChatRole::User);
        assert_eq!(deserialized.content, "Hello");
    }

    #[test]
    fn test_conversation_history() {
        let mut history = ConversationHistory::new(5);
        history.push_user("Hello");
        history.push_assistant("Hi there!");
        history.push_user("How are you?");
        history.push_assistant("I'm good!");

        assert_eq!(history.message_count(), 4);
        assert!(!history.is_empty());

        let messages = history.to_rig_messages();
        assert_eq!(messages.len(), 4);
    }

    #[test]
    fn test_conversation_history_trim() {
        let mut history = ConversationHistory::new(2);
        history.push_user("Turn 1");
        history.push_assistant("Response 1");
        history.push_user("Turn 2");
        history.push_assistant("Response 2");
        history.push_user("Turn 3");
        history.push_assistant("Response 3");

        assert!(history.message_count() <= 4);
    }
}
