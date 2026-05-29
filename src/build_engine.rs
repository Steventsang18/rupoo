//! Agent engine initialization — extracted from main.rs.

use std::sync::Arc;
use tracing::info;

use rupoo::safety::SafetyContext;
use rupoo::agent::Agent;
use rupoo::db::TaskRepo;
use rupoo::mcp::McpToolExecutor;

pub async fn build_engine(db_path: &str) -> anyhow::Result<(
    Arc<TaskRepo>,
    Agent,
    std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
)> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    // Load safety configuration from file if present
    let safety_ctx = {
        let config_path = std::path::Path::new("rupoo-config.toml");
        if config_path.exists() {
            SafetyContext::from_config(config_path)
        } else {
            SafetyContext::default()
        }
    };

    // Both the Agent and the Arc-wrapped ToolExecutor share the same underlying
    // McpToolExecutor instance (Clone shares the Arc<RwLock<registry>>).
    // The Arc copy is used by AgentUiBridge for direct approval-time tool execution.
    let mcp_executor = McpToolExecutor::with_safety(safety_ctx.clone());
    let tool_executor: Box<dyn rupoo::agent::ToolExecutor> =
        Box::new(mcp_executor.clone());
    let tool_executor_arc: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>> =
        std::sync::Arc::new(Box::new(mcp_executor));

    let jail_root = safety_ctx.jail_root().map(|p| p.to_path_buf());
    let mut agent = Agent::new(Arc::clone(&repo), tool_executor);
    agent.safety_ctx = safety_ctx;

    if let Some(api_key) = repo.get_setting("api_key.anthropic").await? {
        let mut cfg = rupoo::llm::LlmConfig::new(
            rupoo::llm::LlmProvider::Anthropic,
            Some(api_key),
        );
        if let Some(model) = repo.get_setting("model.anthropic").await? {
            cfg.model = model;
        }
        let gateway = if let Some(ref root) = jail_root {
            rupoo::llm::LlmGateway::with_jail(cfg, root.clone())
        } else {
            rupoo::llm::LlmGateway::new(cfg)
        };
        agent = agent.with_llm(gateway);
        info!("Anthropic LLM configured");
    } else if let Some(api_key) = repo.get_setting("api_key.openai").await? {
        let mut cfg = rupoo::llm::LlmConfig::new(
            rupoo::llm::LlmProvider::OpenAI,
            Some(api_key),
        );
        if let Some(model) = repo.get_setting("model.openai").await? {
            cfg.model = model;
        }
        if let Some(base_url) = repo.get_setting("base_url.openai").await? {
            cfg.base_url = Some(base_url);
        }
        let gateway = if let Some(ref root) = jail_root {
            rupoo::llm::LlmGateway::with_jail(cfg, root.clone())
        } else {
            rupoo::llm::LlmGateway::new(cfg)
        };
        agent = agent.with_llm(gateway);
        info!("OpenAI-compatible LLM configured");
    } else if let Some(api_key) = repo.get_setting("api_key.deepseek").await? {
        let mut cfg = rupoo::llm::LlmConfig::new(
            rupoo::llm::LlmProvider::DeepSeek,
            Some(api_key),
        );
        if let Some(model) = repo.get_setting("model.deepseek").await? {
            cfg.model = model;
        }
        if let Some(base_url) = repo.get_setting("base_url.deepseek").await? {
            cfg.base_url = Some(base_url);
        }
        let gateway = if let Some(ref root) = jail_root {
            rupoo::llm::LlmGateway::with_jail(cfg, root.clone())
        } else {
            rupoo::llm::LlmGateway::new(cfg)
        };
        agent = agent.with_llm(gateway);
        info!("DeepSeek LLM configured");
    } else {
        info!("no LLM configured, using dummy think output");
    }

    Ok((repo, agent, tool_executor_arc))
}
