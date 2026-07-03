//! REPL-based CLI — native terminal output, rustyline input.
//!
//! No TUI framework. Terminal handles scrolling and resize.
//! We just render content to stdout and let the terminal do the rest.

pub mod app;
pub mod cmds;

pub mod enhanced_ui;
pub mod markdown;
pub mod output;
pub mod theme;

mod approval;
mod bridge;
mod chat_mode;
mod plan_mode;

pub use app::RupooApp;
pub use rupoo::{AgentToTui, ChatMessage, LayoutMode, PendingTool, ToolPhase, TuiToAgent};

use std::io::{self, Write};

use crossbeam_channel::{Receiver, Sender};
use owo_colors::OwoColorize;
use rupoo::agent::Agent;
use rupoo::db::TaskRepo;
use rupoo::llm::ConversationHistory;
use unicode_width::UnicodeWidthChar;

// Magic number constants
/// Max input history entries to retain.
const MAX_INPUT_HISTORY: usize = 100;
/// Max chars to display for tool approval arguments.
const MAX_APPROVAL_ARGS_DISPLAY: usize = 80;
/// Default token budget for conversation history.
pub(super) const DEFAULT_TOKEN_BUDGET: usize = 60000;
/// Spinner poll interval in milliseconds.
const SPINNER_POLL_MS: u64 = 50;
/// ANSI escape sequence to clear screen and home cursor.
const CLEAR_SCREEN_ESCAPE: &str = "\x1b[2J\x1b[H";
/// Default max turns for conversation history initialization.
pub(super) const HISTORY_DEFAULT_MAX_TURNS: usize = 10;

// ═══════════════════════════════════════════════════════════════════════════
// REPL Session
// ═══════════════════════════════════════════════════════════════════════════

pub struct ReplSession {
    app: RupooApp,
    ui_rx: Option<Receiver<AgentToTui>>,
    /// Streaming state for current assistant response
    stream_state: markdown::StreamState,
    /// Timestamp when current generation started
    gen_start: Option<std::time::Instant>,
}

impl ReplSession {
    pub fn new(
        agent_tx: Option<Sender<TuiToAgent>>,
        ui_rx: Option<Receiver<AgentToTui>>,
        repo: Option<std::sync::Arc<TaskRepo>>,
        sessions_data: Vec<(String, String, String, bool)>,
        model_label: String,
        rt_handle: tokio::runtime::Handle,
    ) -> Result<Self, &'static str> {
        let mut app = RupooApp::new(agent_tx, rt_handle);
        app.model_label = model_label;

        if let Some(r) = repo {
            app = app.set_repo(r);
        }

        let active_id: String = sessions_data
            .iter()
            .find(|(_, _, _, is_active)| *is_active)
            .map(|(id, _, _, _)| id.clone())
            .unwrap_or_else(|| "default".to_string());

        if !sessions_data.is_empty() {
            app.sessions.retain(|s| s.id != "default");
        }

        for (id, label, messages_json, is_active) in &sessions_data {
            app.sessions.push(app::SessionTab {
                id: id.clone(),
                label: label.clone(),
                active: *is_active,
                has_context: true,
            });
            if let Ok(msgs) = serde_json::from_str::<Vec<ChatMessage>>(messages_json) {
                app.session_messages.insert(id.clone(), msgs);
            }
        }

        app.messages = app
            .session_messages
            .get(&active_id)
            .cloned()
            .unwrap_or_default();

