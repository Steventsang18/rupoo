//! Agent engine initialization — extracted from main.rs.
//!
//! Configuration priority: config.toml → credentials.toml → env vars → DB settings (fallback).

use std::sync::Arc;
use tracing::info;

use rupoo::config::RupooConfig;
use rupoo::safety::SafetyContext;
use rupoo::agent::Agent;
use rupoo::db::TaskRepo;
use rupoo::mcp::McpToolExecutor;

pub async fn build_engine(db_path: &str) -> anyhow::Result<(
    Arc<TaskRepo>,
    Agent,
    std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
    Option<rupoo::llm::router::LlmRouter>,
)> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    // ── Load configuration: config.toml first, fallback to defaults ──
    let config = match RupooConfig::load() {
        Ok(c) => {
            info!("loaded config from ~/.rupoo/config.toml");
            c
        }
        Err(e) => {
            info!("config load failed ({}), using defaults", e);
            RupooConfig::default()
        }
    };

    // ── Safety context from config ──
    let safety_ctx = if config.safety.jail_root.is_empty() || config.safety.jail_root == "." {
        SafetyContext::default()
    } else {
        // Try loading from config path if it has custom safety settings
        let config_path = rupoo::config::data_dir().join("config.toml");
        if config_path.exists() {
            SafetyContext::from_config(&config_path)
        } else {
            SafetyContext::default()
        }
    };

    // ── MCP Tool Executor ──
    let mcp_executor = McpToolExecutor::with_safety(safety_ctx.clone());
    let tool_executor: Box<dyn rupoo::agent::ToolExecutor> =
        Box::new(mcp_executor.clone());
    let tool_executor_arc: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>> =
        std::sync::Arc::new(Box::new(mcp_executor));

    let jail_root = safety_ctx.jail_root().map(|p| p.to_path_buf());

    // ── Build agent with approval policy from config ──
    let mut agent = Agent::new(Arc::clone(&repo), tool_executor)
        .with_approval_policy_name(&config.safety.approval_policy);
    agent.safety_ctx = safety_ctx;

    // ── LLM configuration: config.toml → credentials.toml → env → DB fallback ──
    let mut llm_configured = false;

    // 1. Determine active provider: DB override > config.toml > default("ollama")
    //    DB override is important for existing users who configured via /model command
    let db_active_provider: Option<String> = repo.get_setting("active_provider").await
        .ok()
        .flatten();
    let active_provider = db_active_provider
        .as_ref()
        .cloned()
        .unwrap_or_else(|| config.llm.active_provider.clone());

    // Build provider priority list: active first, then fallback, then remaining
    let mut provider_list = vec![active_provider.clone()];
    if let Some(ref fb) = config.llm.fallback_provider {
        if *fb != active_provider {
            provider_list.push(fb.clone());
        }
    }
    // Add remaining known providers not already in the list
    for p in &["anthropic", "openai", "deepseek", "ollama"] {
        if !provider_list.iter().any(|x| x == *p) {
            provider_list.push((*p).to_string());
        }
    }

    for provider in &provider_list {
        // Resolve API key: credentials.toml → env → config → DB
        let api_key_from_config = config.resolve_api_key(provider).await;

        // Also try DB for API key (async, need to await separately)
        let api_key = match api_key_from_config {
            Some(k) => Some(k),
            None => {
                match repo.get_setting(&format!("api_key.{}", provider)).await {
                    Ok(Some(k)) => Some(k),
                    _ => None,
                }
            }
        };

        // Check if provider has config in config.toml
        let provider_config = config.llm.providers.get(provider);

        let llm_provider = match provider.as_str() {
            "anthropic" => rupoo::llm::LlmProvider::Anthropic,
            "openai" | "deepseek" => rupoo::llm::LlmProvider::OpenAI,
            "ollama" => rupoo::llm::LlmProvider::Ollama,
            _ => continue,
        };

        // Ollama doesn't require an API key
        let needs_key = !matches!(provider.as_str(), "ollama");
        if needs_key && api_key.is_none() {
            continue;
        }

        let mut cfg = rupoo::llm::LlmConfig::new(llm_provider, api_key);

        // Model: config.toml → DB fallback
        if let Some(pc) = provider_config {
            if let Some(ref model) = pc.model {
                cfg.model = model.clone();
            }
            if let Some(ref base_url) = pc.base_url {
                cfg.base_url = Some(base_url.clone());
            }
        } else {
            // Fallback to DB settings
            if let Ok(Some(model)) = repo.get_setting(&format!("model.{}", provider)).await {
                cfg.model = model;
            }
            if let Ok(Some(base_url)) = repo.get_setting(&format!("base_url.{}", provider)).await {
                cfg.base_url = Some(base_url);
            }
        }

        // DeepSeek uses OpenAI-compatible API with official base_url
        if provider == "deepseek" && cfg.base_url.is_none() {
            cfg.base_url = Some("https://api.deepseek.com".to_string());
        }

        let gateway = if let Some(ref root) = jail_root {
            rupoo::llm::LlmGateway::with_jail(cfg, root.clone())
        } else {
            rupoo::llm::LlmGateway::new(cfg)
        };

        agent = agent.with_llm(gateway);
        info!("{} LLM configured (via config priority chain)", provider);
        llm_configured = true;
        break;
    }

    if !llm_configured {
        info!("no LLM configured, using dummy think output");
    }

    // ── Build LlmRouter for intent-driven chat routing ──
    let llm_router = if llm_configured {
        // Sync active_provider from DB override into config so router uses the same provider
        let mut router_config = config;
        if db_active_provider.is_some() {
            router_config.llm.active_provider = active_provider.clone();
        }
        let router = if let Some(ref root) = jail_root {
            rupoo::llm::router::LlmRouter::with_jail(router_config, root.clone())
        } else {
            rupoo::llm::router::LlmRouter::new(router_config)
        };
        Some(router)
    } else {
        None
    };

    Ok((repo, agent, tool_executor_arc, llm_router))
}
