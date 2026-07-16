//! Ratatui "humanistic companion" surface — Step 2 of the CLI UX rewrite.
//!
//! This is now the **default** REPL renderer (see `run_tui_with_agent`). The
//! legacy line-by-line terminal printer (`run_loop` / `handle_input` /
//! `drain_and_render`) remains fully intact and can be selected via
//! `RUPOO_TUI=0/false/off/no`. The two paths are isolated, so the legacy
//! printer can never regress, and this module only runs when ratatui is chosen.
//!
//! Design (quality red line):
//! - The agent event stream is turned into `ChatView` state by the pure
//!   `apply_event` reducer from `tui_view` (already unit-tested). This loop only
//!   (a) feeds events to that reducer, (b) applies the *minimal* state
//!   side-effects the app needs (history / persistence / generation flags), and
//!   (c) repaints every frame via `render_frame`. No raw ANSI, no duplicated
//!   rendering logic.
//! - The whole loop is generic over `ratatui::backend::Backend` so it can be
//!   driven by a `TestBackend` in non-interactive smoke tests — we never need a
//!   real terminal to verify the draw + dispatch pipeline.

use std::io;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::Terminal;

use rupoo::{AgentToTui, MessageRole, PendingTool, ToolPhase, TuiToAgent};

use super::tui_view::{
    apply_event, render_frame, GuideOverlay, Phase, StreamItem, ANIM_MS, HINT_ROTATE_MS, HINT_TIPS,
};
use super::ReplSession;
use super::MAX_INPUT_HISTORY;
use std::time::Instant;

