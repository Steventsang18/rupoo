//! Loop Engineering — Core engine for adaptive agentic loops
//!
//! Implements three loop patterns:
//! - A. Adaptive agent loop (execute → evaluate → correct → repeat)
//! - B. Recursive task decomposition (decompose → sub-loops → aggregate)
//! - C. Daemon / continuous watch loop
//!
//! Part of Phase A: Basic infrastructure + adaptive loop

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::agent::ToolExecutor;
use crate::db::TaskRepo;
use crate::error::{AgentError, AgentResult};
use crate::llm::{LlmGateway, TokenUsage};
use crate::task::{PlanStatus, Step, StepStatus};

// ---------------------------------------------------------------------------
// Loop configuration
// ---------------------------------------------------------------------------

/// Autonomy level — controls the approval gating during loop execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum AutonomyLevel {
    /// Manual: every step requires approval.
    L1Manual,
    /// Step check: only high-risk steps require approval.
    L2StepCheck,
    /// Round check: approve after each iteration (default).
    #[default]
    L3RoundCheck,
    /// Auto-correct: autonomous unless an unrecoverable error occurs.
    L4AutoCorrect,
    /// Full auto: no approval gates at all.
    L5FullAuto,
}

/// Configuration for a single Loop execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Hard cap on the total number of iterations. Prevents infinite loops.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Optional token budget (total across all iterations). None = unlimited.
    pub token_budget: Option<u64>,
    /// Optional time budget in seconds. None = unlimited.
    pub time_budget_secs: Option<u64>,
    /// Autonomy / approval level.
    #[serde(default)]
    pub autonomy_level: AutonomyLevel,
    /// When true, the loop runs as a daemon that polls its trigger.
    #[serde(default)]
    pub daemon: bool,
    /// Natural-language trigger condition for daemon mode.
    pub daemon_trigger: Option<String>,
    /// Polling interval in seconds for daemon mode (default 60s).
    #[serde(default = "default_poll_interval")]
    pub daemon_poll_interval_secs: u64,
    /// When true, decompositions run child loops in parallel.
    #[serde(default)]
    pub parallel_decomposition: bool,
}

fn default_max_iterations() -> u32 {
    10
}
fn default_poll_interval() -> u64 {
    60
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            token_budget: None,
            time_budget_secs: None,
            autonomy_level: AutonomyLevel::default(),
            daemon: false,
            daemon_trigger: None,
            daemon_poll_interval_secs: 60,
            parallel_decomposition: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Loop status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopStatus {
    Pending,
    Running,
    StepComplete,
    Evaluating,
    WaitingForApproval,
    WaitingForInput,
    Decomposing,
    Paused,
    Completed,
    Failed,
    BudgetExceeded,
    TimedOut,
    Cancelled,
}

impl LoopStatus {
    /// Is this a terminal (non-resumable) status?
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            LoopStatus::Completed | LoopStatus::Failed | LoopStatus::Cancelled
        )
    }

    /// Is this a resumable stopped status?
    pub fn is_stopped(&self) -> bool {
        matches!(
            self,
            LoopStatus::Paused
                | LoopStatus::WaitingForApproval
                | LoopStatus::WaitingForInput
                | LoopStatus::BudgetExceeded
                | LoopStatus::TimedOut
        )
    }
}

// ---------------------------------------------------------------------------
// Loop data model
// ---------------------------------------------------------------------------

/// A Loop is the top-level unit of iterative work.
/// It owns many LoopRuns, each wrapping a Plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loop {
    pub id: String,
    pub goal: String,
    pub status: LoopStatus,
    pub config: LoopConfig,
    pub current_run_id: Option<String>,
    /// Unix timestamp (seconds).
    pub created_at: i64,
    pub updated_at: i64,
}

impl Loop {
    pub fn new(goal: &str, config: LoopConfig) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: Uuid::new_v4().to_string(),
            goal: goal.to_string(),
            status: LoopStatus::Pending,
            config,
            current_run_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

// ---------------------------------------------------------------------------
// LoopRun — a single iteration within a Loop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopRunStatus {
    Running,
    Completed,
    Failed,
}

/// A single iteration: one Plan execution + its evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRun {
    pub id: String,
    pub loop_id: String,
    pub iteration: u32,
    pub plan_id: String,
    pub status: LoopRunStatus,
    pub evaluation: Option<EvaluationResult>,
    pub decision: Option<LoopDecision>,
    pub token_usage: Option<TokenUsage>,
    /// Unix timestamp (seconds).
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

impl LoopRun {
    pub fn new(loop_id: &str, iteration: u32, plan_id: &str) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: Uuid::new_v4().to_string(),
            loop_id: loop_id.to_string(),
            iteration,
            plan_id: plan_id.to_string(),
            status: LoopRunStatus::Running,
            evaluation: None,
            decision: None,
            token_usage: None,
            started_at: now,
            finished_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation — the LLM's verdict on a LoopRun
// ---------------------------------------------------------------------------

/// Decision made by the LLM evaluator after examining the Plan's output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopDecision {
    /// Goal has been satisfied.
    #[serde(alias = "done")]
    Done,
    /// More work is needed; generate a correction plan.
    #[serde(alias = "continue")]
    Continue,
    /// The remaining work is too large; break it into sub-goals.
    #[serde(alias = "decompose")]
    Decompose,
    /// The goal is impossible or malformed.
    #[serde(alias = "impossible")]
    Impossible,
}

/// Structured output from the LLM evaluator.
/// Uses the A+C fusion approach: structured JSON + diff checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub verdict: LoopDecision,
    pub confidence: f64,
    /// Requirements satisfied, each with evidence.
    pub met: Vec<String>,
    /// Requirements NOT yet satisfied, each linked to the goal.
    pub unmet: Vec<String>,
    /// Problems discovered that the goal didn't mention.
    pub new_issues: Vec<String>,
    /// Concrete next action for the correction plan.
    pub next_action: String,
}

