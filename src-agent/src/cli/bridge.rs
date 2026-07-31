//! AgentUiBridge — runs in a separate thread, bridges async agent to TUI

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};

use super::approval::ApprovalExt;
use super::ChatMessage;
use super::{AgentToTui, TuiToAgent};
use rupoo::agent::ToolExecutor;
use rupoo::{Density, LayoutMode, TuiControlAction};

// Magic number constants
const BRIDGE_POLL_MS: u64 = 100;

// ═══════════════════════════════════════════════════════════════════════════
// AgentUiBridge — bridges async agent to TUI
// ═══════════════════════════════════════════════════════════════════════════

pub(super) struct AgentUiBridge {
    pub(super) agent: rupoo::agent::Agent,
    pub(super) repo: Arc<rupoo::db::TaskRepo>,
    pub(super) rx: Receiver<TuiToAgent>,
    pub(super) ui_tx: Sender<AgentToTui>,
    /// Current plan being executed (Shared with Mutex for interior mutability).
    pub(super) pending_plan: Mutex<Option<rupoo::task::Plan>>,
    /// Step index of the tool-call that is blocked on approval.
    pub(super) pending_step_index: Mutex<Option<usize>>,
    /// Direct reference to the tool executor for approval-time tool execution.
    pub(super) tool_executor: Arc<dyn ToolExecutor>,
    /// When true, automatically approve all future tool calls without user prompt.
    pub(super) approve_all: bool,
    /// Conversation history for multi-turn Chat Mode.
    pub(super) conversation_history: rupoo::llm::ConversationHistory,
    /// Currently configured model label (e.g. "anthropic/claude-sonnet-4"),
    /// used by the `/context` diagnostic (Phase C). Mirrors the TUI's
    /// `model_label`; kept here so the bridge can report it without round-tripping
    /// through the UI channel. Seeds from startup config, refreshed on `/model`.
    pub(super) model_label: String,
    /// Session ID for persisting conversation history.
    pub(super) session_id: String,
    /// Intent state for token-efficient history compression.
    pub(super) intent_state: rupoo::signal::IntentState,
    /// Cancel flag — set by TUI when user interrupts generation.
    pub(super) cancelled: Arc<AtomicBool>,
}

impl AgentUiBridge {
    /// Emit the live context-usage percentage (computed from the conversation
    /// history budget) followed by `Idle`. Centralizing this keeps every
    /// "turn finished" exit consistent so the TUI footer gauge is always fresh.
    pub(super) fn send_idle(&self) -> Result<(), crossbeam_channel::SendError<AgentToTui>> {
        let pct = self.context_usage_pct();
        self.ui_tx.send(AgentToTui::ContextUsage { pct })?;
        self.ui_tx.send(AgentToTui::Idle)
    }

    /// Current context-window usage as a percentage of the history token budget.
    pub(super) fn context_usage_pct(&self) -> u8 {
        let budget = self.conversation_history.token_budget();
        if budget == 0 {
            return 0;
        }
        let est = self.conversation_history.estimated_tokens();
        (((est * 100) / budget) as u8).clamp(0, 100)
    }

    pub(super) async fn run(mut self) {
        loop {
            match self
                .rx
                .recv_timeout(std::time::Duration::from_millis(BRIDGE_POLL_MS))
            {
                Ok(TuiToAgent::SubmitMessage(text)) => {
                    self.handle_submit(&text).await;
                }
                Ok(TuiToAgent::ApproveTool(_tool_name)) => {
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::ApproveAll) => {
                    self.approve_all = true;
                    if let Err(e) = self.repo.set_setting("approve_all", "true").await {
                        tracing::warn!(error = %e, "failed to persist approve_all");
                    }
                    self.handle_approval().await;
                }
                Ok(TuiToAgent::DenyTool) => {
                    self.handle_denial().await;
                }
                Ok(TuiToAgent::Cancel) => {
                    // Cancel signal — the cancelled flag is already set by the TUI,
                    // just continue the loop; the chat callback will check the flag.
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message — continue loop
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // TUI closed — shutdown
                    break;
                }
            }
        }
    }