/// Largest char boundary at or before `pos` (moves the caret one whole
/// character to the left). `str::is_char_boundary` is stable, so this works on
/// any Rust toolchain and never wraps a multibyte UTF-8 sequence in half.
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let mut i = pos - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary at or after `pos + 1` (moves the caret one whole
/// character to the right). Clamps to `s.len()` so the caret can never run past
/// the end of the string.
fn next_char_boundary(s: &str, pos: usize) -> usize {
    let mut i = (pos + 1).min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

impl ReplSession {
    /// Rows scrolled per mouse-wheel notch (a few lines feels natural for
    /// reading; trackpads emit many small events so this stays responsive).
    const MOUSE_SCROLL_STEP: i32 = 3;

    /// Entry point for the opt-in ratatui companion surface.
    ///
    /// Sets up the alternate screen + raw mode, runs the loop, and **always**
    /// restores the terminal (leaves alt screen, disables raw mode) on exit.
    pub(crate) fn run_ratatui(&mut self) -> Result<(), &'static str> {
        enable_raw_mode().map_err(|_| "raw mode failed")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|_| "enter alt screen failed")?;
        // Mouse-wheel scrolling of the conversation stream. Safe to enable in
        // the alt screen; disabled again on the way out.
        execute!(stdout, EnableMouseCapture).map_err(|_| "mouse capture failed")?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|_| "terminal init failed")?;
        terminal.show_cursor().ok();

        let result = self.run_ratatui_terminal(&mut terminal);

        // Restore terminal no matter what.
        disable_raw_mode().ok();
        execute!(io::stdout(), LeaveAlternateScreen).ok();
        execute!(io::stdout(), DisableMouseCapture).ok();
        result
    }

    /// Core loop, generic over the backend so tests can pass a `TestBackend`.
    ///
    /// The loop owns `ui_rx` exclusively in ratatui mode (it `take()`s it), so
    /// there is no interaction with the legacy `drain_and_render` path.
    fn run_ratatui_terminal<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), &'static str> {
        let ui_rx = self.ui_rx.take();

        // First-launch getting-started guide (unless previously dismissed).
        if let (Some(repo), Some(handle)) = (self.app.repo.clone(), self.app.rt_handle.clone()) {
            let dismissed = handle
                .block_on(repo.get_setting("guide_dismissed"))
                .ok()
                .flatten()
                .is_some_and(|v| v == "true");
            if !dismissed {
                self.chat_view.guide = Some(GuideOverlay {
                    dismiss_checked: false,
                    scroll: 0,
                });
            }
        }

        self.draw_frame(terminal)?;

        loop {
            if self.app.quit {
                break;
            }

            // 1) Drain any pending agent events (non-blocking) and repaint.
            if let Some(rx) = &ui_rx {
                while let Ok(msg) = rx.try_recv() {
                    if let AgentToTui::RequestApproval(pending) = &msg {
                        // Interactive approval prompt (blocks until answered),
                        // exactly like the legacy path, but rendered in-stream.
                        self.handle_approval_tui(terminal, pending.clone())?;
                        continue;
                    }
                    self.pump_agent_event(terminal, msg)?;
                }
            }

            // 1.5) Idle animation tick: advance the status pulse + rotate the
            // bottom hint tip on a steady cadence, independent of user input, so
            // the waiting state feels alive. Driven by wall-clock, not the 50ms
            // poll, so input latency is untouched.
            let now = Instant::now();
            if now.duration_since(self.anim_clock) >= Duration::from_millis(ANIM_MS) {
                self.anim_clock = now;
                self.chat_view.anim_frame = self.chat_view.anim_frame.wrapping_add(1);
                if now.duration_since(self.hint_clock) >= Duration::from_millis(HINT_ROTATE_MS) {
                    self.hint_clock = now;
                    self.chat_view.hint_index = (self.chat_view.hint_index + 1) % HINT_TIPS.len();
                }
                self.draw_frame(terminal)?;
            }

            // 2) Poll user input with a short timeout so streaming keeps
            //    repainting and the prompt stays responsive.
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        self.handle_key_tui(terminal, key)?;
                    }
                    Ok(Event::Mouse(m)) => {
                        // Mouse wheel scrolls the conversation stream (or the
                        // getting-started overlay when it is open).
                        match m.kind {
                            MouseEventKind::ScrollUp => {
                                self.scroll_chat_by(-Self::MOUSE_SCROLL_STEP, terminal)?;
                            }
                            MouseEventKind::ScrollDown => {
                                self.scroll_chat_by(Self::MOUSE_SCROLL_STEP, terminal)?;
                            }
                            MouseEventKind::Down(_) => {
                                // Click on the status panel toggles expand/collapse.
                                let r = self.chat_view.status_panel_rect;
                                if m.row >= r.y
                                    && m.row < r.y.saturating_add(r.height)
                                    && m.column >= r.x
                                    && m.column < r.x.saturating_add(r.width)
                                {
                                    self.chat_view.status_expanded =
                                        !self.chat_view.status_expanded;
                                    self.chat_view.status_scroll = 0;
                                    // Expanding (re)starts live-follow (Issue 3).
                                    if self.chat_view.status_expanded {
                                        self.chat_view.status_follow = true;
                                    }
                                    self.draw_frame(terminal)?;
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::Resize(..)) => {
                        // A resize changes the backend size; the next draw
                        // recomputes the layout, so repaint immediately.
                        self.draw_frame(terminal)?;
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    /// Apply one agent event to the view and repaint. Isolated so tests can
    /// drive the full pipeline (reducer + side-effects + draw) without a live
    /// input loop.
    fn pump_agent_event<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        msg: AgentToTui,
    ) -> Result<(), &'static str> {
        self.on_agent_event_tui(&msg);
        self.draw_frame(terminal)?;
        Ok(())
    }

    #[inline]
    fn draw_frame<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), &'static str> {
        let view = &mut self.chat_view;
        terminal
            .draw(|f| render_frame(f, view))
            .map_err(|_| "tui draw failed")?;
        Ok(())
    }

    /// Scroll the conversation stream by `delta` visual rows (negative = up).
    ///
    /// Scrolling up pauses auto-follow (so history stays put); scrolling down
    /// lets `render_frame` snap back to the newest line once the bottom is
    /// reached. Shared by the arrow/PageUp-Down keys and the mouse wheel.
    fn scroll_chat_by<B: Backend>(
        &mut self,
        delta: i32,
        terminal: &mut Terminal<B>,
    ) -> Result<(), &'static str> {
        if let Some(guide) = self.chat_view.guide.as_mut() {
            // While the getting-started overlay is open, the wheel scrolls it.
            if delta < 0 {
                guide.scroll = guide.scroll.saturating_sub((-delta) as u16);
            } else {
                guide.scroll = guide.scroll.saturating_add(delta as u16).min(200);
            }
            self.draw_frame(terminal)?;
            return Ok(());
        }
        if delta < 0 {
            self.chat_view.follow = false;
            self.chat_view.scroll = self.chat_view.scroll.saturating_sub((-delta) as u16);
        } else {
            self.chat_view.scroll = self.chat_view.scroll.saturating_add(delta as u16);
        }
        self.draw_frame(terminal)?;
        Ok(())
    }

    /// Map an agent event onto the view + the minimal app state side-effects.
    ///
    /// Rendering goes exclusively through `apply_event` (the tested reducer).
    /// The only addition here is the small set of side-effects the legacy
    /// `handle_agent_event` also performs (history, persistence, generation
    /// flags, token counters) so the rest of the app keeps working.
    fn on_agent_event_tui(&mut self, msg: &AgentToTui) {
        // The bridge echoes the user turn as a `Message(User)`; the input path
        // already committed that turn to the stream, so skip it here to avoid
        // a duplicate inline user line.
        match msg {
            AgentToTui::Message(m) if m.role == MessageRole::User => {}
            _ => {
                apply_event(&mut self.chat_view, msg);
            }
        }

        match msg {
            AgentToTui::LayoutModeHint(mode) => {
                self.app.layout_mode = *mode;
            }
            AgentToTui::Message(m) => {
                self.app.push_message(m.clone());
                self.app.persist_sessions();
            }
            AgentToTui::Thinking => self.app.set_thinking(),
            AgentToTui::Idle => {
                self.app.set_idle();
                let duration = self
                    .gen_start
                    .map(|s| s.elapsed().as_secs_f64())
                    .unwrap_or(0.0);
                self.gen_start = None;
                self.chat_view.push_token_footer(duration);
            }
            AgentToTui::TokenUpdate {
                in_count,
                out_count,
            } => {
                self.app.token_in = self.app.token_in.saturating_add(*in_count);
                self.app.token_out = self.app.token_out.saturating_add(*out_count);
            }
            AgentToTui::ToolStatus { tool_name, phase } => {
                let phase_str = match phase {
                    ToolPhase::Calling => "calling",
                    ToolPhase::Completed => "completed",
                };
                self.app.current_tool_status = Some((tool_name.clone(), phase_str.to_string()));
            }
            AgentToTui::LlmStatus {
                configured,
                provider,
                model_label,
            } => {
                self.app.llm_configured = *configured;
                self.app.llm_provider = provider.clone();
                self.app.model_label = model_label.clone();
                self.chat_view.model_label = model_label.clone();
            }
            AgentToTui::HybridSearchUpdate { enabled } => {
                self.app.hybrid_search = *enabled;
            }
            // Plan / step / file-change progress have no reducer mapping yet;
            // surface them as inline system lines so the stream stays
            // informative without the legacy `println!`.
            AgentToTui::StepProgress {
                step_index,
                total,
                step_name,
            } => {
                self.chat_view.items.push(StreamItem::System(format!(
                    "▸ {}/{}: {}",
                    step_index + 1,
                    total,
                    step_name
                )));
            }
            AgentToTui::PlanTaskList { tasks } => {
                for (name, _status) in tasks {
                    self.chat_view
                        .items
                        .push(StreamItem::System(format!("• {name}")));
                }
            }
            AgentToTui::FileChanges { files } => {
                for f in files {
                    self.chat_view
                        .items
                        .push(StreamItem::System(format!("⎇ {}", f.path)));
                }
            }
            _ => {}
        }
    }

    /// Inline input editor for the ratatui surface.
    fn handle_key_tui<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        key: crossterm::event::KeyEvent,
    ) -> Result<(), &'static str> {
        // Keep the cursor index within bounds (it is a byte offset) AND on a
        // char boundary. Anything that mutates `input` keeps the cursor on a
        // boundary, but we still re-clamp defensively: a cursor not on a char
        // boundary would make the next `&input[..cursor]` slice (in the renderer)
        // panic on multibyte text.
        let input_len = self.chat_view.input.len();
        if self.chat_view.cursor > input_len {
            self.chat_view.cursor = input_len;
        }
        self.chat_view.cursor = self
            .chat_view
            .input
            .floor_char_boundary(self.chat_view.cursor);

        // First-launch guide overlay captures all keys until dismissed.
        if let Some(guide) = self.chat_view.guide.as_mut() {
            match key.code {
                KeyCode::Up => {
                    guide.scroll = guide.scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    guide.scroll = guide.scroll.saturating_add(1).min(200);
                }
                KeyCode::PageUp => {
                    guide.scroll = guide.scroll.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    guide.scroll = guide.scroll.saturating_add(5).min(200);
                }
                KeyCode::Char('d' | 'D') => {
                    guide.dismiss_checked = !guide.dismiss_checked;
                }
                KeyCode::Enter | KeyCode::Esc => {
                    self.close_guide();
                }
                _ => {}
            }
            self.draw_frame(terminal)?;
            return Ok(());
        }

        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                let input = self.chat_view.input.trim().to_string();
                // Mirror the legacy Enter history bookkeeping (done in
                // handle_input there) so Up/Down history works here too.
                let is_dup = self.app.input_history.last() == Some(&input);
                if !is_dup && !input.is_empty() {
                    self.app.input_history.push(input.clone());
                    if self.app.input_history.len() > MAX_INPUT_HISTORY {
                        self.app.input_history.remove(0);
                    }
                }
                self.app.input_history_index = self.app.input_history.len();

                self.chat_view.input.clear();
                self.chat_view.cursor = 0;
                self.draw_frame(terminal)?;
                if !input.is_empty() {
                    // Mark turn start so the footer can report wall-clock time.
                    self.gen_start = Some(std::time::Instant::now());
                    self.process_input(&input);
                }
                self.draw_frame(terminal)?;
            }
            (KeyCode::Esc, _) => {
                // Gentle cancel during streaming (same as the first Ctrl+C),
                // without forcing a quit. Only meaningful while a reply is in
                // flight; pressing Esc when idle is a no-op.
                if self.chat_view.phase != Phase::Idle {
                    self.app.cancel_flag.store(true, Ordering::Relaxed);
                    self.chat_view
                        .items
                        .push(StreamItem::System("⏹ cancelled".to_string()));
                    self.draw_frame(terminal)?;
                }
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.app.cancel_flag.load(Ordering::Relaxed) {
                    self.app.quit = true;
                } else {
                    self.app.cancel_flag.store(true, Ordering::Relaxed);
                    self.chat_view
                        .items
                        .push(StreamItem::System("⏹ cancelled".to_string()));
                }
                self.draw_frame(terminal)?;
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.app.quit = true;
            }
            (KeyCode::Backspace, _) if self.chat_view.cursor > 0 => {
                // `pos` is floored to a char boundary so the `&input[..pos]`
                // slice below can never split a multibyte sequence and panic.
                let pos = self
                    .chat_view
                    .input
                    .floor_char_boundary(self.chat_view.cursor);
                let s = &self.chat_view.input[..pos];
                let boundary = s.floor_char_boundary(s.len().saturating_sub(1));
                self.chat_view.input.drain(boundary..pos);
                self.chat_view.cursor = boundary;
                self.draw_frame(terminal)?;
            }
            (KeyCode::Left, _) if self.chat_view.cursor > 0 => {
                // Move by one whole character (not one byte) so the caret on a
                // multibyte sequence (CJK / emoji) stays on a char boundary and
                // keeps `&input[..cursor]` slices valid.
                self.chat_view.cursor =
                    prev_char_boundary(&self.chat_view.input, self.chat_view.cursor);
                self.draw_frame(terminal)?;
            }
            (KeyCode::Right, _) if self.chat_view.cursor < self.chat_view.input.len() => {
                self.chat_view.cursor =
                    next_char_boundary(&self.chat_view.input, self.chat_view.cursor);
                self.draw_frame(terminal)?;
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.chat_view.cursor = 0;
                self.draw_frame(terminal)?;
            }
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.chat_view.cursor = self.chat_view.input.len();
                self.draw_frame(terminal)?;
            }
            // Scroll the conversation stream. Arrows give fine control
            // (1/3 page); PageUp/PageDown move a full page. History recall moves
            // to Ctrl+P / Ctrl+N so the arrows are free for scrolling.
            (KeyCode::Up, _) => {
                if self.chat_view.status_expanded {
                    // Scroll the expanded mini-log up (toward older activity).
                    self.chat_view.status_scroll = self.chat_view.status_scroll.saturating_add(1);
                    // Manual scroll takes control: freeze the panel (Issue 3).
                    self.chat_view.status_follow = false;
                    self.draw_frame(terminal)?;
                } else {
                    let step = (self.chat_view.height as usize / 3).max(1) as i32;
                    self.scroll_chat_by(-step, terminal)?;
                }
            }
            (KeyCode::Down, _) => {
                if self.chat_view.status_expanded {
                    // Scroll the expanded mini-log down (toward newest).
                    self.chat_view.status_scroll = self.chat_view.status_scroll.saturating_sub(1);
                    // Manual scroll takes control: freeze the panel (Issue 3).
                    self.chat_view.status_follow = false;
                    self.draw_frame(terminal)?;
                } else {
                    let step = (self.chat_view.height as usize / 3).max(1) as i32;
                    self.scroll_chat_by(step, terminal)?;
                }
            }
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                if self.app.input_history_index > 0 {
                    self.app.input_history_index -= 1;
                    self.chat_view.input =
                        self.app.input_history[self.app.input_history_index].clone();
                    self.chat_view.cursor = self.chat_view.input.len();
                    self.draw_frame(terminal)?;
                }
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                let max = self.app.input_history.len();
                if self.app.input_history_index < max {
                    self.app.input_history_index += 1;
                    if self.app.input_history_index >= max {
                        self.chat_view.input.clear();
                    } else {
                        self.chat_view.input =
                            self.app.input_history[self.app.input_history_index].clone();
                    }
                    self.chat_view.cursor = self.chat_view.input.len();
                    self.draw_frame(terminal)?;
                }
            }
            // Scroll the conversation stream a full page. Up/Down stay mapped
            // to input-history recall (conventional for a focused input line).
            (KeyCode::PageUp, _) => {
                let step = self.chat_view.height.max(1) as i32;
                self.scroll_chat_by(-step, terminal)?;
            }
            (KeyCode::PageDown, _) => {
                let step = self.chat_view.height.max(1) as i32;
                self.scroll_chat_by(step, terminal)?;
            }
            (KeyCode::Tab, _) => {
                if ReplSession::complete_input(
                    &mut self.chat_view.input,
                    &mut self.chat_view.cursor,
                    false,
                ) {
                    self.draw_frame(terminal)?;
                }
            }
            // Toggle the bottom status panel expand/collapse.
            (KeyCode::Char(']'), _) => {
                self.chat_view.status_expanded = !self.chat_view.status_expanded;
                self.chat_view.status_scroll = 0;
                // Expanding (re)starts live-follow; collapsing drops the state.
                if self.chat_view.status_expanded {
                    self.chat_view.status_follow = true;
                }
                self.draw_frame(terminal)?;
            }
            (KeyCode::Char(ch), _) => {
                self.chat_view.input.insert(self.chat_view.cursor, ch);
                self.chat_view.cursor += ch.len_utf8();
                self.draw_frame(terminal)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Dismiss the getting-started overlay. If the user ticked "不再显示",
    /// persist a flag so it won't auto-pop up on future launches.
    fn close_guide(&mut self) {
        let dismiss = self
            .chat_view
            .guide
            .as_ref()
            .is_some_and(|g| g.dismiss_checked);
        self.chat_view.guide = None;
        if dismiss {
            if let (Some(repo), Some(handle)) = (self.app.repo.clone(), self.app.rt_handle.clone())
            {
                std::thread::spawn(move || {
                    let _ = handle.block_on(repo.set_setting("guide_dismissed", "true"));
                });
            }
        }
    }

    /// Interactive approval prompt rendered in-stream (ratatui equivalent of the
    /// legacy `handle_approval`). Blocks until the user answers y/n/a.
    fn handle_approval_tui<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        pending: PendingTool,
    ) -> Result<(), &'static str> {
        let args_disp = if pending.args.len() > 80 {
            format!("{}…", &pending.args[..77])
        } else {
            pending.args.clone()
        };
        self.chat_view.items.push(StreamItem::System(format!(
            "⚠ Approval required: {} ({})  [y/n/a]",
            pending.tool_name, args_disp
        )));
        self.draw_frame(terminal)?;

        if self.app.approve_all {
            if let Some(tx) = &self.app.agent_tx {
                let _ = tx.send(TuiToAgent::ApproveTool("approved".to_string()));
            }
            self.chat_view
                .items
                .push(StreamItem::System("✓ auto-approved".to_string()));
            self.draw_frame(terminal)?;
            return Ok(());
        }

        loop {
            if event::poll(Duration::from_millis(500)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('y' | 'Y') => {
                            if let Some(tx) = &self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::ApproveTool("approved".to_string()));
                            }
                            self.chat_view
                                .items
                                .push(StreamItem::System("✓ approved".to_string()));
                            break;
                        }
                        KeyCode::Char('n' | 'N') => {
                            if let Some(tx) = &self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::DenyTool);
                            }
                            self.chat_view
                                .items
                                .push(StreamItem::System("✗ denied".to_string()));
                            break;
                        }
                        KeyCode::Char('a' | 'A') => {
                            self.app.approve_all = true;
                            if let Some(tx) = &self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::ApproveAll);
                            }
                            self.chat_view
                                .items
                                .push(StreamItem::System("✓ auto-approve enabled".to_string()));
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        self.draw_frame(terminal)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::app;
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use rupoo::ChatMessage;

    /// Build a minimal `ReplSession` for the ratatui pipeline. The default
    /// (`Terminal`) path is never exercised here — this only drives the
    /// `apply_event` + side-effects + `render_frame` pipeline via
    /// `pump_agent_event`, which needs no real terminal / stdin.
    fn make_session() -> ReplSession {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        // The handle is stored in `app.rt_handle`; we don't block_on anything in
        // the test, so dropping the runtime is safe.
        drop(rt);
        ReplSession::new(None, None, None, vec![], "test-model".to_string(), handle).unwrap()
    }

    #[test]
    fn char_boundary_helpers_move_by_full_chars() {
        // 'a' (1 byte), '你' (3 bytes, bytes 1..4), 'b' (1 byte) -> len 5.
        let s = "a你b";
        assert_eq!(next_char_boundary(s, 0), 1); // into '你'
        assert_eq!(prev_char_boundary(s, 5), 4); // before 'b'
        assert_eq!(prev_char_boundary(s, 4), 1); // before '你' (skips its bytes)
        assert_eq!(next_char_boundary(s, 1), 4); // after '你' (skips to 'b')
        assert_eq!(prev_char_boundary(s, 0), 0); // clamp at start
        assert_eq!(next_char_boundary(s, 5), 5); // clamp at end
    }

    #[test]
    fn ratatui_smoke_renders_full_turn() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        let backend = TestBackend::new(80, 16);
        let mut term = Terminal::new(backend).unwrap();

        // The user turn is committed by the input path (submit_message), which
        // the live loop calls on Enter. Drive that first, then the agent stream.
        session.submit_message("fix the bug");

        let events = vec![
            AgentToTui::Thinking,
            AgentToTui::ThinkingSummary {
                text: "looking at main.rs".to_string(),
            },
            AgentToTui::ToolStatus {
                tool_name: "read_file".to_string(),
                phase: ToolPhase::Calling,
            },
            AgentToTui::ToolStatus {
                tool_name: "read_file".to_string(),
                phase: ToolPhase::Completed,
            },
            AgentToTui::StreamChunk {
                text: "The fix is ".to_string(),
            },
            AgentToTui::StreamChunk {
                text: "to add a guard.".to_string(),
            },
            AgentToTui::Message(ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                is_command_output: false,
                timestamp: None,
            }),
            AgentToTui::Idle,
        ];

        for e in events {
            session.pump_agent_event(&mut term, e).unwrap();
        }

        let buf: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        // CJK glyphs occupy two buffer cells (a space placeholder in the second
        // cell), so strip whitespace before substring checks on Chinese text.
        let compact: String = buf.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(buf.contains("fix the bug"), "user line missing");
        assert!(buf.contains("looking at main.rs"), "thinking missing");
        assert!(compact.contains("读取文件"), "tool missing (summary/panel)");
        assert!(compact.contains("完成"), "collapsed tool summary missing");
        assert!(
            buf.contains("The fix is to add a guard."),
            "assistant missing"
        );
        assert!(buf.contains("Rupoo"), "status bar / brand missing");

        // History side-effects fired: user (from submit) + assistant recorded.
        let has_assistant = session
            .app
            .messages
            .iter()
            .any(|m| m.role == MessageRole::Assistant);
        assert!(has_assistant, "assistant message recorded");
        assert!(!session.app.thinking, "idle cleared thinking flag");
    }

    #[test]
    fn ratatui_user_message_not_duplicated() {
        // The bridge echoes the user turn as Message(User); the input path
        // already committed it, so on_agent_event_tui must skip it.
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        // Simulate what submit_message does in tui mode.
        session
            .chat_view
            .items
            .push(StreamItem::User("hi there".to_string()));

        let backend = TestBackend::new(80, 16);
        let mut term = Terminal::new(backend).unwrap();
        session
            .pump_agent_event(
                &mut term,
                AgentToTui::Message(ChatMessage::user("hi there".to_string())),
            )
            .unwrap();

        let user_count = session
            .chat_view
            .items
            .iter()
            .filter(|i| matches!(i, StreamItem::User(_)))
            .count();
        assert_eq!(user_count, 1, "user line must not be duplicated");
    }

    /// Regression guard for Step 3: slash-command output must render *inline*
    /// in the ratatui companion stream (not via a bare `println!` that the
    /// next per-frame repaint would overwrite → flicker).
    ///
    /// We drive the real command path (`process_input` → `handle_command` →
    /// `emit`) the same way the live `handle_key_tui` Enter branch does, then
    /// assert (a) the text appears in the ratatui buffer, and (b) it survives a
    /// *second* repaint — proving it lives in the retained `chat_view` and is
    /// not a transient raw-stdout write.
    #[test]
    fn ratatui_command_output_inline_no_flicker() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        // Generous viewport so nothing scrolls off and every assertion is stable.
        let backend = TestBackend::new(100, 80);
        let mut term = Terminal::new(backend).unwrap();

        // Same sequence the live loop runs on Enter for "/help".
        session.process_input("/help");
        session.draw_frame(&mut term).unwrap();

        let buf: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(buf.contains("Commands:"), "help header missing inline");
        assert!(
            buf.contains("Quick Actions:"),
            "quick actions missing inline"
        );
        assert!(buf.contains("/tools"), "tool command missing inline");

        // A subsequent frame must still show the command output — no flicker.
        session.draw_frame(&mut term).unwrap();
        let buf2: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            buf2.contains("Commands:"),
            "help header flickered away on repaint"
        );
        assert!(buf2.contains("/help"), "slash command text flickered away");

        // /tools also routes through `emit` and must render inline.
        session.process_input("/tools");
        session.draw_frame(&mut term).unwrap();
        let buf3: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            buf3.contains("Available Tools:"),
            "tools list missing inline"
        );
        assert!(buf3.contains("file_read"), "tool name missing inline");
    }

    /// Regression guard for the manual-scroll fix: PageUp must scroll the
    /// stream up a full page and pause auto-follow, while the default state
    /// pins to the newest line.
    #[test]
    fn pageup_scrolls_up_and_pauses_follow() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        for i in 0..100 {
            session
                .chat_view
                .items
                .push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 8);
        let mut term = Terminal::new(backend).unwrap();

        session.draw_frame(&mut term).unwrap(); // follow → newest
        assert!(
            session.chat_view.follow,
            "fresh view should follow to bottom"
        );

        session
            .handle_key_tui(
                &mut term,
                KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
            )
            .unwrap();
        assert!(!session.chat_view.follow, "manual scroll pauses follow");
        assert!(
            session.chat_view.scroll > 0 && session.chat_view.scroll < 94,
            "first PageUp should step ~one page up, not jump to top or stay at bottom"
        );
    }

    /// Regression guard for resize: the loop only repaints on resize because
    /// ratatui recomputes the layout from the (now changed) backend size on the
    /// next draw. This drives that repaint path and proves the stream adapts
    /// to a smaller window without losing the newest line.
    #[test]
    fn resize_repaints_and_keeps_newest() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        for i in 0..50 {
            session
                .chat_view
                .items
                .push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.draw_frame(&mut term).unwrap();

        // Simulate the terminal shrinking (the loop calls draw_frame on Resize).
        term.backend_mut().resize(60, 10);
        session.draw_frame(&mut term).unwrap();

        let s: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(s.contains("line 49"), "newest line must survive a resize");
    }

    /// The bottom scroll-hint bar must always render so the keybindings are
    /// discoverable. Assert on ASCII substrings (CJK glyphs occupy two cells in
    /// the TestBackend buffer and would not match a contiguous `contains`).
    #[test]
    fn scroll_hint_bar_visible() {
        let mut session = make_session();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.draw_frame(&mut term).unwrap();

        let s: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(s.contains(HINT_TIPS[0]), "guide tip missing: {s}");
        assert!(
            !s.contains("mouse wheel"),
            "redundant scroll hint must be removed from the hint bar: {s}"
        );
    }

    /// Mouse wheel up must pause auto-follow and move the view up the stream.
    #[test]
    fn mouse_wheel_up_pauses_follow() {
        let mut session = make_session();
        for i in 0..100 {
            session
                .chat_view
                .items
                .push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.draw_frame(&mut term).unwrap(); // follow -> bottom
        assert!(session.chat_view.follow, "fresh view should follow");

        session.scroll_chat_by(-3, &mut term).unwrap();
        assert!(!session.chat_view.follow, "wheel up pauses follow");
        assert!(
            session.chat_view.scroll > 0,
            "wheel up should move the view away from the bottom"
        );
    }

    /// Scrolling back down to the bottom (wheel down) must resume following.
    #[test]
    fn mouse_wheel_down_resumes_follow() {
        let mut session = make_session();
        for i in 0..100 {
            session
                .chat_view
                .items
                .push(StreamItem::Assistant(format!("line {i}")));
        }
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.draw_frame(&mut term).unwrap();

        session.scroll_chat_by(-10, &mut term).unwrap();
        assert!(!session.chat_view.follow, "scrolled up should pause follow");
        session.scroll_chat_by(200, &mut term).unwrap();
        assert!(session.chat_view.follow, "reaching bottom resumes follow");
    }

    /// The `]` key toggles the bottom status panel expand/collapse.
    #[test]
    fn status_panel_bracket_toggles_expand() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.draw_frame(&mut term).unwrap();
        assert!(
            !session.chat_view.status_expanded,
            "panel defaults to collapsed"
        );

        session
            .handle_key_tui(
                &mut term,
                KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
            )
            .unwrap();
        assert!(
            session.chat_view.status_expanded,
            "bracket expands the panel"
        );

        session
            .handle_key_tui(
                &mut term,
                KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE),
            )
            .unwrap();
        assert!(
            !session.chat_view.status_expanded,
            "bracket collapses it again"
        );
    }

    /// Issue 3 (pipeline): driving a full turn with a tool leaves the bottom
    /// status panel frozen at the last item (`status_follow` off) once the
    /// workflow ends, so it no longer auto-scrolls into the user's view.
    #[test]
    fn status_panel_frozen_after_workflow_pipeline() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        session.chat_view.status_expanded = true;
        session.draw_frame(&mut term).unwrap();
        assert!(
            session.chat_view.status_follow,
            "panel follows while idle/empty"
        );

        session
            .pump_agent_event(
                &mut term,
                AgentToTui::ToolStatus {
                    tool_name: "read_file".to_string(),
                    phase: ToolPhase::Calling,
                },
            )
            .unwrap();
        assert!(
            session.chat_view.status_follow,
            "panel follows while running"
        );

        session
            .pump_agent_event(&mut term, AgentToTui::Idle)
            .unwrap();
        assert!(
            !session.chat_view.status_follow,
            "panel freezes after the workflow ends"
        );
        assert!(
            session.chat_view.status_expanded,
            "panel stays expanded, frozen at the last item"
        );
    }

    /// End-to-end guard that replays the *exact* agent event sequence the live
    /// `chat_mode.rs` bridge emits for a tool-using turn (ToolCall→Calling,
    /// ToolResult→Completed, TextDelta→StreamChunk, Message(Assistant) → Idle)
    /// through the real `pump_agent_event` pipeline. This proves both fixes land
    /// in the running UI, not just in the unit-level reducer: the inline tool
    /// "done" rows collapse into one summary, and the status panel freezes
    /// (stops auto-scrolling) at the last item.
    #[test]
    fn live_chat_sequence_collapses_tools_and_freezes_panel() {
        let mut session = make_session();
        session.app.render_mode = app::RenderMode::Ratatui;
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();

        // The user turn is committed by submit_message, as the live loop does.
        session.submit_message("fix the bug");
        session.chat_view.status_expanded = true; // user had the panel open

        let events = vec![
            AgentToTui::Thinking,
            AgentToTui::ToolStatus {
                tool_name: "read_file".to_string(),
                phase: ToolPhase::Calling,
            },
            AgentToTui::ToolStatus {
                tool_name: "read_file".to_string(),
                phase: ToolPhase::Completed,
            },
            AgentToTui::StreamChunk {
                text: "The fix is ".to_string(),
            },
            AgentToTui::StreamChunk {
                text: "to add a guard.".to_string(),
            },
            AgentToTui::Message(ChatMessage::assistant(
                "The fix is to add a guard.".to_string(),
            )),
            AgentToTui::Idle,
        ];
        for e in events {
            session.pump_agent_event(&mut term, e).unwrap();
        }

        // Issue 2: no inline tool "done" rows remain; exactly one summary.
        let tool_count = session
            .chat_view
            .items
            .iter()
            .filter(|i| matches!(i, StreamItem::Tool { .. }))
            .count();
        assert_eq!(tool_count, 0, "inline tool rows must be collapsed away");
        assert!(
            session
                .chat_view
                .items
                .iter()
                .any(|i| matches!(i, StreamItem::Summary(_))),
            "collapsed summary must be present"
        );

        // Issue 3: panel frozen at the last item (no auto-scroll churn).
        assert!(
            !session.chat_view.status_follow,
            "panel must freeze after the workflow ends"
        );
        assert!(
            session.chat_view.status_expanded,
            "panel stays expanded, frozen at the last item"
        );
    }
}
