//! LLM module - unified interface for multiple LLM providers.
//!
//! ## Module Structure
//!
//! - [`gateway`](gateway::LlmGateway) - Core LLM gateway with chat and streaming capabilities
//! - [`history`](history::ConversationHistory) - Conversation history management
//! - [`providers`](providers) - Provider-specific agent builders

pub mod gateway;
pub mod history;
pub mod providers;

use serde::{Deserialize, Serialize};

// Re-export public types for convenience
pub use gateway::LlmGateway;
pub use history::{ConversationHistory, LlmChatMessage, LlmChatRole};
pub use providers::{extract_text_from_assistant_content, extract_text_from_user_content, role_label};

// Re-export config and types
pub use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

// ---------------------------------------------------------------------------
// AgentEvent for streaming callbacks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall { tool_name: String, args: String },
    ToolResult { tool_name: String, result: String },
}

// ---------------------------------------------------------------------------
// StepSpec for plan generation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    #[serde(rename = "type")]
    pub step_type: String,
    pub instruction: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: Option<String>,
    /// Base URL override (for proxies or local servers).
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmProvider {
    Anthropic,
    OpenAI,
    Ollama,
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::Ollama => write!(f, "ollama"),
        }
    }
}

impl LlmConfig {
    /// Create a default config for a given provider.
    pub fn new(provider: LlmProvider, api_key: Option<String>) -> Self {
        let (model, base_url) = match &provider {
            LlmProvider::Anthropic => ("claude-sonnet-4-20250514".into(), None),
            LlmProvider::OpenAI => ("gpt-4o".into(), None),
            LlmProvider::Ollama => {
                ("llama3.2".into(), Some("http://localhost:11434".into()))
            }
        };
        Self {
            provider,
            model,
            api_key,
            base_url,
            max_tokens: 2048,
            temperature: 0.7,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_defaults() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, Some("sk-ant-xxx".into()));
        assert_eq!(cfg.provider, LlmProvider::Anthropic);
        assert!(cfg.model.contains("claude"));
        assert!((cfg.max_tokens as f64 - 2048.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ollama_default_base_url() {
        let cfg = LlmConfig::new(LlmProvider::Ollama, None);
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:11434"));
    }

    #[test]
    fn test_step_spec_serde() {
        let spec = StepSpec {
            step_type: "think".to_string(),
            instruction: "Analyze this".to_string(),
            tool_name: "".to_string(),
            params: serde_json::json!({}),
            prompt: "What is 2+2?".to_string(),
            summary: "Math analysis".to_string(),
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"type\":\"think\""));

        let back: StepSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.step_type, "think");
    }

    #[test]
    fn test_llm_config_f64_types() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, None);
        let _: f64 = cfg.temperature;
    }
}
