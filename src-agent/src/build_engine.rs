//! Agent engine initialization — extracted from main.rs.

use std::sync::Arc;
use tracing::info;

use crate::agent::Agent;
use crate::db::TaskRepo;
use crate::embedding::EmbeddingService;
use crate::mcp::McpToolExecutor;
use crate::safety::SafetyContext;

pub async fn build_engine(
    db_path: &str,
) -> anyhow::Result<(
    Arc<TaskRepo>,
    Agent,
    std::sync::Arc<dyn crate::agent::ToolExecutor>,
)> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    // Use default safety context (config is now integrated into the main config)
    let safety_ctx = SafetyContext::default();

    // Both the Agent and the Arc-wrapped ToolExecutor share the same underlying
    // McpToolExecutor instance (Clone shares the Arc<RwLock<registry>>).
    // The Arc copy is used by AgentUiBridge for direct approval-time tool execution.
    let mcp_executor = McpToolExecutor::with_safety(safety_ctx.clone());
    let tool_executor: std::sync::Arc<dyn crate::agent::ToolExecutor> =
        std::sync::Arc::new(mcp_executor.clone());
    let tool_executor_arc = std::sync::Arc::clone(&tool_executor);

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
    let mut embedding_config: Option<(crate::llm::LlmProvider, String)> = None;

    for provider in &provider_list {
        if let Some(api_key) = repo.get_setting(&format!("api_key.{}", provider)).await? {
            let llm_provider = match *provider {
                "anthropic" => crate::llm::LlmProvider::Anthropic,
                "openai" | "deepseek" => crate::llm::LlmProvider::OpenAI,
                "ollama" => crate::llm::LlmProvider::Ollama,
                _ => continue,
            };
            let mut cfg = crate::llm::LlmConfig::new(llm_provider.clone(), Some(api_key.clone()));
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
            // Save embedding config before passing cfg to gateway
            embedding_config = Some((llm_provider, api_key));
            let gateway = crate::llm::LlmGateway::with_http_client(
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

    // Initialize embedding service for vector/hybrid search
    if let Some((llm_provider, api_key)) = embedding_config {
        let embedding_cfg = crate::llm::LlmConfig::new(llm_provider, Some(api_key));
        match EmbeddingService::new(&embedding_cfg, &agent.http_client) {
            Ok(svc) => {
                info!(
                    provider = %svc.provider(),
                    dimension = svc.dimension(),
                    "embedding service initialized"
                );
                agent = agent.with_embedding_service(svc);
            }
            Err(e) => {
                info!(
                    "embedding service not available: {} (vector search disabled)",
                    e
                );
            }
        }
    }

    Ok((repo, agent, tool_executor_arc))
}
