# LLM 网关混合架构 + 思考链支持 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将当前单纯依赖 rig-core 的 LLM 网关升级为"rig-core 兜底 + 原生 provider 层"混合架构，同时支持 Anthropic thinking 和 OpenAI reasoning_effort

**Architecture:** 保持 `LlmGateway` 公开接口不变（向后兼容），内部新增 `LlmRouter` 负责在原生 provider 和 rig-core fallback 之间派发。原生 provider 层通过原生 HTTP 请求直接调用 provider API，获得 thinking/reasoning 等高级参数控制能力。rig-core 保留作为 Ollama 和未知 provider 的 fallback。

**Tech Stack:** Rust, reqwest (已有依赖), serde, rig-core (已有依赖)

---

## 文件结构

```
src/
├── llm.rs              # [MODIFY] 精简为 re-exports + 小型兼容层
├── llm/
│   ├── mod.rs          # [NEW] 模块声明，re-exports
│   ├── config.rs       # [NEW] LlmConfig, ThinkingConfig, LlmProvider（从 llm.rs 提取）
│   ├── types.rs        # [NEW] ChatMessage, ChatRole, TokenUsage（从 llm.rs 提取）
│   ├── gateway.rs      # [NEW] LlmGateway（保持 pub 接口, 内部路由）
│   ├── router.rs       # [NEW] LlmRouter — 原生 vs rig-core 判断
│   ├── native/
│   │   ├── mod.rs      # [NEW] NativeProvider trait
│   │   ├── anthropic.rs # [NEW] 原生 Anthropic HTTP 客户端
│   │   └── openai.rs   # [NEW] 原生 OpenAI HTTP 客户端
│   └── rig_adapter.rs  # [NEW] 从旧 llm.rs 迁移过来的 rig-core builder
```

---

### Task 1: 模块提取 — 将 config 和 types 从 llm.rs 分离

**Files:**
- Create: `src/llm/mod.rs`
- Create: `src/llm/config.rs`
- Create: `src/llm/types.rs`
- Create: `src/llm/gateway.rs` (骨架)
- Create: `src/llm/router.rs` (骨架)
- Create: `src/llm/rig_adapter.rs` (从旧 llm.rs 迁移)
- Delete: `src/llm.rs`
- Modify: `src/lib.rs`
- Modify: `src/build_engine.rs`
- Modify: `src/agent.rs`

**Key constraint:** 保持所有现有 pub 类型和路径不变。现有代码 `rupooro::llm::*` 必须继续可用。

- [ ] **Step 1: Create `src/llm/config.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmProvider {
    Anthropic,
    OpenAI,
    Ollama,
    Google,
    // rig-* prefix indicates these use rig-core fallback path
    Custom(String),
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::Ollama => write!(f, "ollama"),
            LlmProvider::Google => write!(f, "google"),
            LlmProvider::Custom(s) => write!(f, "custom-{s}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
    XHigh,
}

impl Default for ThinkingLevel {
    fn default() -> Self { Self::Off }
}

impl ThinkingLevel {
    /// Anthropic thinking.budget_tokens mapping
    pub fn anthropic_budget(&self) -> Option<u32> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Low => Some(2048),
            ThinkingLevel::Medium => Some(8192),
            ThinkingLevel::High => Some(16384),
            ThinkingLevel::XHigh => Some(32768),
        }
    }

    /// OpenAI reasoning_effort mapping
    pub fn openai_reasoning_effort(&self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::XHigh => Some("high"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: Option<String>,
    /// Base URL override (for proxies or local servers).
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f64,
    /// Thinking / reasoning level. Only supported on native provider path.
    /// rig-core fallback will silently ignore this.
    #[serde(default)]
    pub thinking_level: ThinkingLevel,
}

impl LlmConfig {
    pub fn new(provider: LlmProvider, api_key: Option<String>) -> Self {
        let (model, base_url) = match &provider {
            LlmProvider::Anthropic => ("claude-sonnet-4-20250514".into(), None),
            LlmProvider::OpenAI => ("gpt-4o".into(), None),
            LlmProvider::Ollama => {
                ("llama3.2".into(), Some("http://localhost:11434".into()))
            }
            LlmProvider::Google => ("gemini-2.0-flash".into(), None),
            LlmProvider::Custom(_) => ("gpt-4o".into(), None),
        };
        Self {
            provider,
            model,
            api_key,
            base_url,
            max_tokens: 2048,
            temperature: 0.7,
            thinking_level: ThinkingLevel::Off,
        }
    }

    /// Returns true if this provider should use the native (non-rig-core) path.
    pub fn use_native_path(&self) -> bool {
        matches!(self.provider, LlmProvider::Anthropic | LlmProvider::OpenAI)
            || (self.thinking_level != ThinkingLevel::Off)
    }
}
```

- [ ] **Step 2: Write test for ThinkingConfig**

