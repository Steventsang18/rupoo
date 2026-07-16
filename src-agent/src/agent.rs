use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::{debug, error, info, warn};

use crate::context::ConversationContext;
use crate::db::TaskRepo;
use crate::embedding::EmbeddingService;
use crate::error::{AgentError, AgentResult};
use crate::llm::{AgentEvent, ConversationHistory, LlmGateway, TokenUsage};
use crate::memory::{HybridSearchConfig, MemoryStore, MemorySystemBridge};
use crate::memory_cache::MemoryCache;
use crate::tool_selector::{ToolRegistry, ToolUsageTracker};

use crate::task::{
    Checkpoint, CheckpointStatus, McpToolResult, MemoryEntry, Plan, PlanStatus, Step, StepStatus,
};

use crate::safety::SafetyContext;

/// Result of running a single step.
#[derive(Debug)]
pub enum StepOutcome {
    /// Step executed successfully; continue to next.
    Advanced,
    /// Plan is fully finished.
    Finished,
    /// Agent is waiting for human input.
    WaitingForInput(String),
    /// Tool call requires user approval before execution.
    /// Bridge should call store_pending_plan then break the loop.
    RequiresApproval {
        tool_name: String,
        params: serde_json::Value,
        step_index: usize,
    },
    /// Step failed (fatal for the plan).
    Failed(String),
}

// ---------------------------------------------------------------------------
// Tool executor trait — allows plugging in different tool backends
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult>;

    /// Execute multiple tools in parallel.
    /// Returns a vector of results in the same order as the input.
    async fn execute_tools_parallel(
        &self,
        tool_calls: Vec<(String, serde_json::Value)>,
    ) -> Vec<AgentResult<McpToolResult>> {
        // Use Arc to share string references across async tasks
        let tool_calls: Vec<_> = tool_calls
            .into_iter()
            .map(|(name, params)| (Arc::new(name), params))
            .collect();

        // Spawn all tasks concurrently
        let tasks: Vec<_> = tool_calls
            .into_iter()
            .map(|(name, params)| {
                let executor = self as &Self;
                async move { executor.execute_tool(&name, params).await }
            })
            .collect();

        // Wait for all tasks to complete
        futures::future::join_all(tasks).await
    }
}

/// Dummy tool executor for testing — echoes back the params as result.
pub struct DummyToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for DummyToolExecutor {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult> {
        let content = format!(
            "dummy execute '{}' with params: {}",
            tool_name,
            serde_json::to_string_pretty(&params).unwrap_or_default()
        );
        Ok(McpToolResult::Success { content })
    }
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

pub struct Agent {
    repo: Arc<TaskRepo>,
    memory_cache: std::sync::Arc<MemoryCache>,
    /// Full memory store with hybrid search support
    memory_store: std::sync::Arc<MemoryStore>,
    /// Embedding service for vector search
    embedding_service: Option<std::sync::Arc<EmbeddingService>>,
    /// Memory feature enabled flag
    memory_enabled: AtomicBool,
    /// Hybrid search (deep search) enabled flag
    hybrid_search_enabled: AtomicBool,
    pub tool_executor: std::sync::Arc<dyn ToolExecutor>,
    llm_gateway: Option<LlmGateway>,
    pub safety_ctx: SafetyContext,
    /// Cached system prompt to avoid re-reading files on every Think step.
    cached_system_prompt: std::sync::Mutex<Option<String>>,
    /// Token usage from the most recent chat() call.
    /// Uses Mutex for interior mutability (Cell is not Sync).
    last_usage: std::sync::Mutex<Option<TokenUsage>>,
    /// Cancellation flag. Set to true to abort the running plan at the next step.
    cancelled: std::sync::atomic::AtomicBool,
    /// Shared HTTP client for connection pooling (reqwest, tools, LLM providers).
    pub http_client: std::sync::Arc<reqwest::Client>,
    /// Plan cache for storing and reusing generated plans
    plan_cache: std::sync::Arc<PlanCache>,
    /// Unified conversation context for environment + intent + memory + behavior.
    conversation_context: std::sync::Mutex<ConversationContext>,
    /// Tool registry for intelligent tool selection and scoring.
    tool_registry: ToolRegistry,
    /// Tool usage tracker for adaptive optimization.
    tool_usage_tracker: std::sync::Arc<ToolUsageTracker>,
    /// Loop engine for adaptive iterative execution (Loop Engineering).
    /// Shared via Arc so the lock can be released before long-running awaits.
    pub loop_engine: Option<std::sync::Arc<crate::loop_engine::LoopEngine>>,
    /// Trait-based memory system bridge (shared with Orchestrator).
    pub memory_system: std::sync::Arc<MemorySystemBridge>,
    /// Source tag for stored memories — "agent" for CLI, "feishu" for Feishu.
    pub memory_source: String,
}

// ---------------------------------------------------------------------------
// Plan Cache - LRU cache for storing generated plans
// ---------------------------------------------------------------------------

use lru::LruCache;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Cache entry for a generated plan
#[derive(Debug, Clone)]
pub struct CachedPlan {
    pub steps: Vec<crate::llm::StepSpec>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub task_hash: String,
}

/// Plan cache configuration
#[derive(Debug, Clone)]
pub struct PlanCacheConfig {
    pub capacity: usize,
    pub ttl_seconds: u64,
}

impl Default for PlanCacheConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            ttl_seconds: 3600, // 1 hour default TTL
        }
    }
}

/// Thread-safe LRU cache for storing generated plans
pub struct PlanCache {
    cache: std::sync::RwLock<LruCache<String, CachedPlan>>,
    config: PlanCacheConfig,
}

impl PlanCache {
    pub fn new(config: PlanCacheConfig) -> Self {
        Self {
            cache: std::sync::RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(config.capacity).unwrap_or(std::num::NonZeroUsize::MIN),
            )),
            config,
        }
    }

    /// Generate a cache key from task input using simple hashing
    fn generate_key(task: &str, context: Option<&str>) -> String {
        let mut hasher = DefaultHasher::new();
        task.hash(&mut hasher);
        if let Some(ctx) = context {
            ctx.hash(&mut hasher);
        }
        let hash = hasher.finish();
        format!("{:016x}", hash)
    }

    /// Check if a plan exists in cache and is valid
    pub fn get(&self, task: &str, context: Option<&str>) -> Option<Vec<crate::llm::StepSpec>> {
        let key = Self::generate_key(task, context);
        let mut cache = self.cache.write().ok()?;

        if let Some(cached) = cache.get(&key) {
            // Check TTL
            let now = chrono::Utc::now();
            let age = now.signed_duration_since(cached.created_at).num_seconds() as u64;
            if age < self.config.ttl_seconds {
                debug!(key = %key, age_secs = age, "plan cache hit");
                return Some(cached.steps.clone());
            } else {
                debug!(key = %key, age_secs = age, "plan cache expired");
            }
        }
        None
    }

    /// Store a plan in cache
    pub fn put(&self, task: &str, context: Option<&str>, steps: Vec<crate::llm::StepSpec>) {
        let key = Self::generate_key(task, context);
        let entry = CachedPlan {
            steps,
            created_at: chrono::Utc::now(),
            task_hash: key.clone(),
        };

        if let Ok(mut cache) = self.cache.write() {
            cache.put(key, entry);
            debug!("plan cached");
        }
    }

    /// Clear the entire cache
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
            info!("plan cache cleared");
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let cache = self.cache.read().ok();
        let len = cache.as_ref().map(|c| c.len()).unwrap_or(0);
        (len, self.config.capacity)
    }
}