    /// Handle a submitted message from the user, dispatching to the appropriate handler.
    async fn handle_submit(&mut self, text: &str) {
        // Check skill triggers first — before Chat/Plan mode routing
        let skill_manager =
            rupoo::skill::SkillManager::new(rupoo::skill::SkillManager::default_dir());
        if let Ok(Some(skill)) = skill_manager.match_trigger(text) {
            self.handle_skill_trigger(skill).await;
        } else if text.starts_with("/plan ") {
            let task = text.trim_start_matches("/plan ").trim();
            self.handle_plan_mode(task).await;
        } else if text.starts_with("/loop") {
            self.handle_loop_command(text).await;
        } else if text.starts_with("/cron") {
            self.handle_cron_command(text).await;
        } else if text.starts_with("/model ") {
            self.handle_model_switch(text).await;
        } else if text.starts_with("/memory") {
            self.handle_memory(text).await;
        } else if text == "/clear" {
            self.handle_clear().await;
        } else if text == "/status" {
            self.handle_status().await;
        } else if text == "/context" {
            self.handle_context().await;
        } else if text == "/activity" {
            self.toggle_activity_overlay().await;
        } else if text.starts_with("/ui") {
            self.handle_ui(text).await;
        } else if text.starts_with("/deep") {
            self.handle_deep(text).await;
        } else if text == "/help" || text == "/?" {
            self.handle_help().await;
        } else {
            // Detect intent → send LayoutModeHint before chat
            if rupoo::signal::IntentState::looks_like_development_demand(text) {
                let _ = self
                    .ui_tx
                    .send(AgentToTui::LayoutModeHint(LayoutMode::Work));
            } else {
                let _ = self
                    .ui_tx
                    .send(AgentToTui::LayoutModeHint(LayoutMode::Chat));
            }

            // Chat Mode: multi-turn agent chat
            self.handle_chat_message(text).await;
        }
    }