In `src/llm/config.rs` `#[cfg(test)]`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_level_default_is_off() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, None);
        assert_eq!(cfg.thinking_level, ThinkingLevel::Off);
    }

    #[test]
    fn test_anthropic_thinking_budget() {
        assert_eq!(ThinkingLevel::Low.anthropic_budget(), Some(2048));
        assert_eq!(ThinkingLevel::Off.anthropic_budget(), None);
        assert_eq!(ThinkingLevel::XHigh.anthropic_budget(), Some(32768));
    }

    #[test]
    fn test_openai_reasoning_mapping() {
        assert_eq!(ThinkingLevel::Low.openai_reasoning_effort(), Some("low"));
        assert_eq!(ThinkingLevel::Off.openai_reasoning_effort(), None);
        assert_eq!(ThinkingLevel::XHigh.openai_reasoning_effort(), Some("high"));
    }

    #[test]
    fn test_use_native_path_for_thinking() {
        let mut cfg = LlmConfig::new(LlmProvider::Anthropic, Some("key".into()));
        assert!(cfg.use_native_path()); // Anthropic always goes native
        cfg.thinking_level = ThinkingLevel::High;
        assert!(cfg.use_native_path());
    }

    #[test]
    fn test_ollama_uses_rig_path() {
        let cfg = LlmConfig::new(LlmProvider::Ollama, None);
        assert!(!cfg.use_native_path()); // Ollama always rig-core
    }
}
```

Run: `cargo test test_anthropic_thinking_budget`
Expected: PASS

- [ ] **Step 3: Create `src/llm/types.rs`**

```rust
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage() {
        let u = TokenUsage { prompt_tokens: 10, completion_tokens: 20 };
        assert_eq!(u.total(), 30);
    }

    #[test]
    fn test_chat_message_serde() {
        let msg = ChatMessage { role: ChatRole::User, content: "Hello".into() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, ChatRole::User);
    }
}
```

- [ ] **Step 4: Create `src/llm/rig_adapter.rs` — 从旧 llm.rs 迁移 builder**

```rust
use std::path::Path;
use rig::agent::AgentBuilder;

use crate::error::{AgentError, AgentResult};
use super::config::LlmConfig;
use super::config::LlmProvider;

/// Build a rig-core agent for the given provider.
/// These are used as fallback when native path is not available.
pub(crate) fn build_rig_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&Path>,
) -> AgentResult<rig::agent::Agent<Box<dyn rig::completion::CompletionModel>>> {
    match &config.provider {
        LlmProvider::Anthropic => {
            let agent = build_rig_anthropic(config, preamble, jail_root)?;
            Ok(agent)
        }
        LlmProvider::OpenAI => {
            let agent = build_rig_openai(config, preamble, jail_root)?;
            Ok(agent)
        }
        LlmProvider::Ollama => {
            let agent = build_rig_ollama(config, preamble, jail_root)?;
            Ok(agent)
        }
        LlmProvider::Google => {
            let agent = build_rig_google(config, preamble, jail_root)?;
            Ok(agent)
        }
        LlmProvider::Custom(_) => {
            // Treat custom as OpenAI-compatible
            let agent = build_rig_openai(config, preamble, jail_root)?;
            Ok(agent)
        }
    }
}

fn build_rig_anthropic(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::providers::anthropic;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Other(
            "Anthropic requires an API key.".into()
        ))?;

    let client = anthropic::client::Client::new(api_key)
        .map_err(|e| AgentError::Other(format!("Anthropic client init failed: {e}")))?;
    let model = anthropic::completion::CompletionModel::new(client, &config.model);

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    attach_jail_tools(&mut builder, jail_root);
    Ok(builder.build())
}

fn build_rig_openai(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::providers::openai;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Other("OpenAI requires an API key.".into()))?;

    let client = match &config.base_url {
        Some(custom_url) => {
            openai::client::Client::builder()
                .api_key(api_key)
                .base_url(custom_url)
                .build()
                .map_err(|e| AgentError::Other(format!("OpenAI client init failed: {e}")))?
        }
        None => {
            openai::client::Client::new(api_key)
                .map_err(|e| AgentError::Other(format!("OpenAI client init failed: {e}")))?
        }
    };

    let model = openai::completion::CompletionModel::new(client.completions_api(), &config.model);

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    attach_jail_tools(&mut builder, jail_root);
    Ok(builder.build())
}

fn build_rig_ollama(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::providers::ollama;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");
    let client = ollama::Client::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| AgentError::Other(format!("Ollama client init failed: {e}")))?;
    let model = ollama::CompletionModel::new(client, &config.model);

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    attach_jail_tools(&mut builder, jail_root);
    Ok(builder.build())
}