impl Agent {
    pub fn new(repo: Arc<TaskRepo>, tool_executor: std::sync::Arc<dyn ToolExecutor>) -> Self {
        let memory_cache = std::sync::Arc::new(MemoryCache::new(Arc::clone(&repo), 64));
        let memory_store = std::sync::Arc::new(MemoryStore::new(Arc::clone(&repo)));
        let plan_cache = std::sync::Arc::new(PlanCache::new(PlanCacheConfig::default()));
        let conversation_context = std::sync::Mutex::new(ConversationContext::collect());
        let tool_registry = ToolRegistry::new();
        let tool_usage_tracker = std::sync::Arc::new(ToolUsageTracker::new());
        let loop_engine = Some(std::sync::Arc::new(crate::loop_engine::LoopEngine::new(
            Arc::clone(&repo),
            Arc::clone(&memory_cache),
            SafetyContext::default(),
        )));
        let memory_system = std::sync::Arc::new(MemorySystemBridge::new(Arc::clone(&repo)));
        Self {
            repo,
            memory_cache,
            memory_store,
            embedding_service: None,
            memory_enabled: AtomicBool::new(true),
            hybrid_search_enabled: AtomicBool::new(false),
            tool_executor,
            llm_gateway: None,
            safety_ctx: SafetyContext::default(),
            cached_system_prompt: std::sync::Mutex::new(None),
            last_usage: std::sync::Mutex::new(None),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            http_client: crate::http_client::HTTP_CLIENT.clone(),
            plan_cache,
            conversation_context,
            tool_registry,
            tool_usage_tracker,
            loop_engine,
            memory_system,
            memory_source: "agent".to_string(),
        }
    }

    /// Return token usage from the most recent think step, if available.
    pub fn last_usage(&self) -> Option<TokenUsage> {
        self.last_usage.lock().ok().and_then(|g| *g)
    }

