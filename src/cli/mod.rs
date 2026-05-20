pub mod app;
pub mod cmds;
pub mod handlers;

mod ui;

pub use rupoo::{AgentToTui, ApprovalChoice, ChatMessage, PendingTool, TuiToAgent};
pub use ui::render;
pub use app::{FocusTarget, InputMode, OverlayState, RupooApp, SessionTab};

use std::io::{self, stdout, Write};

use crossterm::{
    event::{
        Event, KeyCode, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
        LeaveAlternateScreen},
};

use crossbeam_channel::{Receiver, Sender};
use tracing::warn;
use rupoo::db::TaskRepo;
use rupoo::agent::Agent;

// ═══════════════════════════════════════════════════════════════════════════
// E1: TuiSession — explicit lifecycle (init → run → cleanup)
// ═══════════════════════════════════════════════════════════════════════════

/// RAII guard: restores the terminal on drop (normal exit or panic unwind).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Disable mouse tracking modes before leaving alternate screen
        let _ = write!(stdout(), "\x1b[?1000l\x1b[?1006l");
        let _ = execute!(
            stdout(),
            LeaveAlternateScreen,
        );
        let _ = disable_raw_mode();
    }
}

/// E1: TuiSession — owns terminal, app state, and agent channels.
///
/// Lifecycle:
///   1. `TuiSession::new(...)` — raw mode, alt screen, DB loading
///   2. `session.run()` — event loop (blocks until quit)
///   3. On exit/drop — TerminalGuard restores terminal
pub struct TuiSession {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    app: RupooApp,
    /// Agent-to-TUI message channel (taken out before event loop to
    /// avoid borrow conflicts with crossbeam_channel::select!).
    ui_rx: Option<crossbeam_channel::Receiver<AgentToTui>>,
}

impl TuiSession {
    /// E1: Initialize terminal, create app state with pre-loaded data.
    ///
    /// Steps:
    ///   - Enable raw mode + alternate screen + mouse capture
    ///   - Create the RupooApp with pre-loaded sessions and model label
    ///   - No new threads or tokio runtimes — data is loaded by caller
    pub fn new(
        agent_tx: Option<Sender<TuiToAgent>>,
        ui_rx: Option<Receiver<AgentToTui>>,
        repo: Option<std::sync::Arc<TaskRepo>>,
        sessions_data: Vec<(String, String, String, bool)>,
        model_label: String,
        rt_handle: tokio::runtime::Handle,
    ) -> Result<Self, &'static str> {
        // ── Terminal init ───────────────────────────────────────────────
        if enable_raw_mode().is_err() {
            return Err("not_a_tty");
        }
        let mut sout = stdout();
        if execute!(
            sout,
            EnterAlternateScreen,
        )
        .is_err()
        {
            return Err("terminal_setup_failed");
        }
        // Enable mouse modes 1000 (basic press/release/scroll) + 1006 (SGR coordinates).
        // NOT mode 1002 (drag tracking) — this lets terminal-native text selection work.
        let _ = write!(sout, "\x1b[?1000h\x1b[?1006h");
        let backend = ratatui::backend::CrosstermBackend::new(sout);
        let terminal =
            ratatui::Terminal::new(backend).map_err(|_| "terminal_setup_failed")?;

        // ── Create app with pre-loaded data (no thread spawns) ───────────
        let mut app = RupooApp::new(agent_tx, rt_handle);
        app.model_label = model_label;

        // ── Attach repo and restore sessions ────────────────────────────
        if let Some(r) = repo {
            app = app.set_repo(r);
        }

        let active_id: String = sessions_data
            .iter()
            .find(|(_, _, _, is_active)| *is_active)
            .map(|(id, _, _, _)| id.clone())
            .unwrap_or_else(|| "default".to_string());

        // If DB has sessions, remove the constructor's default "New Chat"
        if !sessions_data.is_empty() {
            app.sessions.retain(|s| s.id != "default");
        }

        for (id, label, messages_json, is_active) in &sessions_data {
            app.sessions.push(SessionTab {
                id: id.clone(),
                label: label.clone(),
                active: *is_active,
                has_context: true,
            });
            if let Ok(msgs) = serde_json::from_str::<Vec<ChatMessage>>(messages_json) {
                app.session_messages.insert(id.clone(), msgs);
            }
        }

