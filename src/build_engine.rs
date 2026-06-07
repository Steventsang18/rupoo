//! Agent engine initialization — extracted from main.rs.

use std::sync::Arc;
use tracing::info;

use rupoo::agent::Agent;
use rupoo::db::TaskRepo;
use rupoo::mcp::McpToolExecutor;
use rupoo::safety::SafetyContext;

pub async fn build_engine(
    db_path: &str,
) -> anyhow::Result<(
    Arc<TaskRepo>,
    Agent,
    std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
)> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    // Load safety configuration from file if present
    let safety_ctx = {
        // Priority: ~/.rupoo/rupoo-config.toml > ./rupoo-config.toml
        let home_dir = std::env::var("HOME").unwrap_or_default();
        let home_config = std::path::Path::new(&home_dir)
            .join(".rupoo")
            .join("rupoo-config.toml");
        let cwd_config = std::path::Path::new("rupoo-config.toml");

        if home_config.exists() {
            info!(path = %home_config.display(), "loading safety config from home directory");
            SafetyContext::from_config(&home_config)
        } else if cwd_config.exists() {
            info!(path = %cwd_config.display(), "loading safety config from current directory");
            SafetyContext::from_config(cwd_config)
        } else {
            SafetyContext::default()
        }
    };

    // Both the Agent and the Arc-wrapped ToolExecutor share the same underlying
    // McpToolExecutor instance (Clone shares the Arc<RwLock<registry>>).
    // The Arc copy is used by AgentUiBridge for direct approval-time tool execution.
    let mcp_executor = McpToolExecutor::with_safety(safety_ctx.clone());
    let tool_executor: Box<dyn rupoo::agent::ToolExecutor> = Box::new(mcp_executor.clone());
    let tool_executor_arc: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>> =
        std::sync::Arc::new(Box::new(mcp_executor));

    let jail_root = safety_ctx.jail_root().map(|p| p.to_path_buf());
    let mut agent = Agent::new(Arc::clone(&repo), tool_executor);
    agent.safety_ctx = safety_ctx;

    // Check active_provider first (set by /model switch), then fall back to priority order
    let active_provider: Option<String> = repo.get_setting("active_provider").await?;

    let provider_list = if let Some(ref ap) = active_provider {
        // Try active provider first, then fall back to others as backup
        let mut list = vec![ap.as_str()];
        for p in &["anthropic", "openai", "deepseek", "ollama"] {
            if *p != ap.as_str() {
                list.push(p);
            }
        }
        list
    } else {
        vec!["anthropic", "openai", "deepseek", "ollama"]
    };

    let mut llm_configured = false;
    for provider in &provider_list {
        if let Some(api_key) = repo.get_setting(&format!("api_key.{}", provider)).await? {
            let llm_provider = match *provider {
                "anthropic" => rupoo::llm::LlmProvider::Anthropic,
                "openai" | "deepseek" => rupoo::llm::LlmProvider::OpenAI,
                "ollama" => rupoo::llm::LlmProvider::Ollama,
                _ => continue,
            };
            let mut cfg = rupoo::llm::LlmConfig::new(llm_provider, Some(api_key));
            if let Some(model) = repo.get_setting(&format!("model.{}", provider)).await? {
                cfg.model = model;
            }
            // DeepSeek uses OpenAI-compatible API with official base_url
            if *provider == "deepseek" && cfg.base_url.is_none() {
                cfg.base_url = Some("https://api.deepseek.com".to_string());
            }
            if let Some(base_url) = repo.get_setting(&format!("base_url.{}", provider)).await? {
                cfg.base_url = Some(base_url);
            }
            let gateway = rupoo::llm::LlmGateway::with_http_client(
                cfg,
                jail_root.clone(),
                agent.http_client.clone(),
            );
            agent = agent.with_llm(gateway);
            info!("{} LLM configured", provider);
            llm_configured = true;
            break;
        }
    }
    if !llm_configured {
        info!("no LLM configured, using dummy think output");
    }

    Ok((repo, agent, tool_executor_arc))
}