    /// Request cancellation of the currently running plan.
    /// The agent will abort at the next step boundary.
    pub fn request_cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Reset the cancellation flag (e.g., before starting a new plan).
    pub fn reset_cancel(&self) {
        self.cancelled
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Return a reference to the task repository.
    pub fn repo(&self) -> &std::sync::Arc<TaskRepo> {
        &self.repo
    }

    /// Return a reference to the tool executor (used by AgentUiBridge for
    /// direct approval-time tool execution, bypassing needs_approval checks).
    pub fn get_tool_executor(&self) -> &std::sync::Arc<dyn ToolExecutor> {
        &self.tool_executor
    }

    /// Attach an LLM gateway so Think steps produce real LLM responses.
    pub fn with_llm(mut self, gateway: LlmGateway) -> Self {
        self.llm_gateway = Some(gateway);
        self
    }

    /// Get a reference to the LLM gateway if available.
    pub fn llm_gateway_ref(&self) -> Option<&LlmGateway> {
        self.llm_gateway.as_ref()
    }

    /// Check if LLM is configured.
    pub fn has_llm(&self) -> bool {
        self.llm_gateway.is_some()
    }

    // ------------------------------------------------------------------
    // Plan Cache Management
    // ------------------------------------------------------------------

    /// Get a cached plan for the given task.
    pub fn get_cached_plan(
        &self,
        task: &str,
        context: Option<&str>,
    ) -> Option<Vec<crate::llm::StepSpec>> {
        self.plan_cache.get(task, context)
    }

    /// Store a plan in cache.
    pub fn cache_plan(&self, task: &str, context: Option<&str>, steps: Vec<crate::llm::StepSpec>) {
        self.plan_cache.put(task, context, steps);
    }

    /// Clear the plan cache.
    pub fn clear_plan_cache(&self) {
        self.plan_cache.clear();
    }

    /// Get plan cache statistics.
    pub fn plan_cache_stats(&self) -> (usize, usize) {
        self.plan_cache.stats()
    }

    /// Access the unified conversation context.
    /// Returns a MutexGuard for thread-safe read/write access.
    pub fn context(&self) -> std::sync::MutexGuard<'_, ConversationContext> {
        self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        })
    }

    /// Reset the conversation context (start a new conversation).
    pub fn reset_context(&self) {
        let mut ctx = self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        });
        ctx.reset();
        info!("conversation context reset");
    }

    /// Record a user message in the conversation context.
    pub fn record_user_message(&self, content: &str) {
        let mut ctx = self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        });
        ctx.record_user_message(content);
    }

    /// Record an assistant response in the conversation context.
    pub fn record_assistant_response(&self, content: &str) {
        let mut ctx = self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        });
        ctx.record_assistant_response(content);
    }

    /// Record a tool call in the conversation context.
    pub fn record_tool_call(&self, tool_name: &str) {
        let mut ctx = self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        });
        ctx.record_tool_call(tool_name);
    }

    /// Inject memory context for the current conversation turn.
    pub fn inject_memory_context(&self, memories: &[MemoryEntry]) {
        let mut ctx = self.conversation_context.lock().unwrap_or_else(|e| {
            warn!("agent conversation_context lock poisoned, recovering");
            e.into_inner()
        });
        *ctx = std::mem::take(&mut *ctx).with_memories(memories.to_vec());
    }

    /// Get the current context block for the system prompt.
    pub fn get_context_block(&self) -> String {
        self.conversation_context
            .lock()
            .unwrap_or_else(|e| {
                warn!("agent conversation_context lock poisoned, recovering");
                e.into_inner()
            })
            .to_system_context_block()
    }

    /// Get a reference to the tool registry.
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Get a reference to the tool usage tracker.
    pub fn tool_usage_tracker(&self) -> &std::sync::Arc<ToolUsageTracker> {
        &self.tool_usage_tracker
    }

    /// Record tool execution result for adaptive optimization.
    pub fn record_tool_result(
        &self,
        tool_name: &str,
        success: bool,
        duration_ms: u64,
        output_tokens: usize,
    ) {
        self.tool_usage_tracker
            .record(tool_name, success, duration_ms, output_tokens);
    }

    /// Recommend better tool alternatives based on task context.
    pub fn recommend_tool_alternatives(
        &self,
        tool_name: &str,
        task: &str,
    ) -> Vec<(&'static str, f64)> {
        self.tool_registry.recommend_alternatives(tool_name, task)
    }

    /// Get tool effectiveness summary for context injection.
    pub fn tool_effectiveness_summary(&self) -> String {
        self.tool_usage_tracker.summary()
    }

    // ------------------------------------------------------------------
    // Memory Management
    // ------------------------------------------------------------------

    /// Enable or disable memory feature.
    pub fn set_memory_enabled(&self, enabled: bool) {
        self.memory_enabled.store(enabled, Ordering::SeqCst);
        info!(
            enabled = enabled,
            "memory feature {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// Check if memory feature is enabled.
    pub fn is_memory_enabled(&self) -> bool {
        self.memory_enabled.load(Ordering::SeqCst)
    }

    /// Enable or disable hybrid search (deep search) feature.
    pub fn set_hybrid_search_enabled(&self, enabled: bool) {
        self.hybrid_search_enabled.store(enabled, Ordering::SeqCst);
        info!(
            enabled = enabled,
            "hybrid search (deep search) feature {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// Check if hybrid search (deep search) feature is enabled.
    pub fn is_hybrid_search_enabled(&self) -> bool {
        self.hybrid_search_enabled.load(Ordering::SeqCst)
    }

    /// Attach embedding service for vector search.
    pub fn with_embedding_service(mut self, service: EmbeddingService) -> Self {
        self.embedding_service = Some(Arc::new(service));
        // Upgrade memory store to support hybrid search
        let config = HybridSearchConfig::default();
        self.memory_store = Arc::new(MemoryStore::with_hybrid_search(
            Arc::clone(&self.repo),
            self.embedding_service.clone(),
            config,
        ));
        info!("embedding service attached, hybrid search enabled");
        self
    }

    /// Store a memory entry with tags.
    pub async fn remember(&self, content: &str, tags: &[&str]) -> AgentResult<String> {
        if !self.is_memory_enabled() {
            return Err(AgentError::MemoryDisabled);
        }
        let id = self.memory_store.remember(content, tags).await?;
        // Invalidate cache to ensure fresh data is used
        self.memory_cache.invalidate().await;
        Ok(id)
    }

    /// Store a memory with explicit source.
    pub async fn remember_from(
        &self,
        content: &str,
        tags: &[&str],
        source: &str,
    ) -> AgentResult<String> {
        if !self.is_memory_enabled() {
            return Err(AgentError::MemoryDisabled);
        }
        let id = self
            .memory_store
            .remember_from(content, tags, source)
            .await?;
        self.memory_cache.invalidate().await;
        Ok(id)
    }

    /// Retrieve relevant memories using hybrid search.
    pub async fn recall(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        if !self.is_memory_enabled() {
            return Ok(Vec::new());
        }
        self.memory_store.recall(query, limit).await
    }

    /// Get recent memories.
    pub async fn recent_memories(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        if !self.is_memory_enabled() {
            return Ok(Vec::new());
        }
        self.memory_store.recent(limit).await
    }

    /// Get memory count.
    pub async fn memory_count(&self) -> AgentResult<usize> {
        self.memory_store.count().await
    }

    // ------------------------------------------------------------------
    // Hot LLM reconfiguration — switch provider/model at runtime
    // ------------------------------------------------------------------

    /// Switch the LLM provider/model at runtime.
    /// Reconfigures the LLM gateway based on DB settings or explicit args.
    pub async fn switch_llm(
        &mut self,
        provider: &str,
        model: Option<&str>,
        repo: &TaskRepo,
    ) -> AgentResult<String> {
        let api_key = repo
            .get_setting(&format!("api_key.{}", provider))
            .await
            .map_err(|e| AgentError::Config(format!("DB error: {}", e)))?
            .ok_or_else(|| AgentError::Config(format!("No API key for '{}'", provider)))?;

        let llm_provider = match provider {
            "anthropic" => crate::llm::LlmProvider::Anthropic,
            "openai" => crate::llm::LlmProvider::OpenAI,
            "ollama" => crate::llm::LlmProvider::Ollama,
            // 所有 OpenAI 兼容的国产模型都走 OpenAI 驱动
            "deepseek" | "qwen" | "glm" | "moonshot" | "yi" | "baichuan" | "minimax" | "spark" => {
                crate::llm::LlmProvider::OpenAI
            }
            _ => {
                return Err(AgentError::Config(format!(
                    "Unknown provider: '{}'",
                    provider
                )))
            }
        };

        let mut cfg = crate::llm::LlmConfig::new(llm_provider, Some(api_key));
        if let Some(m) = model {
            cfg.model = m.to_string();
        } else if let Ok(Some(m)) = repo.get_setting(&format!("model.{}", provider)).await {
            cfg.model = m;
        }
        // 国产大模型预设 base_url — 无需用户记忆地址
        let default_base_url = match provider {
            "deepseek" => Some("https://api.deepseek.com"),
            "qwen" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            "glm" => Some("https://open.bigmodel.cn/api/paas/v4"),
            "moonshot" => Some("https://api.moonshot.cn/v1"),
            "yi" => Some("https://api.lingyiwanwu.com/v1"),
            "baichuan" => Some("https://api.baichuan-ai.com/v1"),
            "minimax" => Some("https://api.minimax.chat/v1"),
            "spark" => Some("https://spark-api-open.xf-yun.com/v1"),
            _ => None,
        };
        if let Some(url) = default_base_url {
            if cfg.base_url.is_none() {
                cfg.base_url = Some(url.to_string());
            }
        }
        // Load base_url if explicitly configured (overrides default)
        if let Ok(Some(base_url)) = repo.get_setting(&format!("base_url.{}", provider)).await {
            cfg.base_url = Some(base_url);
        }

        let model_label = cfg.model.clone();

        let jail_root = self.safety_ctx.jail_root().map(|p| p.to_path_buf());
        let gateway =
            crate::llm::LlmGateway::with_http_client(cfg, jail_root, self.http_client.clone());

        self.llm_gateway = Some(gateway);
        self.cached_system_prompt = std::sync::Mutex::new(None); // invalidate cache on model switch
        let label = format!("{}/{}", provider, model_label);
        Ok(label)
    }

    /// Reload LLM configuration from database settings.
    /// Call this after `rupoo config set` to apply changes without restart.
    pub async fn reconfigure_from_db(&mut self, repo: &TaskRepo) -> AgentResult<String> {
        // Try providers in priority order
        for provider in &[
            "anthropic",
            "openai",
            "deepseek",
            "qwen",
            "glm",
            "moonshot",
            "yi",
            "baichuan",
            "minimax",
            "spark",
            "ollama",
        ] {
            if let Ok(Some(_api_key)) = repo.get_setting(&format!("api_key.{}", provider)).await {
                return self.switch_llm(provider, None, repo).await;
            }
        }
        // No LLM configured
        self.llm_gateway = None;
        Ok("no LLM configured".to_string())
    }

    // ------------------------------------------------------------------
    // Loop Engineering — adaptive iterative execution
    // ------------------------------------------------------------------

    /// Start a new loop with the given goal.
    /// Requires an LLM gateway to be configured.
    pub async fn start_loop(&self, goal: &str) -> AgentResult<crate::loop_engine::Loop> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        let llm = self.llm_gateway.as_ref();
        let config = crate::loop_engine::LoopConfig::default();
        let agent = Arc::new(self.try_clone_lightweight()?);
        // Engine takes &self — no lock held across await
        engine.start_loop(goal, config, agent, llm).await
    }

    /// Resume a paused/budget-exceeded loop.
    pub async fn resume_loop(&self, loop_id: &str) -> AgentResult<crate::loop_engine::Loop> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        let llm = self.llm_gateway.as_ref();
        let agent = Arc::new(self.try_clone_lightweight()?);
        engine.resume_loop(loop_id, agent, llm).await
    }

    /// Pause a running loop.
    pub async fn pause_loop(&self, loop_id: &str) -> AgentResult<()> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        engine.pause_loop(loop_id).await
    }

    /// Cancel a loop.
    pub async fn cancel_loop(&self, loop_id: &str) -> AgentResult<()> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        engine.cancel_loop(loop_id).await
    }

    /// Get the current status of a loop.
    pub async fn get_loop_status(&self, loop_id: &str) -> AgentResult<crate::loop_engine::Loop> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        engine.get_loop_status(loop_id).await
    }

    /// List all loops.
    pub async fn list_loops(
        &self,
        limit: usize,
        offset: usize,
    ) -> AgentResult<Vec<crate::loop_engine::Loop>> {
        let engine = self
            .loop_engine
            .as_ref()
            .ok_or_else(|| AgentError::Other("loop engine not initialized".into()))?;
        engine.list_loops(limit, offset).await
    }

    /// Create a lightweight clone of the Agent for background task spawning.
    /// Shares the same underlying resources (repo, tool_executor, llm_gateway, etc.)
    /// but does NOT own the loop_engine (child tasks use the parent's via Arc).
    pub fn try_clone_lightweight(&self) -> AgentResult<Self> {
        Ok(Self {
            repo: Arc::clone(&self.repo),
            memory_cache: Arc::clone(&self.memory_cache),
            memory_store: Arc::clone(&self.memory_store),
            embedding_service: self.embedding_service.clone(),
            memory_enabled: AtomicBool::new(
                self.memory_enabled
                    .load(std::sync::atomic::Ordering::SeqCst),
            ),
            hybrid_search_enabled: AtomicBool::new(
                self.hybrid_search_enabled
                    .load(std::sync::atomic::Ordering::SeqCst),
            ),
            tool_executor: std::sync::Arc::clone(&self.tool_executor),
            llm_gateway: self.llm_gateway.as_ref().map(|g| {
                // Create a new gateway with the same config
                LlmGateway::with_http_client(
                    g.config().clone(),
                    g.jail_root_absolute().cloned(),
                    Arc::clone(&self.http_client),
                )
            }),
            safety_ctx: self.safety_ctx.clone(),
            cached_system_prompt: std::sync::Mutex::new(None),
            last_usage: std::sync::Mutex::new(None),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            http_client: Arc::clone(&self.http_client),
            plan_cache: Arc::clone(&self.plan_cache),
            conversation_context: std::sync::Mutex::new(ConversationContext::collect()),
            tool_registry: ToolRegistry::new(),
            tool_usage_tracker: Arc::clone(&self.tool_usage_tracker),
            loop_engine: None, // child agents share parent's engine via Arc
            memory_system: std::sync::Arc::clone(&self.memory_system),
            memory_source: self.memory_source.clone(),
        })
    }

    // ------------------------------------------------------------------
    // Memory Search
    // ------------------------------------------------------------------

    /// Search across all stored memories.
    pub async fn search_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> AgentResult<Vec<crate::task::MemoryEntry>> {
        if self.is_memory_enabled() {
            self.memory_store.recall(query, limit).await
        } else {
            Err(crate::error::AgentError::MemoryDisabled)
        }
    }

    // ------------------------------------------------------------------
    // Agent Chat Mode — multi-turn conversation with memory
    // ------------------------------------------------------------------

    /// Run an agent chat with the given message, history, and callbacks.
    /// Returns the final response and token usage.
    /// When `system_prompt_override` is `Some`, it replaces the cached system prompt
    /// for this call (e.g. a Feishu channel can inject its own identity).
    #[allow(clippy::too_many_arguments)]
    pub async fn agent_chat<F>(
        &self,
        user_message: &str,
        history: &ConversationHistory,
        max_turns: usize,
        safe_mode: bool,
        on_event: F,
        intent: Option<&crate::signal::IntentState>,
        system_prompt_override: Option<String>,
    ) -> AgentResult<(String, TokenUsage)>
    where
        F: FnMut(AgentEvent) + Send,
    {
        // Check if LLM is configured
        let gateway = self.llm_gateway.as_ref().ok_or_else(|| {
            AgentError::Config("LLM not configured. Set api_key and provider first.".into())
        })?;

        // Search memories for context
        let memory_context = self
            .memory_cache
            .search(user_message, 5)
            .await
            .ok()
            .filter(|memories| !memories.is_empty())
            .map(|memories| {
                memories
                    .iter()
                    .map(|m| format!("- [{}] {}", m.created_at, m.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            });

        let context_ref = memory_context.as_deref();

        // Determine system prompt: explicit override > DB setting > cached default
        let db_preamble = self.repo.get_setting("system_prompt").await.ok().flatten();
        let custom_preamble: Option<&str> = match system_prompt_override {
            Some(ref prompt) if !prompt.is_empty() => Some(prompt.as_str()),
            _ => db_preamble.as_deref(),
        };

        // Run the agent loop
        let (response, usage) = gateway
            .chat_agent_loop(
                user_message,
                history,
                max_turns,
                safe_mode,
                context_ref,
                on_event,
                custom_preamble,
                intent,
            )
            .await?;

        // Store conversation memory after successful chat
        if self.is_memory_enabled() {
            let mem_content = format!("User: {}\nAssistant: {}", user_message, response);
            match self
                .remember_from(&mem_content, &["chat", "conversation"], &self.memory_source)
                .await
            {
                Ok(id) => {
                    info!(memory_id = %id, "conversation memory stored successfully");
                }
                Err(e) => {
                    warn!(error = %e, "failed to store conversation memory");
                }
            }
        } else {
            debug!("memory feature disabled, skipping memory storage");
        }

        Ok((response, usage))
    }

    // ------------------------------------------------------------------
    // Heartbeat — write a Running checkpoint for the current step
    // ------------------------------------------------------------------

    /// Emit a heartbeat checkpoint for the given step. Call this before
    /// long-running operations so the recovery logic knows the step is
    /// actively executing (not silently crashed).
    pub async fn heartbeat(&self, plan_id: &str, step_index: usize) -> AgentResult<()> {
        let ckpt = Checkpoint::new(plan_id, step_index, CheckpointStatus::Running);
        self.repo.save_checkpoint(&ckpt).await?;
        info!(
            plan_id = %plan_id,
            step = step_index,
            "heartbeat checkpoint"
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // Inject input — fulfill a WaitForInput step and advance the plan
    // ------------------------------------------------------------------

    /// Provide a response to a `WaitForInput` step. Updates the step,
    /// writes a completed checkpoint, and advances the plan's index.
    /// Returns `StepOutcome::Advanced` on success.
    pub async fn inject_input(
        &self,
        plan: &mut Plan,
        step_index: usize,
        input: &str,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();
        info!(
            plan_id = %pid,
            step = step_index,
            input = %input,
            "injecting user input into WaitForInput step"
        );

        // Store the response in the step
        if let Some(Step::WaitForInput {
            ref mut response,
            ref mut status,
            ..
        }) = plan.steps.get_mut(step_index)
        {
            *response = Some(input.to_string());
            *status = StepStatus::Completed;
        }

        // Atomically commit checkpoint + plan update
        self.repo
            .record_step_completion(
                &pid,
                step_index,
                StepStatus::Completed,
                Some(input.to_string()),
            )
            .await?;

        plan.status = PlanStatus::Running;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();

        Ok(StepOutcome::Advanced)
    }

    // ------------------------------------------------------------------
    // Resume — load plan and determine where to continue
    // ------------------------------------------------------------------

    /// Load the plan, run crash recovery, and return a plan ready to execute.
    /// Returns `None` if the plan is already complete.
    pub async fn resume(&self, plan_id: &str) -> AgentResult<Option<Plan>> {
        // Reset cancellation flag at the start of a new execution
        self.reset_cancel();

        // 1. Clean up any plans left in Running state from a previous crash
        let recovered = self.repo.reset_running_plans_to_pending().await?;
        if !recovered.is_empty() {
            info!(plans = ?recovered, "recovered plans from crash");
        }

        // 2. Load plan
        let mut plan = self.repo.load_plan(plan_id).await?;

        // 3. Check if already complete
        if plan.is_complete() {
            info!(plan_id = %plan_id, "plan already completed");
            return Ok(None);
        }

        // 4. Find the last checkpoint to determine resume point
        if let Some(ckpt) = self.repo.get_last_checkpoint(plan_id).await? {
            match ckpt.status {
                crate::task::CheckpointStatus::Completed => {
                    // Last completed step was ckpt.step_index, resume from next
                    let resume_index = ckpt.step_index + 1;
                    if resume_index < plan.steps.len() {
                        info!(
                            plan_id = %plan_id,
                            from_step = resume_index,
                            "resuming after completed checkpoint"
                        );
                        plan.current_step_index = resume_index;
                    } else {
                        // Already at or past the end
                        plan.status = PlanStatus::Completed;
                        return Ok(None);
                    }
                }
                crate::task::CheckpointStatus::Running => {
                    // Step was running when crash occurred — retry from same index
                    info!(
                        plan_id = %plan_id,
                        step = ckpt.step_index,
                        "resuming from interrupted step"
                    );
                    plan.current_step_index = ckpt.step_index;
                }
                crate::task::CheckpointStatus::Failed => {
                    // Step failed — retry from the failed step index
                    warn!(
                        plan_id = %plan_id,
                        step = ckpt.step_index,
                        "resuming from previously failed step"
                    );
                    plan.current_step_index = ckpt.step_index;
                }
            }
        } else {
            info!(plan_id = %plan_id, "no checkpoint found, starting from beginning");
        }

        plan.status = PlanStatus::Running;
        Ok(Some(plan))
    }

    // ------------------------------------------------------------------
    // Run next step
    // ------------------------------------------------------------------

    /// Execute the current step of the plan and return the outcome.
    pub async fn run_next_step(&self, plan: &mut Plan) -> AgentResult<StepOutcome> {
        // Check cancellation before executing any step
        if self.is_cancelled() {
            self.reset_cancel();
            return Ok(StepOutcome::Failed("Cancelled by user".to_string()));
        }

        let step_index = plan.current_step_index;

        // Clone the step to avoid borrow conflicts (we need &mut plan later)
        let cloned = plan
            .steps
            .get(step_index)
            .cloned()
            .ok_or(AgentError::InvalidStepIndex(step_index))?;

        info!(
            plan_id = %plan.id,
            step = step_index,
            step_type = ?std::mem::discriminant(&cloned),
            "executing step"
        );

        match cloned {
            Step::Think { instruction, .. } => {
                self.exec_think(plan, step_index, &instruction).await
            }
            Step::ToolCall {
                tool_name, params, ..
            } => {
                self.exec_tool_call(plan, step_index, &tool_name, &params)
                    .await
            }
            Step::WaitForInput { prompt, .. } => {
                self.exec_wait_for_input(plan, step_index, &prompt).await
            }
            Step::Finish { summary, .. } => self.exec_finish(plan, step_index, &summary).await,
            Step::Exec {
                command,
                args,
                timeout_secs,
                ..
            } => {
                self.exec_command(plan, step_index, &command, &args, timeout_secs)
                    .await
            }
            Step::HttpRequest {
                url,
                method,
                body,
                headers,
                ..
            } => {
                self.exec_http_req(
                    plan,
                    step_index,
                    &url,
                    &method,
                    body.as_deref(),
                    headers.as_ref(),
                )
                .await
            }
            Step::BrowserAction {
                action,
                url,
                timeout_secs,
                ..
            } => {
                self.exec_browser(plan, step_index, &action, url.as_deref(), timeout_secs)
                    .await
            }
        }
    }

    // ------------------------------------------------------------------
    // Step executors
    // ------------------------------------------------------------------

    // ---------------------------------------------------------------------------
    // System prompt loading: file → fallback → hardcoded default
    // ---------------------------------------------------------------------------

    /// Build the system prompt for LLM reasoning.
    ///
    /// Load order:
    /// 1. `$RUPOO_HOME/prompt.toml` — per-user customization
    /// 2. `$RUPOO_HOME/prompt.default.toml` — shipped defaults
    /// 3. Compiled-in `prompt.default.toml` via `include_str!` — always in sync, no drift
    fn build_system_prompt() -> String {
        let base = crate::rupoo_home();
        let paths = [
            // User config in home directory
            Some(base.join("prompt.toml")),
            // Shipped default (RUPOO_HOME/prompt.default.toml)
            Some(base.join("prompt.default.toml")),
        ];

        for path in paths.into_iter().flatten() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(config) = content.parse::<toml::Value>() {
                        if let Some(template) = config
                            .get("system_prompt")
                            .and_then(|v| v.get("template"))
                            .and_then(|v| v.as_str())
                        {
                            return template.to_string();
                        }
                    }
                }
            }
        }

        // Fallback: compiled-in prompt.default.toml — always in sync with the file, no hardcoded drift
        const DEFAULT_PROMPT_TOML: &str = include_str!("../prompt.default.toml");
        if let Ok(config) = DEFAULT_PROMPT_TOML.parse::<toml::Value>() {
            if let Some(template) = config
                .get("system_prompt")
                .and_then(|v| v.get("template"))
                .and_then(|v| v.as_str())
            {
                return template.to_string();
            }
        }

        // Absolute last resort (should never happen if prompt.default.toml is valid)
        "You are Rupoo, an AI-powered terminal assistant.".to_string()
    }

    // ---------------------------------------------------------------------------
    // Think step with streaming support for Plan Mode
    // ---------------------------------------------------------------------------

    async fn exec_think(
        &self,
        plan: &mut Plan,
        step_index: usize,
        instruction: &str,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();

        // Mark as running
        self.repo
            .update_step_progress(&pid, step_index, StepStatus::Running)
            .await?;

        // Emit heartbeat before potentially long work
        self.heartbeat(&pid, step_index).await?;

        // Retrieve relevant memories to inject as context
        let memory_context = self
            .memory_cache
            .search(instruction, 5)
            .await
            .unwrap_or_default();

        // Call LLM if configured, otherwise fall back to dummy output
        let think_result = if let Some(gateway) = &self.llm_gateway {
            let mut system = {
                let mut cache = self.cached_system_prompt.lock().unwrap_or_else(|e| {
                    error!("cached_system_prompt lock poisoned, recovering");
                    e.into_inner()
                });
                if cache.is_none() {
                    *cache = Some(Self::build_system_prompt());
                }
                cache
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| AgentError::Other("failed to build system prompt".to_string()))?
            };

            if !memory_context.is_empty() {
                system.push_str("\n\nRelevant context from memory:");
                for mem in &memory_context {
                    system.push_str(&format!("\n- [{}] {}", mem.created_at, mem.content));
                }
            }

            use crate::llm::LlmChatMessage;

            let messages = vec![
                LlmChatMessage::system(&system),
                LlmChatMessage::user(instruction),
            ];
            match gateway.chat(&messages).await {
                Ok((response, usage)) => {
                    if let Ok(mut g) = self.last_usage.lock() {
                        *g = Some(usage);
                    }
                    response
                }
                Err(e) => {
                    error!(error = %e, "LLM call failed — returning placeholder");
                    format!("[⚠️ LLM unavailable — placeholder response for: {instruction}]")
                }
            }
        } else {
            // No LLM configured — use warning placeholder
            warn!("LLM not configured for think step — returning placeholder");
            format!("[⚠️ LLM unavailable — placeholder response for: {instruction}]")
        };

        // Record the output in the step
        if let Some(Step::Think { ref mut output, .. }) = plan.steps.get_mut(step_index) {
            *output = Some(think_result.clone());
        }

        // Atomically commit checkpoint + plan update
        self.repo
            .record_step_completion(&pid, step_index, StepStatus::Completed, Some(think_result))
            .await?;

        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(StepOutcome::Advanced)
    }

    async fn exec_tool_call(
        &self,
        plan: &mut Plan,
        step_index: usize,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();

        // High-risk tools require user approval before execution.
        // Return RequiresApproval and let the bridge pause the run_plan loop.
        if self.safety_ctx.needs_approval(tool_name) {
            return Ok(StepOutcome::RequiresApproval {
                tool_name: tool_name.to_string(),
                params: params.clone(),
                step_index,
            });
        }

        // Mark as running
        self.repo
            .update_step_progress(&pid, step_index, StepStatus::Running)
            .await?;

        // Emit heartbeat before potentially long tool execution
        self.heartbeat(&pid, step_index).await?;

        // Execute via tool executor
        let result = self
            .tool_executor
            .execute_tool(tool_name, params.clone())
            .await;

        match result {
            Ok(mcp_result) => {
                // Record result in step
                if let Some(Step::ToolCall { ref mut result, .. }) = plan.steps.get_mut(step_index)
                {
                    *result = Some(serde_json::json!({
                        "success": mcp_result.is_success(),
                        "content": mcp_result.content(),
                    }));
                }

                let output = match &mcp_result {
                    McpToolResult::Success { content } => content.clone(),
                    McpToolResult::Error { message } => {
                        error!(tool = tool_name, error = %message, "tool call failed");

                        // Record failure checkpoint instead
                        self.repo
                            .record_step_completion(
                                &pid,
                                step_index,
                                StepStatus::Failed,
                                Some(format!("error: {message}")),
                            )
                            .await?;

                        plan.updated_at = chrono::Utc::now();
                        return Ok(StepOutcome::Failed(message.clone()));
                    }
                };

                // Atomically commit checkpoint
                self.repo
                    .record_step_completion(&pid, step_index, StepStatus::Completed, Some(output))
                    .await?;

                plan.current_step_index = step_index + 1;
                plan.updated_at = chrono::Utc::now();
                Ok(StepOutcome::Advanced)
            }
            Err(e) => {
                let err_msg = e.to_string();
                error!(tool = tool_name, error = %err_msg, "tool executor error");

                self.repo
                    .record_step_completion(
                        &pid,
                        step_index,
                        StepStatus::Failed,
                        Some(format!("error: {err_msg}")),
                    )
                    .await?;

                plan.updated_at = chrono::Utc::now();
                Ok(StepOutcome::Failed(err_msg))
            }
        }
    }

    /// Generic step execution skeleton for tool-like steps (Exec, HttpRequest, BrowserAction).
    /// Handles the common pattern: mark running → heartbeat → execute → record result → advance.
    async fn exec_tool_step<F, Fut>(
        &self,
        plan: &mut Plan,
        step_index: usize,
        step_label: &str,
        execute: F,
        set_output: impl FnOnce(&mut Step, Option<String>),
    ) -> AgentResult<StepOutcome>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = AgentResult<String>>,
    {
        let pid = plan.id.clone();
        self.repo
            .update_step_progress(&pid, step_index, StepStatus::Running)
            .await?;
        self.heartbeat(&pid, step_index).await?;

        // Execute the operation once
        let result = execute().await;
        let (output, outcome) = match result {
            Ok(out) => (Some(out), StepOutcome::Advanced),
            Err(e) => {
                warn!(tool = step_label, error = %e, "operation failed");
                (
                    Some(format!("error: {}", e)),
                    StepOutcome::Failed(e.to_string()),
                )
            }
        };

        // Update step status
        let step_status = match outcome {
            StepOutcome::Advanced => StepStatus::Completed,
            _ => StepStatus::Failed,
        };
        if let Some(step) = plan.steps.get_mut(step_index) {
            step.set_status(step_status.clone());
            set_output(step, output.clone());
        }

        self.repo
            .record_step_completion(&pid, step_index, step_status, output)
            .await?;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(outcome)
    }

    // ------------------------------------------------------------------
    // Tool execution with retry support
    // ------------------------------------------------------------------

    /// Execute a tool with intelligent retry mechanism.
    /// Returns the result and retry count.
    pub async fn execute_tool_with_retry(
        &self,
        tool_name: &str,
        params: serde_json::Value,
        max_retries: usize,
    ) -> (AgentResult<McpToolResult>, usize) {
        let mut attempt = 0;
        let mut last_error = None;

        while attempt <= max_retries {
            match self
                .tool_executor
                .execute_tool(tool_name, params.clone())
                .await
            {
                Ok(result) => {
                    if attempt > 0 {
                        info!(
                            tool = tool_name,
                            attempt = attempt + 1,
                            "tool succeeded after retry"
                        );
                    }
                    return (Ok(result), attempt);
                }
                Err(e) => {
                    if e.is_retryable() && attempt < max_retries {
                        attempt += 1;
                        let delay_ms = 1000u64 * (2_u64.pow(attempt as u32));
                        warn!(
                            tool = tool_name,
                            attempt = attempt,
                            max_retries = max_retries,
                            delay_ms = delay_ms,
                            error = %e,
                            "retrying after transient error"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        last_error = Some(e);
                    } else {
                        warn!(
                            tool = tool_name,
                            total_attempts = attempt + 1,
                            error = %e,
                            "tool failed after all retries"
                        );
                        return (Err(e), attempt);
                    }
                }
            }
        }

        (
            Err(last_error.unwrap_or_else(|| AgentError::Other("max retries exceeded".to_string()))),
            attempt,
        )
    }

    async fn exec_command(
        &self,
        plan: &mut Plan,
        step_index: usize,
        command: &str,
        args: &[String],
        timeout_secs: Option<u64>,
    ) -> AgentResult<StepOutcome> {
        // Exec steps also need approval for dangerous commands
        if self.safety_ctx.needs_approval(command) {
            return Ok(StepOutcome::RequiresApproval {
                tool_name: command.to_string(),
                params: serde_json::json!({"command": command, "args": args}),
                step_index,
            });
        }

        let command_owned = command.to_string();
        let args_owned = args.to_vec();
        let timeout = timeout_secs;
        let safety = self.safety_ctx.clone();

        self.exec_tool_step(
            plan,
            step_index,
            "exec_command",
            || async move {
                crate::tools::terminal::execute_command(
                    &command_owned,
                    &args_owned,
                    timeout,
                    &safety,
                )
                .await
            },
            |step, result| {
                if let Step::Exec { ref mut output, .. } = step {
                    *output = result;
                }
            },
        )
        .await
    }

    async fn exec_http_req(
        &self,
        plan: &mut Plan,
        step_index: usize,
        url: &str,
        method: &crate::task::HttpMethod,
        body: Option<&str>,
        headers: Option<&std::collections::HashMap<String, String>>,
    ) -> AgentResult<StepOutcome> {
        let url_owned = url.to_string();
        let method_owned = method.clone();
        let body_owned = body.map(|s| s.to_string());
        let headers_owned = headers.cloned();

        self.exec_tool_step(
            plan,
            step_index,
            "exec_http_req",
            || async move {
                crate::tools::network::execute_http_request(
                    &url_owned,
                    &method_owned,
                    body_owned.as_deref(),
                    headers_owned.as_ref(),
                )
                .await
            },
            |step, result| {
                if let Step::HttpRequest {
                    ref mut response, ..
                } = step
                {
                    *response = result;
                }
            },
        )
        .await
    }

    async fn exec_browser(
        &self,
        plan: &mut Plan,
        step_index: usize,
        action: &crate::task::BrowserActionType,
        url: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> AgentResult<StepOutcome> {
        let action_owned = action.clone();
        let url_owned = url.map(|s| s.to_string());
        let timeout = timeout_secs;
        let safety = self.safety_ctx.clone();

        self.exec_tool_step(
            plan,
            step_index,
            "exec_browser",
            || async move {
                crate::tools::browser::execute_browser_action(
                    &action_owned,
                    url_owned.as_deref(),
                    timeout,
                    &safety,
                )
                .await
            },
            |step, result| {
                if let Step::BrowserAction { ref mut output, .. } = step {
                    *output = result;
                }
            },
        )
        .await
    }

    async fn exec_wait_for_input(
        &self,
        plan: &mut Plan,
        step_index: usize,
        prompt: &str,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();

        // Mark step as WaitingForInput
        self.repo
            .update_step_progress(&pid, step_index, StepStatus::WaitingForInput)
            .await?;

        // Update plan status
        plan.status = PlanStatus::WaitingForInput;
        plan.updated_at = chrono::Utc::now();

        // Do NOT increment step index — we stay on this step until input arrives
        info!(step = step_index, prompt = %prompt, "waiting for user input");

        Ok(StepOutcome::WaitingForInput(prompt.to_string()))
    }

    async fn exec_finish(
        &self,
        plan: &mut Plan,
        step_index: usize,
        summary: &str,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();

        self.repo
            .update_step_progress(&pid, step_index, StepStatus::Completed)
            .await?;

        // Mark step as completed and plan as finished
        if let Some(step) = plan.steps.get_mut(step_index) {
            step.set_status(StepStatus::Completed);
        }

        // Record final checkpoint
        self.repo
            .record_step_completion(
                &pid,
                step_index,
                StepStatus::Completed,
                Some(format!("[finish] {summary}")),
            )
            .await?;

        plan.status = PlanStatus::Completed;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();

        // Auto-learn a skill from the completed plan (non-blocking)
        {
            let plan_clone = plan.clone();
            let pid = pid.clone();
            tokio::spawn(async move {
                if let Err(e) = async {
                    let manager =
                        crate::skill::SkillManager::new(crate::skill::SkillManager::default_dir());
                    let skill_name = format!("auto-{}", pid.split('-').next().unwrap_or("plan"));
                    let skill = crate::skill::SkillManager::plan_to_skill(
                        &plan_clone,
                        &skill_name,
                        &format!("Auto-learned from plan '{}'", plan_clone.name),
                    );
                    if !skill.steps.is_empty() {
                        match manager.save_skill(&skill) {
                            Ok(()) => info!(
                                skill = %skill_name,
                                plan = %pid,
                                steps = skill.steps.len(),
                                "auto-learned skill from completed plan"
                            ),
                            Err(e) => warn!(
                                error = %e,
                                plan = %pid,
                                "failed to auto-learn skill"
                            ),
                        }
                    }
                    Ok::<(), AgentError>(())
                }
                .await
                {
                    error!(error = %e, plan = %pid, "auto-learn skill task failed");
                }
            });
        }

        info!(plan_id = %pid, summary = %summary, "plan completed");
        Ok(StepOutcome::Finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{finish_step, think_step, tool_call_step, wait_for_input_step};

    fn setup() -> (Arc<TaskRepo>, Agent) {
        let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
        let agent = Agent::new(Arc::clone(&repo), std::sync::Arc::new(DummyToolExecutor));
        (repo, agent)
    }

    #[tokio::test]
    async fn test_resume_completed_plan_returns_none() {
        let (repo, agent) = setup();
        let mut plan = Plan::new("completed-plan", vec![think_step("1"), finish_step("done")]);
        plan.status = PlanStatus::Completed;
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let result = agent.resume(&id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_run_think_step_advances() {
        let (repo, agent) = setup();
        let plan = Plan::new("think-test", vec![think_step("analyze"), finish_step("ok")]);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let mut plan = agent.resume(&id).await.unwrap().unwrap();
        let outcome = agent.run_next_step(&mut plan).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Advanced));
        assert_eq!(plan.current_step_index, 1);
    }

    #[tokio::test]
    async fn test_run_full_plan() {
        let (repo, agent) = setup();
        let steps = vec![
            think_step("explore"),
            tool_call_step("echo", serde_json::json!({"msg": "hello"})),
            finish_step("all done"),
        ];
        let plan = Plan::new("full-test", steps);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let mut plan = agent.resume(&id).await.unwrap().unwrap();

        assert!(matches!(
            agent.run_next_step(&mut plan).await.unwrap(),
            StepOutcome::Advanced
        ));
        assert!(matches!(
            agent.run_next_step(&mut plan).await.unwrap(),
            StepOutcome::Advanced
        ));
        assert!(matches!(
            agent.run_next_step(&mut plan).await.unwrap(),
            StepOutcome::Finished
        ));
        assert!(plan.is_complete());
    }

    #[tokio::test]
    async fn test_wait_for_input_does_not_advance() {
        let (repo, agent) = setup();
        let steps = vec![
            think_step("prepare"),
            wait_for_input_step("confirm?"),
            finish_step("done"),
        ];
        let plan = Plan::new("wait-test", steps);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let mut plan = agent.resume(&id).await.unwrap().unwrap();
        agent.run_next_step(&mut plan).await.unwrap(); // think
        let outcome = agent.run_next_step(&mut plan).await.unwrap(); // wait

        match outcome {
            StepOutcome::WaitingForInput(prompt) => {
                assert_eq!(prompt, "confirm?");
            }
            _ => panic!("expected WaitingForInput"),
        }
        assert_eq!(plan.current_step_index, 1); // did NOT advance
    }

    #[tokio::test]
    async fn test_heartbeat_writes_running_checkpoint() {
        let (repo, agent) = setup();
        let plan = Plan::new("hb-test", vec![think_step("hb"), finish_step("done")]);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        agent.heartbeat(&id, 0).await.unwrap();

        let ckpt = repo.get_last_checkpoint(&id).await.unwrap().unwrap();
        assert_eq!(ckpt.step_index, 0);
        assert_eq!(ckpt.status, CheckpointStatus::Running);
    }

    #[tokio::test]
    async fn test_inject_input_advances_wait_step() {
        let (repo, agent) = setup();
        let steps = vec![
            wait_for_input_step("what is your name?"),
            finish_step("done"),
        ];
        let plan = Plan::new("inject-test", steps);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let mut plan = agent.resume(&id).await.unwrap().unwrap();

        // First call to run_next_step sets it to waiting
        let outcome = agent.run_next_step(&mut plan).await.unwrap();
        assert!(matches!(outcome, StepOutcome::WaitingForInput(_)));

        // Now inject input
        let outcome = agent.inject_input(&mut plan, 0, "Alice").await.unwrap();
        assert!(matches!(outcome, StepOutcome::Advanced));
        assert_eq!(plan.current_step_index, 1);

        // Verify the step stored the response
        if let Some(Step::WaitForInput { response, .. }) = plan.steps.first() {
            assert_eq!(response.as_deref(), Some("Alice"));
        } else {
            panic!("expected WaitForInput step");
        }

        // Verify via checkpoint too
        let ckpt = repo.get_last_checkpoint(&id).await.unwrap().unwrap();
        assert_eq!(ckpt.step_index, 0);

        // Plan should complete now
        let outcome = agent.run_next_step(&mut plan).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Finished));
    }

    #[tokio::test]
    async fn test_crash_recovery_works() {
        let (repo, agent) = setup();
        let steps = vec![
            think_step("phase1"),
            think_step("phase2"),
            finish_step("recovered"),
        ];
        let plan = Plan::new("crash-test", steps);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        // Simulate: step 0 completed, then crash
        repo.record_step_completion(&id, 0, StepStatus::Completed, None)
            .await
            .unwrap();

        // Recovery should start from step 1
        let plan = agent.resume(&id).await.unwrap().unwrap();
        assert_eq!(plan.current_step_index, 1);
    }

    #[test]
    fn test_has_llm_when_not_configured() {
        let (_, agent) = setup();
        assert!(!agent.has_llm());
        assert!(agent.llm_gateway_ref().is_none());
    }
}
