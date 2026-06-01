//! Per-provider agent builders for LLM Gateway.

use std::sync::Arc;

use crate::error::{AgentError, AgentResult};
use crate::llm::history::LlmChatRole;
use crate::llm::LlmConfig;

/// Register tools on the builder based on safe_mode setting.
/// Returns AgentBuilderSimple because .tool() transitions from AgentBuilder to AgentBuilderSimple.
pub fn register_tools<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilderSimple<M>,
    jail_root: Option<&std::path::Path>,
    _safe_mode: bool,
) -> rig::agent::AgentBuilderSimple<M> {
    // _safe_mode is retained for API compatibility but no longer gates FileWriteTool.
    // File writes are always available; path jail enforces project-boundary safety.
    let mut builder = builder;

    // Web search is read-only and safe — always register
    builder = builder.tool(crate::rig_tools::WebSearchTool::new());

    // Shell execution with safety validation (sudo/rm/etc. blocked)
    builder = builder.tool(crate::rig_tools::ShellExecTool::new());

    // FileReadTool is safe
    if let Some(root) = jail_root {
        builder = builder.tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder.tool(crate::rig_tools::FileReadTool::new());
    }

    // ListDirTool is safe
    if let Some(root) = jail_root {
        builder = builder.tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder.tool(crate::rig_tools::ListDirTool::new());
    }

    // FileWriteTool — always register so the LLM knows it can write files.
    // The jail_root still enforces that writes stay inside the project directory.
    if let Some(root) = jail_root {
        builder = builder.tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder.tool(crate::rig_tools::FileWriteTool::new());
    }

    builder
}

/// Register tools (all tools, no safe_mode filtering) for legacy non-streaming agents.
pub fn register_tools_legacy<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilderSimple<M>,
    jail_root: Option<&std::path::Path>,
) -> rig::agent::AgentBuilderSimple<M> {
    let builder = builder.tool(crate::rig_tools::WebSearchTool::new())
        .tool(crate::rig_tools::ShellExecTool::new());
    if let Some(root) = jail_root {
        builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()))
    } else {
        builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new())
    }
}

pub fn build_anthropic_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>".into()))?;
    let client = <rig::providers::anthropic::client::Client<reqwest::Client>>::builder()
        .api_key(api_key)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Anthropic client init failed: {e}")))?;
    let model = rig::providers::anthropic::completion::CompletionModel::new(client, &config.model)
        .with_prompt_caching();

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(50)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

pub fn build_openai_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into()))?;
    let client: rig::providers::openai::client::Client = match &config.base_url {
        Some(custom_url) => {
            <rig::providers::openai::client::Client<reqwest::Client>>::builder()
                .api_key(api_key)
                .base_url(custom_url)
                .http_client((**http_client).clone())
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
        None => {
            <rig::providers::openai::client::Client<reqwest::Client>>::builder()
                .api_key(api_key)
                .http_client((**http_client).clone())
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
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
        .default_max_turns(50)
        .tool(crate::rig_tools::EchoTool::new());

    // Disable thinking mode for custom base_url (e.g. DeepSeek)
    if config.base_url.is_some() {
        builder = builder.additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));
    }

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

pub fn build_ollama_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");
    let client = <rig::providers::ollama::Client<reqwest::Client>>::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Ollama client init failed: {e}")))?;
    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(50)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

/// Helper to finish building a streaming agent: apply common settings, register tools, build.
fn finish_streaming_agent<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilder<M>,
    preamble: &str,
    config: &LlmConfig,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<M>> {
    let builder = builder
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(50)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools(builder, jail_root, safe_mode);

    Ok(builder.build())
}

/// Streaming agent for Anthropic with safe_mode + prompt caching.
pub fn build_anthropic_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>".into()))?;
    let client = <rig::providers::anthropic::client::Client<reqwest::Client>>::builder()
        .api_key(api_key)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Anthropic client init failed: {e}")))?;
    // Enable prompt caching — Anthropic caches the system prompt prefix,
    // saving ~90% on input tokens for cached turns. The preamble is kept
    // pure static (no dynamic context) specifically to maximize cache hits.
    let model = rig::providers::anthropic::completion::CompletionModel::new(client, &config.model)
        .with_prompt_caching();

    finish_streaming_agent(AgentBuilder::new(model), preamble, config, jail_root, safe_mode)
}

/// Streaming agent for OpenAI with safe_mode.
pub fn build_openai_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into()))?;
    let client: rig::providers::openai::client::Client = match &config.base_url {
        Some(custom_url) => {
            <rig::providers::openai::client::Client<reqwest::Client>>::builder()
                .api_key(api_key)
                .base_url(custom_url)
                .http_client((**http_client).clone())
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
        None => {
            <rig::providers::openai::client::Client<reqwest::Client>>::builder()
                .api_key(api_key)
                .http_client((**http_client).clone())
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
    };
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    // When using a custom base_url (e.g. DeepSeek), disable thinking mode
    // to prevent reasoning_content from being returned. DeepSeek V4 Flash
    // defaults to Think mode, which returns reasoning_content that must be
    // passed back on subsequent turns — but rig's OpenAI handler drops it,
    // causing API 400 errors.
    let builder = if config.base_url.is_some() {
        AgentBuilder::new(model)
            .additional_params(serde_json::json!({
                "thinking": {"type": "disabled"}
            }))
    } else {
        AgentBuilder::new(model)
    };

    finish_streaming_agent(builder, preamble, config, jail_root, safe_mode)
}

/// Streaming agent for Ollama with safe_mode.
pub fn build_ollama_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");
    let client = <rig::providers::ollama::Client<reqwest::Client>>::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Ollama client init failed: {e}")))?;
    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    finish_streaming_agent(AgentBuilder::new(model), preamble, config, jail_root, safe_mode)
}

pub fn role_label(role: &LlmChatRole) -> &'static str {
    match role {
        LlmChatRole::System => "System",
        LlmChatRole::User => "User",
        LlmChatRole::Assistant => "Assistant",
    }
}

/// Extract text content from UserContent.
pub fn extract_text_from_user_content(content: &rig::OneOrMany<rig::message::UserContent>) -> String {
    content.iter().filter_map(|item| {
        if let rig::message::UserContent::Text(text) = item {
            Some(text.text.clone())
        } else {
            None
        }
    }).collect::<Vec<_>>().join("\n")
}

/// Extract text content from AssistantContent.
pub fn extract_text_from_assistant_content(content: &rig::OneOrMany<rig::message::AssistantContent>) -> String {
    content.iter().filter_map(|item| {
        if let rig::message::AssistantContent::Text(text) = item {
            Some(text.text.clone())
        } else {
            None
        }
    }).collect::<Vec<_>>().join("\n")
}
