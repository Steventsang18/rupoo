//! Conversation context management for Rupoo.
//!
//! This module provides unified context assembly for LLM prompts. It combines:
//!
//! - **Environment signals** - PWD, git status, project type, system resources
//! - **Intent state** - user's current goal and clarification progress
//! - **Memory context** - relevant past memories and knowledge
//! - **Conversation history** - recent dialogue with smart trimming
//!
//! All sub-contexts compete for a shared token budget. The assembly prioritizes
//! recency and relevance to maximize information density within budget.

use crate::llm::history::ConversationHistory;
use crate::signal::{EnvironmentSignals, IntentState};

// ---------------------------------------------------------------------------
// Token budget
// ---------------------------------------------------------------------------

/// Rough character-to-token ratio. Using 2 chars ≈ 1 token as a conservative
/// estimate for Chinese + English mixed text.
const CHARS_PER_TOKEN: usize = 2;

/// Default token budget allocation by context category.
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// Total token budget for all context (excluding system prompt and user message).
    pub total: usize,
    /// Tokens reserved for environment signals.
    pub environment: usize,
    /// Tokens reserved for intent state.
    pub intent: usize,
    /// Tokens reserved for memory context.
    pub memory: usize,
    /// Tokens reserved for conversation history.
    pub history: usize,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            total: 4096,
            environment: 256,
            intent: 384,
            memory: 512,
            history: 2944,
        }
    }
}

impl TokenBudget {
    /// Create a compact budget for resource-constrained scenarios.
    pub fn compact() -> Self {
        Self {
            total: 2048,
            environment: 128,
            intent: 192,
            memory: 256,
            history: 1472,
        }
    }

    /// Create an expanded budget for complex tasks.
    pub fn expanded() -> Self {
        Self {
            total: 8192,
            environment: 384,
            intent: 512,
            memory: 1024,
            history: 6272,
        }
    }

    /// Estimate tokens from character count.
    pub fn chars_to_tokens(chars: usize) -> usize {
        chars / CHARS_PER_TOKEN
    }
}

// ---------------------------------------------------------------------------
// System resource info
// ---------------------------------------------------------------------------

/// Lightweight system resource snapshot for context injection.
#[derive(Debug, Clone, Default)]
pub struct SystemResourceInfo {
    /// Current working directory display name
    pub cwd_display: String,
    /// Approximate memory usage of rupoo process (MB)
    pub process_memory_mb: Option<u64>,
    /// Number of available CPU cores
    pub cpu_cores: Option<usize>,
}

impl SystemResourceInfo {
    pub fn collect() -> Self {
        let mut info = Self::default();

        // PWD display
        if let Ok(pwd) = std::env::var("PWD").or_else(|_| std::env::var("HOME")) {
            info.cwd_display = pwd;
        }

        // CPU cores
        info.cpu_cores = std::thread::available_parallelism().ok().map(|n| n.get());

        // Process memory (macOS-specific via sysctl or /proc fallback)
        info.process_memory_mb = Self::macos_memory_usage();

        info
    }

    #[cfg(target_os = "macos")]
    fn macos_memory_usage() -> Option<u64> {
        use std::process::Command;
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let kb: u64 = stdout.trim().parse().ok()?;
        Some(kb / 1024) // KB -> MB
    }

    #[cfg(not(target_os = "macos"))]
    fn macos_memory_usage() -> Option<u64> {
        None
    }

