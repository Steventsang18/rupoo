//! Per-provider agent builders for LLM Gateway.

use std::sync::Arc;

use crate::error::{AgentError, AgentResult};
use crate::llm::history::LlmChatRole;
use crate::llm::LlmConfig;

// Magic number constants
const DEFAULT_MAX_TURNS: usize = 50;

pub fn build_anthropic_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>"
                .into(),
        )
    })?;
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
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ));

    Ok(builder.build())
}

pub fn build_openai_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into(),
        )
    })?;
    let client: rig::providers::openai::client::Client = match &config.base_url {
        Some(custom_url) => <rig::providers::openai::client::Client<reqwest::Client>>::builder()
            .api_key(api_key)
            .base_url(custom_url)
            .http_client((**http_client).clone())
            .build()
            .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?,
        None => <rig::providers::openai::client::Client<reqwest::Client>>::builder()
            .api_key(api_key)
            .http_client((**http_client).clone())
            .build()
            .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?,
    };
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    let mut builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ));

    // Disable thinking mode for custom base_url (e.g. DeepSeek)
    if config.base_url.is_some() {
        builder = builder.additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));
    }

    Ok(builder.build())
}

pub fn build_deepseek_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "DeepSeek requires an API key. Set it via: rupoo config set api_key.deepseek <key>"
                .into(),
        )
    })?;
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("https://api.deepseek.com");
    let client = <rig::providers::openai::client::Client<reqwest::Client>>::builder()
        .api_key(api_key)
        .base_url(base_url)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("DeepSeek client init failed: {e}")))?;
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ))
        // DeepSeek: disable thinking mode to avoid reasoning_content issues
        .additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));

    Ok(builder.build())
}

pub fn build_ollama_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:11434");
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
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ));

    Ok(builder.build())
}

pub fn build_gemini_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::gemini::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "Gemini requires an API key. Set it via: rupoo config set api_key.gemini <key>".into(),
        )
    })?;
    let client = <rig::providers::gemini::Client<reqwest::Client>>::builder()
        .api_key(api_key.to_string())
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Gemini client init failed: {e}")))?;
    let model = rig::providers::gemini::CompletionModel::new(client, &config.model);

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ));

    Ok(builder.build())
}
fn finish_streaming_agent<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilder<M>,
    preamble: &str,
    config: &LlmConfig,
    jail_root: Option<&std::path::Path>,
    _safe_mode: bool,
) -> AgentResult<rig::agent::Agent<M>> {
    let builder = builder
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(DEFAULT_MAX_TURNS)
        .tools(crate::rig_tools::build_boxed_tools(
            jail_root.map(|p| p.to_path_buf()),
        ));

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

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>"
                .into(),
        )
    })?;
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

    finish_streaming_agent(
        AgentBuilder::new(model),
        preamble,
        config,
        jail_root,
        safe_mode,
    )
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

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into(),
        )
    })?;
    let client: rig::providers::openai::client::Client = match &config.base_url {
        Some(custom_url) => <rig::providers::openai::client::Client<reqwest::Client>>::builder()
            .api_key(api_key)
            .base_url(custom_url)
            .http_client((**http_client).clone())
            .build()
            .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?,
        None => <rig::providers::openai::client::Client<reqwest::Client>>::builder()
            .api_key(api_key)
            .http_client((**http_client).clone())
            .build()
            .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?,
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
        AgentBuilder::new(model).additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }))
    } else {
        AgentBuilder::new(model)
    };

    finish_streaming_agent(builder, preamble, config, jail_root, safe_mode)
}

/// Streaming agent for DeepSeek with safe_mode.
/// Uses the OpenAI-compatible API with thinking mode disabled.
pub fn build_deepseek_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "DeepSeek requires an API key. Set it via: rupoo config set api_key.deepseek <key>"
                .into(),
        )
    })?;
    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("https://api.deepseek.com");
    let client = <rig::providers::openai::client::Client<reqwest::Client>>::builder()
        .api_key(api_key)
        .base_url(base_url)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("DeepSeek client init failed: {e}")))?;
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    // DeepSeek: disable thinking mode to avoid reasoning_content issues
    let builder = AgentBuilder::new(model).additional_params(serde_json::json!({
        "thinking": {"type": "disabled"}
    }));

    finish_streaming_agent(builder, preamble, config, jail_root, safe_mode)
}

/// Streaming agent for Gemini with safe_mode.
pub fn build_gemini_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::gemini::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref().ok_or_else(|| {
        AgentError::Config(
            "Gemini requires an API key. Set it via: rupoo config set api_key.gemini <key>".into(),
        )
    })?;
    let client = <rig::providers::gemini::Client<reqwest::Client>>::builder()
        .api_key(api_key.to_string())
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Gemini client init failed: {e}")))?;
    let model = rig::providers::gemini::CompletionModel::new(client, &config.model);

    finish_streaming_agent(
        AgentBuilder::new(model),
        preamble,
        config,
        jail_root,
        safe_mode,
    )
}

/// Helper to finish building a streaming agent: apply common settings, register tools, build.
pub fn build_ollama_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
    http_client: &Arc<reqwest::Client>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:11434");
    let client = <rig::providers::ollama::Client<reqwest::Client>>::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .http_client((**http_client).clone())
        .build()
        .map_err(|e| AgentError::Llm(format!("Ollama client init failed: {e}")))?;
    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    finish_streaming_agent(
        AgentBuilder::new(model),
        preamble,
        config,
        jail_root,
        safe_mode,
    )
}

pub fn role_label(role: &LlmChatRole) -> &'static str {
    match role {
        LlmChatRole::System => "System",
        LlmChatRole::User => "User",
        LlmChatRole::Assistant => "Assistant",
    }
}

/// Extract text content from UserContent.
pub fn extract_text_from_user_content(
    content: &rig::OneOrMany<rig::message::UserContent>,
) -> String {
    content
        .iter()
        .filter_map(|item| {
            if let rig::message::UserContent::Text(text) = item {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract text content from AssistantContent.
pub fn extract_text_from_assistant_content(
    content: &rig::OneOrMany<rig::message::AssistantContent>,
) -> String {
    content
        .iter()
        .filter_map(|item| {
            if let rig::message::AssistantContent::Text(text) = item {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
