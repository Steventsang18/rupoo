use std::sync::Arc;

use tracing::{error, info, warn};

use crate::db::TaskRepo;
use crate::error::{AgentError, AgentResult};
use crate::llm::{ChatMessage, ChatRole, LlmGateway};
use crate::task::{
    Checkpoint, CheckpointStatus, McpToolResult, Plan, PlanStatus, Step, StepStatus,
};

// Submodules declared here (cannot modify lib.rs per project constraints).
#[path = "safety.rs"]
pub mod safety;
#[path = "tools/mod.rs"]
pub mod tools;

use self::safety::SafetyContext;

/// Result of running a single step.
#[derive(Debug)]
pub enum StepOutcome {
    /// Step executed successfully; continue to next.
    Advanced,
    /// Plan is fully finished.
    Finished,
    /// Agent is waiting for human input.
    WaitingForInput(String),
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
    tool_executor: Box<dyn ToolExecutor>,
    llm_gateway: Option<LlmGateway>,
    pub safety_ctx: SafetyContext,
}

impl Agent {
    pub fn new(repo: Arc<TaskRepo>, tool_executor: Box<dyn ToolExecutor>) -> Self {
        Self {
            repo,
            tool_executor,
            llm_gateway: None,
            safety_ctx: SafetyContext::default(),
        }
    }

    /// Attach an LLM gateway so Think steps produce real LLM responses.
    pub fn with_llm(mut self, gateway: LlmGateway) -> Self {
        self.llm_gateway = Some(gateway);
        self
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
            let mut system = "\
	You are Rupoo, an AI-powered terminal assistant running inside the user's terminal.
	You help with software development, file operations, and system tasks.
	
	## Your Capabilities
	- File Operations: file_read, file_write, list_directory
	- Terminal Commands: execute shell commands (dangerous commands blocked)
	- HTTP Requests: GET/POST to public URLs (localhost blocked for security)
	- Browser Automation: headless navigation and screenshots
	- Memory: stores and retrieves context across sessions (FTS5 search)
	- Skills: reusable workflows as JSON files
	- Git: status, commit, create PR
	- MCP Server: exposes tools via JSON-RPC over stdio
	
	## Output Format
	Be concise and structured.
	
	### Reading files:
	Show the file path, then the relevant content or summary.
	
	### Listing directories:
	Show the structure clearly.
	
	### Running commands:
	Show the command, then the output.
	
	### Analyzing code:
	Be specific about what you find. Show relevant snippets.
	
	### Errors:
	Be specific about the problem and the fix.
	
	Keep responses tight. Use Markdown naturally for structure.
".to_string();

            if !memory_context.is_empty() {
                system.push_str("\n\nRelevant context from memory:");
                for mem in &memory_context {
                    system.push_str(&format!("\n- [{}] {}", mem.created_at, mem.content));
                }
            }

            let messages = vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: system,
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: instruction.to_string(),
                },
            ];
            match gateway.chat(&messages).await {
                Ok(response) => response,
                Err(e) => {
                    error!(error = %e, "LLM call failed, falling back to dummy");
                    format!("[think] processed: {instruction}")
                }
            }
        } else {
            // No LLM configured — use dummy output
            info!("no LLM configured, using dummy think output");
            format!("[think] processed: {instruction}")
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

    async fn exec_command(
        &self,
        plan: &mut Plan,
        step_index: usize,
        command: &str,
        args: &[String],
        timeout_secs: Option<u64>,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();
        self.repo.update_step_progress(&pid, step_index, StepStatus::Running).await?;
        self.heartbeat(&pid, step_index).await?;

        let result = self::tools::terminal::execute_command(command, args, timeout_secs, &self.safety_ctx).await;
        let (cmd_output, outcome) = match result {
            Ok(out) => (Some(out), StepOutcome::Advanced),
            Err(e) => (Some(format!("error: {e}")), StepOutcome::Advanced),
        };

        if let Some(step) = plan.steps.get_mut(step_index) {
            step.set_status(StepStatus::Completed);
            if let Step::Exec { ref mut output, .. } = step {
                *output = cmd_output.clone();
            }
        }

        self.repo
            .record_step_completion(&pid, step_index, StepStatus::Completed, cmd_output)
            .await?;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(outcome)
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
        let pid = plan.id.clone();
        self.repo.update_step_progress(&pid, step_index, StepStatus::Running).await?;
        self.heartbeat(&pid, step_index).await?;

        let result = self::tools::network::execute_http_request(url, method, body, headers).await;
        let (http_output, outcome) = match result {
            Ok(out) => (Some(out), StepOutcome::Advanced),
            Err(e) => (Some(format!("error: {e}")), StepOutcome::Advanced),
        };

        if let Some(step) = plan.steps.get_mut(step_index) {
            step.set_status(StepStatus::Completed);
            if let Step::HttpRequest { ref mut response, .. } = step {
                *response = http_output.clone();
            }
        }

        self.repo
            .record_step_completion(&pid, step_index, StepStatus::Completed, http_output)
            .await?;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(outcome)
    }

    async fn exec_browser(
        &self,
        plan: &mut Plan,
        step_index: usize,
        action: &crate::task::BrowserActionType,
        url: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> AgentResult<StepOutcome> {
        let pid = plan.id.clone();
        self.repo.update_step_progress(&pid, step_index, StepStatus::Running).await?;
        self.heartbeat(&pid, step_index).await?;

        let result = self::tools::browser::execute_browser_action(action, url, timeout_secs, &self.safety_ctx).await;
        let (browser_output, outcome) = match result {
            Ok(out) => (Some(out), StepOutcome::Advanced),
            Err(e) => (Some(format!("error: {e}")), StepOutcome::Advanced),
        };

        if let Some(step) = plan.steps.get_mut(step_index) {
            step.set_status(StepStatus::Completed);
            if let Step::BrowserAction { ref mut output, .. } = step {
                *output = browser_output.clone();
            }
        }

        self.repo
            .record_step_completion(&pid, step_index, StepStatus::Completed, browser_output)
            .await?;
        plan.current_step_index = step_index + 1;
        plan.updated_at = chrono::Utc::now();
        Ok(outcome)
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
}