impl EvaluationResult {
    /// Conservative fallback used when evaluation itself fails.
    pub fn conservative_fallback() -> Self {
        Self {
            verdict: LoopDecision::Continue,
            confidence: 0.0,
            met: vec![],
            unmet: vec!["评估失败，需要人工复查".into()],
            new_issues: vec![],
            next_action: "人工复查本轮结果".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Budget tracking  (extracted to crate::budget_tracker)
// ---------------------------------------------------------------------------

pub use crate::budget_tracker::{BudgetStatus, BudgetTracker};

// ---------------------------------------------------------------------------
// Convergence guard utilities (pure functions, testable without LLM)
// ---------------------------------------------------------------------------

/// Returns true if the last 3 decisions form an oscillation pattern.
/// Oscillation = [Done, Continue, Done] or [Continue, Done, Continue].
pub fn detect_oscillation(history: &[LoopDecision]) -> bool {
    if history.len() < 3 {
        return false;
    }
    let last3 = &history[history.len() - 3..];
    matches!(
        last3,
        [
            LoopDecision::Done,
            LoopDecision::Continue,
            LoopDecision::Done
        ]
    ) || matches!(
        last3,
        [
            LoopDecision::Continue,
            LoopDecision::Done,
            LoopDecision::Continue
        ]
    )
}

/// Returns true if the unmet count has not decreased over the last N rounds.
/// `unmet_counts` is in chronological order (oldest first).
pub fn detect_stall(unmet_counts: &[usize]) -> bool {
    if unmet_counts.len() < 3 {
        return false;
    }
    let len = unmet_counts.len();
    let a = unmet_counts[len - 3];
    let b = unmet_counts[len - 2];
    let c = unmet_counts[len - 1];
    // Non-decreasing and the last value > 0 (still work to do but no progress)
    a <= b && b <= c && c > 0
}

/// Consistency check: if the previous evaluation had unmet items that are
/// completely absent from the current `met` list, the verdict is suspicious.
/// Returns the list of vanished unmet items.
pub fn vanished_unmet(prev_unmet: &[String], current_met: &[String]) -> Vec<String> {
    prev_unmet
        .iter()
        .filter(|u| !current_met.iter().any(|m| fuzzy_match(m, u)))
        .cloned()
        .collect()
}

/// Fuzzy token-based match for comparing evaluation items.
/// Extracts meaningful tokens (alphabetic words + individual CJK chars)
/// and checks for overlap between the two strings.
fn fuzzy_match(a: &str, b: &str) -> bool {
    fn tokens(s: &str) -> Vec<String> {
        let s = s.to_lowercase();
        let mut result = Vec::new();
        let mut alpha_buf = String::new();

        for c in s.chars() {
            if is_cjk(c) {
                // Flush alpha buffer
                if !alpha_buf.is_empty() && !is_stop_word(&alpha_buf) {
                    result.push(alpha_buf.clone());
                }
                alpha_buf.clear();
                // CJK characters become individual tokens
                if !is_stop_word_char(c) {
                    result.push(c.to_string());
                }
            } else if c.is_ascii_alphabetic() || c.is_ascii_digit() {
                alpha_buf.push(c);
            } else {
                // Non-CJK, non-alpha separator — flush buffer
                if !alpha_buf.is_empty() && !is_stop_word(&alpha_buf) {
                    result.push(alpha_buf.clone());
                }
                alpha_buf.clear();
            }
        }
        // Flush remaining
        if !alpha_buf.is_empty() && !is_stop_word(&alpha_buf) {
            result.push(alpha_buf);
        }
        result
    }

    fn is_cjk(c: char) -> bool {
        ('\u{4E00}'..='\u{9FFF}').contains(&c)
            || ('\u{3400}'..='\u{4DBF}').contains(&c)
            || ('\u{F900}'..='\u{FAFF}').contains(&c)
    }

    fn is_stop_word(t: &str) -> bool {
        matches!(
            t,
            "a" | "an"
                | "the"
                | "is"
                | "are"
                | "was"
                | "were"
                | "to"
                | "for"
                | "of"
                | "in"
                | "on"
                | "at"
                | "by"
                | "and"
                | "or"
                | "not"
                | "but"
                | "it"
                | "be"
                | "has"
                | "have"
                | "do"
                | "does"
                | "with"
                | "from"
                | "this"
                | "that"
                | "these"
                | "those"
                | "can"
                | "will"
                | "should"
                | "could"
                | "would"
                | "may"
                | "just"
        )
    }

    fn is_stop_word_char(c: char) -> bool {
        matches!(
            c,
            '的' | '了' | '是' | '在' | '和' | '与' | '或' | '不' | '也' | '就' | '都'
        )
    }

    let tokens_a = tokens(a);
    let tokens_b = tokens(b);

    if tokens_a.is_empty() || tokens_b.is_empty() {
        return false;
    }

    // At least one meaningful token must match
    tokens_b.iter().any(|tb| tokens_a.contains(tb))
}

/// Sanitise a Done verdict: if confidence is too low or unmet items vanished,
/// downgrade to Continue.
pub fn sanitise_verdict(result: &mut EvaluationResult, prev_unmet: Option<&[String]>) {
    // Rule 1: low confidence Done → Continue
    if result.verdict == LoopDecision::Done && result.confidence < 0.7 {
        result.verdict = LoopDecision::Continue;
        result.unmet.push("置信度过低，需要重新验证".into());
    }

    // Rule 2: vanished unmet items → Continue
    if let Some(prev) = prev_unmet {
        let vanished = vanished_unmet(prev, &result.met);
        if !vanished.is_empty() && result.verdict == LoopDecision::Done {
            result.verdict = LoopDecision::Continue;
            result.unmet.extend(vanished);
        }
    }
}

// ---------------------------------------------------------------------------
// LoopEngine — orchestrates the loop lifecycle
// ---------------------------------------------------------------------------

use crate::agent::Agent;
use crate::memory_cache::MemoryCache;
use crate::safety::SafetyContext;

/// In-memory state for an actively running Loop.
#[allow(dead_code)] // fields used in later phases
struct LoopState {
    loop_data: Loop,
    current_run: LoopRun,
    agent: Arc<Agent>,
    /// Child loop IDs created during decomposition (this iteration only).
    child_loops: Vec<String>,
}

pub struct LoopEngine {
    repo: Arc<TaskRepo>,
    #[allow(dead_code)]
    memory: Arc<MemoryCache>,
    #[allow(dead_code)]
    safety: SafetyContext,
    cancel_flag: Arc<AtomicBool>,
    /// Tool executor for dispatching ToolCall steps in child loops.
    /// Injected via `new()` to avoid circular Agent<->LoopEngine reference.
    tool_executor: Arc<dyn ToolExecutor>,
}

impl LoopEngine {
    /// Create a new LoopEngine.
    ///
    /// # Preconditions
    /// - `tool_executor` must be a valid `Arc<dyn ToolExecutor>` implementation.
    ///
    /// # Postconditions
    /// - The engine is ready to execute adaptive loops including child loop step dispatch.
    pub fn new(
        repo: Arc<TaskRepo>,
        memory: Arc<MemoryCache>,
        safety: SafetyContext,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        Self {
            repo,
            memory,
            safety,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            tool_executor,
        }
    }

    /// Cancel all running loops (used on shutdown).
    pub fn cancel_all(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Create and start a new Loop. Sets up DB state, then enters the main
    /// execution loop. Takes `&self` so callers can release their lock before
    /// the long-running await.
    pub async fn start_loop(
        &self,
        goal: &str,
        config: LoopConfig,
        agent: Arc<Agent>,
        llm: Option<&LlmGateway>,
    ) -> AgentResult<Loop> {
        // Reset global cancel flag for fresh start
        self.cancel_flag.store(false, Ordering::SeqCst);
        let mut loop_data = Loop::new(goal, config);

        // Step 1: generate initial Plan
        let plan = if let Some(gw) = llm {
            let step_specs = gw.generate_plan(goal).await?;
            Self::step_specs_to_plan(goal, &step_specs)
        } else {
            return Err(AgentError::Config("no LLM gateway available".into()));
        };

        // Step 2: save Plan to DB
        self.repo.save_plan(&plan).await?;

        // Step 3: save Loop first (so LoopRun can reference it via FK)
        let loop_id = loop_data.id.clone();
        loop_data.status = LoopStatus::Running;
        self.repo.save_loop(&loop_data).await?;

        // Step 4: create and save first LoopRun
        let current_run = LoopRun::new(&loop_id, 0, &plan.id);
        self.repo.save_loop_run(&current_run).await?;

        loop_data.current_run_id = Some(current_run.id.clone());
        self.repo
            .update_loop_status(&loop_id, &LoopStatus::Running, Some(&current_run.id))
            .await?;

        info!(loop_id = %loop_id, goal = %goal, "loop started");

        // Step 5: enter main loop with explicit loop_id
        self.run_loop(&loop_id, agent, llm).await
    }

    /// Resume a stopped Loop from its last checkpoint.
    pub async fn resume_loop(
        &self,
        loop_id: &str,
        agent: Arc<Agent>,
        llm: Option<&LlmGateway>,
    ) -> AgentResult<Loop> {
        let mut loop_data = self.repo.load_loop(loop_id).await?;

        // Validate resumable status
        if loop_data.status.is_terminal() {
            return Err(AgentError::Other(format!(
                "loop {} is in terminal status {:?}, cannot resume",
                loop_id, loop_data.status
            )));
        }

        // Find latest LoopRun
        let latest_run = self
            .repo
            .get_latest_loop_run(loop_id)
            .await?
            .ok_or_else(|| AgentError::Other(format!("no runs found for loop {loop_id}")))?;

        // Determine resume point
        match &loop_data.status {
            LoopStatus::BudgetExceeded | LoopStatus::TimedOut => {
                // Check if budget has recovered
                let tracker = BudgetTracker {
                    total_tokens: 0, // will be recalculated
                    started_at: chrono::Utc::now().timestamp(),
                };
                match tracker.check(
                    loop_data.config.token_budget,
                    loop_data.config.time_budget_secs,
                ) {
                    BudgetStatus::Ok => {
                        // Budget recovered — go straight to evaluating the last run
                        loop_data.status = LoopStatus::Evaluating;
                    }
                    _ => {
                        return Err(AgentError::Other(
                            "budget still exceeded; cannot resume".into(),
                        ));
                    }
                }
            }
            LoopStatus::Running | LoopStatus::Evaluating | LoopStatus::StepComplete => {
                // Crashed mid-loop — restart from this run's Plan checkpoint
                loop_data.status = LoopStatus::Running;
            }
            LoopStatus::Paused | LoopStatus::WaitingForApproval | LoopStatus::WaitingForInput => {
                // User-initiated pause — resume as-is
            }
            _ => {}
        }

        loop_data.current_run_id = Some(latest_run.id.clone());
        self.repo
            .update_loop_status(loop_id, &loop_data.status, Some(&latest_run.id))
            .await?;

        info!(loop_id = %loop_id, status = ?loop_data.status, "loop resumed");

        let lid = loop_id.to_string();
        self.run_loop(&lid, agent, llm).await
    }

    /// Pause an active Loop.
    pub async fn pause_loop(&self, loop_id: &str) -> AgentResult<()> {
        self.repo
            .update_loop_status(loop_id, &LoopStatus::Paused, None)
            .await
    }

    /// Cancel an active Loop. Also sets the global cancel flag so the
    /// in-flight run_loop picks it up at the next check.
    pub async fn cancel_loop(&self, loop_id: &str) -> AgentResult<()> {
        self.cancel_flag.store(true, Ordering::SeqCst);
        self.repo
            .update_loop_status(loop_id, &LoopStatus::Cancelled, None)
            .await
    }

    /// Get the current status of a Loop (with its latest evaluation).
    pub async fn get_loop_status(&self, loop_id: &str) -> AgentResult<Loop> {
        self.repo.load_loop(loop_id).await
    }

    /// Approve a waiting Loop (move from WaitingForApproval → Running).
    pub async fn approve_loop(&self, loop_id: &str) -> AgentResult<()> {
        let loop_data = self.repo.load_loop(loop_id).await?;
        if loop_data.status != LoopStatus::WaitingForApproval {
            return Err(AgentError::Other(format!(
                "loop {} is not waiting for approval",
                loop_id
            )));
        }
        self.repo
            .update_loop_status(loop_id, &LoopStatus::Running, None)
            .await
    }

    /// Deny a waiting Loop (move from WaitingForApproval → Cancelled).
    pub async fn deny_loop(&self, loop_id: &str) -> AgentResult<()> {
        let loop_data = self.repo.load_loop(loop_id).await?;
        if loop_data.status != LoopStatus::WaitingForApproval {
            return Err(AgentError::Other(format!(
                "loop {} is not waiting for approval",
                loop_id
            )));
        }
        self.repo
            .update_loop_status(loop_id, &LoopStatus::Cancelled, None)
            .await
    }

    /// List all loops.
    pub async fn list_loops(&self, limit: usize, offset: usize) -> AgentResult<Vec<Loop>> {
        self.repo.list_loops(limit, offset).await
    }

    // -----------------------------------------------------------------------
    // Main execution loop (private)
    // -----------------------------------------------------------------------

    async fn run_loop(
        &self,
        loop_id: &str,
        agent: Arc<Agent>,
        llm: Option<&LlmGateway>,
    ) -> AgentResult<Loop> {
        // This is the core loop. It drives the state machine for a single Loop.
        // Takes `&self` so callers can release their mutex before the long-running await.

        // Check daemon mode at entry — warn if enabled since it's not yet implemented.
        let loop_config = self.repo.load_loop(loop_id).await?.config;
        if loop_config.daemon {
            warn!("守护模式（daemon=true）尚未完全实现，回退到标准循环模式");
        }

        loop {
            // --- Guard checks ---
            if self.is_cancelled() {
                self.repo
                    .update_loop_status(loop_id, &LoopStatus::Cancelled, None)
                    .await?;
                return Ok(Loop {
                    id: loop_id.to_string(),
                    goal: String::new(),
                    status: LoopStatus::Cancelled,
                    config: LoopConfig::default(),
                    current_run_id: None,
                    created_at: 0,
                    updated_at: 0,
                });
            }

            let mut loop_data = self.repo.load_loop(loop_id).await?;

            // Budget check
            let tracker = BudgetTracker {
                total_tokens: self.compute_total_tokens(loop_id).await?,
                started_at: loop_data.created_at,
            };
            match tracker.check(
                loop_data.config.token_budget,
                loop_data.config.time_budget_secs,
            ) {
                BudgetStatus::TokenExceeded { .. } => {
                    self.repo
                        .update_loop_status(loop_id, &LoopStatus::BudgetExceeded, None)
                        .await?;
                    loop_data.status = LoopStatus::BudgetExceeded;
                    return Ok(loop_data);
                }
                BudgetStatus::TimeExceeded { .. } => {
                    self.repo
                        .update_loop_status(loop_id, &LoopStatus::TimedOut, None)
                        .await?;
                    loop_data.status = LoopStatus::TimedOut;
                    return Ok(loop_data);
                }
                BudgetStatus::Ok => {}
            }

            // Iteration cap — allow up to max_iterations total LoopRuns
            let total_iters = self.repo.count_loop_runs(loop_id).await?;
            if total_iters > loop_data.config.max_iterations {
                self.repo
                    .update_loop_status(loop_id, &LoopStatus::Failed, None)
                    .await?;
                loop_data.status = LoopStatus::Failed;
                warn!(loop_id = %loop_id, iterations = total_iters, "loop exhausted max iterations");
                return Ok(loop_data);
            }

            // Oscillation / stall detection
            let recent_decisions = self.repo.recent_loop_decisions(loop_id, 3).await?;
            if detect_oscillation(&recent_decisions) {
                self.repo
                    .update_loop_status(loop_id, &LoopStatus::Paused, None)
                    .await?;
                loop_data.status = LoopStatus::Paused;
                warn!(loop_id = %loop_id, "loop oscillating, paused for human intervention");
                return Ok(loop_data);
            }
            let recent_unmet = self.repo.recent_unmet_counts(loop_id, 3).await?;
            if detect_stall(&recent_unmet) {
                self.repo
                    .update_loop_status(loop_id, &LoopStatus::Paused, None)
                    .await?;
                loop_data.status = LoopStatus::Paused;
                warn!(loop_id = %loop_id, "loop stalled (no progress in unmet count), paused");
                return Ok(loop_data);
            }

            // --- State machine ---
            match &loop_data.status {
                LoopStatus::Running => {
                    // Execute the Plan wrapped by the current LoopRun
                    let current_run = self
                        .repo
                        .get_latest_loop_run(loop_id)
                        .await?
                        .ok_or_else(|| AgentError::Other("no loop run".into()))?;

                    let mut plan = self.repo.load_plan(&current_run.plan_id).await?;

                    // Execute plan steps until terminal
                    let plan_result = self.execute_plan(&agent, &mut plan).await;

                    match plan_result {
                        Ok(()) => {
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::StepComplete, None)
                                .await?;
                        }
                        Err(e) => {
                            // Step failure — let evaluation decide
                            warn!(loop_id = %loop_id, error = %e, "plan step failed, moving to evaluation");
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::StepComplete, None)
                                .await?;
                        }
                    }
                }

                LoopStatus::StepComplete => {
                    // Approval gating: check autonomy level before proceeding
                    let needs_round_approval = matches!(
                        loop_data.config.autonomy_level,
                        AutonomyLevel::L1Manual
                            | AutonomyLevel::L2StepCheck
                            | AutonomyLevel::L3RoundCheck
                    );

                    if needs_round_approval {
                        self.repo
                            .update_loop_status(loop_id, &LoopStatus::WaitingForApproval, None)
                            .await?;
                        loop_data.status = LoopStatus::WaitingForApproval;
                        info!(
                            loop_id = %loop_id,
                            autonomy = ?loop_data.config.autonomy_level,
                            "loop waiting for user approval before evaluation"
                        );
                    } else {
                        // L4AutoCorrect or L5FullAuto — proceed directly
                        self.repo
                            .update_loop_status(loop_id, &LoopStatus::Evaluating, None)
                            .await?;
                    }
                }

                LoopStatus::Evaluating => {
                    // Run the LLM evaluation
                    let llm = llm.ok_or_else(|| {
                        AgentError::Config("no LLM gateway for evaluation".into())
                    })?;

                    let current_run = self
                        .repo
                        .get_latest_loop_run(loop_id)
                        .await?
                        .ok_or_else(|| AgentError::Other("no loop run to evaluate".into()))?;

                    // Load previous evaluation for continuity
                    let prev_eval = if current_run.iteration > 0 {
                        self.load_prev_evaluation(loop_id, current_run.iteration)
                            .await?
                    } else {
                        None
                    };

                    let mut eval = self
                        .evaluate(llm, &loop_data.goal, &current_run, prev_eval.as_ref())
                        .await?;

                    // Sanitise the verdict
                    let prev_unmet: Option<Vec<String>> =
                        prev_eval.as_ref().map(|e| e.unmet.clone());
                    sanitise_verdict(&mut eval, prev_unmet.as_deref());

                    // Save evaluation result
                    self.repo
                        .update_loop_run_result(
                            &current_run.id,
                            &LoopRunStatus::Completed,
                            Some(&eval),
                            Some(&eval.verdict),
                            None,
                        )
                        .await?;

                    // Act on the verdict
                    match eval.verdict {
                        LoopDecision::Done => {
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Completed, None)
                                .await?;
                            loop_data.status = LoopStatus::Completed;
                            info!(loop_id = %loop_id, "loop completed");
                            return Ok(loop_data);
                        }
                        LoopDecision::Continue => {
                            // Generate correction plan
                            let correction_plan = self
                                .generate_correction_plan(llm, &loop_data.goal, &eval)
                                .await?;
                            self.repo.save_plan(&correction_plan).await?;

                            let next_iter = current_run.iteration + 1;
                            let new_run = LoopRun::new(loop_id, next_iter, &correction_plan.id);
                            self.repo.save_loop_run(&new_run).await?;
                            self.repo
                                .update_loop_status(
                                    loop_id,
                                    &LoopStatus::Running,
                                    Some(&new_run.id),
                                )
                                .await?;
                        }
                        LoopDecision::Decompose => {
                            // Phase B: recursive decomposition
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Decomposing, None)
                                .await?;
                            info!(loop_id = %loop_id, unmet_count = eval.unmet.len(), "decomposing goal into sub-goals");
                        }
                        LoopDecision::Impossible => {
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Failed, None)
                                .await?;
                            loop_data.status = LoopStatus::Failed;
                            warn!(loop_id = %loop_id, "loop deemed impossible by evaluator");
                            return Ok(loop_data);
                        }
                    }
                }

                LoopStatus::Decomposing => {
                    // Phase B: recursive decomposition
                    let llm = llm.ok_or_else(|| {
                        AgentError::Config("no LLM gateway for decomposition".into())
                    })?;

                    // Load the evaluation that triggered decomposition
                    let trigger_run = self
                        .repo
                        .get_latest_loop_run(loop_id)
                        .await?
                        .ok_or_else(|| AgentError::Other("no loop run for decomposition".into()))?;

                    let eval = trigger_run.evaluation.as_ref().ok_or_else(|| {
                        AgentError::Other("no evaluation to drive decomposition".into())
                    })?;

                    // 1. Generate sub-goals
                    let sub_goals = self
                        .decompose_goal(llm, &loop_data.goal, &eval.unmet)
                        .await?;

                    info!(
                        loop_id = %loop_id,
                        sub_goal_count = sub_goals.len(),
                        "decomposed into sub-goals"
                    );

                    // 2. Compute budget inheritance
                    let total_tokens = self.compute_total_tokens(loop_id).await.unwrap_or(0);
                    let child_budget = loop_data.config.token_budget.map(|b| {
                        let remaining = b.saturating_sub(total_tokens);
                        (remaining / sub_goals.len().max(1) as u64).max(1)
                    });
                    let child_time_budget = loop_data.config.time_budget_secs.map(|t| {
                        let elapsed =
                            (chrono::Utc::now().timestamp() - loop_data.created_at).max(0) as u64;
                        let remaining = t.saturating_sub(elapsed);
                        (remaining / sub_goals.len().max(1) as u64).max(1)
                    });

                    // 3. Execute child loops sequentially (parallel_decomposition: false default)
                    let mut child_results: Vec<(String, AgentResult<Loop>)> = Vec::new();

                    for (i, sub_goal) in sub_goals.iter().enumerate() {
                        if self.is_cancelled() {
                            break;
                        }

                        let mut child_config = LoopConfig {
                            max_iterations: loop_data
                                .config
                                .max_iterations
                                .saturating_sub(sub_goals.len() as u32 * 2),
                            token_budget: child_budget,
                            time_budget_secs: child_time_budget,
                            ..Default::default()
                        };
                        child_config.max_iterations = child_config.max_iterations.max(3);

                        let child_loop = Loop::new(sub_goal, child_config);
                        self.repo.save_loop(&child_loop).await?;

                        // Generate initial plan for child
                        let child_plan = {
                            let step_specs = llm.generate_plan(sub_goal).await?;
                            Self::step_specs_to_plan(sub_goal, &step_specs)
                        };
                        self.repo.save_plan(&child_plan).await?;

                        let child_run = LoopRun::new(&child_loop.id, 0, &child_plan.id);
                        self.repo.save_loop_run(&child_run).await?;
                        self.repo
                            .update_loop_status(
                                &child_loop.id,
                                &LoopStatus::Running,
                                Some(&child_run.id),
                            )
                            .await?;

                        info!(
                            parent = %loop_id,
                            child = %child_loop.id,
                            index = i,
                            goal = %sub_goal,
                            "executing child loop"
                        );

                        // Track child in the parent's LoopRun (via a separate tracking mechanism)
                        // For now, we drive the child loop inline by recursing into run_loop_inner
                        let result = self.run_child_loop(&child_loop.id, llm).await;
                        child_results.push((sub_goal.clone(), result));
                    }

                    // 4. Aggregate child results
                    let completed_children: Vec<Loop> = child_results
                        .iter()
                        .filter_map(|(_, r)| r.as_ref().ok().cloned())
                        .filter(|l| l.status == LoopStatus::Completed)
                        .collect();

                    let failed_count = child_results.len() - completed_children.len();

                    let aggregate_summary = self
                        .aggregate_children(llm, &loop_data.goal, &completed_children)
                        .await?;

                    // 5. Create a synthetic evaluation from the aggregation
                    let agg_eval = EvaluationResult {
                        verdict: if failed_count == 0 {
                            LoopDecision::Done
                        } else if completed_children.is_empty() {
                            LoopDecision::Impossible
                        } else {
                            LoopDecision::Continue
                        },
                        confidence: if completed_children.len() >= child_results.len() / 2 {
                            0.8
                        } else {
                            0.4
                        },
                        met: completed_children
                            .iter()
                            .map(|c| format!("{}: completed", c.goal))
                            .collect(),
                        unmet: if failed_count > 0 {
                            vec![format!("{} sub-goals failed to complete", failed_count)]
                        } else {
                            vec![]
                        },
                        new_issues: vec![],
                        next_action: aggregate_summary,
                    };

                    // Save the aggregated evaluation on the parent run
                    self.repo
                        .update_loop_run_result(
                            &trigger_run.id,
                            &LoopRunStatus::Completed,
                            Some(&agg_eval),
                            Some(&agg_eval.verdict),
                            None,
                        )
                        .await?;

                    // Act on aggregated verdict directly — don't re-evaluate via LLM
                    match agg_eval.verdict {
                        LoopDecision::Done => {
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Completed, None)
                                .await?;
                            loop_data.status = LoopStatus::Completed;
                            info!(loop_id = %loop_id, children = completed_children.len(), "loop completed via decomposition");
                            return Ok(loop_data);
                        }
                        LoopDecision::Impossible => {
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Failed, None)
                                .await?;
                            loop_data.status = LoopStatus::Failed;
                            warn!(loop_id = %loop_id, "loop deemed impossible after decomposition");
                            return Ok(loop_data);
                        }
                        LoopDecision::Continue | LoopDecision::Decompose => {
                            // There's remaining work — go back to Evaluating for
                            // a fresh LLM assessment of what to do next.
                            self.repo
                                .update_loop_status(loop_id, &LoopStatus::Evaluating, None)
                                .await?;
                            info!(
                                loop_id = %loop_id,
                                completed = completed_children.len(),
                                failed = failed_count,
                                "child loops completed, continuing evaluation"
                            );
                        }
                    }
                }