        // Mirror active session's messages into `messages` for rendering.
        app.messages = app
            .session_messages
            .get(&active_id)
            .cloned()
            .unwrap_or_default();

        Ok(Self {
            terminal,
            app,
            ui_rx,
        })
    }

    /// E1: Run the TUI event loop.
    ///
    /// Terminal cleanup is guaranteed by `TerminalGuard` drop, even on panic.
    pub fn run(&mut self) -> Result<(), &'static str> {
        let _guard = TerminalGuard;
        // bracketed paste is implicitly enabled

        // Take the receiver so crossbeam_channel doesn't borrow self.
        let ui_rx = self.ui_rx.take();
        let mut needs_redraw = true;

        loop {
            if self.app.quit {
                break Ok(());
            }

            // ── Drain agent events (non-blocking, always) ────────────────
            if let Some(ref rx) = ui_rx {
                while let Ok(msg) = rx.try_recv() {
                    self.app.apply_agent_event(msg);
                    needs_redraw = true;
                }
            }

            // ── Render only when state changed ────────────────────────────
            if needs_redraw {
                self.terminal
                    .draw(|frame| render(frame, &self.app))
                    .map_err(|_| "terminal_draw_failed")?;
                needs_redraw = false;
            }

            // ── Wait for terminal event (never block forever) ────────────────
            //   thinking=true  → 80ms poll for smooth spinner animation
            //   thinking=false → 500ms poll to check for agent events
            use crossterm::event::{poll, read};
            use std::time::Duration;
            let poll_dur = if self.app.thinking {
                Duration::from_millis(80)
            } else {
                Duration::from_millis(500)
            };
            if poll(poll_dur).map_err(|_| "poll_failed")? {
                if let Ok(e) = read() {
                    self.handle_event(e);
                    needs_redraw = true;
                }
            } else if self.app.thinking {
                // timeout — advance spinner, redraw needed
                self.app.spinner_frame =
                    self.app.spinner_frame.wrapping_add(1);
                needs_redraw = true;
            }
            // idle timeout — no spinner, just loop back to drain agent events
        }
    }

    // ═════════════════════════════════════════════════════════════════════
    // E2: Event dispatch — routes events to the mode-specific handler
    // ═════════════════════════════════════════════════════════════════════

    /// Route a raw crossterm event through the appropriate handler strategy.
    fn handle_event(&mut self, event: Event) {
        let app = &mut self.app;

        // ── Key events: delegate to the mode-specific handler ───────────
        let mut handled = if let Event::Key(key) = &event {
            handlers::dispatch(app, key)
        } else {
            false
        };

        // ── Mouse events (limited mode 1000+1006: scroll + click only) ──
        // Drag events are NOT tracked, so terminal-native text selection works.
        if let Event::Mouse(m) = &event {
            match m.kind {
                crossterm::event::MouseEventKind::ScrollDown => {
                    if !app.scroll_bottom {
                        app.scroll_offset = app.scroll_offset.saturating_add(3);
                    }
                    handled = true;
                }
                crossterm::event::MouseEventKind::ScrollUp => {
                    if app.scroll_bottom {
                        app.scroll_bottom = false;
                        app.scroll_offset = app.max_scroll_cache.get();
                    }
                    app.scroll_offset = app.scroll_offset.saturating_sub(3);
                    handled = true;
                }
                crossterm::event::MouseEventKind::Down(_) => {
                    // Left column click → switch session
                    if m.column < 20 && !app.sessions.is_empty() {
                        let idx = (m.row.saturating_sub(1) as usize) / 2;
                        if idx < app.sessions.len() {
                            let new_id = app.sessions[idx].id.clone();
                            if app.current_session_id() != new_id {
                                app.switch_session(&new_id);
                            }
                        }
                    }
                    handled = true;
                }
                _ => {}
            }
        }

        // ── Tab: command completion / focus switching (Chat mode) ──────
        if !handled
            && app.input_mode == InputMode::Chat
            && matches!(
                &event,
                Event::Key(key)
                    if key.code == KeyCode::Tab && key.kind == KeyEventKind::Press
            )
        {
            let input_text = app.input.lines().join("");
            if input_text.starts_with('/') {
                let query =
                    input_text.trim_start_matches('/').to_lowercase();
                if let Some(cmd) = app
                    .available_commands
                    .iter()
                    .find(|c| c.name.starts_with(&query))
                {
                    let mut ta = tui_textarea::TextArea::default();
                    ta.insert_str(format!("/{}", cmd.name));
                    app.input = ta;
                }
            } else {
                app.focus = match app.focus {
                    FocusTarget::Input => FocusTarget::Sessions,
                    FocusTarget::Sessions => FocusTarget::Input,
                    FocusTarget::Chat => FocusTarget::Input,
                };
            }
            handled = true;
        }

        // ── Session navigation (↑/↓/←/→ when Sessions focused) ──────────
        if !handled
            && app.input_mode == InputMode::Chat
            && app.focus == FocusTarget::Sessions
            && app.sessions.len() > 1
        {
            if let Event::Key(key) = &event {
                if key.kind == KeyEventKind::Press {
                    let active_id = app.current_session_id();
                    let active_pos = app.sessions.iter().position(|s| s.id == active_id);
                    match key.code {
                        KeyCode::Down | KeyCode::Right => {
                            if let Some(pos) = active_pos {
                                let next = (pos + 1) % app.sessions.len();
                                let new_id = app.sessions[next].id.clone();
                                if new_id != active_id {
                                    app.switch_session(&new_id);
                                }
                            }
                            handled = true;
                        }
                        KeyCode::Up | KeyCode::Left => {
                            if let Some(pos) = active_pos {
                                let prev = (pos + app.sessions.len() - 1) % app.sessions.len();
                                let new_id = app.sessions[prev].id.clone();
                                if new_id != active_id {
                                    app.switch_session(&new_id);
                                }
                            }
                            handled = true;
                        }
                        KeyCode::Enter => {
                            app.focus = FocusTarget::Input;
                            handled = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        // ── Input history ↑/↓ (Chat mode) ──────────────────────────────
        if !handled && app.input_mode == InputMode::Chat {
            if let Event::Key(key) = &event {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up if app.input_history_index > 0 => {
                            app.input_history_index -= 1;
                            if let Some(prev) =
                                app.input_history.get(app.input_history_index)
                            {
                                let mut ta = tui_textarea::TextArea::default();
                                ta.insert_str(prev);
                                app.input = ta;
                            }
                            handled = true;
                        }
                        KeyCode::Down => {
                            let next = app.input_history_index + 1;
                            if next < app.input_history.len() {
                                app.input_history_index = next;
                                if let Some(prev) =
                                    app.input_history.get(next)
                                {
                                    let mut ta =
                                        tui_textarea::TextArea::default();
                                    ta.insert_str(prev);
                                    app.input = ta;
                                }
                            } else {
                                app.input_history_index =
                                    app.input_history.len();
                                app.input = tui_textarea::TextArea::default();
                            }
                            handled = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        // ── Fall through to TextArea (Chat mode) ────────────────────────
        if !handled && app.input_mode == InputMode::Chat {
            if let Event::Key(key) = &event {
                if key.kind == KeyEventKind::Press
                    || key.kind == KeyEventKind::Repeat
                {
                    app.input.input(*key);
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Public entry points
// ═══════════════════════════════════════════════════════════════════════════

/// Run the interactive TUI with a Rupoo agent engine.
/// `rt_handle` is the tokio runtime handle — must be captured on a tokio thread.
/// `tool_executor` is stored in AgentUiBridge for direct approval-time tool
/// execution (bypassing needs_approval checks to avoid infinite loops).
pub fn run_tui_with_agent(
    repo: std::sync::Arc<TaskRepo>,
    agent: Agent,
    tool_executor: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
    rt_handle: tokio::runtime::Handle,
) -> Result<(), &'static str> {
    // Pre-load UI data on the shared tokio runtime (no new thread, no new runtime).
    // Must happen before passing rt_handle into the TUI event loop.
    let (sessions_data, model_label) = rt_handle.block_on(async {
        let sessions = repo.load_ui_sessions().await.unwrap_or_default();
        let provider = repo
            .get_setting("active_provider")
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        let label = if !provider.is_empty() {
            let model = repo
                .get_setting(&format!("model.{provider}"))
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "default".into());
            format!("{}/{}", provider, model)
        } else {
            "not configured".to_string()
        };
        (sessions, label)
    });

    // Create channel pair
    let (tx, ui_rx) = crossbeam_channel::unbounded::<AgentToTui>();
    let (tx_to_agent, rx) = crossbeam_channel::unbounded::<TuiToAgent>();
    let agent_tx = Some(tx_to_agent);

    // Spawn the async agent task with panic protection
    let repo_clone = std::sync::Arc::clone(&repo);
    let handle_for_agent = rt_handle.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_for_agent.block_on(async move {
                let agent_task = AgentUiBridge {
                    agent,
                    repo: repo_clone,
                    rx,
                    ui_tx: tx, // moved here — thread owns the AgentToTui sender
                    pending_plan: std::sync::Mutex::new(None),
                    pending_step_index: std::sync::Mutex::new(None),
                    tool_executor: std::sync::Arc::clone(&tool_executor),
                    approve_all: false,
                };
                agent_task.run().await;
            });
        }));
        if result.is_err() {
            eprintln!("[rupoo] agent thread panicked — TUI will be unresponsive");
        }
    });

    // E1: Use TuiSession with pre-loaded data — no more thread spawns inside new()
    let mut session = TuiSession::new(
        agent_tx,
        Some(ui_rx),
        Some(repo),
        sessions_data,
        model_label,
        rt_handle,
    )?;
    session.run()
}

// ═══════════════════════════════════════════════════════════════════════════
// Agent bridge — runs in a separate thread, bridges async agent to TUI
// ═══════════════════════════════════════════════════════════════════════════

struct AgentUiBridge {
    agent: Agent,
    repo: std::sync::Arc<TaskRepo>,
    rx: Receiver<TuiToAgent>,
    ui_tx: Sender<AgentToTui>,
    /// Current plan being executed (Shared with Mutex for interior mutability).
    pending_plan: std::sync::Mutex<Option<rupoo::task::Plan>>,
    /// Step index of the tool-call that is blocked on approval.
    pending_step_index: std::sync::Mutex<Option<usize>>,
    /// Direct reference to the tool executor for approval-time tool execution.
    tool_executor: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
    /// When true, automatically approve all future tool calls without user prompt.
    approve_all: bool,
}

impl AgentUiBridge {
    async fn run(mut self) {
        loop {
            // Wait for a message from TUI or check for agent output
            // For now: receive from TUI, call agent, send back result
            match self.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(TuiToAgent::SubmitMessage(text)) => {
                    // Send thinking state
                    let _ = self.ui_tx.send(AgentToTui::Thinking);

                    // Build a minimal plan and run it
                    let plan = match self.build_plan(&text).await {
                        Ok(p) => p,
                        Err(e) => {
                            let _ = self.ui_tx.send(AgentToTui::Message(
                                ChatMessage::assistant(format!("Error: {}", e)),
                            ));
                            let _ = self.ui_tx.send(AgentToTui::Idle);
                            continue;
                        }
                    };

                    if let Err(e) = self.repo.save_plan(&plan).await {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant(format!("DB error: {}", e)),
                        ));
                        let _ = self.ui_tx.send(AgentToTui::Idle);
                        continue;
                    }

                    // Run plan steps
                    match self.agent.resume(&plan.id).await {
                        Ok(Some(mut plan)) => {
                            self.run_plan(&mut plan).await;
                        }
                        Ok(None) => {
                            // Plan already complete
                            let _ = self.ui_tx.send(AgentToTui::Idle);
                        }
                        Err(e) => {
                            let _ = self.ui_tx.send(AgentToTui::Message(
                                ChatMessage::assistant(format!("Agent error: {}", e)),
                            ));
                            let _ = self.ui_tx.send(AgentToTui::Idle);
                        }
                    }
                }
                Ok(TuiToAgent::ApproveTool(_tool_name)) => {
                    // Resume from the blocked approval step.
                    let pending = self.pending_plan.lock().unwrap().clone();
                    let step_idx = *self.pending_step_index.lock().unwrap();
                    if let (Some(mut plan), Some(step_index)) = (pending, step_idx) {
                        self.execute_approved_tool(&mut plan, step_index).await;
                        self.run_plan(&mut plan).await;
                    } else {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant("No pending tool to approve.".to_string()),
                        ));
                    }
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
                Ok(TuiToAgent::ApproveAll) => {
                    self.approve_all = true;
                    let pending = self.pending_plan.lock().unwrap().clone();
                    let step_idx = *self.pending_step_index.lock().unwrap();
                    if let (Some(mut plan), Some(step_index)) = (pending, step_idx) {
                        self.execute_approved_tool(&mut plan, step_index).await;
                        self.run_plan(&mut plan).await;
                    } else {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant("No pending tool to approve.".to_string()),
                        ));
                    }
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                }
                Ok(TuiToAgent::DenyTool) => {
                    // User denied the tool call — mark the step as Failed, do not execute.
                    let pending = self.pending_plan.lock().unwrap().clone();
                    let step_idx = *self.pending_step_index.lock().unwrap();
                    if let (Some(mut plan), Some(step_index)) = (pending, step_idx) {
                        let pid = plan.id.clone();

                        // Mark step as Failed (not Completed — user explicitly denied it)
                        if let Some(step) = plan.steps.get_mut(step_index) {
                            step.set_status(rupoo::task::StepStatus::Failed);
                        }
                        let _ = self
                            .repo
                            .record_step_completion(
                                &pid,
                                step_index,
                                rupoo::task::StepStatus::Failed,
                                Some("denied by user".to_string()),
                            )
                            .await;

                        plan.updated_at = chrono::Utc::now();

                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant("Tool call denied by user.".to_string()),
                        ));

                        // Continue running the plan — it will handle the failure
                        // gracefully (agent decides how to proceed with a failed step).
                        self.run_plan(&mut plan).await;
                    }
                    let _ = self.ui_tx.send(AgentToTui::Idle);
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

    async fn build_plan(
        &self,
        text: &str,
    ) -> anyhow::Result<rupoo::task::Plan> {
        use rupoo::task::{think_step, Plan};
        let label: String = text.chars().take(40).collect();
        let plan = Plan::new(&label, vec![think_step(text)]);
        Ok(plan)
    }

    /// Execute an approved tool: mark step Running, execute, record result.
    async fn execute_approved_tool(
        &self,
        plan: &mut rupoo::task::Plan,
        step_index: usize,
    ) {
        let pid = plan.id.clone();
        let (tool_name, params) = if let Some(rupoo::task::Step::ToolCall { ref tool_name, ref params, .. }) =
            plan.steps.get(step_index)
        {
            (tool_name.clone(), params.clone())
        } else {
            return;
        };

        // Mark step Running
        if let Err(e) = self
            .repo
            .update_step_progress(&pid, step_index, rupoo::task::StepStatus::Running)
            .await
        {
            warn!(error = %e, "failed to mark step running");
        }

        // Execute the tool directly (bypass needs_approval check)
        let mcp_result = self.tool_executor.execute_tool(&tool_name, params).await;

        match mcp_result {
            Ok(mcp) => {
                if let Some(step) = plan.steps.get_mut(step_index) {
                    step.set_status(rupoo::task::StepStatus::Completed);
                    if let rupoo::task::Step::ToolCall { ref mut result, .. } = step {
                        *result = Some(serde_json::json!({
                            "success": mcp.success,
                            "content": mcp.content,
                        }));
                    }
                }
                let _ = self
                    .repo
                    .record_step_completion(
                        &pid,
                        step_index,
                        rupoo::task::StepStatus::Completed,
                        Some(mcp.content.clone()),
                    )
                    .await;
                plan.current_step_index = step_index + 1;
                plan.updated_at = chrono::Utc::now();
            }
            Err(e) => {
                if let Some(step) = plan.steps.get_mut(step_index) {
                    step.set_status(rupoo::task::StepStatus::Failed);
                }
                let _ = self
                    .repo
                    .record_step_completion(
                        &pid,
                        step_index,
                        rupoo::task::StepStatus::Failed,
                        Some(format!("execution error: {e}")),
                    )
                    .await;
                plan.updated_at = chrono::Utc::now();
            }
        }
    }

    async fn store_pending_plan(
        &self,
        plan: &rupoo::task::Plan,
        step_index: usize,
    ) {
        *self.pending_plan.lock().unwrap() = Some(plan.clone());
        *self.pending_step_index.lock().unwrap() = Some(step_index);
        let (tool_name, params_json) =
            if let Some(rupoo::task::Step::ToolCall {
                ref tool_name,
                ref params,
                ..
            }) = plan.steps.get(step_index)
            {
                (
                    tool_name.clone(),
                    serde_json::to_string_pretty(params).unwrap_or_default(),
                )
            } else {
                ("unknown".into(), "null".into())
            };
        let _ = self.ui_tx.send(AgentToTui::RequestApproval(PendingTool {
            tool_name,
            args: params_json,
        }));
    }

    async fn run_plan(&self, plan: &mut rupoo::task::Plan) {
        *self.pending_plan.lock().unwrap() = Some(plan.clone());
        loop {
            match self.agent.run_next_step(plan).await {
                Ok(rupoo::agent::StepOutcome::Advanced) => {
                    // Send token update BEFORE last-step check so it's always emitted
                    self.send_token_update();
                    if plan.current_step_index >= plan.steps.len() {
                        // Send the final output before going idle
                        let output = self.extract_output(plan);
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            rupoo::ChatMessage::assistant(output),
                        ));
                        let _ = self.ui_tx.send(AgentToTui::Idle);
                        *self.pending_plan.lock().unwrap() = None;
                        *self.pending_step_index.lock().unwrap() = None;
                        break;
                    }
                }
                Ok(rupoo::agent::StepOutcome::Finished) => {
                    self.send_token_update();
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(self.extract_output(plan)),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::Failed(e)) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Failed: {e}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::WaitingForInput(p)) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Input needed: {p}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::RequiresApproval { ref tool_name, ref params, step_index }) => {
                    if self.approve_all {
                        // Auto-approve: execute tool directly without user prompt.
                        // Clone params to avoid borrow conflict with store_pending_plan.
                        let p = params.clone();
                        let tn = tool_name.clone();
                        if let Err(e) = self.repo.update_step_progress(
                            &plan.id, step_index, rupoo::task::StepStatus::Running,
                        ).await {
                            warn!(error = %e, "failed to mark step running");
                        }
                        let mcp_result = self.tool_executor.execute_tool(&tn, p).await;
                        match mcp_result {
                            Ok(mcp) => {
                                if let Some(step) = plan.steps.get_mut(step_index) {
                                    step.set_status(rupoo::task::StepStatus::Completed);
                                    if let rupoo::task::Step::ToolCall { ref mut result, .. } = step {
                                        *result = Some(serde_json::json!({
                                            "success": mcp.success,
                                            "content": mcp.content,
                                        }));
                                    }
                                }
                                let _ = self.repo.record_step_completion(
                                    &plan.id, step_index,
                                    rupoo::task::StepStatus::Completed,
                                    Some(mcp.content),
                                ).await;
                                plan.current_step_index = step_index + 1;
                                plan.updated_at = chrono::Utc::now();
                            }
                            Err(e) => {
                                if let Some(step) = plan.steps.get_mut(step_index) {
                                    step.set_status(rupoo::task::StepStatus::Failed);
                                }
                                let _ = self.repo.record_step_completion(
                                    &plan.id, step_index,
                                    rupoo::task::StepStatus::Failed,
                                    Some(format!("execution error: {e}")),
                                ).await;
                                plan.updated_at = chrono::Utc::now();
                            }
                        }
                        // Continue the plan loop — don't break.
                        continue;
                    }
                    // Normal approval flow: pause and wait for user.
                    self.store_pending_plan(plan, step_index).await;
                    break;
                }
                Err(e) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Error: {e}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
            }
        }
    }

    fn send_token_update(&self) {
        if let Some(u) = self.agent.last_usage() {
            let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                in_count: u.prompt_tokens as u64,
                out_count: u.completion_tokens as u64,
            });
        }
    }

    fn extract_output(
        &self,
        plan: &rupoo::task::Plan,
    ) -> String {
        plan.steps
            .first()
            .and_then(|s| {
                if let rupoo::task::Step::Think { output, .. } = s {
                    output.clone()
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "(no output)".into())
    }
}