        Ok(Self {
            app,
            ui_rx,
            stream_state: markdown::StreamState::new(),
            gen_start: None,
        })
    }

    /// Run the REPL main loop.
    pub fn run(&mut self) -> Result<(), &'static str> {
        // Set green blinking bar cursor
        output::set_cursor_style_bar();

        // Print welcome
        output::welcome(env!("CARGO_PKG_VERSION"), &self.app.model_label);

        let result = self.run_loop();

        // Reset cursor style on exit
        output::reset_cursor_style();

        result
    }

    /// Inner REPL loop — uses crossterm raw mode for input, no rustyline.
    fn run_loop(&mut self) -> Result<(), &'static str> {
        loop {
            if self.app.quit {
                break Ok(());
            }

            // If thinking, drain events and show spinner
            if self.app.thinking {
                self.drain_and_render()?;
                continue;
            }

            // Drain pending background messages
            if let Some(ref rx) = self.ui_rx {
                while let Ok(msg) = rx.try_recv() {
                    if let AgentToTui::Message(m) = msg {
                        if m.role == rupoo::MessageRole::System && !m.content.is_empty() {
                            println!();
                            output::system(&m.content);
                        }
                    }
                }
            }

            // Read user input with custom handler (crossterm raw mode)
            self.handle_input()?;
        }
    }

    /// Read one line of user input using crossterm raw mode.
    /// Blocks until Enter, Ctrl+C (×2 to quit), or Ctrl+D.
    /// Draws "> " prompt + bottom bar, handles editing, history, tab completion.
    fn handle_input(&mut self) -> Result<(), &'static str> {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
        use crossterm::terminal;

        terminal::enable_raw_mode().map_err(|_| "raw mode failed")?;

        use unicode_width::UnicodeWidthStr;
        let mut buf = String::with_capacity(256);
        let mut cursor_pos: usize = 0;

        // Show initial prompt with bottom bar (no previous bar to erase)
        self.redraw_prompt(&buf, cursor_pos, None);

        loop {
            // Poll with timeout so we can check cancelled flag
            if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                if let Event::Key(key) = event::read().ok().unwrap() {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    match (key.code, key.modifiers) {
                        // ── Enter ──
                        (KeyCode::Enter, _) => {
                            let input = buf.trim().to_string();
                            terminal::disable_raw_mode().ok();
                            if input.is_empty() {
                                self.redraw_prompt(&buf, cursor_pos, None);
                                continue;
                            }
                            // Save to history (skip if same as last)
                            let is_dup =
                                self.app.input_history.last().map_or(false, |h| h == &input);
                            if !is_dup {
                                self.app.input_history.push(input.clone());
                                if self.app.input_history.len() > MAX_INPUT_HISTORY {
                                    self.app.input_history.remove(0);
                                }
                            }
                            self.app.input_history_index = self.app.input_history.len();
                            self.process_input(&input);
                            return Ok(());
                        }

                        // ── Ctrl+C — cancel generation, twice to quit ──
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            if self
                                .app
                                .cancel_flag
                                .load(std::sync::atomic::Ordering::Relaxed)
                            {
                                // Second Ctrl+C → quit
                                self.app.quit = true;
                                terminal::disable_raw_mode().ok();
                                return Ok(());
                            }
                            // First Ctrl+C → cancel current generation
                            self.app
                                .cancel_flag
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                            // Show cancel feedback on the input line
                            print!("\r\x1b[2K");
                            println!("  {} 已取消", "⏹".to_string());
                            // Erase bottom bar
                            for _ in 0..3 {
                                print!("\x1b[1A\x1b[2K");
                            }
                            let _ = io::stdout().flush();
                            terminal::disable_raw_mode().ok();
                            return Ok(());
                        }

                        // ── Ctrl+D — quit ──
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            println!("\n  Bye! 👋");
                            self.app.quit = true;
                            terminal::disable_raw_mode().ok();
                            return Ok(());
                        }

                        // ── Backspace ──
                        (KeyCode::Backspace, _) if cursor_pos > 0 => {
                            if !buf.is_char_boundary(cursor_pos) {
                                cursor_pos = buf.floor_char_boundary(cursor_pos);
                            }
                            let s = &buf[..cursor_pos];
                            let char_boundary = s.floor_char_boundary(s.len().saturating_sub(1));
                            if char_boundary < cursor_pos {
                                buf.drain(char_boundary..cursor_pos);
                                cursor_pos = char_boundary;
                                self.redraw_prompt(&buf, cursor_pos, None);
                            }
                        }

                        // ── Left arrow ──
                        (KeyCode::Left, _) if cursor_pos > 0 => {
                            cursor_pos -= 1;
                            print!("\x1b[D");
                            let _ = io::stdout().flush();
                        }

                        // ── Right arrow ──
                        (KeyCode::Right, _) if cursor_pos < buf.len() => {
                            cursor_pos += 1;
                            print!("\x1b[C");
                            let _ = io::stdout().flush();
                        }

                        // ── Home / Ctrl+A ──
                        (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                            if cursor_pos > 0 {
                                print!("\x1b[{}D", cursor_pos);
                                cursor_pos = 0;
                                let _ = io::stdout().flush();
                            }
                        }

                        // ── End / Ctrl+E ──
                        (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                            if cursor_pos < buf.len() {
                                let n = buf.len() - cursor_pos;
                                print!("\x1b[{}C", n);
                                cursor_pos = buf.len();
                                let _ = io::stdout().flush();
                            }
                        }

                        // ── Up arrow — history back ──
                        (KeyCode::Up, _) => {
                            if self.app.input_history_index > 0 {
                                self.app.input_history_index -= 1;
                                buf = self.app.input_history[self.app.input_history_index].clone();
                                cursor_pos = buf.len();
                                let hint = format!(
                                    "历史 {}/{}",
                                    self.app.input_history_index + 1,
                                    self.app.input_history.len()
                                );
                                self.redraw_prompt(&buf, cursor_pos, Some(&hint));
                            }
                        }

                        // ── Down arrow — history forward ──
                        (KeyCode::Down, _) => {
                            let max = self.app.input_history.len();
                            if self.app.input_history_index < max {
                                self.app.input_history_index += 1;
                                if self.app.input_history_index >= max {
                                    buf.clear();
                                    cursor_pos = 0;
                                } else {
                                    buf = self.app.input_history[self.app.input_history_index]
                                        .clone();
                                    cursor_pos = buf.len();
                                }
                                self.redraw_prompt(&buf, cursor_pos, None);
                            }
                        }

                        // ── Shift+Tab — cycle mode ──
                        (KeyCode::BackTab, _) => {
                            terminal::disable_raw_mode().ok();
                            // Erase prompt + bottom bar
                            print!("\r\x1b[1B");
                            for _ in 0..3 {
                                print!("\x1b[2K\x1b[1B");
                            }
                            print!("\x1b[4A\r\x1b[2K");
                            let _ = io::stdout().flush();

                            // Cycle mode
                            self.app.layout_mode = match self.app.layout_mode {
                                LayoutMode::Chat => LayoutMode::Work,
                                LayoutMode::Work => LayoutMode::Summary,
                                LayoutMode::Summary => LayoutMode::Chat,
                            };

                            let mode_name = match self.app.layout_mode {
                                LayoutMode::Chat => "auto mode",
                                LayoutMode::Work => "plan mode",
                                LayoutMode::Summary => "summary mode",
                            };
                            println!(
                                "  {} ⏵ {}",
                                "⏵".to_string().color(theme::current().think),
                                mode_name.color(theme::current().ai_header)
                            );
                            terminal::enable_raw_mode().ok();

                            // Debounce — drain any queued repeats
                            use std::time::Duration;
                            std::thread::sleep(Duration::from_millis(150));
                            while event::poll(Duration::from_millis(0)).unwrap_or(false) {
                                if let Ok(Event::Key(_)) = event::read() { /* discard */ }
                            }
                        }

                        // ── Tab — command completion ──
                        (KeyCode::Tab, _) => {
                            if self.complete_input(&mut buf, &mut cursor_pos) {
                                self.redraw_prompt(&buf, cursor_pos, None);
                            }
                        }

                        // ── Printable character ──
                        (KeyCode::Char(ch), _) => {
                            buf.insert(cursor_pos, ch);
                            cursor_pos += ch.len_utf8(); // handle multi-byte UTF-8
                            self.redraw_prompt(&buf, cursor_pos, None);
                        }

                        _ => {}
                    }
                }
            }
        }
    }

    /// Redraw the "> " prompt line and bottom bar (3 lines) below it.
    /// No erase — just overwrite. Keeps cursor on prompt line for typing.
    fn redraw_prompt(&self, buf: &str, cursor_pos: usize, history_hint: Option<&str>) {
        use std::fmt::Write;
        use unicode_width::UnicodeWidthStr;
        let mut out = String::with_capacity(256);

        // ── Draw prompt (colored) ──
        let t = theme::current();
        let (prompt_color, buf_color) = if buf.starts_with("/read ") {
            (t.think, t.think)
        } else if buf.starts_with("/cmd ") {
            (t.error, t.error)
        } else if buf.starts_with("/search ") {
            (t.ai_accent, t.ai_accent)
        } else if buf.starts_with('/') {
            (t.ai_header, t.ai_header)
        } else {
            (t.prompt, t.user_bright)
        };

        let _ = write!(out, "\r\x1b[2K{}", "> ".color(prompt_color).bold());
        if buf.is_empty() {
            let _ = write!(out, "{}", "输入消息，/help 查看命令...".color(t.dim));
        } else {
            let _ = write!(out, "{}", buf.color(buf_color));
        }

        // ── Draw bottom bar (separator, mode, hint) ──
        let width = (console::Term::stdout().size().1 as usize).max(40);
        let sep = format!("{}", "─".repeat(width.min(60)).color(t.border));

        // Mode text (real)
        let mode_label = match self.app.layout_mode {
            LayoutMode::Chat => "auto",
            LayoutMode::Work => "dev",
            LayoutMode::Summary => "auto",
        };

        // Token counts (real)
        let tok_in = if self.app.token_in > 10000 {
            format!(
                "{}.{}k",
                self.app.token_in / 1000,
                (self.app.token_in % 1000) / 100
            )
        } else if self.app.token_in > 1000 {
            format!("{:.1}k", self.app.token_in as f64 / 1000.0)
        } else {
            self.app.token_in.to_string()
        };
        let tok_out = if self.app.token_out > 10000 {
            format!(
                "{}.{}k",
                self.app.token_out / 1000,
                (self.app.token_out % 1000) / 100
            )
        } else if self.app.token_out > 1000 {
            format!("{:.1}k", self.app.token_out as f64 / 1000.0)
        } else {
            self.app.token_out.to_string()
        };

        // Model name (real, truncated)
        let model_short = if self.app.model_label.len() > 28 {
            format!("{}…", &self.app.model_label[..28])
        } else {
            self.app.model_label.clone()
        };

        // Hint text
        let hint = match history_hint {
            Some(h) => h.to_string(),
            None => "Shift+Tab:切换模式 · /help:查看命令".to_string(),
        };

        let _ = write!(
            out,
            "\r\n{}\r\n  {} {} · {} in · {} out · {}\r\n  {} {}",
            sep,
            "⏵".color(t.think),
            mode_label.color(t.ai_header),
            tok_in.color(t.dim),
            tok_out.color(t.dim),
            model_short.color(t.dim),
            "⏵".color(t.think),
            hint.color(t.dim),
        );

        // ── Move cursor to prompt line, column 0, then right to cursor_pos ──
        out.push_str("\x1b[3A\r"); // up 3 from hint → prompt line, col 0
        let right = 2 + buf[..cursor_pos].width(); // "> " + text before cursor
        let _ = write!(out, "\x1b[{}C", right);

        // Atomic write + flush
        use std::io::Write as IoWrite;
        let _ = io::stdout().write_all(out.as_bytes());
        let _ = io::stdout().flush();
    }

    /// Process an input string: quit, command, quick action, or submit.
    fn process_input(&mut self, input: &str) {
        if input == "/quit" || input == "/exit" || input == "/q" {
            self.app.quit = true;
            return;
        }
        if input.starts_with('/') && self.handle_command(input) {
            return;
        }
        if self.handle_quick_action(input) {
            return;
        }
        self.submit_message(input);
    }

    /// Tab completion: commands and file paths.
    /// Returns true if completion was attempted.
    fn complete_input(&self, buf: &mut String, cursor_pos: &mut usize) -> bool {
        // Command completion
        if buf.starts_with('/') {
            let cmd_part = &buf[1..];
            let commands = [
                "help", "h", "?", "tools", "ts", "new", "sessions", "ls", "switch", "s", "model",
                "m", "theme", "t", "plan", "clear", "cls", "quit", "q", "exit", "history", "alias",
                "read", "cmd", "search", "memory", "mem", "deep", "status",
            ];
            let candidates: Vec<&&str> = commands
                .iter()
                .filter(|c| c.starts_with(cmd_part))
                .collect();

            if candidates.len() == 1 {
                *buf = format!("/{}", candidates[0]);
                *cursor_pos = buf.len();
                return true;
            } else if candidates.len() > 1 && !cmd_part.is_empty() {
                print!("\r\x1b[2K");
                println!(
                    "  {}",
                    candidates
                        .iter()
                        .map(|c| format!("/{}", c))
                        .collect::<Vec<_>>()
                        .join("  ")
                );
                return true;
            }
        }

        // Path completion for /read
        if buf.starts_with("/read ") {
            let path_part = &buf[6..];
            let candidates = complete_path(path_part);
            if candidates.len() == 1 && !candidates[0].is_empty() {
                *buf = format!("/read {}", candidates[0]);
                *cursor_pos = buf.len();
                return true;
            } else if candidates.len() > 1 && !path_part.is_empty() {
                print!("\r\x1b[2K");
                println!("  {}", candidates.join("  "));
                return true;
            }
        }

        false
    }

    /// Drain agent events and render streaming output.
    fn drain_and_render(&mut self) -> Result<(), &'static str> {
        let mut spinner_frame = 0;
        let mut tool_card_open = false;

        // Take the receiver to avoid borrow conflicts
        let rx = self.ui_rx.take();

        loop {
            // Wait for the next event with a timeout.
            // recv_timeout returns immediately when a message arrives (zero latency)
            // and blocks at most 50ms before waking to drive the spinner animation.
            if let Some(ref rx_ref) = rx {
                match rx_ref.recv_timeout(std::time::Duration::from_millis(SPINNER_POLL_MS)) {
                    Ok(msg) => {
                        // Process this event, then drain any remaining queued events
                        if !self.handle_agent_event(
                            msg,
                            &mut spinner_frame,
                            &mut tool_card_open,
                            &rx,
                        ) {
                            // Idle received — already put rx back and returned
                            return Ok(());
                        }
                        // Drain all immediately available events without blocking
                        while let Ok(msg) = rx_ref.try_recv() {
                            if !self.handle_agent_event(
                                msg,
                                &mut spinner_frame,
                                &mut tool_card_open,
                                &rx,
                            ) {
                                return Ok(());
                            }
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        // No events — update spinner animation
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        self.ui_rx = rx;
                        return Err("agent channel disconnected");
                    }
                }
            }

            // Show spinner animation while thinking
            if self.app.thinking {
                let tool_name = self
                    .app
                    .current_tool_status
                    .as_ref()
                    .map(|(n, _)| n.clone());
                output::thinking_spinner(spinner_frame, tool_name.as_deref());
                spinner_frame += 1;
            }
        }
    }

    /// Submit a user message to the agent.
    fn submit_message(&mut self, message: &str) {
        output::replace_readline_with_user_message(message);

        self.app
            .push_message(ChatMessage::user(message.to_string()));
        self.app.persist_sessions();
        self.app.scroll_bottom = true;

        // Save input history
        self.app.input_history.push(message.to_string());
        if self.app.input_history.len() > MAX_INPUT_HISTORY {
            self.app.input_history.remove(0);
        }
        self.app.input_history_index = self.app.input_history.len();

        // Send to agent
        if let Some(ref tx) = self.app.agent_tx {
            let _ = tx.send(TuiToAgent::SubmitMessage(message.to_string()));
        }

        self.app.set_thinking();
        self.gen_start = Some(std::time::Instant::now());
        self.stream_state = markdown::StreamState::new();
    }

    /// Handle a single agent event. Returns false when Idle is received
    /// (meaning the receiver has been put back and the caller should return).
    fn handle_agent_event(
        &mut self,
        msg: AgentToTui,
        spinner_frame: &mut usize,
        _tool_card_open: &mut bool,
        rx: &Option<crossbeam_channel::Receiver<AgentToTui>>,
    ) -> bool {
        // ── LayoutModeHint always fires first — update state immediately ──
        if let AgentToTui::LayoutModeHint(mode) = &msg {
            if self.app.layout_mode != *mode {
                self.app.layout_mode = *mode;
                output::layout_mode_banner(*mode);
            }
        }

        // ── Dispatch by event type, mode-aware where needed ──
        match msg {
            AgentToTui::LayoutModeHint(_) => {
                // Already handled above
            }
            AgentToTui::StreamChunk { text } => match self.app.layout_mode {
                LayoutMode::Chat => {
                    output::clear_thinking_summary();
                    markdown::render_stream_chunk(&text, &mut self.stream_state);
                }
                LayoutMode::Work => {
                    output::clear_thinking_summary();
                    markdown::render_stream_chunk(&text, &mut self.stream_state);
                }
                LayoutMode::Summary => {
                    // Summary mode doesn't render stream chunks
                }
            },
            AgentToTui::Thinking => {
                match self.app.layout_mode {
                    LayoutMode::Chat => {
                        output::thinking_spinner(*spinner_frame, None);
                    }
                    LayoutMode::Work | LayoutMode::Summary => {
                        // Work mode: use thinking_summary instead of spinner
                    }
                }
            }
            AgentToTui::Message(m) => {
                output::clear_spinner();
                output::clear_thinking_summary();

                match self.app.layout_mode {
                    LayoutMode::Chat => {
                        // Chat mode: clean bubble-style rendering
                        if m.role == rupoo::MessageRole::User {
                            // Already printed by submit_message
                        } else if m.role == rupoo::MessageRole::Assistant {
                            // Content already streamed via StreamChunk → markdown rendering.
                            // Just flush stream state and store to history — don't re-render.
                            markdown::flush_stream(&mut self.stream_state);
                            self.stream_state = markdown::StreamState::new();
                        } else if m.role == rupoo::MessageRole::System {
                            // In Chat mode, suppress tool call noise (🔧/✅/执行工具)
                            // Only show non-tool system messages
                            if !m.content.starts_with("🔧")
                                && !m.content.starts_with("✅")
                                && !m.content.starts_with("⠋")
                            {
                                output::chat_bubble(&m.content, m.role);
                            }
                        } else if m.content.contains("Error") {
                            output::error(&m.content);
                        }
                    }
                    LayoutMode::Work => {
                        // Work mode: show user messages and final results only
                        if m.role == rupoo::MessageRole::User {
                            // Already printed by submit_message
                        } else if m.role == rupoo::MessageRole::Assistant {
                            markdown::flush_stream(&mut self.stream_state);
                            self.stream_state = markdown::StreamState::new();
                            // Print assistant output as-is (it's the work result)
                            for line in m.content.lines() {
                                println!("  {}", line);
                            }
                        } else if m.role == rupoo::MessageRole::System {
                            // Work mode: suppress tool call noise
                            if !m.content.starts_with("🔧")
                                && !m.content.starts_with("✅")
                                && !m.content.starts_with("⠋")
                            {
                                output::system(&m.content);
                            }
                        } else if m.content.contains("Error") {
                            output::error(&m.content);
                        }
                    }
                    LayoutMode::Summary => {
                        // Summary mode: just pass through to message history
                    }
                }
                self.app.push_message(m);
                self.app.persist_sessions();
            }
            AgentToTui::Idle => {
                output::clear_spinner();
                output::clear_thinking_summary();
                markdown::flush_stream(&mut self.stream_state);
                self.stream_state = markdown::StreamState::new();

                if let Some(start) = self.gen_start.take() {
                    let duration = start.elapsed().as_secs_f64();

                    match self.app.layout_mode {
                        LayoutMode::Chat => {
                            // Compact timing line — no thick separator (footer_bar shown by main loop)
                            let t = theme::current();
                            println!(
                                "{} {:.1}s │ {} in │ {} out",
                                "⏱".color(t.dim),
                                duration,
                                self.app.token_in.to_string().color(t.dim),
                                self.app.token_out.to_string().color(t.dim),
                            );
                        }
                        LayoutMode::Work => {
                            // Work mode: compact footer
                            println!(
                                "{} {:.1}s · {} in · {} out",
                                "⏱".to_string(),
                                duration,
                                self.app.token_in,
                                self.app.token_out,
                            );
                        }
                        LayoutMode::Summary => {
                            // Summary is rendered separately
                        }
                    }
                }

                self.app.set_idle();
                self.ui_rx = rx.clone();
                return false;
            }
            AgentToTui::TokenUpdate {
                in_count,
                out_count,
            } => {
                self.app.token_in = self.app.token_in.saturating_add(in_count);
                self.app.token_out = self.app.token_out.saturating_add(out_count);
            }
            AgentToTui::ToolStatus { tool_name, phase } => {
                let phase_str = match phase {
                    ToolPhase::Calling => "calling",
                    ToolPhase::Completed => "completed",
                };
                self.app.current_tool_status = Some((tool_name.clone(), phase_str.to_string()));
            }
            AgentToTui::RequestApproval(t) => {
                output::clear_spinner();
                self.handle_approval(t);
            }
            AgentToTui::LlmStatus {
                configured,
                provider,
                model_label,
            } => {
                self.app.llm_configured = configured;
                self.app.llm_provider = provider.clone();
                self.app.model_label = model_label;
            }
            AgentToTui::StepProgress {
                step_index,
                total,
                step_name,
            } => {
                output::clear_spinner();
                println!(
                    "  {} {}/{}: {}",
                    "▸".yellow().bold(),
                    step_index + 1,
                    total,
                    step_name.dimmed()
                );
            }
            AgentToTui::PlanTaskList { tasks } => {
                output::clear_spinner();
                output::plan_task_list(&tasks);
            }
            AgentToTui::HybridSearchUpdate { enabled } => {
                self.app.hybrid_search = enabled;
            }
            // ═══ 方案 C 新增事件处理 ═══
            AgentToTui::ThinkingSummary { text } => {
                output::thinking_summary(&text);
            }
            AgentToTui::PhaseProgress {
                phase_name,
                percentage,
            } => {
                output::clear_spinner();
                output::phase_progress(&phase_name, percentage);
            }
            AgentToTui::FileChanges { ref files } => {
                output::clear_spinner();
                for f in files {
                    output::file_change(f);
                }
            }
        }
        true
    }

    /// Handle slash commands.
    ///
    /// # Arguments
    /// * `input` - Command string starting with '/'
    ///
    /// # Supported Commands
    /// * `/help` - Show help message
    /// * `/tools (/ts)` - List available tools
    /// * `/new` - Create new session
    /// * `/sessions` - List all sessions
    /// * `/switch <n>` - Switch to session #n
    /// * `/model` - Show current model
    /// * `/theme (/t) <name>` - Switch UI theme
    /// * `/plan <msg>` - Enter plan mode
    /// * `/clear` - Clear terminal screen
    /// * `/quit` - Exit application
    ///
    /// # Returns
    /// `true` if command was handled, `false` otherwise
    fn handle_command(&mut self, input: &str) -> bool {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("");

        match cmd {
            "/help" | "/h" | "/?" => {
                println!();
                println!("  {}", "Commands:".cyan().bold());
                println!("  {} /help        — show this help", "›".dimmed());
                println!("  {} /tools (/ts) — list available tools", "›".dimmed());
                println!("  {} /new         — new session", "›".dimmed());
                println!("  {} /sessions    — list sessions", "›".dimmed());
                println!("  {} /switch <n>  — switch to session #n", "›".dimmed());
                println!("  {} /model       — show current model", "›".dimmed());
                println!(
                    "  {} /theme (/t)  — switch theme (dark/light/monokai)",
                    "›".dimmed()
                );
                println!("  {} /plan <msg>  — plan mode", "›".dimmed());
                println!("  {} /history     — show command history", "›".dimmed());
                println!("  {} /alias       — show command aliases", "›".dimmed());
                println!("  {} /clear       — clear screen", "›".dimmed());
                println!("  {} /quit        — exit rupoo", "›".dimmed());
                println!();
                println!("  {}", "Quick Actions:".cyan().bold());
                println!(
                    "  {} /read <path>   — read file (e.g., /read ./src/main.rs)",
                    "›".dimmed()
                );
                println!(
                    "  {} /cmd <cmd>     — execute shell command (e.g., /cmd ls -la)",
                    "›".dimmed()
                );
                println!(
                    "  {} /search <query> — web search (e.g., /search Rust async)",
                    "›".dimmed()
                );
                println!("  {} /ls [path]    — list directory", "›".dimmed());
                println!();
                println!("  {}", "Tips:".cyan().bold());
                println!("  {} Press Tab for autocomplete", "›".dimmed());
                println!("  {} Ctrl+R to search history", "›".dimmed());
                println!();
                true
            }
            "/tools" | "/ts" => {
                self.show_available_tools();
                true
            }
            "/new" => {
                self.create_new_session();
                true
            }
            "/sessions" | "/ls" => {
                self.list_sessions();
                true
            }
            "/switch" | "/s" => {
                if let Ok(idx) = arg.parse::<usize>() {
                    self.switch_to_session(idx);
                } else {
                    println!("  {} Usage: /switch <number>", "✗".red());
                }
                true
            }
            "/model" | "/m" => {
                if arg.is_empty() {
                    println!(
                        "  {} {}",
                        "Model:".cyan(),
                        self.app.model_label.cyan().bold()
                    );
                    true
                } else {
                    // /model <provider> [model] — forward to bridge for hot switch
                    // Bridge handles the actual switch_llm call and sends back status
                    if let Some(ref tx) = self.app.agent_tx {
                        let _ = tx.send(TuiToAgent::SubmitMessage(format!("/model {}", arg)));
                        self.app.set_thinking();
                        self.gen_start = Some(std::time::Instant::now());
                        self.stream_state = markdown::StreamState::new();
                    } else {
                        println!("  {} Agent not available", "✗".red());
                    }
                    true
                }
            }
            "/theme" | "/t" => {
                if arg.is_empty() {
                    // Show current theme and available options
                    let current = theme::current_name();
                    let names: Vec<String> = theme::Theme::all_names()
                        .iter()
                        .map(|n| {
                            if *n == current {
                                format!("{} (active)", n)
                            } else {
                                n.to_string()
                            }
                        })
                        .collect();
                    println!("  {} Themes: {}", "▸".cyan(), names.join(", "));
                } else if let Some(t) = theme::Theme::from_name(arg) {
                    theme::set(t);
                    output::set_cursor_style_bar();
                    println!("  {} Switched to {} theme", "✓".green(), arg);
                    // Persist theme preference
                    if let Some(ref repo) = self.app.repo {
                        if let Some(ref handle) = self.app.rt_handle {
                            let repo = std::sync::Arc::clone(repo);
                            let theme_name = arg.to_string();
                            let _handle = handle.spawn(async move {
                                let _ = repo.set_setting("theme", &theme_name).await;
                            });
                        }
                    }
                } else {
                    let names = theme::Theme::all_names().join("/");
                    println!(
                        "  {} Unknown theme '{}'. Available: {}",
                        "✗".red(),
                        arg,
                        names
                    );
                }
                true
            }
            "/clear" | "/cls" => {
                print!("{}", CLEAR_SCREEN_ESCAPE); // Clear screen + cursor home
                let _ = io::stdout().flush();
                true
            }
            "/quit" | "/q" | "/exit" => {
                self.app.quit = true;
                true
            }
            "/plan" => {
                if !arg.is_empty() {
                    output::user_message(arg);
                    if let Some(ref tx) = self.app.agent_tx {
                        let _ = tx.send(TuiToAgent::SubmitMessage(format!("/plan {}", arg)));
                    }
                    self.app.set_thinking();
                    self.gen_start = Some(std::time::Instant::now());
                    self.stream_state = markdown::StreamState::new();
                } else {
                    println!("  {} Usage: /plan <your goal>", "✗".red());
                }
                true
            }
            "/memory" | "/mem" => {
                self.handle_memory_command(arg);
                true
            }
            "/history" => {
                self.show_history(arg);
                true
            }
            "/alias" => {
                self.handle_alias(arg);
                true
            }
            _ => false, // Not a recognized command, treat as regular input
        }
    }

    /// Show command history
    fn show_history(&self, arg: &str) {
        if arg.is_empty() {
            let count = self.app.input_history.len().min(10);
            let start = self.app.input_history.len().saturating_sub(count);
            println!();
            println!("  {} Recent History:", "📜".cyan().bold());
            for (i, entry) in self.app.input_history[start..].iter().enumerate() {
                println!("  {} [{}] {}", "▸".dimmed(), start + i + 1, entry);
            }
            println!();
        } else {
            let query = arg.to_lowercase();
            let results: Vec<(usize, &String)> = self
                .app
                .input_history
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.to_lowercase().contains(&query))
                .collect();

            if results.is_empty() {
                println!("  {} No history found matching '{}'", "✗".red(), arg);
            } else {
                println!();
                println!("  {} Search Results:", "🔍".cyan().bold());
                for (idx, entry) in results {
                    println!("  {} [{}] {}", "▸".dimmed(), idx + 1, entry);
                }
                println!();
            }
        }
    }

    /// Handle alias command
    fn handle_alias(&self, arg: &str) {
        if arg.is_empty() {
            // Show available aliases
            println!();
            println!("  {} Command Aliases:", "⚡".cyan().bold());
            println!("  {} /h      → /help", "▸".dimmed());
            println!("  {} /ts     → /tools", "▸".dimmed());
            println!("  {} /ls     → /sessions", "▸".dimmed());
            println!("  {} /s      → /switch", "▸".dimmed());
            println!("  {} /m      → /model", "▸".dimmed());
            println!("  {} /t      → /theme", "▸".dimmed());
            println!("  {} /q      → /quit", "▸".dimmed());
            println!("  {} /cls    → /clear", "▸".dimmed());
            println!("  {} /mem    → /memory", "▸".dimmed());
            println!();
        } else {
            println!("  {} Usage: /alias (no arguments)", "✗".red());
        }
    }

    /// Handle memory command
    fn handle_memory_command(&mut self, arg: &str) {
        let cmd = if arg.is_empty() {
            "/memory".to_string()
        } else {
            format!("/memory {}", arg)
        };

        if let Some(ref tx) = self.app.agent_tx {
            let _ = tx.send(TuiToAgent::SubmitMessage(cmd));
            self.app.set_thinking();
            self.gen_start = Some(std::time::Instant::now());
            self.stream_state = markdown::StreamState::new();
        } else {
            println!("  {} Agent not available", "✗".red());
        }
    }

    /// Handle tool approval request.
    fn handle_approval(&mut self, pending: PendingTool) {
        println!();
        println!("  {} Approval Required", "⚠".yellow().bold());
        println!(
            "  {} Tool: {}",
            "│".dimmed(),
            pending.tool_name.cyan().bold()
        );
        let display_args = if pending.args.len() > MAX_APPROVAL_ARGS_DISPLAY {
            format!("{}…", &pending.args[..(MAX_APPROVAL_ARGS_DISPLAY - 3)])
        } else {
            pending.args.clone()
        };
        println!("  {} Args:  {}", "│".dimmed(), display_args);
        println!();

        // Auto-approve if approve_all is set
        if self.app.approve_all {
            if let Some(ref tx) = self.app.agent_tx {
                let _ = tx.send(TuiToAgent::ApproveTool("approved".to_string()));
            }
            println!("  {} Auto-approved", "✓".green());
            return;
        }

        // Ask user using crossterm raw input
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};
        use crossterm::terminal;
        let _ = terminal::enable_raw_mode();
        print!("  Approve? [y/n/a(ll)] ");
        let _ = io::stdout().flush();
        let answer = loop {
            if let Event::Key(key) = event::read().ok().unwrap() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => break "y",
                        KeyCode::Char('n') | KeyCode::Char('N') => break "n",
                        KeyCode::Char('a') | KeyCode::Char('A') => break "a",
                        _ => {}
                    }
                }
            }
        };
        let _ = terminal::disable_raw_mode();
        println!("{}", answer);
        match answer {
            "y" => {
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::ApproveTool("approved".to_string()));
                }
            }
            "n" => {
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::DenyTool);
                }
            }
            "a" => {
                self.app.approve_all = true;
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::ApproveAll);
                }
                println!("  {} Auto-approve enabled for this session", "✓".green());
            }
            _ => {}
        }
    }

    /// Create a new session.
    fn create_new_session(&mut self) {
        let id = uuid::Uuid::new_v4().to_string();
        let tab = app::SessionTab {
            id: id.clone(),
            label: format!("Chat {}", self.app.sessions.len() + 1),
            active: true,
            has_context: false,
        };

        // Deactivate all
        for s in &mut self.app.sessions {
            s.active = false;
        }

        // Save current messages
        let old_id = self.app.current_session_id();
        self.app
            .session_messages
            .insert(old_id, self.app.messages.clone());

        // Switch to new
        self.app.sessions.push(tab);
        self.app.messages = Vec::new();
        self.app.conversation_history = ConversationHistory::new(HISTORY_DEFAULT_MAX_TURNS)
            .with_token_budget(DEFAULT_TOKEN_BUDGET);
        self.app.intent_state = rupoo::signal::IntentState::new();
        self.app.persist_sessions();

        println!("  {} New session started", "✓".green());
    }

    /// List all sessions.
    fn list_sessions(&self) {
        println!();
        println!("  {}", "Sessions:".cyan().bold());
        for (i, s) in self.app.sessions.iter().enumerate() {
            let marker = if s.active { "▸" } else { " " };
            let color = if s.active {
                "●".green().to_string()
            } else {
                "○".dimmed().to_string()
            };
            println!("  {} {} [{}] {}", marker, color, i + 1, s.label);
        }
        println!();
    }

    /// Show available tools with descriptions.
    fn show_available_tools(&self) {
        println!();
        println!("  {}", "Available Tools:".cyan().bold());
        println!("  ──────────────────────────────────────────────────");
        println!("  {} {}", "📄".bold(), "file_read".cyan());
        println!("      Read file contents");
        println!("      Example: /read ./Cargo.toml");
        println!();
        println!("  {} {}", "📁".bold(), "list_dir".cyan());
        println!("      List directory contents");
        println!("      Example: /ls ./src");
        println!();
        println!("  {} {}", "🔧".bold(), "shell_exec".cyan());
        println!("      Execute shell command");
        println!("      Example: /cmd ls -la");
        println!();
        println!("  {} {}", "🔍".bold(), "web_search".cyan());
        println!("      Search the web");
        println!("      Example: /search Rust async programming");
        println!();
        println!("  {} {}", "✏️".bold(), "file_write".cyan());
        println!("      Write content to file");
        println!("      Example: Write to ./output.txt");
        println!();
        println!("  {}", "Quick Actions:".cyan().bold());
        println!("    /read <path>   - Read file directly");
        println!("    /cmd <cmd>     - Execute shell command");
        println!("    /search <query> - Web search");
        println!("    /ls [path]    - List directory");
        println!();
    }

    /// Handle quick action shortcuts (/read, /cmd, /search, /ls).
    ///
    /// # Quick Actions
    /// * `/read <path>` - Read file at path
    /// * `/cmd <cmd>` - Execute shell command
    /// * `/search <query>` - Web search for query
    /// * `/ls [path]` - List directory (default: current directory)
    ///
    /// # Arguments
    /// * `input` - User input string to check for quick action
    ///
    /// # Returns
    /// `true` if quick action was matched and executed, `false` otherwise
    fn handle_quick_action(&mut self, input: &str) -> bool {
        let trimmed = input.trim();

        // /read <path> - Read file
        if let Some(path) = trimmed.strip_prefix("/read ") {
            let path = path.trim();
            if !path.is_empty() {
                output::user_message(input);
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::SubmitMessage(format!("Read file at: {}", path)));
                }
                self.app.set_thinking();
                self.gen_start = Some(std::time::Instant::now());
                self.stream_state = markdown::StreamState::new();
                return true;
            }
        }

        // /cmd <cmd> - Execute shell command
        if let Some(cmd) = trimmed.strip_prefix("/cmd ") {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                output::user_message(input);
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::SubmitMessage(format!(
                        "Execute command: {}",
                        cmd
                    )));
                }
                self.app.set_thinking();
                self.gen_start = Some(std::time::Instant::now());
                self.stream_state = markdown::StreamState::new();
                return true;
            }
        }

        // /search <query> - Web search
        if let Some(query) = trimmed.strip_prefix("/search ") {
            let query = query.trim();
            if !query.is_empty() {
                output::user_message(input);
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::SubmitMessage(format!(
                        "Search the web for: {}",
                        query
                    )));
                }
                self.app.set_thinking();
                self.gen_start = Some(std::time::Instant::now());
                self.stream_state = markdown::StreamState::new();
                return true;
            }
        }

        // /ls [path] - List directory
        if let Some(path) = trimmed.strip_prefix("/ls") {
            let path = path.trim();
            let dir_path = if path.is_empty() { "." } else { path };
            output::user_message(input);
            if let Some(ref tx) = self.app.agent_tx {
                let _ = tx.send(TuiToAgent::SubmitMessage(format!(
                    "List directory: {}",
                    dir_path
                )));
            }
            self.app.set_thinking();
            self.gen_start = Some(std::time::Instant::now());
            self.stream_state = markdown::StreamState::new();
            return true;
        }

        false
    }

    /// Switch to a session by index (1-based).
    fn switch_to_session(&mut self, idx: usize) {
        if idx == 0 || idx > self.app.sessions.len() {
            println!("  {} Invalid session number", "✗".red());
            return;
        }

        let new_id = self.app.sessions[idx - 1].id.clone();
        let old_id = self.app.current_session_id();

        if new_id == old_id {
            println!("  {} Already on this session", "│".dimmed());
            return;
        }

        // Save current messages
        self.app
            .session_messages
            .insert(old_id, self.app.messages.clone());

        // Switch
        for s in &mut self.app.sessions {
            s.active = s.id == new_id;
        }
        self.app.messages = self
            .app
            .session_messages
            .get(&new_id)
            .cloned()
            .unwrap_or_default();

        // Load conversation history
        if let (Some(repo), Some(handle)) = (self.app.repo.as_ref(), self.app.rt_handle.as_ref()) {
            let repo = std::sync::Arc::clone(repo);
            let new_id_clone = new_id.clone();
            if let Ok(Some(ch)) =
                handle.block_on(async { repo.load_conversation_history(&new_id_clone).await })
            {
                self.app.conversation_history = ch;
                if self.app.conversation_history.token_budget() == 0 {
                    self.app.conversation_history = self
                        .app
                        .conversation_history
                        .clone()
                        .with_token_budget(DEFAULT_TOKEN_BUDGET);
                }
            }
        }

        self.app.persist_sessions();
        println!("  {} Switched to session {}", "✓".green(), idx);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Parse "🔧 tool_name(args)" into (tool_name, args).
#[allow(dead_code)]
fn parse_tool_call(content: &str) -> (String, String) {
    let rest = content.strip_prefix("🔧 ").unwrap_or(content);
    if let Some(paren_pos) = rest.find('(') {
        let name = rest[..paren_pos].to_string();
        let args = rest[paren_pos..]
            .trim_end_matches(')')
            .trim_start_matches('(')
            .to_string();
        (name, args)
    } else {
        (rest.to_string(), String::new())
    }
}

/// Simple path completion for /read and /cmd commands.
/// Returns matching file/directory paths.
fn complete_path(prefix: &str) -> Vec<String> {
    let dir = if prefix.is_empty() {
        std::path::PathBuf::from(".")
    } else {
        let p = std::path::Path::new(prefix);
        if prefix.ends_with('/') {
            p.to_path_buf()
        } else if let Some(parent) = p.parent() {
            if parent.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                parent.to_path_buf()
            }
        } else {
            std::path::PathBuf::from(".")
        }
    };

    let file_prefix = if prefix.is_empty() {
        String::new()
    } else {
        std::path::Path::new(prefix)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };

    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if !name.starts_with(&file_prefix) {
                continue;
            }
            let full = if prefix.is_empty() {
                name.clone()
            } else if prefix.ends_with('/') {
                format!("{}{}", prefix, name)
            } else {
                let parent = dir.join(&name);
                parent.to_string_lossy().to_string()
            };
            // Append / for directories
            let display = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                format!("{}/", full)
            } else {
                full
            };
            candidates.push(display);
        }
    }
    candidates.sort();
    candidates
}