    /// Format as a compact prompt block.
    pub fn to_prompt_block(&self) -> String {
        let mut parts = vec![format!("- CWD: {}", self.cwd_display)];

        if let Some(cores) = self.cpu_cores {
            parts.push(format!("- CPU cores: {}", cores));
        }
        if let Some(mem) = self.process_memory_mb {
            parts.push(format!("- Process memory: {mem} MB"));
        }

        if parts.len() <= 1 {
            return String::new();
        }
        format!("## System Resources\n{}", parts.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// User behavior profile
// ---------------------------------------------------------------------------

/// Lightweight user behavior tracking for context adaptation.
#[derive(Debug, Clone, Default)]
pub struct UserBehaviorProfile {
    /// Total conversation turns in this session.
    pub total_turns: u64,
    /// Number of tool calls made in this session.
    pub tool_calls: u64,
    /// Tools used in this session (deduplicated).
    pub tools_used: Vec<String>,
    /// Average message length from user.
    pub avg_user_msg_len: f64,
    /// Whether user tends to use plan execution mode.
    pub prefers_plan_mode: bool,
    /// Number of web searches initiated.
    pub search_queries: u64,
}

impl UserBehaviorProfile {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a user message for tracking.
    pub fn record_user_message(&mut self, content: &str) {
        self.total_turns += 1;
        let len = content.len() as f64;
        let n = self.total_turns as f64;
        // Exponential moving average
        self.avg_user_msg_len = if n <= 1.0 {
            len
        } else {
            self.avg_user_msg_len * 0.9 + len * 0.1
        };
    }

    /// Record a tool call.
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.tool_calls += 1;
        if !self.tools_used.contains(&tool_name.to_string()) {
            self.tools_used.push(tool_name.to_string());
        }
    }

    /// Check if this is a new user (few turns).
    pub fn is_new_user(&self) -> bool {
        self.total_turns < 5
    }

    /// Estimate the user's interaction style.
    pub fn interaction_style(&self) -> &'static str {
        if self.prefers_plan_mode {
            "plan-oriented"
        } else if self.tool_calls as f64 / self.total_turns.max(1) as f64 > 0.3 {
            "tool-heavy"
        } else if self.avg_user_msg_len > 500.0 {
            "verbose"
        } else {
            "concise"
        }
    }
}

// ---------------------------------------------------------------------------
// Conversation context — unified context assembly
// ---------------------------------------------------------------------------

/// Unified context object that assembles all sub-contexts into an
/// LLM-ready prompt block, respecting token budget constraints.
#[derive(Debug)]
pub struct ConversationContext {
    /// Token budget configuration.
    pub budget: TokenBudget,
    /// Environment signals (PWD, git, project type, system resources).
    pub environment: EnvironmentSignals,
    /// Intent state (what the user wants, progress).
    pub intent: IntentState,
    /// System resource info.
    pub system_resources: SystemResourceInfo,
    /// Conversation history.
    pub history: ConversationHistory,
    /// Memory context (retrieved from memory store).
    pub memory_context: Option<String>,
    /// User behavior profile.
    pub behavior: UserBehaviorProfile,
}

impl ConversationContext {
    /// Create with defaults, collecting environment signals and system resources.
    pub fn collect() -> Self {
        Self {
            budget: TokenBudget::default(),
            environment: EnvironmentSignals::collect(),
            intent: IntentState::default(),
            system_resources: SystemResourceInfo::collect(),
            history: ConversationHistory::new(20)
                .with_token_budget(TokenBudget::default().history * CHARS_PER_TOKEN),
            memory_context: None,
            behavior: UserBehaviorProfile::new(),
        }
    }

    /// Set a custom token budget.
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        let history_budget = budget.history * CHARS_PER_TOKEN;
        self.budget = budget;
        self.history = ConversationHistory::new(20).with_token_budget(history_budget);
        self
    }