    /// Handle a skill trigger match.
    async fn handle_skill_trigger(&mut self, skill: rupoo::skill::SkillDef) {
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(format!(
                "Triggered skill: {}",
                skill.name
            ))))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        let skill_manager =
            rupoo::skill::SkillManager::new(rupoo::skill::SkillManager::default_dir());
        let plan = skill_manager.skill_to_plan(&skill);
        if let Err(e) = self.repo.save_plan(&plan).await {
            if let Err(e) = self
                .ui_tx
                .send(AgentToTui::Message(ChatMessage::error(format!(
                    "Failed to save skill plan: {}",
                    e
                ))))
            {
                tracing::warn!("failed to send UI event: {}", e);
            }
        } else {
            match self.agent.resume(&plan.id).await {
                Ok(Some(mut plan)) => {
                    self.run_plan(&mut plan).await;
                }
                Ok(None) => {
                    if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::assistant(
                        "Skill plan already completed".to_string(),
                    ))) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self.send_idle() {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
                Err(e) => {
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::error(format!(
                                "Skill execution error: {}",
                                e
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self.send_idle() {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
            }
        }
    }

    /// Handle /model command to switch LLM provider/model.
    async fn handle_model_switch(&mut self, text: &str) {
        let args = text.trim_start_matches("/model ").trim();
        if args.is_empty() {
            let current = if self.agent.has_llm() {
                "configured".to_string()
            } else {
                "not configured".to_string()
            };
            if let Err(e) = self
                .ui_tx
                .send(AgentToTui::Message(ChatMessage::system(format!(
                    "Current LLM: {}",
                    current
                ))))
            {
                tracing::warn!("failed to send UI event: {}", e);
            }
        } else {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            let provider = parts[0].to_string();
            let model = parts.get(1).map(|s| s.to_string());
            match self
                .agent
                .switch_llm(&provider, model.as_deref(), &self.repo)
                .await
            {
                Ok(label) => {
                    if let Err(e) = self.repo.set_setting("active_provider", &provider).await {
                        tracing::warn!("failed to save setting: {}", e);
                    }
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::system(format!(
                                "Switched to {}",
                                label
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self.ui_tx.send(AgentToTui::LlmStatus {
                        configured: true,
                        provider: provider.clone(),
                        model_label: label.clone(),
                    }) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    self.model_label = label.clone();
                }
                Err(e) => {
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::error(format!(
                                "Failed to switch: {}",
                                e
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
            }
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /clear command to reset conversation history.
    async fn handle_clear(&mut self) {
        self.conversation_history.clear();
        self.intent_state = rupoo::signal::IntentState::new();
        if let Err(e) = self
            .repo
            .save_conversation_history(&self.session_id, &self.conversation_history)
            .await
        {
            tracing::warn!(error = %e, "failed to clear history in DB");
        }
        if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
            "Conversation history cleared".to_string(),
        ))) {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /status command.
    async fn handle_status(&self) {
        let llm_status = if self.agent.has_llm() {
            "configured"
        } else {
            "not configured"
        };
        let status = format!(
            "Session: {}\nLLM: {}\nApprove all: {}\nHistory: {} turns",
            self.session_id,
            llm_status,
            self.approve_all,
            self.conversation_history.len(),
        );
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(status)))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /context command (Phase C, §6.3) — print a context-usage diagnosis
    /// so the user can see why the window is (or isn't) near its limit.
    async fn handle_context(&self) {
        let est = self.conversation_history.estimated_tokens();
        let budget = self.conversation_history.token_budget();
        let turns = self.conversation_history.len();
        let real_window = crate::cli::tui_view::model_context_window(&self.model_label);
        let report = crate::cli::tui_view::build_context_report(
            est,
            budget,
            turns,
            &self.model_label,
            real_window,
        );
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(report)))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Toggle the running-activity overlay (Shift+A / `/activity`).
    async fn toggle_activity_overlay(&self) {
        if let Err(e) = self.ui_tx.send(AgentToTui::TuiControl {
            action: TuiControlAction::ToggleActivity,
        }) {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle `/ui` subcommands — currently only `density [compact|comfortable]`.
    async fn handle_ui(&self, text: &str) {
        let arg = text.trim_start_matches("/ui").trim();
        let (density, note) = if arg.starts_with("density") {
            let mode = arg.trim_start_matches("density").trim();
            match mode {
                "compact" => (Density::Compact, "排版密度：compact"),
                "comfortable" | "" => (Density::Comfortable, "排版密度：comfortable"),
                other => {
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::system(format!(
                                "未知密度：{other}，可选 compact / comfortable"
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self.send_idle() {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    return;
                }
            }
        } else {
            if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                "用法：/ui density [compact|comfortable]".to_string(),
            ))) {
                tracing::warn!("failed to send UI event: {}", e);
            }
            if let Err(e) = self.send_idle() {
                tracing::warn!("failed to send UI event: {}", e);
            }
            return;
        };

        if let Err(e) = self.ui_tx.send(AgentToTui::TuiControl {
            action: TuiControlAction::SetDensity(density),
        }) {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(note.to_string())))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /help command.
    async fn handle_help(&self) {
        let help = "\
Available commands:
  /plan <task>          — Generate and execute a step-by-step plan
  /loop <goal>          — Start adaptive iterative loop (Loop Engineering)
  /loop status <id>     — Show loop status
  /loop list            — List all loops
  /loop pause <id>      — Pause a running loop
  /loop resume <id>     — Resume a paused loop
  /loop cancel <id>     — Cancel a loop
  /model <prov> [model] — Switch LLM provider/model
  /memory [on/off/list/search <query>] — Manage memory feature
  /deep [on/off]        — Enable/disable deep search (hybrid FTS5 + vector)
  /clear                — Clear conversation history
  /status               — Show current session status
  /context              — Show context-window usage diagnosis
  /activity             — Toggle running-activity overlay (also Shift+A)
  /ui density [compact|comfortable] — Set TUI layout density
  /help                 — Show this help message
  Ctrl+C               — Cancel current generation (press twice to quit)";
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(help.to_string())))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /loop commands — Loop Engineering (Phase A).
    async fn handle_loop_command(&mut self, text: &str) {
        let input = text.trim_start_matches("/loop").trim();

        // Parse subcommands
        if input.is_empty() {
            self.send_system("Usage: /loop <goal> | /loop status <id> | /loop list | /loop pause <id> | /loop resume <id> | /loop cancel <id>")
                .await;
            return;
        }

        if input == "list" {
            match self.agent.list_loops(20, 0).await {
                Ok(loops) => {
                    if loops.is_empty() {
                        self.send_system("No loops found.").await;
                    } else {
                        let mut msg = String::from("Loops:\n");
                        for l in &loops {
                            msg.push_str(&format!(
                                "  {} | {:?} | \"{}\"\n",
                                &l.id[..8.min(l.id.len())],
                                l.status,
                                &l.goal[..60.min(l.goal.len())]
                            ));
                        }
                        self.send_system(&msg).await;
                    }
                }
                Err(e) => {
                    self.send_error(&format!("Failed to list loops: {e}")).await;
                }
            }
        } else if let Some(id) = input.strip_prefix("status ") {
            let id = id.trim();
            match self.agent.get_loop_status(id).await {
                Ok(l) => {
                    let msg = format!(
                        "Loop: {} | Goal: \"{}\" | Status: {:?} | Max iterations: {}",
                        &l.id[..8.min(l.id.len())],
                        l.goal,
                        l.status,
                        l.config.max_iterations
                    );
                    self.send_system(&msg).await;
                }
                Err(e) => {
                    self.send_error(&format!("Loop not found: {e}")).await;
                }
            }
        } else if let Some(id) = input.strip_prefix("pause ") {
            let id = id.trim();
            match self.agent.pause_loop(id).await {
                Ok(()) => self.send_system(&format!("Loop {} paused.", id)).await,
                Err(e) => self.send_error(&format!("Failed to pause: {e}")).await,
            }
        } else if let Some(id) = input.strip_prefix("resume ") {
            let id = id.trim();
            self.send_system(&format!("Resuming loop {}... (requires LLM)", id))
                .await;
            // Full resume requires async execution with LLM — stub for now
            match self.agent.resume_loop(id).await {
                Ok(l) => {
                    self.send_system(&format!(
                        "Loop {} resumed, status: {:?}",
                        &l.id[..8],
                        l.status
                    ))
                    .await
                }
                Err(e) => self.send_error(&format!("Failed to resume: {e}")).await,
            }
        } else if let Some(id) = input.strip_prefix("cancel ") {
            let id = id.trim();
            match self.agent.cancel_loop(id).await {
                Ok(()) => self.send_system(&format!("Loop {} cancelled.", id)).await,
                Err(e) => self.send_error(&format!("Failed to cancel: {e}")).await,
            }
        } else {
            // Treat as a goal to start a new loop — spawn in background
            let goal = input.trim_matches('"').trim().to_string();
            let ui_tx = self.ui_tx.clone();
            let ctx_pct = self.context_usage_pct();

            // Extract shared resources from parent agent for the background task
            let engine = self.agent.loop_engine.clone();
            let agent = match self.agent.try_clone_lightweight() {
                Ok(a) => Arc::new(a),
                Err(e) => {
                    self.send_error(&format!("Failed to prepare agent: {e}"))
                        .await;
                    return;
                }
            };

            self.send_system(&format!(
                "Starting loop: \"{}\" (running in background, use /loop status <id> to check progress)...",
                goal
            )).await;

            tokio::spawn(async move {
                use futures::future::FutureExt;
                use std::panic::AssertUnwindSafe;

                let engine = match engine {
                    Some(e) => e,
                    None => {
                        let _ = ui_tx.send(AgentToTui::Message(ChatMessage::error(
                            "Loop engine not initialized".into(),
                        )));
                        let _ = ui_tx.send(AgentToTui::ContextUsage { pct: ctx_pct });
                        let _ = ui_tx.send(AgentToTui::Idle);
                        return;
                    }
                };

                let config = rupoo::loop_engine::LoopConfig {
                    autonomy_level: rupoo::loop_engine::AutonomyLevel::L4AutoCorrect,
                    ..Default::default()
                };
                let agent_clone = Arc::clone(&agent);
                let llm_ref = agent_clone.llm_gateway_ref();
                // Clone repo from agent before start_loop consumes agent
                let repo = Arc::clone(agent_clone.repo());

                // Catch panics (e.g. LLM HTTP buffer overflow) so TUI gets notified
                let result = AssertUnwindSafe(engine.start_loop(&goal, config, agent, llm_ref))
                    .catch_unwind()
                    .await;

                match result {
                    Ok(Ok(l)) => {
                        // Build a detailed completion message with evaluation info
                        let summary = format_loop_summary(&l, &repo).await;
                        let _ = ui_tx.send(AgentToTui::Message(ChatMessage::system(summary)));
                    }
                    Ok(Err(e)) => {
                        let msg = format!("Loop failed: {e}");
                        let _ = ui_tx.send(AgentToTui::Message(ChatMessage::error(msg)));
                    }
                    Err(_panic) => {
                        let msg = "Loop crashed: internal error (LLM response too large or connection issue). Try again with a simpler goal: /loop <simple goal>".to_string();
                        let _ = ui_tx.send(AgentToTui::Message(ChatMessage::error(msg)));
                    }
                }
                let _ = ui_tx.send(AgentToTui::ContextUsage { pct: ctx_pct });
                let _ = ui_tx.send(AgentToTui::Idle);
            });
        }
    }

    /// Handle /cron commands — cron job management.
    async fn handle_cron_command(&mut self, text: &str) {
        let input = text.trim_start_matches("/cron").trim();
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let subcmd = parts.first().copied().unwrap_or("");

        match subcmd {
            "list" => {
                let repo = self.repo.clone();
                let jobs = rupoo::cron::list_cron_jobs(&repo, 100, 0).await;
                match jobs {
                    Ok(list) if list.is_empty() => {
                        self.send_system("没有定时任务。使用 /cron add 添加一个。")
                            .await;
                    }
                    Ok(list) => {
                        let mut msg = String::from("📋 Cron 任务列表:\n");
                        let now = chrono::Utc::now().timestamp();
                        for j in &list {
                            let status = if j.enabled {
                                match j.next_run_at {
                                    Some(ts) if ts <= now => "● 待执行",
                                    Some(_) => "● 活跃",
                                    None => "○ 无计划",
                                }
                            } else {
                                "○ 已暂停"
                            };
                            let next_str = j
                                .next_run_at
                                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                                .map(|dt| {
                                    dt.with_timezone(&chrono::Local)
                                        .format("%m-%d %H:%M")
                                        .to_string()
                                })
                                .unwrap_or_else(|| "-".to_string());
                            msg.push_str(&format!(
                                "  {} | {} | {} | {}\n",
                                &j.id[..8],
                                j.name,
                                next_str,
                                status,
                            ));
                        }
                        self.send_system(&msg).await;
                    }
                    Err(e) => {
                        self.send_error(&format!("获取 cron 列表失败: {e}")).await;
                    }
                }
            }
            "add" => {
                let args = parts.get(1).copied().unwrap_or("");
                // Parse: "name" "schedule" "task message"
                let parsed = parse_cron_add_args(args);
                match parsed {
                    Some((name, schedule, task)) => {
                        let repo = self.repo.clone();
                        match rupoo::cron::calculate_next_run(schedule) {
                            Ok(_) => match rupoo::cron::CronJob::new(name, schedule, task) {
                                Ok(job) => match rupoo::cron::save_cron_job(&repo, &job).await {
                                    Ok(()) => {
                                        let desc = job
                                            .next_run_at
                                            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                                            .map(|dt| {
                                                dt.with_timezone(&chrono::Local)
                                                    .format("%Y-%m-%d %H:%M")
                                                    .to_string()
                                            })
                                            .unwrap_or_else(|| "未知".to_string());
                                        self.send_system(&format!(
                                            "✅ Cron '{}' 已添加，下次执行: {desc}",
                                            name
                                        ))
                                        .await;
                                    }
                                    Err(e) => self.send_error(&format!("保存失败: {e}")).await,
                                },
                                Err(e) => self.send_error(&format!("创建失败: {e}")).await,
                            },
                            Err(e) => {
                                self.send_error(&format!("无效 cron 表达式: {e}")).await;
                            }
                        }
                    }
                    None => {
                        self.send_system("用法: /cron add \"名称\" \"cron表达式\" \"任务描述\"")
                            .await;
                        self.send_system(
                            "示例: /cron add \"日报\" \"0 9 * * 1-5\" \"生成每日工作报告\"",
                        )
                        .await;
                    }
                }
            }
            "remove" | "delete" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    self.send_system("用法: /cron remove <job_id>").await;
                    return;
                }
                let repo = self.repo.clone();
                match rupoo::cron::delete_cron_job(&repo, id).await {
                    Ok(()) => self.send_system("✅ Cron 任务已删除").await,
                    Err(e) => self.send_error(&format!("删除失败: {e}")).await,
                }
            }
            "pause" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    self.send_system("用法: /cron pause <job_id>").await;
                    return;
                }
                let repo = self.repo.clone();
                match rupoo::cron::toggle_cron_job(&repo, id, false).await {
                    Ok(()) => self.send_system("⏸️ Cron 任务已暂停").await,
                    Err(e) => self.send_error(&format!("暂停失败: {e}")).await,
                }
            }
            "resume" => {
                let id = parts.get(1).copied().unwrap_or("");
                if id.is_empty() {
                    self.send_system("用法: /cron resume <job_id>").await;
                    return;
                }
                let repo = self.repo.clone();
                match rupoo::cron::toggle_cron_job(&repo, id, true).await {
                    Ok(()) => self.send_system("▶️ Cron 任务已恢复").await,
                    Err(e) => self.send_error(&format!("恢复失败: {e}")).await,
                }
            }
            _ => {
                self.send_system("📋 Cron 命令:\n  /cron list                    — 列出所有任务\n  /cron add \"name\" \"schedule\" \"task\" — 添加任务\n  /cron remove <id>            — 删除任务\n  /cron pause <id>             — 暂停任务\n  /cron resume <id>            — 恢复任务").await;
            }
        }
    }

    async fn send_system(&self, msg: &str) {
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::system(msg.to_string())))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    async fn send_error(&self, msg: &str) {
        if let Err(e) = self
            .ui_tx
            .send(AgentToTui::Message(ChatMessage::error(msg.to_string())))
        {
            tracing::warn!("failed to send UI event: {}", e);
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /memory command.
    async fn handle_memory(&mut self, text: &str) {
        let args = text.trim_start_matches("/memory ").trim();

        if args.is_empty() {
            // Show memory status
            let enabled = self.agent.is_memory_enabled();
            let count = match self.agent.memory_count().await {
                Ok(c) => c.to_string(),
                Err(e) => format!("Error: {}", e),
            };
            let status = format!(
                "Memory Status:\n  Enabled: {}\n  Entries: {}",
                if enabled { "Yes" } else { "No" },
                count
            );
            if let Err(e) = self
                .ui_tx
                .send(AgentToTui::Message(ChatMessage::system(status)))
            {
                tracing::warn!("failed to send UI event: {}", e);
            }
        } else {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            let subcmd = parts[0];
            let query = parts.get(1).copied().unwrap_or("");

            match subcmd {
                "on" => {
                    self.agent.set_memory_enabled(true);
                    if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                        "Memory feature enabled".to_string(),
                    ))) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
                "off" => {
                    self.agent.set_memory_enabled(false);
                    if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                        "Memory feature disabled".to_string(),
                    ))) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
                "list" => match self.agent.recent_memories(10).await {
                    Ok(memories) => {
                        if memories.is_empty() {
                            if let Err(e) = self.ui_tx.send(AgentToTui::Message(
                                ChatMessage::system("No memories found".to_string()),
                            )) {
                                tracing::warn!("failed to send UI event: {}", e);
                            }
                        } else {
                            let mut list = "Recent Memories:\n".to_string();
                            for (i, mem) in memories.iter().enumerate() {
                                list.push_str(&format!("  [{}] {}\n", i + 1, mem.content));
                            }
                            if let Err(e) = self
                                .ui_tx
                                .send(AgentToTui::Message(ChatMessage::system(list)))
                            {
                                tracing::warn!("failed to send UI event: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        if let Err(e) =
                            self.ui_tx
                                .send(AgentToTui::Message(ChatMessage::error(format!(
                                    "Failed to list memories: {}",
                                    e
                                ))))
                        {
                            tracing::warn!("failed to send UI event: {}", e);
                        }
                    }
                },
                "search" => {
                    if query.is_empty() {
                        if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::error(
                            "Usage: /memory search <query>".to_string(),
                        ))) {
                            tracing::warn!("failed to send UI event: {}", e);
                        }
                    } else {
                        match self.agent.recall(query, 10).await {
                            Ok(memories) => {
                                if memories.is_empty() {
                                    if let Err(e) =
                                        self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                                            format!("No memories found matching '{}'", query),
                                        )))
                                    {
                                        tracing::warn!("failed to send UI event: {}", e);
                                    }
                                } else {
                                    let mut results = format!("Search Results for '{}':\n", query);
                                    for (i, mem) in memories.iter().enumerate() {
                                        results.push_str(&format!(
                                            "  [{}] {}\n",
                                            i + 1,
                                            mem.content
                                        ));
                                    }
                                    if let Err(e) = self
                                        .ui_tx
                                        .send(AgentToTui::Message(ChatMessage::system(results)))
                                    {
                                        tracing::warn!("failed to send UI event: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                if let Err(e) = self.ui_tx.send(AgentToTui::Message(
                                    ChatMessage::error(format!("Failed to search memories: {}", e)),
                                )) {
                                    tracing::warn!("failed to send UI event: {}", e);
                                }
                            }
                        }
                    }
                }
                _ => {
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::error(format!(
                                "Unknown memory command: {}",
                                subcmd
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
            }
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }

    /// Handle /deep command for controlling hybrid search (deep search) feature.
    async fn handle_deep(&mut self, text: &str) {
        let args = text.trim_start_matches("/deep ").trim();

        if args.is_empty() {
            // Show deep search status
            let enabled = self.agent.is_hybrid_search_enabled();
            let status = format!(
                "Deep Search (Hybrid) Status:\n  Enabled: {}",
                if enabled { "Yes" } else { "No" }
            );
            if let Err(e) = self
                .ui_tx
                .send(AgentToTui::Message(ChatMessage::system(status)))
            {
                tracing::warn!("failed to send UI event: {}", e);
            }
        } else {
            match args {
                "on" => {
                    self.agent.set_hybrid_search_enabled(true);
                    if let Err(e) = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::system("Deep search (hybrid) enabled. Now using FTS5 + vector semantic search for better memory retrieval.".to_string()),
                    )) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self
                        .ui_tx
                        .send(AgentToTui::HybridSearchUpdate { enabled: true })
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
                "off" => {
                    self.agent.set_hybrid_search_enabled(false);
                    if let Err(e) = self.ui_tx.send(AgentToTui::Message(ChatMessage::system(
                        "Deep search (hybrid) disabled. Using FTS5 full-text search only."
                            .to_string(),
                    ))) {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                    if let Err(e) = self
                        .ui_tx
                        .send(AgentToTui::HybridSearchUpdate { enabled: false })
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
                _ => {
                    if let Err(e) =
                        self.ui_tx
                            .send(AgentToTui::Message(ChatMessage::error(format!(
                                "Unknown deep command: {}\nUsage: /deep [on/off]",
                                args
                            ))))
                    {
                        tracing::warn!("failed to send UI event: {}", e);
                    }
                }
            }
        }
        if let Err(e) = self.send_idle() {
            tracing::warn!("failed to send UI event: {}", e);
        }
    }
}

/// Format a detailed loop completion summary using evaluation data from the DB.
async fn format_loop_summary(
    l: &rupoo::loop_engine::Loop,
    repo: &std::sync::Arc<rupoo::db::TaskRepo>,
) -> String {
    let mut msg = format!(
        "\n=== Loop {} ===\nGoal: {}\nStatus: {:?}",
        &l.id[..8.min(l.id.len())],
        l.goal,
        l.status,
    );

    // Try to load the latest evaluation for extra detail
    if let Ok(Some(run)) = repo.get_latest_loop_run(&l.id).await {
        if let Some(ref eval) = run.evaluation {
            msg.push_str(&format!(
                "\nVerdict: {:?} (confidence: {:.0}%)",
                eval.verdict,
                eval.confidence * 100.0
            ));
            if !eval.met.is_empty() {
                msg.push_str(&format!("\n✓ Met ({}):", eval.met.len()));
                for m in eval.met.iter().take(5) {
                    let short: String = m.chars().take(200).collect();
                    msg.push_str(&format!("\n  · {}", short));
                }
            }
            if !eval.unmet.is_empty() {
                msg.push_str(&format!("\n✗ Unmet ({}):", eval.unmet.len()));
                for u in eval.unmet.iter().take(3) {
                    let short: String = u.chars().take(200).collect();
                    msg.push_str(&format!("\n  · {}", short));
                }
            }
            if !eval.new_issues.is_empty() {
                msg.push_str(&format!("\n⚠ Issues ({}):", eval.new_issues.len()));
                for n in eval.new_issues.iter().take(3) {
                    msg.push_str(&format!("\n  · {}", n));
                }
            }
        }
    }

    msg
}

/// Parse /cron add arguments: "name" "schedule" "task message"
/// Supports both quoted strings and positional arguments.
fn parse_cron_add_args(args: &str) -> Option<(&str, &str, &str)> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }

    // Try quoted parsing: "name" "schedule" "task message"
    let mut parts = Vec::new();
    let mut remaining = args;
    while let Some(start) = remaining.find('"') {
        let after = &remaining[start + 1..];
        if let Some(end) = after.find('"') {
            parts.push(&after[..end]);
            remaining = &after[end + 1..];
        } else {
            break;
        }
    }

    if parts.len() >= 3 {
        Some((parts[0], parts[1], parts[2]))
    } else if parts.len() == 2 {
        Some((parts[0], parts[1], remaining.trim()))
    } else {
        // Fallback: parse by whitespace, first 2 args as name+schedule, rest as task
        let words: Vec<&str> = args.split_whitespace().collect();
        if words.len() >= 3 {
            Some((
                words[0],
                words[1],
                &args[words[0].len() + words[1].len() + 2..],
            ))
        } else {
            None
        }
    }
}
