use std::sync::Arc;
use std::path::PathBuf;
use std::fs;

use tracing::{error, info, warn};

use crate::db::TaskRepo;
use crate::error::{AgentError, AgentResult};
use crate::llm::{LlmGateway, TokenUsage, ConversationHistory, AgentEvent};

use crate::task::{
    Checkpoint, CheckpointStatus, McpToolResult, Plan, PlanStatus, Step, StepStatus,
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
        Ok(McpToolResult {
            success: true,
            content,
            error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

pub struct Agent {
    repo: Arc<TaskRepo>,
    pub tool_executor: Box<dyn ToolExecutor>,
    llm_gateway: Option<LlmGateway>,
    pub safety_ctx: SafetyContext,
    /// Token usage from the most recent chat() call.
    /// Uses Mutex for interior mutability (Cell is not Sync).
    last_usage: std::sync::Mutex<Option<TokenUsage>>,
    /// Cancellation flag. Set to true to abort the running plan at the next step.
    cancelled: std::sync::atomic::AtomicBool,
}

impl Agent {
    pub fn new(repo: Arc<TaskRepo>, tool_executor: Box<dyn ToolExecutor>) -> Self {
        Self {
            repo,
            tool_executor,
            llm_gateway: None,
            safety_ctx: SafetyContext::default(),
            last_usage: std::sync::Mutex::new(None),
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Return token usage from the most recent think step, if available.
    pub fn last_usage(&self) -> Option<TokenUsage> {
        self.last_usage.lock().ok().and_then(|g| *g)
    }

    /// Request cancellation of the currently running plan.
    /// The agent will abort at the next step boundary.
    pub fn request_cancel(&self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Reset the cancellation flag (e.g., before starting a new plan).
    pub fn reset_cancel(&self) {
        self.cancelled.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Return a reference to the tool executor (used by AgentUiBridge for
    /// direct approval-time tool execution, bypassing needs_approval checks).
    #[allow(clippy::borrowed_box)]
    pub fn get_tool_executor(&self) -> &Box<dyn ToolExecutor> {
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
        let api_key = repo.get_setting(&format!("api_key.{}", provider)).await
            .map_err(|e| AgentError::Config(format!("DB error: {}", e)))?
            .ok_or_else(|| AgentError::Config(format!("No API key for '{}'", provider)))?;

        let llm_provider = match provider {
            "anthropic" => crate::llm::LlmProvider::Anthropic,
            "openai" => crate::llm::LlmProvider::OpenAI,
            "ollama" => crate::llm::LlmProvider::Ollama,
            _ => return Err(AgentError::Config(format!("Unknown provider: '{}'", provider))),
        };

        let mut cfg = crate::llm::LlmConfig::new(llm_provider, Some(api_key));
        if let Some(m) = model {
            cfg.model = m.to_string();
        } else if let Ok(Some(m)) = repo.get_setting(&format!("model.{}", provider)).await {
            cfg.model = m;
        }

        let model_label = cfg.model.clone();

        let jail_root = self.safety_ctx.jail_root().map(|p| p.to_path_buf());
        let gateway = if let Some(ref root) = jail_root {
            crate::llm::LlmGateway::with_jail(cfg, root.clone())
        } else {
            crate::llm::LlmGateway::new(cfg)
        };

        self.llm_gateway = Some(gateway);
        let label = format!("{}/{}", provider, model_label);
        Ok(label)
    }

    /// Reload LLM configuration from database settings.
    /// Call this after `rupoo config set` to apply changes without restart.
    pub async fn reconfigure_from_db(&mut self, repo: &TaskRepo) -> AgentResult<String> {
        // Try providers in priority order
        for provider in &["anthropic", "openai", "ollama"] {
            if let Ok(Some(_api_key)) = repo.get_setting(&format!("api_key.{}", provider)).await {
                return self.switch_llm(provider, None, repo).await;
            }
        }
        // No LLM configured
        self.llm_gateway = None;
        Ok("no LLM configured".to_string())
    }

    // ------------------------------------------------------------------
    // Agent Chat Mode — multi-turn conversation with memory
    // ------------------------------------------------------------------

    /// Run an agent chat with the given message, history, and callbacks.
    /// Returns the final response and token usage.
    pub async fn agent_chat<F>(
        &self,
        user_message: &str,
        history: &ConversationHistory,
        max_turns: usize,
        safe_mode: bool,
        on_event: F,
        intent: Option<&crate::signal::IntentState>,
    ) -> AgentResult<(String, TokenUsage)>
    where
        F: FnMut(AgentEvent) + Send,
    {
        // Check if LLM is configured
        let gateway = self.llm_gateway.as_ref()
            .ok_or_else(|| AgentError::Config("LLM not configured. Set api_key and provider first.".into()))?;

        // Search memories for context
        let memory_context = self
            .repo
            .search_memories(user_message, 5)
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

        // Check for DB-stored system prompt override
        let custom_preamble = self.repo.get_setting("system_prompt").await
            .ok()
            .flatten();

        // Run the agent loop
        gateway
            .chat_agent_loop(user_message, history, max_turns, safe_mode, context_ref, on_event, custom_preamble.as_deref(), intent)
            .await
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
        if let Some(Step::WaitForInput { ref mut response, ref mut status, .. }) = plan.steps.get_mut(step_index) {
            *response = Some(input.to_string());
            *status = StepStatus::Completed;
        }

        // Atomically commit checkpoint + plan update
        self.repo
            .record_step_completion(&pid, step_index, StepStatus::Completed, Some(input.to_string()))
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
            Step::ToolCall { tool_name, params, .. } => {
                self.exec_tool_call(plan, step_index, &tool_name, &params).await
            }
            Step::WaitForInput { prompt, .. } => {
                self.exec_wait_for_input(plan, step_index, &prompt).await
            }
            Step::Finish { summary, .. } => self.exec_finish(plan, step_index, &summary).await,
            Step::Exec { command, args, timeout_secs, .. } => {
                self.exec_command(plan, step_index, &command, &args, timeout_secs).await
            }
            Step::HttpRequest { url, method, body, headers, .. } => {
                self.exec_http_req(plan, step_index, &url, &method, body.as_deref(), headers.as_ref()).await
            }
            Step::BrowserAction { action, url, timeout_secs, .. } => {
                self.exec_browser(plan, step_index, &action, url.as_deref(), timeout_secs).await
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
/// 1. `~/.rupoo/prompt.toml` — per-user customization
/// 2. `~/.rupoo/prompt.default.toml` — shipped defaults
/// 3. Compiled-in `prompt.default.toml` via `include_str!` — always in sync, no drift
fn build_system_prompt() -> String {
    let paths = [
        // User config in home directory
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".rupoo").join("prompt.toml")),
        // Shipped default (~/.rupoo/prompt.default.toml)
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".rupoo").join("prompt.default.toml")),
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
            .repo
            .search_memories(instruction, 5)
            .await
            .unwrap_or_default();

        // Call LLM if configured, otherwise fall back to dummy output
        let think_result = if let Some(gateway) = &self.llm_gateway {
            let mut system = Self::build_system_prompt();

            if !memory_context.is_empty() {
                system.push_str("\n\nRelevant context from memory:");
                for mem in &memory_context {
                    system.push_str(&format!("\n- [{}] {}", mem.created_at, mem.content));
                }
            }

            use crate::llm::LlmChatMessage;

            let messages = vec![
                LlmChatMessage::system(&system),
                LlmChatMessage::user(&instruction.to_string()),
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
        let result = self.tool_executor.execute_tool(tool_name, params.clone()).await;

        match result {
            Ok(mcp_result) => {
                // Record result in step
                if let Some(Step::ToolCall { ref mut result, .. }) = plan.steps.get_mut(step_index) {
                    *result = Some(serde_json::json!({
                        "success": mcp_result.success,
                        "content": mcp_result.content,
                    }));
                }

                let output = if mcp_result.success {
                    mcp_result.content
                } else {
                    let err = mcp_result.error.unwrap_or_else(|| "unknown error".into());
                    error!(tool = tool_name, error = %err, "tool call failed");

                    // Record failure checkpoint instead
                    self.repo
                        .record_step_completion(
                            &pid,
                            step_index,
                            StepStatus::Failed,
                            Some(format!("error: {err}")),
                        )
                        .await?;

                    plan.updated_at = chrono::Utc::now();
                    return Ok(StepOutcome::Failed(err));
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
        self.repo.update_step_progress(&pid, step_index, StepStatus::Running).await?;
        self.heartbeat(&pid, step_index).await?;

        let result = execute().await;
        let (output, outcome) = match result {
            Ok(out) => (Some(out), StepOutcome::Advanced),
            Err(e) => {
                warn!(%e, "{} failed", step_label);
                (Some(format!("error: {e}")), StepOutcome::Failed(e.to_string()))
            }
        };

        if let Some(step) = plan.steps.get_mut(step_index) {
            let step_status = match outcome {
                StepOutcome::Advanced => StepStatus::Completed,
                _ => StepStatus::Failed,
            };
            step.set_status(step_status);
            set_output(step, output.clone());
        }

        let step_status = match outcome {
            StepOutcome::Advanced => StepStatus::Completed,
            _ => StepStatus::Failed,
        };
        self.repo
            .record_step_completion(&pid, step_index, step_status, output)
            .await?;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(outcome)
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
                crate::tools::terminal::execute_command(&command_owned, &args_owned, timeout, &safety).await
            },
            |step, result| {
                if let Step::Exec { ref mut output, .. } = step {
                    *output = result;
                }
            },
        ).await
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
                ).await
            },
            |step, result| {
                if let Step::HttpRequest { ref mut response, .. } = step {
                    *response = result;
                }
            },
        ).await
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
                ).await
            },
            |step, result| {
                if let Step::BrowserAction { ref mut output, .. } = step {
                    *output = result;
                }
            },
        ).await
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
                let manager = crate::skill::SkillManager::new(
                    crate::skill::SkillManager::default_dir(),
                );
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
        let agent = Agent::new(Arc::clone(&repo), Box::new(DummyToolExecutor));
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

        assert!(matches!(agent.run_next_step(&mut plan).await.unwrap(), StepOutcome::Advanced));
        assert!(matches!(agent.run_next_step(&mut plan).await.unwrap(), StepOutcome::Advanced));
        assert!(matches!(agent.run_next_step(&mut plan).await.unwrap(), StepOutcome::Finished));
        assert!(plan.is_complete());
    }

    #[tokio::test]
    async fn test_wait_for_input_does_not_advance() {
        let (repo, agent) = setup();
        let steps = vec![think_step("prepare"), wait_for_input_step("confirm?"), finish_step("done")];
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
        if let Some(Step::WaitForInput { response, .. }) = plan.steps.get(0) {
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