fn build_rig_google(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::google::CompletionModel>> {
    use rig::providers::google;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Other("Google AI requires an API key.".into()))?;

    let client = google::Client::new(api_key);
    let model = google::CompletionModel::new(client, &config.model);

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .tool(crate::rig_tools::EchoTool::new())
        .default_max_turns(10);

    attach_jail_tools(&mut builder, jail_root);
    Ok(builder.build())
}

fn attach_jail_tools<T: rig::completion::CompletionModel>(
    builder: &mut AgentBuilder<T>,
    jail_root: Option<&Path>,
) {
    if let Some(root) = jail_root {
        *builder = builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        *builder = builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new());
    }
}
```

- [ ] **Step 5: Create `src/llm/native/mod.rs` — NativeProvider trait**

```rust
use super::config::LlmConfig;
use super::types::{ChatMessage, TokenUsage};
use crate::error::AgentResult;

/// A native (non-rig-core) LLM provider that supports advanced features
/// like thinking/reasoning, prompt caching, etc.
#[async_trait::async_trait]
pub trait NativeProvider: Send + Sync {
    /// Send messages to the LLM and return response + token usage.
    async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)>;

    /// Create a new instance of this provider from a LlmConfig.
    fn from_config(config: &LlmConfig) -> AgentResult<Self> where Self: Sized;

    /// Returns the provider name for logging.
    fn name(&self) -> &'static str;
}
```

- [ ] **Step 6: Create `src/llm/native/anthropic.rs` — 原生 Anthropic 客户端（含 thinking）**

```rust
use serde::{Deserialize, Serialize};
use tracing::info;

use super::NativeProvider;
use super::super::config::{LlmConfig, ThinkingLevel};
use super::super::types::{ChatMessage, ChatRole, TokenUsage};
use crate::error::{AgentError, AgentResult};

#[derive(Serialize)]
struct MessageRequest {
    model: String,
    max_tokens: u32,
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingBlock>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ContentBlock {
    Text { text: String, #[serde(rename = "type")] _type: String },
    Thinking { #[serde(rename = "type")] _type: String, signature: Option<String> },
}

#[derive(Serialize)]
struct ThinkingBlock {
    #[serde(rename = "type")]
    _type: String,
    budget_tokens: u32,
}

#[derive(Deserialize)]
struct MessageResponse {
    content: Vec<ResponseContent>,
    usage: UsageInfo,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    _type: String,
    text: Option<String>,
    thinking: Option<String>,
    signature: Option<String>,
}

#[derive(Deserialize)]
struct UsageInfo {
    input_tokens: u32,
    output_tokens: u32,
}

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    max_tokens: u32,
    thinking_level: ThinkingLevel,
    http_client: reqwest::Client,
}

#[async_trait::async_trait]
impl NativeProvider for AnthropicProvider {
    fn from_config(config: &LlmConfig) -> AgentResult<Self> {
        let api_key = config.api_key.as_deref()
            .ok_or_else(|| AgentError::Other("Anthropic requires an API key.".into()))?
            .to_string();
        Ok(Self {
            api_key,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            thinking_level: config.thinking_level,
            http_client: reqwest::Client::new(),
        })
    }

    fn name(&self) -> &'static str { "anthropic-native" }

    async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        let (system_text, chat_msgs): (Option<String>, Vec<&ChatMessage>) = {
            let mut sys = None;
            let mut rest = Vec::new();
            for m in messages {
                if m.role == ChatRole::System {
                    sys = Some(m.content.clone());
                } else {
                    rest.push(m);
                }
            }
            (sys, rest)
        };

        let anthropic_messages: Vec<AnthropicMessage> = chat_msgs.iter().map(|m| {
            AnthropicMessage {
                role: match m.role {
                    ChatRole::User => "user".to_string(),
                    ChatRole::Assistant => "assistant".to_string(),
                    _ => "user".to_string(),
                },
                content: vec![ContentBlock::Text {
                    text: m.content.clone(),
                    _type: "text".to_string(),
                }],
            }
        }).collect();

        let mut request = MessageRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system_text,
            messages: anthropic_messages,
            thinking: self.thinking_level.anthropic_budget().map(|budget| ThinkingBlock {
                _type: "enabled".to_string(),
                budget_tokens: budget,
            }),
        };

        let resp = self.http_client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::Other(format!("Anthropic request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Other(
                format!("Anthropic API error {status}: {body}")
            ));
        }

        let parsed: MessageResponse = resp
            .json()
            .await
            .map_err(|e| AgentError::Other(format!("Anthropic parse failed: {e}")))?;

        let text: String = parsed.content
            .iter()
            .filter_map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let usage = TokenUsage {
            prompt_tokens: parsed.usage.input_tokens,
            completion_tokens: parsed.usage.output_tokens,
        };

        info!(
            model = %self.model,
            thinking = ?self.thinking_level,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "Anthropic native response received"
        );

