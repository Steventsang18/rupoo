//! TUI application state — Phase 2 agent integration.

use serde::{Deserialize, Serialize};
use tui_textarea::TextArea;
use std::sync::Arc;
use crossbeam_channel::Sender;
use rupoo::{AgentToTui, ApprovalChoice, ChatMessage, PendingTool, TuiToAgent};
use rupoo::db::TaskRepo;
use rupoo::llm::ConversationHistory;
use rupoo::task::Plan;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Chat,
    Input,
    Sessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputMode {
    Chat,
    CommandPalette,
    Approval,
    Thinking,
    Rename,
    Disabled,
}

impl Default for InputMode {
    fn default() -> Self { InputMode::Chat }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverlayState {
    None,
    CommandPalette { query: String, selected: usize },
    ContextMenu { session_id: String, selected: usize },
    Approval { tool_name: String, args: String, approved: Option<bool> },
}

impl Default for OverlayState {
    fn default() -> Self { OverlayState::None }
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTab {
    pub id: String,
    pub label: String,
    pub active: bool,
    pub has_context: bool,
}

impl SessionTab {
    pub fn new(id: &str, label: &str) -> Self {
        Self { id: id.to_string(), label: label.to_string(), active: false, has_context: true }
    }
}

/// A slash command registered in the app.
pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    handler: Arc<dyn Fn(&mut RupooApp) + Send + Sync>,
}

impl std::fmt::Debug for CommandDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandDef")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("category", &self.category)
            .finish()
    }
}

impl Clone for CommandDef {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            description: self.description,
            category: self.category,
            handler: Arc::clone(&self.handler),
        }
    }
}