                // Stopped states — exit the loop
                LoopStatus::Paused
                | LoopStatus::WaitingForApproval
                | LoopStatus::WaitingForInput
                | LoopStatus::BudgetExceeded
                | LoopStatus::TimedOut
                | LoopStatus::Completed
                | LoopStatus::Failed
                | LoopStatus::Cancelled => {
                    return Ok(loop_data);
                }

                LoopStatus::Pending => {
                    // Should not reach here
                    self.repo
                        .update_loop_status(loop_id, &LoopStatus::Running, None)
                        .await?;
                }
            }
        }
    }

    /// Execute a Plan step-by-step, returning Ok(()) on successful completion
    /// or the error from a failed step (so the evaluator can decide).
    async fn execute_plan(
        &self,
        agent: &Arc<Agent>,
        plan: &mut crate::task::Plan,
    ) -> AgentResult<()> {
        use crate::task::PlanStatus;

        // Resume from checkpoint if needed
        if plan.status == PlanStatus::Running || plan.status == PlanStatus::Pending {
            plan.status = PlanStatus::Running;
        }

        loop {
            if self.is_cancelled() {
                return Err(AgentError::Other("cancelled".into()));
            }

            let outcome = agent.run_next_step(plan).await?;

            match outcome {
                crate::agent::StepOutcome::Advanced => continue,
                crate::agent::StepOutcome::Finished => return Ok(()),
                crate::agent::StepOutcome::WaitingForInput(_) => {
                    return Err(AgentError::Other("plan requires input".into()));
                }
                crate::agent::StepOutcome::RequiresApproval { .. } => {
                    return Err(AgentError::ToolRequiresApproval {
                        name: "unknown".into(),
                        params: serde_json::Value::Null,
                    });
                }
                crate::agent::StepOutcome::Failed(msg) => {
                    return Err(AgentError::Other(format!("step failed: {msg}")));
                }
            }
        }
    }

    /// Run LLM evaluation of the current LoopRun's result.
    async fn evaluate(
        &self,
        llm: &LlmGateway,
        goal: &str,
        run: &LoopRun,
        prev_eval: Option<&EvaluationResult>,
    ) -> AgentResult<EvaluationResult> {
        let plan = self.repo.load_plan(&run.plan_id).await?;

        // Build compressed context (avoid linear context growth)
        let prev_unmet_str = prev_eval.map(|e| e.unmet.join("\n")).unwrap_or_default();

        let plan_summary = plan.name.clone();

        // Compress the actual output: first 200 + last 200 chars + step count
        let actual_output = self.compress_plan_output(&plan);

        let eval_prompt = format!(
            r#"You are a rigorous technical evaluator. Compare actual execution results against the stated goal. Be precise and skeptical.

[RULES]
1. "met" must cite specific evidence from the actual output
2. "unmet" must reference a specific requirement from the goal
3. "new_issues" are problems you discovered that the goal didn't mention but are clearly wrong (security flaws, bugs, incomplete logic)
4. Prefer "decompose" over "continue" for complex goals
5. Only verdict="done" if ALL aspects of the goal are satisfied with evidence
6. Do not inflate scores. confidence=0.95 means you are 95% certain, not that the result "looks good"

[INPUT]
Goal: {goal}
Previous unmet items: {prev_unmet}
Plan summary: {plan_summary}
Actual results: {actual_output}

[OUTPUT FORMAT - JSON]
{{
  "verdict": "done" | "continue" | "decompose" | "impossible",
  "confidence": 0.0-1.0,
  "met": ["specific requirement → evidence from output"],
  "unmet": ["specific requirement → what's missing"],
  "new_issues": ["problem found → why it matters"],
  "next_action": "concrete next step, or empty if done"
}}"#,
            goal = goal,
            prev_unmet = prev_unmet_str,
            plan_summary = plan_summary,
            actual_output = actual_output,
        );

        // Try evaluation with retry (max 2 retries)
        const MAX_EVAL_RETRIES: u32 = 2;
        let mut _last_err = None;

        for attempt in 0..=MAX_EVAL_RETRIES {
            let messages = vec![
                crate::llm::LlmChatMessage::system(
                    "You are a rigorous technical evaluator. You MUST respond with ONLY valid JSON, no other text."
                ),
                crate::llm::LlmChatMessage::user(&eval_prompt),
            ];

            match llm.chat(&messages).await {
                Ok((response_text, _usage)) => {
                    // Parse JSON from the response
                    match serde_json::from_str::<EvaluationResult>(response_text.trim()) {
                        Ok(result) => {
                            // Sanity check
                            if result.met.is_empty()
                                && result.unmet.is_empty()
                                && result.verdict == LoopDecision::Done
                                && attempt < MAX_EVAL_RETRIES
                            {
                                warn!(attempt, "empty evaluation with Done verdict, retrying");
                                continue;
                            }
                            return Ok(result);
                        }
                        Err(parse_err) => {
                            warn!(attempt, error = %parse_err, "failed to parse evaluation JSON");
                            _last_err = Some(AgentError::Other(format!(
                                "evaluation JSON parse error: {parse_err}"
                            )));
                            if attempt < MAX_EVAL_RETRIES {
                                tokio::time::sleep(std::time::Duration::from_secs(
                                    2u64.pow(attempt),
                                ))
                                .await;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(attempt, error = %e, "evaluation attempt failed");
                    _last_err = Some(e);
                    if attempt < MAX_EVAL_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt))).await;
                    }
                }
            }
        }

        // All retries exhausted — conservative fallback
        warn!("all evaluation retries exhausted, using conservative fallback");
        Ok(EvaluationResult::conservative_fallback())
    }

    /// Generate a focused correction plan targeting only the unmet items and new issues.
    async fn generate_correction_plan(
        &self,
        llm: &LlmGateway,
        goal: &str,
        eval: &EvaluationResult,
    ) -> AgentResult<crate::task::Plan> {
        let met_str = eval.met.join("\n");
        let unmet_str = eval.unmet.join("\n");
        let issues_str = eval.new_issues.join("\n");

        let prompt = format!(
            r#"Generate a focused plan that ONLY addresses the missing items and new problems.
Do NOT include steps for things already done.

Original goal: {goal}
ALREADY DONE (do NOT redo):
{met}

MISSING (focus ONLY on these):
{unmet}

NEW PROBLEMS to fix:
{issues}

Suggested approach: {next_action}"#,
            goal = goal,
            met = met_str,
            unmet = unmet_str,
            issues = issues_str,
            next_action = eval.next_action,
        );

        let step_specs = llm.generate_plan(&prompt).await?;
        Ok(Self::step_specs_to_plan(goal, &step_specs))
    }

    /// Convert StepSpecs (from generate_plan) into a Plan with proper Step variants.
    fn step_specs_to_plan(name: &str, specs: &[crate::llm::StepSpec]) -> crate::task::Plan {
        let steps: Vec<crate::task::Step> = specs
            .iter()
            .map(|spec| match spec.step_type.as_str() {
                "think" => crate::task::think_step(&spec.instruction),
                "exec" => {
                    // Extract command from params.command if available,
                    // otherwise fall back to tool_name or instruction
                    let cmd = spec
                        .params
                        .as_object()
                        .and_then(|m| m.get("command"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            let t = spec.tool_name.as_str();
                            if !t.is_empty() && t != "shell_exec" && t != "exec" {
                                Some(t)
                            } else {
                                None
                            }
                        })
                        .unwrap_or("bash");
                    let args = spec
                        .params
                        .as_object()
                        .and_then(|m| m.get("args"))
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let timeout = spec
                        .params
                        .as_object()
                        .and_then(|m| m.get("timeout"))
                        .and_then(|v| v.as_u64());
                    crate::task::exec_step(cmd, args, timeout)
                }
                "file_read" => crate::task::tool_call_step(
                    "file_read",
                    serde_json::json!({"path": spec.instruction}),
                ),
                "file_write" => crate::task::tool_call_step(
                    "file_write",
                    serde_json::json!({"path": spec.instruction}),
                ),
                "finish" => crate::task::finish_step(&spec.summary),
                "wait_for_input" => crate::task::wait_for_input_step(&spec.prompt),
                _ => crate::task::think_step(&spec.instruction),
            })
            .collect();

        let label: String = name.chars().take(40).collect();
        crate::task::Plan::new(&label, steps)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Compress a Plan's output into a compact string suitable for evaluation context.
    fn compress_plan_output(&self, plan: &crate::task::Plan) -> String {
        let step_count = plan.steps.len();
        let mut summary = format!("Plan '{}': {} steps\n", plan.name, step_count);

        for (i, step) in plan.steps.iter().enumerate() {
            let status = step.status();
            let detail = match step {
                crate::task::Step::Think { output, .. } => output.clone().unwrap_or_default(),
                crate::task::Step::ToolCall { result, .. } => {
                    result.as_ref().map(|r| r.to_string()).unwrap_or_default()
                }
                crate::task::Step::Exec { output, .. } => output.clone().unwrap_or_default(),
                crate::task::Step::HttpRequest { response, .. } => {
                    response.clone().unwrap_or_default()
                }
                crate::task::Step::Finish { summary, .. } => summary.clone(),
                crate::task::Step::WaitForInput { .. } => "(waiting for input)".into(),
                crate::task::Step::BrowserAction { output, .. } => {
                    output.clone().unwrap_or_default()
                }
            };

            // Truncate each step's detail to head+tail
            let compressed = if detail.len() > 400 {
                let head: String = detail.chars().take(200).collect();
                let tail: String = detail
                    .chars()
                    .rev()
                    .take(200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("{}...{}", head, tail)
            } else {
                detail
            };

            summary.push_str(&format!(
                "  Step {} [{:?}]: {}\n",
                i + 1,
                status,
                compressed,
            ));
        }

        summary
    }

    /// Load the previous iteration's evaluation for continuity checking.
    async fn load_prev_evaluation(
        &self,
        loop_id: &str,
        current_iteration: u32,
    ) -> AgentResult<Option<EvaluationResult>> {
        if current_iteration == 0 {
            return Ok(None);
        }
        let prev_iter = current_iteration - 1;
        self.repo
            .load_loop_run_by_iteration(loop_id, prev_iter)
            .await
    }

    /// Compute total tokens consumed across ALL LoopRuns in this Loop.
    async fn compute_total_tokens(&self, loop_id: &str) -> AgentResult<u64> {
        self.repo.sum_loop_run_tokens(loop_id).await
    }

    // -----------------------------------------------------------------------
    // Phase B: recursive decomposition
    // -----------------------------------------------------------------------

    /// Run a child loop (simplified child execution, takes &self).
    async fn run_child_loop(&self, child_loop_id: &str, llm: &LlmGateway) -> AgentResult<Loop> {
        let mut loop_data = self.repo.load_loop(child_loop_id).await?;
        let max_iterations = loop_data.config.max_iterations;

        for _round in 0..max_iterations {
            if self.is_cancelled() {
                self.repo
                    .update_loop_status(child_loop_id, &LoopStatus::Cancelled, None)
                    .await?;
                loop_data.status = LoopStatus::Cancelled;
                return Ok(loop_data);
            }

            loop_data = self.repo.load_loop(child_loop_id).await?;

            match &loop_data.status {
                LoopStatus::Running => {
                    let run = self
                        .repo
                        .get_latest_loop_run(child_loop_id)
                        .await?
                        .ok_or_else(|| AgentError::Other("no child run".into()))?;

                    let mut plan = self.repo.load_plan(&run.plan_id).await?;
                    let plan_result = self.execute_plan_inner(&mut plan).await;
                    if plan_result.is_err() {
                        warn!(child = %child_loop_id, "child plan step failed, moving to evaluation");
                    }
                    self.repo
                        .update_loop_status(child_loop_id, &LoopStatus::StepComplete, None)
                        .await?;
                }

                LoopStatus::StepComplete => {
                    self.repo
                        .update_loop_status(child_loop_id, &LoopStatus::Evaluating, None)
                        .await?;
                }

                LoopStatus::Evaluating => {
                    let run = self
                        .repo
                        .get_latest_loop_run(child_loop_id)
                        .await?
                        .ok_or_else(|| AgentError::Other("no child run to evaluate".into()))?;

                    let prev_eval = if run.iteration > 0 {
                        self.load_prev_evaluation(child_loop_id, run.iteration)
                            .await?
                    } else {
                        None
                    };

                    let prev_unmet: Option<Vec<String>> =
                        prev_eval.as_ref().map(|e| e.unmet.clone());

                    let mut eval = self
                        .evaluate(llm, &loop_data.goal, &run, prev_eval.as_ref())
                        .await?;

                    sanitise_verdict(&mut eval, prev_unmet.as_deref());

                    self.repo
                        .update_loop_run_result(
                            &run.id,
                            &LoopRunStatus::Completed,
                            Some(&eval),
                            Some(&eval.verdict),
                            None,
                        )
                        .await?;

                    match eval.verdict {
                        LoopDecision::Done => {
                            self.repo
                                .update_loop_status(child_loop_id, &LoopStatus::Completed, None)
                                .await?;
                            loop_data.status = LoopStatus::Completed;
                            return Ok(loop_data);
                        }
                        LoopDecision::Continue => {
                            let correction = self
                                .generate_correction_plan(llm, &loop_data.goal, &eval)
                                .await?;
                            self.repo.save_plan(&correction).await?;
                            let next_iter = run.iteration + 1;
                            let new_run = LoopRun::new(child_loop_id, next_iter, &correction.id);
                            self.repo.save_loop_run(&new_run).await?;
                            self.repo
                                .update_loop_status(
                                    child_loop_id,
                                    &LoopStatus::Running,
                                    Some(&new_run.id),
                                )
                                .await?;
                        }
                        LoopDecision::Decompose => {
                            // Children can decompose too — but to limit recursion depth,
                            // treat as Continue
                            warn!(child = %child_loop_id, "child loop requested decompose; treating as Continue to limit recursion");
                            let correction = self
                                .generate_correction_plan(llm, &loop_data.goal, &eval)
                                .await?;
                            self.repo.save_plan(&correction).await?;
                            let next_iter = run.iteration + 1;
                            let new_run = LoopRun::new(child_loop_id, next_iter, &correction.id);
                            self.repo.save_loop_run(&new_run).await?;
                            self.repo
                                .update_loop_status(
                                    child_loop_id,
                                    &LoopStatus::Running,
                                    Some(&new_run.id),
                                )
                                .await?;
                        }
                        LoopDecision::Impossible => {
                            self.repo
                                .update_loop_status(child_loop_id, &LoopStatus::Failed, None)
                                .await?;
                            loop_data.status = LoopStatus::Failed;
                            return Ok(loop_data);
                        }
                    }
                }

                // Terminal states for child loops
                s if s.is_terminal() || s.is_stopped() => {
                    return Ok(loop_data);
                }

                _ => {
                    self.repo
                        .update_loop_status(child_loop_id, &LoopStatus::Running, None)
                        .await?;
                }
            }
        }

        // Exhausted iterations
        self.repo
            .update_loop_status(child_loop_id, &LoopStatus::Failed, None)
            .await?;
        loop_data.status = LoopStatus::Failed;
        Ok(loop_data)
    }

    /// Execute plan steps for child loops via the injected ToolExecutor.
    ///
    /// # Preconditions
    /// - `plan` must be loaded from the database and contain at least one step.
    ///
    /// # Postconditions
    /// - On success, all steps are marked Completed and plan status is Completed.
    /// - On cancellation, returns Err(AgentError::Other("cancelled")).
    /// - ToolCall steps are dispatched via `tool_executor.execute_tool()`.
    /// - Non-tool steps (Think, Finish) are handled inline.
    ///
    /// # Panics
    /// - Does not panic; all errors are propagated via AgentResult.
    async fn execute_plan_inner(&self, plan: &mut crate::task::Plan) -> AgentResult<()> {
        if plan.status == PlanStatus::Pending || plan.status == PlanStatus::Running {
            plan.status = PlanStatus::Running;
        }

        let total_steps = plan.steps.len();

        for _step_idx in plan.current_step_index..total_steps {
            if self.is_cancelled() {
                return Err(AgentError::Other("cancelled".into()));
            }

            let current = plan.current_step_index;
            if current >= plan.steps.len() {
                break;
            }

            // Clone step data to avoid borrow conflict with plan mutation
            let step = plan.steps[current].clone();

            match step {
                Step::ToolCall {
                    tool_name, params, ..
                } => {
                    // Mark as running
                    plan.steps[current].set_status(StepStatus::Running);

                    match self.tool_executor.execute_tool(&tool_name, params).await {
                        Ok(result) => {
                            if result.is_success() {
                                plan.steps[current].set_status(StepStatus::Completed);
                            } else {
                                let msg = result.content();
                                warn!(
                                    tool = %tool_name,
                                    step = current,
                                    error = %msg,
                                    "tool call returned error in child loop"
                                );
                                plan.steps[current].set_status(StepStatus::Failed);
                            }
                        }
                        Err(e) => {
                            warn!(
                                tool = %tool_name,
                                step = current,
                                error = %e,
                                "tool execution failed in child loop"
                            );
                            plan.steps[current].set_status(StepStatus::Failed);
                            return Err(e);
                        }
                    }
                }

                Step::Think { .. } => {
                    // Think steps are LLM reasoning — mark completed inline
                    plan.steps[current].set_status(StepStatus::Completed);
                }

                Step::Finish { .. } => {
                    plan.steps[current].set_status(StepStatus::Completed);
                    plan.current_step_index += 1;
                    plan.status = PlanStatus::Completed;
                    return Ok(());
                }

                // WaitForInput, Exec, HttpRequest, BrowserAction:
                // not applicable in child loop context — mark completed with warning
                _ => {
                    warn!(
                        step_type = ?std::mem::discriminant(&step),
                        step = current,
                        "non-tool step in child loop — marking completed without execution"
                    );
                    plan.steps[current].set_status(StepStatus::Completed);
                }
            }

            plan.current_step_index += 1;
        }

        plan.status = PlanStatus::Completed;
        Ok(())
    }

    /// Decompose a goal into independent sub-goals (max 5).
    async fn decompose_goal(
        &self,
        llm: &LlmGateway,
        goal: &str,
        unmet: &[String],
    ) -> AgentResult<Vec<String>> {
        let unmet_str = unmet.join("\n");

        let prompt = format!(
            r#"Decompose the remaining work into independent sub-goals.
Each sub-goal must be:
1. Independent — can be completed without knowing the results of other sub-goals
2. Verifiable — has a clear success criterion
3. Focused — addresses specific unmet requirements

Original goal: {goal}
Remaining unmet requirements:
{unmet_str}

RULES:
- Produce at most 5 sub-goals
- Each sub-goal on a NEW LINE, prefixed with "- "
- Sub-goals together should cover ALL unmet requirements

Example output:
- Optimize the users table query by adding an index
- Implement code splitting for the dashboard bundle
- Add Redis caching layer for API responses"#,
            goal = goal,
            unmet_str = unmet_str,
        );

        let messages = vec![
            crate::llm::LlmChatMessage::system(
                "You are a task decomposition specialist. Break complex goals into independent, verifiable sub-goals. Respond ONLY with the list of sub-goals, one per line prefixed with '- '."
            ),
            crate::llm::LlmChatMessage::user(&prompt),
        ];

        let (response, _) = llm.chat(&messages).await?;

        // Parse sub-goals: lines starting with "- "
        let sub_goals: Vec<String> = response
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("- ") {
                    let goal = trimmed.strip_prefix("- ").unwrap_or("").trim();
                    if !goal.is_empty() {
                        Some(goal.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .take(5) // Hard cap at 5
            .collect();

        if sub_goals.is_empty() {
            // If parsing fails, create a single sub-goal from the unmet items
            return Ok(vec![format!("Address the following: {}", unmet.join("; "))]);
        }

        Ok(sub_goals)
    }

    /// Aggregate results from completed child loops into a summary.
    async fn aggregate_children(
        &self,
        llm: &LlmGateway,
        goal: &str,
        children: &[Loop],
    ) -> AgentResult<String> {
        if children.is_empty() {
            return Ok("No child loops completed.".into());
        }

        let child_summaries: Vec<String> = children
            .iter()
            .map(|c| format!("- [{}]: {}", c.status_name(), c.goal))
            .collect();

        let prompt = format!(
            r#"Summarize what was accomplished by the following sub-tasks.

Original goal: {goal}

Sub-task results:
{child_list}

Provide a concise summary (2-4 sentences) of what was completed, what the combined result means for the original goal, and whether any gaps remain."#,
            goal = goal,
            child_list = child_summaries.join("\n"),
        );

        let messages = vec![
            crate::llm::LlmChatMessage::system(
                "You are a technical summarizer. Provide concise, factual summaries of sub-task results."
            ),
            crate::llm::LlmChatMessage::user(&prompt),
        ];

        let (response, _) = llm.chat(&messages).await?;
        Ok(response)
    }
}

impl Loop {
    /// Human-readable status name for display.
    fn status_name(&self) -> &str {
        match self.status {
            LoopStatus::Pending => "Pending",
            LoopStatus::Running => "Running",
            LoopStatus::StepComplete => "In Progress",
            LoopStatus::Evaluating => "Evaluating",
            LoopStatus::WaitingForApproval => "Waiting",
            LoopStatus::WaitingForInput => "Waiting",
            LoopStatus::Decomposing => "Decomposing",
            LoopStatus::Paused => "Paused",
            LoopStatus::Completed => "Completed",
            LoopStatus::Failed => "Failed",
            LoopStatus::BudgetExceeded => "Budget",
            LoopStatus::TimedOut => "Timeout",
            LoopStatus::Cancelled => "Cancelled",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests: pure functions ──

    #[test]
    fn test_detect_oscillation_none() {
        let history = vec![
            LoopDecision::Continue,
            LoopDecision::Continue,
            LoopDecision::Continue,
        ];
        assert!(!detect_oscillation(&history));
    }

    #[test]
    fn test_detect_oscillation_dcd() {
        let history = vec![
            LoopDecision::Done,
            LoopDecision::Continue,
            LoopDecision::Done,
        ];
        assert!(detect_oscillation(&history));
    }

    #[test]
    fn test_detect_oscillation_cdc() {
        let history = vec![
            LoopDecision::Continue,
            LoopDecision::Done,
            LoopDecision::Continue,
        ];
        assert!(detect_oscillation(&history));
    }

    #[test]
    fn test_detect_oscillation_short_history() {
        assert!(!detect_oscillation(&[
            LoopDecision::Continue,
            LoopDecision::Done
        ]));
    }

    #[test]
    fn test_detect_stall_no_progress() {
        assert!(detect_stall(&[3, 3, 3])); // stuck
        assert!(detect_stall(&[3, 4, 5])); // getting worse
    }

    #[test]
    fn test_detect_stall_progress() {
        assert!(!detect_stall(&[5, 3, 1])); // improving
        assert!(!detect_stall(&[3, 2, 0])); // done
    }

    #[test]
    fn test_detect_stall_short() {
        assert!(!detect_stall(&[3]));
        assert!(!detect_stall(&[3, 3]));
    }

    #[test]
    fn test_vanished_unmet() {
        let prev = vec!["缺少OAuth".into(), "密码明文存储".into()];
        let met = vec!["OAuth实现".into(), "JWT签发".into()];
        let vanished = vanished_unmet(&prev, &met);
        // "缺少OAuth" should fuzzy-match "OAuth实现"
        // "密码明文存储" should NOT match "JWT签发"
        assert_eq!(vanished.len(), 1);
        assert!(vanished[0].contains("密码"));
    }

    #[test]
    fn test_vanished_unmet_none() {
        let prev = vec!["缺少OAuth".into()];
        let met = vec!["OAuth登录已实现".into()];
        let vanished = vanished_unmet(&prev, &met);
        assert!(vanished.is_empty());
    }

    #[test]
    fn test_sanitise_low_confidence_done() {
        let mut eval = EvaluationResult {
            verdict: LoopDecision::Done,
            confidence: 0.5,
            met: vec!["done".into()],
            unmet: vec![],
            new_issues: vec![],
            next_action: String::new(),
        };
        sanitise_verdict(&mut eval, None);
        assert_eq!(eval.verdict, LoopDecision::Continue);
    }

    #[test]
    fn test_sanitise_vanished_unmet() {
        let mut eval = EvaluationResult {
            verdict: LoopDecision::Done,
            confidence: 0.9,
            met: vec!["something else".into()],
            unmet: vec![],
            new_issues: vec![],
            next_action: String::new(),
        };
        let prev = vec!["missing OAuth".into()];
        sanitise_verdict(&mut eval, Some(&prev));
        assert_eq!(eval.verdict, LoopDecision::Continue);
        assert!(!eval.unmet.is_empty());
    }

    #[test]
    fn test_sanitise_passes_valid_done() {
        let mut eval = EvaluationResult {
            verdict: LoopDecision::Done,
            confidence: 0.95,
            met: vec!["OAuth implemented".into()],
            unmet: vec![],
            new_issues: vec![],
            next_action: String::new(),
        };
        let prev = vec!["缺少OAuth".into()];
        sanitise_verdict(&mut eval, Some(&prev));
        assert_eq!(eval.verdict, LoopDecision::Done); // fuzzy match passes
    }

    #[test]
    fn test_loop_status_terminal() {
        assert!(LoopStatus::Completed.is_terminal());
        assert!(LoopStatus::Failed.is_terminal());
        assert!(LoopStatus::Cancelled.is_terminal());
        assert!(!LoopStatus::Paused.is_terminal());
        assert!(!LoopStatus::Running.is_terminal());
    }

    #[test]
    fn test_loop_status_stopped() {
        assert!(LoopStatus::Paused.is_stopped());
        assert!(LoopStatus::WaitingForApproval.is_stopped());
        assert!(LoopStatus::BudgetExceeded.is_stopped());
        assert!(!LoopStatus::Completed.is_stopped());
        assert!(!LoopStatus::Running.is_stopped());
    }

    #[test]
    fn test_loop_config_defaults() {
        let cfg = LoopConfig::default();
        assert_eq!(cfg.max_iterations, 10);
        assert_eq!(cfg.autonomy_level, AutonomyLevel::L3RoundCheck);
        assert!(!cfg.daemon);
        assert!(!cfg.parallel_decomposition);
        assert_eq!(cfg.token_budget, None);
    }

    #[test]
    fn test_evaluation_result_conservative_fallback() {
        let fb = EvaluationResult::conservative_fallback();
        assert_eq!(fb.verdict, LoopDecision::Continue);
        assert_eq!(fb.confidence, 0.0);
        assert!(!fb.unmet.is_empty());
    }

    #[test]
    fn test_budget_tracker_ok() {
        let tracker = BudgetTracker {
            total_tokens: 100,
            started_at: chrono::Utc::now().timestamp(),
        };
        let status = tracker.check(Some(1000), None);
        assert!(matches!(status, BudgetStatus::Ok));
    }

    #[test]
    fn test_budget_tracker_token_exceeded() {
        let tracker = BudgetTracker {
            total_tokens: 1000,
            started_at: chrono::Utc::now().timestamp(),
        };
        let status = tracker.check(Some(500), None);
        assert!(matches!(status, BudgetStatus::TokenExceeded { .. }));
    }

    #[test]
    fn test_budget_tracker_time_exceeded() {
        let tracker = BudgetTracker {
            total_tokens: 0,
            started_at: chrono::Utc::now().timestamp() - 100, // 100s ago
        };
        let status = tracker.check(None, Some(50));
        assert!(matches!(status, BudgetStatus::TimeExceeded { .. }));
    }

    // ── H1 regression: execute_plan_inner must call tool_executor ──

    use crate::task::{tool_call_step, McpToolResult, Plan};
    use std::sync::atomic::AtomicUsize;

    /// ToolExecutor that counts invocations and records tool names.
    struct TrackingToolExecutor {
        call_count: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::agent::ToolExecutor for TrackingToolExecutor {
        async fn execute_tool(
            &self,
            tool_name: &str,
            _params: serde_json::Value,
        ) -> AgentResult<McpToolResult> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(McpToolResult::Success {
                content: format!("executed {tool_name}"),
            })
        }
    }

    #[tokio::test]
    async fn test_execute_plan_inner_calls_tool_executor() {
        let repo = Arc::new(crate::db::TaskRepo::new(":memory:").expect("in-memory repo"));
        let memory = Arc::new(crate::memory_cache::MemoryCache::new(Arc::clone(&repo), 8));
        let executor = Arc::new(TrackingToolExecutor {
            call_count: AtomicUsize::new(0),
        });
        let engine = LoopEngine::new(
            Arc::clone(&repo),
            memory,
            crate::safety::SafetyContext::default(),
            Arc::clone(&executor) as Arc<dyn crate::agent::ToolExecutor>,
        );

        let steps = vec![
            tool_call_step("read_file", serde_json::json!({"path": "/tmp/a"})),
            tool_call_step("write_file", serde_json::json!({"path": "/tmp/b"})),
        ];
        let mut plan = Plan::new("child-plan", steps);

        let result = engine.execute_plan_inner(&mut plan).await;
        assert!(result.is_ok(), "execute_plan_inner should succeed");

        // The key assertion: tool_executor was actually called (not just marking steps completed)
        assert_eq!(
            executor.call_count.load(Ordering::SeqCst),
            2,
            "tool_executor must be called for each ToolCall step — was the placeholder re-introduced?"
        );
        assert_eq!(plan.status, PlanStatus::Completed);
    }
}
