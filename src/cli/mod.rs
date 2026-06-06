//! REPL-based CLI — native terminal output, rustyline input.
//!
//! No TUI framework. Terminal handles scrolling and resize.
//! We just render content to stdout and let the terminal do the rest.

pub mod app;
pub mod cmds;
pub mod completion;

pub mod output;
pub mod markdown;
pub mod theme;
pub mod enhanced_ui;

mod bridge;
mod chat_mode;
mod plan_mode;
mod approval;

pub use rupoo::{AgentToTui, ChatMessage, PendingTool, ToolPhase, TuiToAgent};
pub use app::RupooApp;

use std::io::{self, Write};

use owo_colors::OwoColorize;
use crossbeam_channel::{Receiver, Sender};
use tracing::warn;
use rupoo::db::TaskRepo;
use rupoo::agent::Agent;
use rupoo::llm::ConversationHistory;
use rustyline::Editor;
use rustyline::history::{FileHistory, History};

// ═══════════════════════════════════════════════════════════════════════════
// REPL Session
// ═══════════════════════════════════════════════════════════════════════════

pub struct ReplSession {
    app: RupooApp,
    ui_rx: Option<Receiver<AgentToTui>>,
    rl: Editor<completion::RupooHelper, FileHistory>,
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

        // Init rustyline with completion support
        let mut rl = completion::create_editor()
            .map_err(|_| "readline_init_failed")?;

        // Session labels are handled internally

        // Persist history to ~/.rupoo/history.txt — survives restarts
        let history_path = crate::tracing_setup::history_path();
        if let Some(parent) = history_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if history_path.exists() {
            if let Err(e) = rl.load_history(&history_path) {
                warn!("Failed to load history: {e}");
            }
        }