    /// Inject memory context from search results.
    pub fn with_memories(mut self, memories: Vec<crate::task::MemoryEntry>) -> Self {
        if memories.is_empty() {
            return self;
        }
        let formatted = memories
            .iter()
            .map(|m| format!("- [{}] {}", m.created_at, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        // Truncate to memory budget
        let max_chars = self.budget.memory * CHARS_PER_TOKEN;
        let truncated = if formatted.len() > max_chars {
            let end = formatted.floor_char_boundary(max_chars);
            format!("{}...", &formatted[..end])
        } else {
            formatted
        };

        self.memory_context = Some(truncated);
        self
    }

    /// Record a user message for behavior tracking and history.
    pub fn record_user_message(&mut self, content: &str) {
        self.behavior.record_user_message(content);
        self.history.push_user(content);
    }

    /// Record an assistant response.
    pub fn record_assistant_response(&mut self, content: &str) {
        self.history.push_assistant(content);
    }

    /// Record a tool call for behavior tracking.
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.behavior.record_tool_call(tool_name);
    }

    /// Build the full context block for injection into system prompt.
    ///
    /// Returns a string containing environment signals, intent state,
    /// memory context, and system resource info — all within budget.
    /// Conversation history is NOT included here (it's sent separately).
    pub fn to_system_context_block(&self) -> String {
        let mut parts = Vec::new();

        // 1. Environment signals (with budget)
        let env_block = self.environment.to_prompt_block();
        if !env_block.is_empty()
            && TokenBudget::chars_to_tokens(env_block.len()) <= self.budget.environment
        {
            parts.push(env_block);
        } else if !env_block.is_empty() {
            // Truncate to fit
            let max_chars = self.budget.environment * CHARS_PER_TOKEN;
            let truncated = self.truncate_to_budget(&env_block, max_chars);
            if !truncated.is_empty() {
                parts.push(truncated);
            }
        }

        // 2. System resources
        let res_block = self.system_resources.to_prompt_block();
        if !res_block.is_empty() {
            parts.push(res_block);
        }

        // 3. Intent state
        let intent_block = self.intent.to_prompt_block();
        if !intent_block.is_empty()
            && TokenBudget::chars_to_tokens(intent_block.len()) <= self.budget.intent
        {
            parts.push(intent_block);
        }

        // 4. Memory context
        if let Some(ref memories) = self.memory_context {
            if !memories.is_empty() {
                parts.push(format!("## Relevant Memories\n{}", memories));
            }
        }

        // 5. User behavior hint (only for recurring users)
        if !self.behavior.is_new_user() {
            let style = self.behavior.interaction_style();
            let hint = format!(
                "- User interaction style: {style} ({} turns, {} tool calls)",
                self.behavior.total_turns, self.behavior.tool_calls
            );
            parts.push(format!("## User Profile\n{hint}"));
        }

        parts.join("\n\n")
    }

    /// Build the full conversation history as rig messages.
    pub fn to_history_messages(&self) -> Vec<rig::message::Message> {
        self.history.to_rig_messages()
    }

    /// Get estimated token usage for this context.
    pub fn estimated_context_tokens(&self) -> usize {
        let block = self.to_system_context_block();
        let context_tokens = TokenBudget::chars_to_tokens(block.len());
        let history_tokens = self.history.estimated_tokens();
        context_tokens + history_tokens
    }

    /// Check if context exceeds budget, needing trim.
    pub fn is_over_budget(&self) -> bool {
        self.estimated_context_tokens() > self.budget.total
    }

    /// Clear conversation history (keep system state, behavior profile).
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Reset the entire context (new conversation).
    pub fn reset(&mut self) {
        self.history.clear();
        self.intent = IntentState::default();
        self.memory_context = None;
        self.behavior = UserBehaviorProfile::new();
        self.environment = EnvironmentSignals::collect();
    }

    // --- Private helpers ---

    fn truncate_to_budget(&self, text: &str, max_chars: usize) -> String {
        if text.len() <= max_chars {
            return text.to_string();
        }
        let end = text.floor_char_boundary(max_chars);
        format!("{}...", &text[..end])
    }
}

impl Default for ConversationContext {
    fn default() -> Self {
        Self::collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_budget_defaults() {
        let budget = TokenBudget::default();
        assert_eq!(budget.total, 4096);
        // Sum of sub-budgets should not exceed total
        let sub_sum = budget.environment + budget.intent + budget.memory + budget.history;
        assert!(sub_sum <= budget.total);
    }

    #[test]
    fn test_token_budget_chars_to_tokens() {
        assert_eq!(TokenBudget::chars_to_tokens(0), 0);
        assert_eq!(TokenBudget::chars_to_tokens(200), 100);
        assert_eq!(TokenBudget::chars_to_tokens(201), 100); // integer division
    }

    #[test]
    fn test_system_resource_info_collect() {
        let info = SystemResourceInfo::collect();
        assert!(!info.cwd_display.is_empty());
        assert!(info.cpu_cores.unwrap() > 0);
    }

    #[test]
    fn test_conversation_context_collect() {
        let ctx = ConversationContext::collect();
        assert!(!ctx.environment.pwd.is_empty());
        assert_eq!(ctx.intent.turn, 0);
        assert!(ctx.behavior.is_new_user());
    }

    #[test]
    fn test_user_behavior_new_user() {
        let profile = UserBehaviorProfile::new();
        assert!(profile.is_new_user());
        assert_eq!(profile.interaction_style(), "concise");
    }

    #[test]
    fn test_user_behavior_record_message() {
        let mut profile = UserBehaviorProfile::new();
        profile.record_user_message("Hello, world!");
        assert_eq!(profile.total_turns, 1);
        // avg for first message should be message length
        assert!((profile.avg_user_msg_len - 13.0).abs() < 0.01);
    }

    #[test]
    fn test_user_behavior_record_tool_call() {
        let mut profile = UserBehaviorProfile::new();
        profile.record_tool_call("bash");
        profile.record_tool_call("bash");
        profile.record_tool_call("file_read");
        assert_eq!(profile.tool_calls, 3);
        assert_eq!(profile.tools_used.len(), 2);
    }

    #[test]
    fn test_context_to_system_block() {
        let ctx = ConversationContext::collect();
        let block = ctx.to_system_context_block();
        // Should contain environment info
        assert!(block.contains("PWD:"));
        // Should contain system resources
        assert!(block.contains("CWD:"));
    }

    #[test]
    fn test_context_with_memories() {
        let mut ctx = ConversationContext::collect();
        let memories = vec![crate::task::MemoryEntry {
            id: "test-1".to_string(),
            content: "User likes dark theme".to_string(),
            tags: vec!["preference".to_string()],
            source: "user".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        }];
        ctx = ctx.with_memories(memories);
        let block = ctx.to_system_context_block();
        assert!(block.contains("Relevant Memories"));
        assert!(block.contains("dark theme"));
    }

    #[test]
    fn test_context_clear_history() {
        let mut ctx = ConversationContext::collect();
        ctx.record_user_message("test");
        ctx.record_assistant_response("response");
        assert_eq!(ctx.history.message_count(), 2);

        ctx.clear_history();
        assert_eq!(ctx.history.message_count(), 0);
        // Intent and behavior should persist
        assert_eq!(ctx.intent.turn, 0);
        assert_eq!(ctx.behavior.total_turns, 1);
    }

    #[test]
    fn test_context_is_over_budget() {
        let ctx = ConversationContext::collect();
        // Fresh context should be within budget
        assert!(!ctx.is_over_budget());
    }

    #[test]
    fn test_compact_budget() {
        let budget = TokenBudget::compact();
        let ctx = ConversationContext::collect().with_budget(budget);
        assert_eq!(ctx.budget.total, 2048);
    }
}