impl CommandDef {
    pub fn with_handler<H>(name: &'static str, description: &'static str, category: &'static str, handler: H) -> Self
    where H: Fn(&mut RupooApp) + 'static + Send + Sync {
        Self { name, description, category, handler: Arc::new(handler) }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct RupooApp {
    pub input_mode: InputMode,
    pub overlay: OverlayState,
    pub sessions: Vec<SessionTab>,
    /// Per-session message history (key = session_id).
    /// The active session's messages are also mirrored in `messages` for
    /// rendering convenience.
    pub messages: Vec<ChatMessage>,
    pub input: TextArea<'static>,
    pub token_in: u64,
    pub token_out: u64,
    pub pending_tool: Option<PendingTool>,
    pub approval_choice: Option<ApprovalChoice>,
    pub cmd_query: String,
    pub cmd_selected: usize,
    pub available_commands: Vec<CommandDef>,
    pub status: String,
    pub loading: bool,
    pub agent_tx: Option<Sender<TuiToAgent>>,
    pub quit: bool,
    pub repo: Option<Arc<TaskRepo>>,
    pub plan: Option<Plan>,
    pub current_step: usize,
    pub thinking: bool,
    /// Focus target for Tab switching
    pub focus: FocusTarget,
    /// Spinner animation frame (incremented each draw)
    pub spinner_frame: usize,
    /// Input history for ↑/↓ navigation
    pub input_history: Vec<String>,
    pub input_history_index: usize,
    /// Chat rendering cache — invalidated when change_counter increments
    pub chat_cache_lines: Vec<String>,
    pub change_counter: u64,
    /// Current model label (read from DB, set during TUI init)
    pub model_label: String,
    /// Per-session message storage — key is session_id.
    /// Used to isolate session histories when switching between tabs.
    pub session_messages: std::collections::HashMap<String, Vec<ChatMessage>>,
    /// Shared tokio runtime handle for async persistence (no new threads).
    pub rt_handle: Option<tokio::runtime::Handle>,
    /// When true, viewport always jumps to bottom on next render.
    pub scroll_bottom: bool,
    /// Manual scroll position (Paragraph::scroll value). Only used when scroll_bottom=false.
    pub scroll_offset: usize,
    /// Last max_scroll value computed during render, used by scroll handlers.
    pub max_scroll_cache: std::cell::Cell<usize>,
    /// Conversation history for Chat Mode (multi-turn)
    pub conversation_history: ConversationHistory,
    /// Whether LLM is configured
    pub llm_configured: bool,
    /// Current LLM provider name
    pub llm_provider: String,
    /// First run flag (for onboarding hints)
    pub is_first_run: bool,
    /// Current step info for Plan Mode progress display
    pub current_step_info: Option<(usize, usize, String)>,
    /// Safe mode for Chat Mode (true = only safe tools)
    pub chat_safe_mode: bool,
    /// Streaming text buffer for incremental display
    pub stream_buffer: String,
}

impl RupooApp {
    /// Attach a TaskRepo for session persistence.
    pub fn set_repo(mut self, repo: Arc<TaskRepo>) -> Self {
        self.repo = Some(repo);
        self
    }

    /// Persist current session state to SQLite.
    pub fn persist_sessions(&self) {
        if let (Some(ref repo), Some(ref handle)) = (self.repo.as_ref(), self.rt_handle.as_ref()) {
            let repo = Arc::clone(repo);
            let messages_json = serde_json::to_string(&self.messages).unwrap_or_else(|_| "[]".to_string());
            let active_id = self.sessions.iter().find(|s| s.active).map(|s| s.id.clone()).unwrap_or_else(|| "default".to_string());
            let active_label = self.sessions.iter().find(|s| s.active).map(|s| s.label.clone()).unwrap_or_else(|| "default".to_string());
            // Spawn on the shared tokio runtime — no new thread, no new runtime.
            handle.spawn(async move {
                let _ = repo.save_ui_session(&active_id, &active_label, &messages_json, true).await;
            });
        }
    }

    /// Create the application with default session and commands.
    pub fn new(agent_tx: Option<Sender<TuiToAgent>>, rt_handle: tokio::runtime::Handle) -> Self {
        let mut sessions = Vec::new();
        sessions.push(SessionTab::new("default", "New Chat"));
        sessions[0].active = true;

        let available_commands = vec![
            CommandDef::with_handler("help", "Show available commands", "General", |app| {
                let cmds: Vec<String> = app.available_commands.iter().map(|c| format!("  /{} — {}", c.name, c.description)).collect();
                app.messages.push(ChatMessage::command_output(cmds.join("\n")));
            }),
            CommandDef::with_handler("clear", "Clear chat history", "General", |app| {
                app.clear_messages();
            }),
            CommandDef::with_handler("sessions", "List sessions", "Session", |app| {
                for (i, s) in app.sessions.iter().enumerate() {
                    let marker = if s.active { "*" } else { " " };
                    let label = &s.label;
                    app.messages.push(ChatMessage::command_output(format!("{marker} [{i}] {label}")));
                }
            }),
            CommandDef::with_handler("model", "Show/set model (also /model <provider> [model])", "Config", |app| {
                app.messages.push(ChatMessage::system("Model: use /model <provider> or rupoo config set".to_string()));
            }),
            CommandDef::with_handler("exit", "Exit Rupoo", "General", |app| {
                app.messages.push(ChatMessage::assistant("Goodbye!".to_string()));
                app.set_quit();
            }),
            CommandDef::with_handler("plan", "Switch to Plan Mode (auto-generate plan from task)", "Mode", |app| {
                app.messages.push(ChatMessage::system("Plan Mode: Type your task and it will be automatically broken into steps.".to_string()));
            }),
            CommandDef::with_handler("trust", "Enable trust mode (allows file writes in Chat Mode)", "Config", |app| {
                app.chat_safe_mode = false;
                app.messages.push(ChatMessage::assistant("Trust mode enabled: file writes are now allowed in Chat Mode.".to_string()));
            }),
            CommandDef::with_handler("clear-history", "Clear conversation history for current session", "Chat", |app| {
                app.conversation_history.clear();
                app.messages.push(ChatMessage::system("Conversation history cleared.".to_string()));
            }),
            CommandDef::with_handler("status", "Show current session status", "General", |app| {
                let llm_status = if app.llm_configured { "configured" } else { "not configured" };
                let status = format!(
                    "LLM: {}\nApprove all: {}\nHistory: {} turns",
                    llm_status,
                    "false",
                    app.conversation_history.len(),
                );
                app.messages.push(ChatMessage::system(status));
            }),
        ];

        let input = TextArea::default();

        Self {
            input_mode: InputMode::Chat,
            overlay: OverlayState::None,
            sessions,
            messages: Vec::new(),
            input,
            token_in: 0,
            token_out: 0,
            pending_tool: None,
            approval_choice: None,
            cmd_query: String::new(),
            cmd_selected: 0,
            available_commands,
            status: "Ready".to_string(),
            loading: false,
            agent_tx,
            quit: false,
            repo: None,
            plan: None,
            current_step: 0,
            thinking: false,
            focus: FocusTarget::Input,
            spinner_frame: 0,
            input_history: Vec::new(),
            input_history_index: 0,
            chat_cache_lines: Vec::new(),
            change_counter: 0,
            model_label: "not configured".to_string(),
            session_messages: std::collections::HashMap::new(),
            rt_handle: Some(rt_handle),
            scroll_bottom: true,
            scroll_offset: 0,
            max_scroll_cache: std::cell::Cell::new(0),
            conversation_history: ConversationHistory::new(10),
            llm_configured: false,
            llm_provider: String::new(),
            is_first_run: true,
            current_step_info: None,
            chat_safe_mode: true,
            stream_buffer: String::new(),
        }
    }

    /// Return the id of the currently active session.
    pub fn current_session_id(&self) -> String {
        self.sessions
            .iter()
            .find(|s| s.active)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    /// Switch to a different session by id — saves the old session's messages
    /// into `session_messages`, then loads the new session's messages into
    /// `app.messages` for rendering.
    pub fn switch_session(&mut self, session_id: &str) {
        // 1. Save current session's messages to HashMap
        let old_id = self.current_session_id();
        if old_id != session_id {
            self.session_messages.insert(old_id, self.messages.clone());
        }

        // 2. Mark new session active
        for s in self.sessions.iter_mut() {
            s.active = s.id == session_id;
        }

        // 3. Load new session's messages (or empty if first time)
        self.messages = self
            .session_messages
            .get(session_id)
            .cloned()
            .unwrap_or_default();

        self.scroll_bottom = true;
        self.change_counter = self.change_counter.wrapping_add(1);
        self.persist_sessions();
    }

    /// Called by apply_agent_event whenever a new message arrives from the agent.
    /// Messages are pushed into app.messages AND persisted into session_messages
    /// for the active session.
    fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg.clone());
        let sid = self.current_session_id();
        self.session_messages
            .entry(sid)
            .or_insert_with(Vec::new)
            .push(msg);
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    pub fn set_quit(&mut self) {
        self.quit = true;
    }

    /// Submit a message, routing to Plan Mode or Chat Mode based on prefix.
    pub fn submit_message(&mut self) {
        let text = self.input.lines().join("\n");
        self.input = TextArea::default();

        if text.trim().is_empty() {
            return;
        }

        self.scroll_bottom = true;
        self.change_counter = self.change_counter.wrapping_add(1);

        // Save to input history
        self.input_history.push(text.clone());
        if self.input_history.len() > 100 {
            self.input_history.remove(0);
        }
        self.input_history_index = self.input_history.len();

        // Always push user message first (clone for both storage paths)
        self.push_message(ChatMessage::user(text.clone()));
        self.persist_sessions();

        // Route to Plan Mode or Chat Mode based on prefix
        if let Some(ref tx) = self.agent_tx {
            if text.starts_with("/plan ") {
                // Plan Mode: send as-is for plan generation
                let _ = tx.send(TuiToAgent::SubmitMessage(text));
            } else {
                // Chat Mode: send as-is
                let _ = tx.send(TuiToAgent::SubmitMessage(text));
            }
        } else {
            self.push_message(ChatMessage::assistant(format!("[demo] You said: {}", text)));
            self.persist_sessions();
        }
    }

    pub fn set_thinking(&mut self) {
        self.thinking = true;
        self.input_mode = InputMode::Thinking;
        self.stream_buffer.clear();
    }

    pub fn set_idle(&mut self) {
        self.thinking = false;
        if self.input_mode == InputMode::Thinking {
            self.input_mode = InputMode::Chat;
        }
        // Flush any remaining stream buffer
        if !self.stream_buffer.is_empty() {
            self.push_message(ChatMessage::assistant(self.stream_buffer.clone()));
            self.stream_buffer.clear();
            self.persist_sessions();
        }
    }

    pub fn update_tokens(&mut self, in_count: u64, out_count: u64) {
        self.token_in = in_count;
        self.token_out = out_count;
    }

    pub fn show_tool_approval(&mut self, tool: PendingTool) {
        self.pending_tool = Some(tool.clone());
        self.input_mode = InputMode::Approval;
        self.overlay = OverlayState::Approval {
            tool_name: tool.tool_name,
            args: tool.args,
            approved: None,
        };
    }

    pub fn apply_agent_event(&mut self, msg: AgentToTui) {
        match msg {
            AgentToTui::Message(m) => {
                self.push_message(m);
                self.persist_sessions();
                self.scroll_bottom = true;
                self.change_counter = self.change_counter.wrapping_add(1);
            }
            AgentToTui::Thinking => self.set_thinking(),
            AgentToTui::Idle => self.set_idle(),
            AgentToTui::TokenUpdate { in_count, out_count } => {
                // Accumulate into existing token counts so right panel
                // always shows the running total, not just the last snapshot.
                self.token_in = self.token_in.saturating_add(in_count);
                self.token_out = self.token_out.saturating_add(out_count);
            }
            AgentToTui::RequestApproval(t) => self.show_tool_approval(t),
            AgentToTui::StreamChunk { text } => {
                // Append to streaming buffer for incremental display
                self.stream_buffer.push_str(&text);
                self.scroll_bottom = true;
                self.change_counter = self.change_counter.wrapping_add(1);
            }
            AgentToTui::LlmStatus { configured, provider } => {
                self.llm_configured = configured;
                self.llm_provider = provider.clone();
                self.status = if configured {
                    format!("Connected: {}", provider)
                } else {
                    "LLM not configured".to_string()
                };
            }
            AgentToTui::StepProgress { step_index, total, step_name } => {
                self.current_step_info = Some((step_index, total, step_name.clone()));
                self.status = format!("Step {}/{}: {}", step_index + 1, total, step_name);
            }
        }
    }

    pub fn close_overlay(&mut self) {
        self.overlay = OverlayState::None;
    }

    pub fn execute_selected_command(&mut self) {
        let cmds = self.filtered_commands();
        if self.cmd_selected < cmds.len() {
            let cmd = &cmds[self.cmd_selected];
            (cmd.handler)(self);
        }
        self.overlay = OverlayState::None;
    }

    pub fn filtered_commands(&self) -> Vec<CommandDef> {
        if self.cmd_query.is_empty() {
            self.available_commands.clone()
        } else {
            let q = self.cmd_query.to_lowercase();
            self.available_commands.iter().filter(|c| {
                c.name.to_lowercase().contains(&q) || c.description.to_lowercase().contains(&q)
            }).cloned().collect()
        }
    }

    /// Get the current stream buffer content for rendering.
    pub fn get_stream_buffer(&self) -> &str {
        &self.stream_buffer
    }
}