        Ok((text, usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_provider_name() {
        let cfg = LlmConfig::new(super::super::super::config::LlmProvider::Anthropic, Some("sk-test".into()));
        let provider = AnthropicProvider::from_config(&cfg).unwrap();
        assert_eq!(provider.name(), "anthropic-native");
    }

    #[test]
    fn test_anthropic_requires_key() {
        let cfg = LlmConfig::new(super::super::super::config::LlmProvider::Anthropic, None);
        let result = AnthropicProvider::from_config(&cfg);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 7: Create `src/llm/native/openai.rs` — 原生 OpenAI 客户端（含 reasoning_effort）**

```rust
use serde::{Deserialize, Serialize};
use tracing::info;

use super::NativeProvider;
use super::super::config::LlmConfig;
use super::super::types::{ChatMessage, ChatRole, TokenUsage};
use crate::error::{AgentError, AgentResult};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: UsageInfo,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct UsageInfo {
    prompt_tokens: u32,
    completion_tokens: u32,
}

pub struct OpenAIProvider {
    api_key: String,
    model: String,
    base_url: String,
    max_tokens: u32,
    reasoning_effort: Option<String>,
    http_client: reqwest::Client,
}

#[async_trait::async_trait]
impl NativeProvider for OpenAIProvider {
    fn from_config(config: &LlmConfig) -> AgentResult<Self> {
        let api_key = config.api_key.as_deref()
            .ok_or_else(|| AgentError::Other("OpenAI requires an API key.".into()))?
            .to_string();
        Ok(Self {
            api_key,
            model: config.model.clone(),
            base_url: config.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".into()),
            max_tokens: config.max_tokens,
            reasoning_effort: config.thinking_level.openai_reasoning_effort().map(String::from),
            http_client: reqwest::Client::new(),
        })
    }

    fn name(&self) -> &'static str { "openai-native" }

    async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        let openai_messages: Vec<OpenAiMessage> = messages.iter().map(|m| {
            OpenAiMessage {
                role: match m.role {
                    ChatRole::System => "system".to_string(),
                    ChatRole::User => "user".to_string(),
                    ChatRole::Assistant => "assistant".to_string(),
                },
                content: m.content.clone(),
            }
        }).collect();

        let request = ChatRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            messages: openai_messages,
            reasoning_effort: self.reasoning_effort.clone(),
        };

        let resp = self.http_client
            .post(format!("{}/chat/completions", self.base_url.trim_end_matches('/')))
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::Other(format!("OpenAI request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Other(format!("OpenAI API error {status}: {body}")));
        }

        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AgentError::Other(format!("OpenAI parse failed: {e}")))?;

        let text = parsed.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        let usage = TokenUsage {
            prompt_tokens: parsed.usage.prompt_tokens,
            completion_tokens: parsed.usage.completion_tokens,
        };

        info!(
            model = %self.model,
            reasoning = ?self.reasoning_effort,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "OpenAI native response received"
        );

        Ok((text, usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_name() {
        let cfg = LlmConfig::new(super::super::super::config::LlmProvider::OpenAI, Some("sk-test".into()));
        let provider = OpenAIProvider::from_config(&cfg).unwrap();
        assert_eq!(provider.name(), "openai-native");
    }

    #[test]
    fn test_openai_requires_key() {
        let cfg = LlmConfig::new(super::super::super::config::LlmProvider::OpenAI, None);
        let result = OpenAIProvider::from_config(&cfg);
        assert!(result.is_err());
    }

    #[test]
    fn test_openai_base_url_default() {
        let cfg = LlmConfig::new(super::super::super::config::LlmProvider::OpenAI, Some("sk-test".into()));
        let provider = OpenAIProvider::from_config(&cfg).unwrap();
        // We can't access base_url since it's private — just check name
        assert_eq!(provider.name(), "openai-native");
    }
}
```

- [ ] **Step 8: Create `src/llm/router.rs`**

```rust
use std::path::PathBuf;

use crate::error::AgentResult;

use super::config::LlmConfig;
use super::types::{ChatMessage, TokenUsage};
use super::rig_adapter;
use super::native::{NativeProvider, anthropic::AnthropicProvider, openai::OpenAIProvider};

/// Routes LLM requests to either a native provider or rig-core fallback.
pub(crate) enum LlmRouter {
    Native(Box<dyn NativeProvider>),
    Rig {
        config: LlmConfig,
        jail_root: Option<PathBuf>,
    },
}

impl LlmRouter {
    pub fn new(config: LlmConfig, jail_root: Option<PathBuf>) -> AgentResult<Self> {
        if config.use_native_path() {
            match &config.provider {
                super::config::LlmProvider::Anthropic => {
                    let provider = AnthropicProvider::from_config(&config)?;
                    Ok(LlmRouter::Native(Box::new(provider)))
                }
                super::config::LlmProvider::OpenAI => {
                    let provider = OpenAIProvider::from_config(&config)?;
                    Ok(LlmRouter::Native(Box::new(provider)))
                }
                _ => {
                    // Fall through to rig-core for unknown native providers
                    Ok(LlmRouter::Rig { config, jail_root })
                }
            }
        } else {
            Ok(LlmRouter::Rig { config, jail_root })
        }
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        match self {
            LlmRouter::Native(provider) => {
                provider.chat(messages).await
            }
            LlmRouter::Rig { config, jail_root } => {
                let preamble = messages
                    .iter()
                    .find(|m| m.role == super::types::ChatRole::System)
                    .map(|m| m.content.as_str())
                    .unwrap_or("You are a helpful AI assistant.");

                let prompt = messages
                    .iter()
                    .filter(|m| m.role != super::types::ChatRole::System)
                    .map(|m| format!("{}: {}", m.role_label(), m.content))
                    .collect::<Vec<_>>()
                    .join("\n");

                let agent = rig_adapter::build_rig_agent(config, preamble, jail_root.as_deref())?;

                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| crate::error::AgentError::Other(format!("LLM request failed: {e}")))?;

                let usage = TokenUsage {
                    prompt_tokens: response.total_usage.input_tokens as u32,
                    completion_tokens: response.total_usage.output_tokens as u32,
                };

                Ok((response.output, usage))
            }
        }
    }
}

// Helper on ChatRole
fn role_label(role: &super::types::ChatRole) -> &'static str {
    match role {
        super::types::ChatRole::System => "System",
        super::types::ChatRole::User => "User",
        super::types::ChatRole::Assistant => "Assistant",
    }
}

impl super::types::ChatRole {
    fn role_label(&self) -> &'static str {
        match self {
            super::types::ChatRole::System => "System",
            super::types::ChatRole::User => "User",
            super::types::ChatRole::Assistant => "Assistant",
        }
    }
}
```

Wait, there's a conflict — I defined `role_label` as a free function AND as a method on ChatRole. Let me just use the method approach since it's cleaner:

```rust
// Remove the free function, keep only the method
```

- [ ] **Step 9: Create `src/llm/gateway.rs` — LlmGateway 新实现**

```rust
use std::path::PathBuf;
use tracing::info;

use super::config::LlmConfig;
use super::types::{ChatMessage, TokenUsage};
use super::router::LlmRouter;
use crate::error::AgentResult;

/// Unified gateway for multiple LLM providers.
/// Internally routes to either native provider or rig-core fallback.
pub struct LlmGateway {
    router: LlmRouter,
}

impl LlmGateway {
    pub fn new(config: LlmConfig) -> Self {
        let router = LlmRouter::new(config, None)
            .expect("LlmRouter::new should only fail on missing API keys, \
                     but LlmGateway::new is called without config checks. \
                     Call with_jail or ensure config has API key.");
        Self { router }
    }

    pub fn with_jail(config: LlmConfig, jail_root: PathBuf) -> Self {
        let router = LlmRouter::new(config, Some(jail_root))
            .expect("LlmRouter::with_jail failed — missing API key?");
        Self { router }
    }

    /// Send messages to the LLM and return the response text and token usage.
    pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        let (text, usage) = self.router.chat(messages).await?;
        Ok((text, usage))
    }
}
```

Actually, the `expect` on `LlmRouter::new` is not great — it could panic. Let me make `LlmGateway::new` infallible by handling the router construction more gracefully: if a native path requires an API key that's missing, it falls back to rig-core (which will also fail, but at chat time with a proper error).

Better approach — make `LlmGateway::new` not call `LlmRouter::new`:

```rust
pub struct LlmGateway {
    config: LlmConfig,
    jail_root: Option<PathBuf>,
    // Lazily initialized router
    router: std::sync::OnceLock<LlmRouter>,
}

impl LlmGateway {
    pub fn new(config: LlmConfig) -> Self {
        Self { config, jail_root: None, router: std::sync::OnceLock::new() }
    }

    pub fn with_jail(config: LlmConfig, jail_root: PathBuf) -> Self {
        Self { config, jail_root: Some(jail_root), router: std::sync::OnceLock::new() }
    }

    fn get_or_init_router(&self) -> AgentResult<&LlmRouter> {
        self.router.get_or_try_init(|| {
            LlmRouter::new(self.config.clone(), self.jail_root.clone())
        })
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
        let router = self.get_or_init_router()?;
        let (text, usage) = router.chat(messages).await?;

        info!(
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "LLM response received"
        );

        Ok((text, usage))
    }
}
```

- [ ] **Step 10: Create `src/llm/native/mod.rs` (final version with mod declarations)**

```rust
pub mod anthropic;
pub mod openai;

use super::config::LlmConfig;
use super::types::{ChatMessage, TokenUsage};
use crate::error::AgentResult;

/// A native (non-rig-core) LLM provider that supports advanced features
/// like thinking/reasoning, prompt caching, etc.
#[async_trait::async_trait]
pub trait NativeProvider: Send + Sync {
    /// Send messages to the LLM and return response + token usage.
    async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)>;

    /// Returns the provider name for logging.
    fn name(&self) -> &'static str;
}
```

Remove `fn from_config` from the trait — each provider handles construction differently via `LlmRouter`.

- [ ] **Step 11: Create `src/llm/mod.rs`**

```rust
pub mod config;
pub mod types;
pub mod gateway;
mod router;
mod rig_adapter;
mod native;

pub use config::{LlmConfig, LlmProvider, ThinkingLevel};
pub use types::{ChatMessage, ChatRole, TokenUsage};
pub use gateway::LlmGateway;
```

- [ ] **Step 12: Update `src/lib.rs`**

```rust
pub mod llm;
// Remove: pub mod llm; (already exists but points to llm.rs — update to dir)
```

Wait, the existing `lib.rs` has:
```rust
pub mod llm;
```

If I delete `src/llm.rs` and create `src/llm/mod.rs`, Rust will automatically resolve `pub mod llm` to the directory. So no change needed in `lib.rs`.

Actually, looking at `lib.rs` more carefully:
```rust
pub mod agent;
pub mod db;
pub mod error;
pub mod git;
pub mod llm;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod rig_tools;
pub mod shared;
pub mod skill;
pub mod task;
```

`pub mod llm;` already exists. When `src/llm.rs` is replaced with `src/llm/mod.rs`, this line continues to work. Good.

But I need to check if there are any `#[path = "..."]` attributes that reference `src/llm.rs`. Let me check agent.rs:

In `agent.rs` line 15:
```rust
#[path = "safety.rs"]
pub mod safety;
```

That's safety, not llm. Good.

- [ ] **Step 13: Update `build_engine.rs` imports**

Change:
```rust
use rupoo::llm::{LlmConfig, LlmGateway, LlmProvider};
```
Keep exactly the same — the re-exports haven't changed.

Also need to import `ThinkingLevel` in build_engine when it's configurable. For now, build_engine stays as-is.

- [ ] **Step 14: Delete old `src/llm.rs`**

```bash
rm src/llm.rs
git add src/llm.rs -u  # staged for deletion
```

- [ ] **Step 15: Run all tests to verify no regressions**

```bash
cargo test --lib
```
Expected: all existing tests pass. Any tests referencing `LlmConfig::new(LlmProvider::Anthropic, ...)` or similar old path should still pass since we preserved the re-exports.

If tests reference the old `ChatMessage` type from `rupooro::llm::ChatMessage`, this should still work since it's re-exported.

Actually, wait — `shared.rs` also defines its own `ChatMessage` type (different from `llm::ChatMessage`). The old `llm.rs` had its own `ChatMessage` and `ChatRole`, and `shared.rs` has its own `ChatMessage` and `MessageRole`. These are different types used for different purposes. This should be fine.

- [ ] **Step 16: Commit**

```bash
git add src/llm/ src/llm.rs -A  # Add all new files and deletion of old
git commit -m "refactor(llm): split monolithic llm.rs into modular directory structure"
```

---

### Task 2: 集成 thinking_level 配置到构建流程

**Files:**
- Modify: `src/build_engine.rs`

- [ ] **Step 1: Build engine 支持 thinking 配置**

In `build_engine.rs`, after setting `cfg.model`, add:
```rust
// Load thinking level from config if available
if let Some(level_str) = repo.get_setting("thinking_level").await? {
    cfg.thinking_level = match level_str.to_lowercase().as_str() {
        "low" => rupoo::llm::ThinkingLevel::Low,
        "medium" => rupoo::llm::ThinkingLevel::Medium,
        "high" => rupoo::llm::ThinkingLevel::High,
        "xhigh" | "extra_high" => rupoo::llm::ThinkingLevel::XHigh,
        _ => rupoo::llm::ThinkingLevel::Off,
    };
}
```

Add this to both the Anthropic and OpenAI config blocks.

- [ ] **Step 2: Build check**

```bash
cargo check
```
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add src/build_engine.rs
git commit -m "feat: integrate thinking_level config into engine build"
```

---

### Task 3: 配置命令支持 thinking_level

**Files:**
- Modify: `src/main_cli.rs` (or equivalent config command handler)

- [ ] **Step 1: Add thinking level CLI commands**

In the config handler, add support for:
```bash
rupoo config set thinking_level high    # sets to DB
rupoo config get thinking_level          # reads from DB
```

The implementation follows the existing config set/get pattern:
```rust
"thinking_level" => {
    let val = value.to_lowercase();
    if !["off", "low", "medium", "high", "xhigh"].contains(&val.as_str()) {
        eprintln!("Invalid thinking_level. Valid: off|low|medium|high|xhigh");
        return;
    }
    cfg.set("thinking_level", &val).await?;
}
```

- [ ] **Step 2: Build and run verify**

```bash
cargo build
./target/debug/rupoo config set thinking_level high
./target/debug/rupoo config get thinking_level
```
Expected: `thinking_level = high`

- [ ] **Step 3: Commit**

```bash
git add src/main_cli.rs
git commit -m "feat(cli): add thinking_level config command"
```

---

### Task 4: 验证测试 — 原生 provider 集成测试（需要 API key）

**Files:**
- Create: `tests/llm_native_integration_test.rs`

- [ ] **Step 1: Write integration tests**

```rust
/// Integration tests for native LLM providers.
/// Requires API keys via environment variables.
use rupoo::llm::{LlmConfig, LlmProvider, LlmGateway, ChatMessage, ChatRole, ThinkingLevel};

/// Helper: check if an environment variable is set for testing.
fn has_env(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| !v.is_empty())
}

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY env"]
async fn test_anthropic_native_basic_chat() {
    if !has_env("ANTHROPIC_API_KEY") {
        eprintln!("skipped: set ANTHROPIC_API_KEY to run");
        return;
    }

    let mut cfg = LlmConfig::new(
        LlmProvider::Anthropic,
        std::env::var("ANTHROPIC_API_KEY").ok(),
    );
    cfg.model = "claude-sonnet-4-20250514".to_string();
    cfg.max_tokens = 100;

    let gateway = LlmGateway::new(cfg);
    let messages = vec![
        ChatMessage { role: ChatRole::User, content: "Say exactly 'hello world' and nothing else.".into() },
    ];

    let (response, usage) = gateway.chat(&messages).await.unwrap();
    assert!(!response.is_empty(), "response should not be empty");
    assert!(usage.prompt_tokens > 0, "prompt tokens should be > 0");
    assert!(usage.completion_tokens > 0, "completion tokens should be > 0");
    eprintln!("Response: {response}");
    eprintln!("Usage: {} in, {} out", usage.prompt_tokens, usage.completion_tokens);
}

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY env"]
async fn test_anthropic_native_with_thinking() {
    if !has_env("ANTHROPIC_API_KEY") {
        eprintln!("skipped: set ANTHROPIC_API_KEY to run");
        return;
    }

    let mut cfg = LlmConfig::new(
        LlmProvider::Anthropic,
        std::env::var("ANTHROPIC_API_KEY").ok(),
    );
    cfg.model = "claude-sonnet-4-20250514".to_string();
    cfg.max_tokens = 500;
    cfg.thinking_level = ThinkingLevel::Low;

    let gateway = LlmGateway::new(cfg);
    let messages = vec![
        ChatMessage {
            role: ChatRole::User,
            content: "What is 47 * 89? Show your work step by step.".into(),
        },
    ];

    let (response, usage) = gateway.chat(&messages).await.unwrap();
    assert!(!response.is_empty());
    assert!(usage.completion_tokens > 0);
    eprintln!("Thinking response (first 200 chars): {}", &response[..response.len().min(200)]);
    eprintln!("Usage: {} in, {} out", usage.prompt_tokens, usage.completion_tokens);
}

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY env"]
async fn test_openai_native_basic_chat() {
    if !has_env("OPENAI_API_KEY") {
        eprintln!("skipped: set OPENAI_API_KEY to run");
        return;
    }

    let mut cfg = LlmConfig::new(
        LlmProvider::OpenAI,
        std::env::var("OPENAI_API_KEY").ok(),
    );
    cfg.model = "gpt-4o-mini".to_string();
    cfg.max_tokens = 100;

    let gateway = LlmGateway::new(cfg);
    let messages = vec![
        ChatMessage { role: ChatRole::User, content: "Say exactly 'hello from openai' and nothing else.".into() },
    ];

    let (response, usage) = gateway.chat(&messages).await.unwrap();
    assert!(!response.is_empty());
    assert!(usage.prompt_tokens > 0);
    assert!(usage.completion_tokens > 0);
    eprintln!("OpenAI Response: {response}");
}

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY env"]
async fn test_openai_native_with_reasoning() {
    if !has_env("OPENAI_API_KEY") {
        eprintln!("skipped: set OPENAI_API_KEY to run");
        return;
    }

    let mut cfg = LlmConfig::new(
        LlmProvider::OpenAI,
        std::env::var("OPENAI_API_KEY").ok(),
    );
    cfg.model = "o4-mini".to_string();
    cfg.max_tokens = 500;
    cfg.thinking_level = ThinkingLevel::High;

    let gateway = LlmGateway::new(cfg);
    let messages = vec![
        ChatMessage {
            role: ChatRole::User,
            content: "What is 47 * 89? Show your work step by step.".into(),
        },
    ];

    let (response, usage) = gateway.chat(&messages).await.unwrap();
    assert!(!response.is_empty());
    eprintln!("OpenAI Reasoning response: {}", &response[..response.len().min(200)]);
    eprintln!("Usage: {} in, {} out", usage.prompt_tokens, usage.completion_tokens);
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --tests
```
Expected: compiles clean.

- [ ] **Step 3: Run with API key (manual)**

```bash
ANTHROPIC_API_KEY=sk-ant-xxx cargo test test_anthropic_native_basic_chat -- --ignored --nocapture
```
Expected: PASS, displays response and usage.

- [ ] **Step 4: Commit**

```bash
git add tests/llm_native_integration_test.rs
git commit -m "test: add native LLM provider integration tests"
```

---

### Task 5: Rig-core fallback 测试

**Files:**
- Modify: `src/llm/rig_adapter.rs`

Verify that the rig-core fallback path still works for providers that don't use native path.

- [ ] **Step 1: Write unit test for fallback routing**

In `src/llm/router.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_uses_native_path() {
        let cfg = super::super::config::LlmConfig::new(
            super::super::config::LlmProvider::Anthropic,
            Some("sk-test".into()),
        );
        let router = LlmRouter::new(cfg, None).unwrap();
        assert!(matches!(router, LlmRouter::Native(_)));
    }

    #[test]
    fn test_ollama_uses_rig_path() {
        let cfg = super::super::config::LlmConfig::new(
            super::super::config::LlmProvider::Ollama,
            None,
        );
        let router = LlmRouter::new(cfg, None).unwrap();
        assert!(matches!(router, LlmRouter::Rig { .. }));
    }

    #[test]
    fn test_thinking_forces_native() {
        let mut cfg = super::super::config::LlmConfig::new(
            super::super::config::LlmProvider::OpenAI,
            Some("sk-test".into()),
        );
        cfg.thinking_level = super::super::config::ThinkingLevel::Medium;
        let router = LlmRouter::new(cfg, None).unwrap();
        assert!(matches!(router, LlmRouter::Native(_)));
    }

    #[test]
    fn test_missing_api_key_on_native_fails() {
        let cfg = super::super::config::LlmConfig::new(
            super::super::config::LlmProvider::Anthropic,
            None,
        );
        let result = LlmRouter::new(cfg, None);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test test_anthropic_uses_native_path test_ollama_uses_rig_path test_thinking_forces_native test_missing_api_key_on_native_fails
```
Expected: all 4 PASS.

- [ ] **Step 3: Commit**

```bash
git add src/llm/router.rs
git commit -m "test: verify router dispatch logic for native vs rig-core"
```

---

### Task 6: Google Gemini provider via rig-core（作为扩展示例）

**Files:**
- Modify: `src/build_engine.rs`

- [ ] **Step 1: Add Google Gemini config block in build_engine**

In `build_engine.rs`, add after the OpenAI block:
```rust
} else if let Some(api_key) = repo.get_setting("api_key.google").await? {
    let mut cfg = rupoo::llm::LlmConfig::new(
        rupoo::llm::LlmProvider::Google,
        Some(api_key),
    );
    if let Some(model) = repo.get_setting("model.google").await? {
        cfg.model = model;
    }
    let gateway = if let Some(ref root) = jail_root {
        rupoo::llm::LlmGateway::with_jail(cfg, root.clone())
    } else {
        rupoo::llm::LlmGateway::new(cfg)
    };
    agent = agent.with_llm(gateway);
    info!("Google AI LLM configured via rig-core");
}
```

- [ ] **Step 2: Build check**

```bash
cargo check
```
Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add src/build_engine.rs
git commit -m "feat: add Google Gemini provider via rig-core fallback"
```

---

## Verification Summary

| 验证点 | 方法 | 预期结果 |
|--------|------|---------|
| 基础聊天 | 集成测试 `test_anthropic_native_basic_chat` | 返回非空响应 + token 计数 |
| Thinking 效果 | 集成测试 `test_anthropic_native_with_thinking` | 响应包含推理过程 |
| Reasoning 效果 | 集成测试 `test_openai_native_with_reasoning` | 响应包含推理过程 |
| 路由逻辑 | 单元测试 `test_anthropic_uses_native_path` | Anthropic → Native path |
| 路由逻辑 | 单元测试 `test_ollama_uses_rig_path` | Ollama → Rig path |
| 路由逻辑 | 单元测试 `test_thinking_forces_native` | Thinking=on → Native path |
| 降级 | 路由测试 `test_missing_api_key_on_native_fails` | 无 key 时构造失败 |
| 兼容性 | `cargo test --lib` | 所有旧测试通过 |
| 配置 CLI | 手动 `rupoo config set thinking_level high` | 配置写入并持久化 |