        Ok(Self {
            app,
            ui_rx,
            rl,
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

        // Save history on exit
        let history_path = crate::tracing_setup::history_path();
        let _ = self.rl.save_history(&history_path);

        // Reset cursor style on exit
        output::reset_cursor_style();

        result
    }

    /// Inner REPL loop.
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

            // Show footer status bar
            output::footer(
                self.app.token_in,
                self.app.token_out,
                self.app.ctx_tokens,
                self.app.ctx_budget,
                &self.app.model_label,
                self.app.hybrid_search,
            );
            
            // Read input
            let prompt = self.build_prompt();
            match self.rl.readline(&prompt) {
                Ok(line) => {
                    let input = line.trim().to_string();
                    if input.is_empty() {
                        continue;
                    }

                    // Add to rustyline history
                    let _ = self.rl.add_history_entry(&input);

                    // Handle slash commands
                    if input.starts_with('/') {
                        if self.handle_command(&input) {
                            continue;
                        }
                    }

                    // Handle quick action shortcuts
                    if self.handle_quick_action(&input) {
                        continue;
                    }

                    // Submit user message
                    self.submit_message(&input);
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    // Ctrl+C — ignored, use Ctrl+D to quit
                    println!("\n  {} Use Ctrl+D to quit", "›".dimmed());
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    // Ctrl+D — quit
                    println!("\n  Bye! 👋");
                    break Ok(());
                }
                Err(_) => {
                    break Err("terminal input error — please restart rupoo");
                }
            }
        }
    }

    /// Build the input prompt string.
    fn build_prompt(&self) -> String {
        format!("{} ", "❯".green().bold())
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
                match rx_ref.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(msg) => {
                        // Process this event, then drain any remaining queued events
                        if !self.handle_agent_event(msg, &mut spinner_frame, &mut tool_card_open, &rx) {
                            // Idle received — already put rx back and returned
                            return Ok(());
                        }
                        // Drain all immediately available events without blocking
                        while let Ok(msg) = rx_ref.try_recv() {
                            if !self.handle_agent_event(msg, &mut spinner_frame, &mut tool_card_open, &rx) {
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
                let tool_name = self.app.current_tool_status.as_ref().map(|(n, _)| n.clone());
                output::thinking_spinner(spinner_frame, tool_name.as_deref());
                spinner_frame += 1;
            }
        }
    }

    /// Submit a user message to the agent.
    fn submit_message(&mut self, message: &str) {
        output::replace_readline_with_user_message(message);

        self.app.push_message(ChatMessage::user(message.to_string()));
        self.app.persist_sessions();
        self.app.scroll_bottom = true;

        // Save input history
        self.app.input_history.push(message.to_string());
        if self.app.input_history.len() > 100 {
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
        tool_card_open: &mut bool,
        rx: &Option<crossbeam_channel::Receiver<AgentToTui>>,
    ) -> bool {
        match msg {
            AgentToTui::StreamChunk { text } => {
                output::clear_spinner();
                markdown::render_stream_chunk(&text, &mut self.stream_state);
            }
            AgentToTui::Thinking => {
                output::thinking_spinner(*spinner_frame, None);
            }
            AgentToTui::Message(m) => {
                output::clear_spinner();
                if m.role == rupoo::MessageRole::User {
                    // User messages are already printed by submit_message
                } else if m.role == rupoo::MessageRole::System {
                    if m.content.starts_with("🔧") {
                        output::clear_spinner();
                        let (tool_name, args) = parse_tool_call(&m.content);
                        output::tool_call_start(&tool_name, &args);
                        *tool_card_open = true;
                    } else if m.content.starts_with("✅") && *tool_card_open {
                        let result = m.content.strip_prefix("✅ ").unwrap_or(&m.content);
                        output::tool_result(result, result.lines().count() > 8);
                        output::tool_call_end(true, None);
                        *tool_card_open = false;
                    } else {
                        if !m.content.is_empty() {
                            output::system(&m.content);
                        }
                    }
                } else if m.role == rupoo::MessageRole::Assistant {
                    markdown::flush_stream(&mut self.stream_state);
                    self.stream_state = markdown::StreamState::new();
                } else if m.content.contains("Error") {
                    output::error(&m.content);
                }
                self.app.push_message(m);
                self.app.persist_sessions();
            }
            AgentToTui::Idle => {
                output::clear_spinner();
                markdown::flush_stream(&mut self.stream_state);
                self.stream_state = markdown::StreamState::new();

                if let Some(start) = self.gen_start.take() {
                    let duration = start.elapsed().as_secs_f64();
                    let ctx_tokens = self.app.conversation_history.estimated_tokens();
                    let ctx_budget = self.app.conversation_history.token_budget();
                    output::assistant_footer(
                        duration,
                        self.app.token_in,
                        self.app.token_out,
                        ctx_tokens,
                        ctx_budget,
                    );
                }

                self.app.set_idle();
                self.ui_rx = rx.clone();
                return false;
            }
            AgentToTui::TokenUpdate { in_count, out_count } => {
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
            AgentToTui::LlmStatus { configured, provider, model_label } => {
                self.app.llm_configured = configured;
                self.app.llm_provider = provider.clone();
                self.app.model_label = model_label;
            }
            AgentToTui::StepProgress { step_index, total, step_name } => {
                output::clear_spinner();
                println!("  {} {}/{}: {}", "▸".yellow().bold(), step_index + 1, total, step_name.dimmed());
            }
            AgentToTui::PlanTaskList { tasks } => {
                output::clear_spinner();
                output::plan_task_list(&tasks);
            }
            AgentToTui::HybridSearchUpdate { enabled } => {
                self.app.hybrid_search = enabled;
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
                println!("  {} /theme (/t)  — switch theme (dark/light/monokai)", "›".dimmed());
                println!("  {} /plan <msg>  — plan mode", "›".dimmed());
                println!("  {} /history     — show command history", "›".dimmed());
                println!("  {} /alias       — show command aliases", "›".dimmed());
                println!("  {} /clear       — clear screen", "›".dimmed());
                println!("  {} /quit        — exit rupoo", "›".dimmed());
                println!();
                println!("  {}", "Quick Actions:".cyan().bold());
                println!("  {} @<path>      — read file (e.g., @./src/main.rs)", "›".dimmed());
                println!("  {} !<cmd>       — execute shell command (e.g., !ls -la)", "›".dimmed());
                println!("  {} ~<query>     — web search (e.g., ~Rust async)", "›".dimmed());
                println!("  {} %% [path]    — list directory", "›".dimmed());
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
                    println!("  {} {}", "Model:".cyan(), self.app.model_label.cyan().bold());
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
                            let _ = handle.spawn(async move {
                                let _ = repo.set_setting("theme", &theme_name).await;
                            });
                        }
                    }
                } else {
                    let names = theme::Theme::all_names().join("/");
                    println!("  {} Unknown theme '{}'. Available: {}", "✗".red(), arg, names);
                }
                true
            }
            "/clear" | "/cls" => {
                print!("\x1b[2J\x1b[H"); // Clear screen + cursor home
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
        let history = self.rl.history();
        
        if arg.is_empty() {
            // Show recent history
            let count = history.len().min(10);
            println!();
            println!("  {} Recent History:", "📜".cyan().bold());
            for (i, entry) in history.iter().rev().take(count).enumerate() {
                let idx = history.len() - i;
                println!("  {} [{}] {}", "▸".dimmed(), idx, entry);
            }
            println!();
        } else {
            // Search history
            let query = arg.to_lowercase();
            let results: Vec<_> = history
                .iter()
                .enumerate()
                .filter(|(_, entry): &(usize, &String)| entry.to_lowercase().contains(&query))
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
        println!("  {} Tool: {}", "│".dimmed(), pending.tool_name.cyan().bold());
        let display_args = if pending.args.len() > 80 {
            format!("{}…", &pending.args[..77])
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

        // Ask user
        loop {
            match self.rl.readline("  Approve? [y/n/a(ll)] ") {
                Ok(line) => {
                    let answer = line.trim().to_lowercase();
                    match answer.as_str() {
                        "y" | "yes" => {
                            if let Some(ref tx) = self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::ApproveTool("approved".to_string()));
                            }
                            break;
                        }
                        "n" | "no" => {
                            if let Some(ref tx) = self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::DenyTool);
                            }
                            break;
                        }
                        "a" | "all" => {
                            self.app.approve_all = true;
                            if let Some(ref tx) = self.app.agent_tx {
                                let _ = tx.send(TuiToAgent::ApproveAll);
                            }
                            println!("  {} Auto-approve enabled for this session", "✓".green());
                            break;
                        }
                        _ => continue,
                    }
                }
                Err(_) => {
                    if let Some(ref tx) = self.app.agent_tx {
                        let _ = tx.send(TuiToAgent::DenyTool);
                    }
                    break;
                }
            }
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
        self.app.session_messages.insert(old_id, self.app.messages.clone());

        // Switch to new
        self.app.sessions.push(tab);
        self.app.messages = Vec::new();
        self.app.conversation_history = ConversationHistory::new(10).with_token_budget(60000);
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
            let color = if s.active { "●".green().to_string() } else { "○".dimmed().to_string() };
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
        println!("      Shortcut: @<path>");
        println!("      Example: @./Cargo.toml");
        println!();
        println!("  {} {}", "📁".bold(), "list_dir".cyan());
        println!("      List directory contents");
        println!("      Shortcut: %% [path]");
        println!("      Example: %% ./src");
        println!();
        println!("  {} {}", "🔧".bold(), "shell_exec".cyan());
        println!("      Execute shell command");
        println!("      Shortcut: !<command>");
        println!("      Example: !ls -la");
        println!();
        println!("  {} {}", "🔍".bold(), "web_search".cyan());
        println!("      Search the web");
        println!("      Shortcut: ~<query>");
        println!("      Example: ~Rust async programming");
        println!();
        println!("  {} {}", "✏️".bold(), "file_write".cyan());
        println!("      Write content to file");
        println!("      Example: Write to ./output.txt");
        println!();
        println!("  {}", "Quick Actions:".cyan().bold());
        println!("    @<path>      - Read file directly");
        println!("    !<cmd>       - Execute shell command");
        println!("    ~<query>     - Web search");
        println!("    %% [path]    - List directory");
        println!();
    }

    /// Handle quick action shortcuts (@path, !cmd, ~query, %%path).
    /// 
    /// # Quick Actions
    /// * `@<path>` - Read file at path
    /// * `!<cmd>` - Execute shell command
    /// * `~<query>` - Web search for query
    /// * `%% [path]` - List directory (default: current directory)
    /// 
    /// # Arguments
    /// * `input` - User input string to check for quick action
    /// 
    /// # Returns
    /// `true` if quick action was matched and executed, `false` otherwise
    fn handle_quick_action(&mut self, input: &str) -> bool {
        let trimmed = input.trim();
        
        // @path - Read file
        if trimmed.starts_with('@') && trimmed.len() > 1 {
            let path = trimmed[1..].trim();
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
        
        // !cmd - Execute shell command
        if trimmed.starts_with('!') && trimmed.len() > 1 {
            let cmd = trimmed[1..].trim();
            if !cmd.is_empty() {
                output::user_message(input);
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::SubmitMessage(format!("Execute command: {}", cmd)));
                }
                self.app.set_thinking();
                self.gen_start = Some(std::time::Instant::now());
                self.stream_state = markdown::StreamState::new();
                return true;
            }
        }
        
        // ~query - Web search
        if trimmed.starts_with('~') && trimmed.len() > 1 {
            let query = trimmed[1..].trim();
            if !query.is_empty() {
                output::user_message(input);
                if let Some(ref tx) = self.app.agent_tx {
                    let _ = tx.send(TuiToAgent::SubmitMessage(format!("Search the web for: {}", query)));
                }
                self.app.set_thinking();
                self.gen_start = Some(std::time::Instant::now());
                self.stream_state = markdown::StreamState::new();
                return true;
            }
        }
        
        // %%path - List directory
        if trimmed.starts_with("%%") {
            let path = trimmed[2..].trim();
            let dir_path = if path.is_empty() { "." } else { path };
            output::user_message(input);
            if let Some(ref tx) = self.app.agent_tx {
                let _ = tx.send(TuiToAgent::SubmitMessage(format!("List directory: {}", dir_path)));
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
        self.app.session_messages.insert(old_id, self.app.messages.clone());

        // Switch
        for s in &mut self.app.sessions {
            s.active = s.id == new_id;
        }
        self.app.messages = self.app.session_messages.get(&new_id).cloned().unwrap_or_default();

        // Load conversation history
        if let Some(ref repo) = self.app.repo {
            if let Some(ref handle) = self.app.rt_handle {
                let repo = std::sync::Arc::clone(repo);
                let new_id_clone = new_id.clone();
                if let Ok(ch) = handle.block_on(async {
                    repo.load_conversation_history(&new_id_clone).await
                }) {
                    if let Some(ch) = ch {
                        self.app.conversation_history = ch;
                        if self.app.conversation_history.token_budget() == 0 {
                            self.app.conversation_history = self.app.conversation_history.clone().with_token_budget(60000);
                        }
                    }
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
fn parse_tool_call(content: &str) -> (String, String) {
    let rest = content.strip_prefix("🔧 ").unwrap_or(content);
    if let Some(paren_pos) = rest.find('(') {
        let name = rest[..paren_pos].to_string();
        let args = rest[paren_pos..].trim_end_matches(')').trim_start_matches('(').to_string();
        (name, args)
    } else {
        (rest.to_string(), String::new())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════════════

pub fn run_tui_with_agent(
    repo: std::sync::Arc<TaskRepo>,
    agent: Agent,
    tool_executor: std::sync::Arc<Box<dyn rupoo::agent::ToolExecutor>>,
    rt_handle: tokio::runtime::Handle,
) -> Result<(), &'static str> {
    let (sessions_data, model_label, llm_configured, llm_provider, conversation_history, approve_all) = rt_handle.block_on(async {
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

        let llm_configured = !provider.is_empty() && repo
            .get_setting(&format!("api_key.{}", provider))
            .await
            .ok()
            .flatten()
            .is_some();

        let active_session_id = sessions.iter()
            .find(|s| s.3)
            .map(|s| s.0.clone())
            .unwrap_or_else(|| "default".to_string());
        let mut conversation_history = repo
            .load_conversation_history(&active_session_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| ConversationHistory::new(10).with_token_budget(60000));
        if conversation_history.token_budget() == 0 {
            conversation_history = conversation_history.with_token_budget(60000);
        }

        let approve_all = repo
            .get_setting("approve_all")
            .await
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);

        (sessions, label, llm_configured, provider, conversation_history, approve_all)
    });

    // Create channel pair
    let (tx, ui_rx) = crossbeam_channel::unbounded::<AgentToTui>();
    let (tx_to_agent, rx) = crossbeam_channel::unbounded::<TuiToAgent>();
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
