use serde::{Deserialize, Serialize};
use tracing::info;

use std::path::PathBuf;

use crate::error::{AgentError, AgentResult};

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
// Chat message types (our public API, provider-agnostic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

// ---------------------------------------------------------------------------
// Gateway — wraps rig-core providers behind a unified interface
//
// Design: AgentBuilder in rig-core is generic over a concrete CompletionModel,
//         and the Prompt trait is not object-safe. Instead of dynamic dispatch,
//         we rebuild the agent per-chat() call. AgentBuilder construction is
//         O(1) — the HTTP client (reqwest) connection pool lives in rig-core
//         internals and is reused across calls.
// ---------------------------------------------------------------------------

/// Unified gateway for multiple LLM providers.
pub struct LlmGateway {
    config: LlmConfig,
    jail_root: Option<PathBuf>,
}

impl LlmGateway {
    pub fn new(config: LlmConfig) -> Self {
        Self { config, jail_root: None }
    }

    pub fn with_jail(config: LlmConfig, jail_root: PathBuf) -> Self {
        Self { config, jail_root: Some(jail_root) }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// Send messages to the LLM and return the response text and token usage.
    /// The first System message is used as the agent's preamble.
    /// Subsequent messages are joined into a single prompt.
    pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        use rig::completion::request::Prompt;

        let (system, rest): (Vec<_>, Vec<_>) =
            messages.iter().partition(|m| m.role == ChatRole::System);

        let preamble = system
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or("You are a helpful AI assistant.");

        let prompt = if rest.is_empty() {
            "Continue.".to_string()
        } else {
            rest.iter()
                .map(|m| format!("{}: {}", role_label(&m.role), m.content))
                .collect::<Vec<_>>()
                .join("\n")
        };

        // Rebuild agent per request (lightweight). Each provider returns
        // a different concrete type, so we keep prompting inside the match.
        let jail_root = self.jail_root.clone();
        let (text, prompt_tokens, completion_tokens): (String, u64, u64) = match &self.config.provider {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Other(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::OpenAI => {
                let agent = build_openai_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Other(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::Ollama => {
                let agent = build_ollama_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Other(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
        };

        let usage = TokenUsage {
            prompt_tokens: prompt_tokens as u32,
            completion_tokens: completion_tokens as u32,
        };

        info!(
            provider = %self.config.provider,
            model = %self.config.model,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "LLM response received"
        );

        Ok((text, usage))
    }
}

// ---------------------------------------------------------------------------
// Per-provider agent builders
// ---------------------------------------------------------------------------

fn build_anthropic_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Other("Anthropic requires an API key. Set it via: agent config set api_key.anthropic <key>".into()))?;

    let client: rig::providers::anthropic::client::Client =
        rig::providers::anthropic::client::Client::new(api_key)
            .map_err(|e| AgentError::Other(format!("Anthropic client init failed: {e}")))?;

    let model = rig::providers::anthropic::completion::CompletionModel::new(
        client,
        &config.model,
    );

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    if let Some(root) = jail_root {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new());
    }

    Ok(builder.build())
}

fn build_openai_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Other("OpenAI requires an API key. Set it via: agent config set api_key.openai <key>".into()))?;

    let client: rig::providers::openai::client::Client =
        match &config.base_url {
            Some(custom_url) => {
                rig::providers::openai::client::Client::builder()
                    .api_key(api_key)
                    .base_url(custom_url)
                    .build()
                    .map_err(|e| AgentError::Other(format!("OpenAI client init failed: {e}")))?
            }
            None => {
                rig::providers::openai::client::Client::new(api_key)
                    .map_err(|e| AgentError::Other(format!("OpenAI client init failed: {e}")))?
            }
        };

    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    if let Some(root) = jail_root {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new());
    }

    if config.base_url.is_some() {
        builder = builder.additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));
    }

    Ok(builder.build())
}

fn build_ollama_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");

    let client: rig::providers::ollama::Client =
        rig::providers::ollama::Client::builder()
            .api_key(rig::client::Nothing)
            .base_url(base_url)
            .build()
            .map_err(|e| AgentError::Other(format!("Ollama client init failed: {e}")))?;

    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    if let Some(root) = jail_root {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new());
    }

    Ok(builder.build())
}

fn role_label(role: &ChatRole) -> &'static str {
    match role {
        ChatRole::System => "System",
        ChatRole::User => "User",
        ChatRole::Assistant => "Assistant",
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
    fn test_chat_message_serde() {
        let msg = ChatMessage {
            role: ChatRole::User,
            content: "Hello".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));

        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, ChatRole::User);
        assert_eq!(deserialized.content, "Hello");
    }

    #[test]
    fn test_llm_config_f64_types() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, None);
        // Verify our config types match rig-core expectations
        let _: f64 = cfg.temperature;
    }
}