// ═══════════════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════════════

pub fn run_tui_with_agent(
    repo: std::sync::Arc<TaskRepo>,
    agent: Agent,
    tool_executor: std::sync::Arc<dyn rupoo::agent::ToolExecutor>,
    rt_handle: tokio::runtime::Handle,
) -> Result<(), &'static str> {
    let (
        sessions_data,
        model_label,
        llm_configured,
        llm_provider,
        conversation_history,
        approve_all,
    ) = rt_handle.block_on(async {
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

        let llm_configured = !provider.is_empty()
            && repo
                .get_setting(&format!("api_key.{}", provider))
                .await
                .ok()
                .flatten()
                .is_some();

        let active_session_id = sessions
            .iter()
            .find(|s| s.3)
            .map(|s| s.0.clone())
            .unwrap_or_else(|| "default".to_string());
        let mut conversation_history = repo
            .load_conversation_history(&active_session_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                ConversationHistory::new(HISTORY_DEFAULT_MAX_TURNS)
                    .with_token_budget(DEFAULT_TOKEN_BUDGET)
            });
        if conversation_history.token_budget() == 0 {
            conversation_history = conversation_history.with_token_budget(DEFAULT_TOKEN_BUDGET);
        }

        let approve_all = repo
            .get_setting("approve_all")
            .await
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);

        (
            sessions,
            label,
            llm_configured,
            provider,
            conversation_history,
            approve_all,
        )
    });

    // Create channel pair
    let (tx, ui_rx) = crossbeam_channel::bounded::<AgentToTui>(1024);
    let (tx_to_agent, rx) = crossbeam_channel::bounded::<TuiToAgent>(256);
    let agent_tx = Some(tx_to_agent);

    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_flag_bridge = std::sync::Arc::clone(&cancel_flag);

    let repo_clone = std::sync::Arc::clone(&repo);
    let handle_for_agent = rt_handle.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_for_agent.block_on(async move {
                let agent_task = crate::cli::bridge::AgentUiBridge {
                    agent,
                    repo: repo_clone,
                    rx,
                    ui_tx: tx,
                    pending_plan: std::sync::Mutex::new(None),
                    pending_step_index: std::sync::Mutex::new(None),
                    tool_executor: std::sync::Arc::clone(&tool_executor),
                    approve_all,
                    conversation_history,
                    session_id: "default".to_string(),
                    intent_state: rupoo::signal::IntentState::new(),
                    cancelled: cancel_flag_bridge,
                };
                agent_task.run().await;
            });
        }));
        if result.is_err() {
            eprintln!("[rupoo] agent thread panicked");
        }
    });

    let mut session = ReplSession::new(
        agent_tx,
        Some(ui_rx),
        Some(repo),
        sessions_data,
        model_label,
        rt_handle,
    )?;

    session.app.llm_configured = llm_configured;
    session.app.llm_provider = llm_provider.clone();
    session.app.cancel_flag = cancel_flag;
    session.app.approve_all = approve_all;

    session.run()
}
