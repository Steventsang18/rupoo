//! REPL application state — stripped down for native terminal output.

use crossbeam_channel::Sender;
use rupoo::db::TaskRepo;
use rupoo::llm::ConversationHistory;
use rupoo::task::Plan;
use rupoo::{AgentToTui, ChatMessage, LayoutMode, PendingTool, TuiToAgent};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OverlayState {
    None,
    Approval {
        tool_name: String,
        args: String,
        approved: bool,
    },
}

// ---------------------------------------------------------------------------
// Session tab
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SessionTab {
    pub id: String,
    pub label: String,
    pub active: bool,
    #[allow(dead_code)]
    pub has_context: bool,
}

impl SessionTab {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            active: false,
            has_context: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Command definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CommandDef {
    #[allow(dead_code)]
    pub name: &'static str,
    #[allow(dead_code)]
    pub description: &'static str,
    #[allow(dead_code)]
    pub category: &'static str,
}

impl CommandDef {
    pub fn new(name: &'static str, description: &'static str, category: &'static str) -> Self {
        Self {
            name,
            description,
            category,
        }
    }
}

// ---------------------------------------------------------------------------
// Main app state
// ---------------------------------------------------------------------------

/// Which rendering backend drives the REPL surface.
///
/// `Ratatui` is the default "humanistic companion" surface — a single
/// downward-growing chat stream rendered with ratatui's retained-mode buffer.
/// `Terminal` is the long-stable line-by-line printer, kept fully intact as a
/// fallback (select via `RUPOO_TUI=0/false/off/no`). The two paths are isolated
/// so the legacy printer can never regress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    Terminal,
    #[default]
    Ratatui,
}

#[allow(dead_code)]
pub struct RupooApp {
    pub overlay: OverlayState,
    pub sessions: Vec<SessionTab>,
    pub messages: Vec<ChatMessage>,
    pub token_in: u64,
    pub token_out: u64,
    pub ctx_tokens: usize,
    pub ctx_budget: usize,
    pub hybrid_search: bool,
    pub pending_tool: Option<PendingTool>,
    pub cmd_query: String,
    pub cmd_selected: usize,
    pub available_commands: Vec<CommandDef>,
    pub status: String,
    pub agent_tx: Option<Sender<TuiToAgent>>,
    pub quit: bool,
    pub repo: Option<Arc<TaskRepo>>,
    pub plan: Option<Plan>,
    pub thinking: bool,
    pub input_history: Vec<String>,
    pub input_history_index: usize,
    pub model_label: String,
    pub session_messages: std::collections::HashMap<String, Vec<ChatMessage>>,
    /// Raw `messages_json` for each session, kept so inactive sessions are
    /// parsed lazily (on first switch) instead of all at startup.
    pub session_raw: std::collections::HashMap<String, String>,
    /// Background session-persistence channel. A single worker thread drains
    /// `PersistMsg`s and writes to the DB, so we never spawn a thread per
    /// `persist_sessions` call (which previously happened on every submit /
    /// switch). Initialized lazily via `init_persister`.
    persister: Option<crossbeam_channel::Sender<PersistMsg>>,
    pub rt_handle: Option<tokio::runtime::Handle>,
    pub scroll_bottom: bool,
    pub conversation_history: ConversationHistory,
    pub intent_state: rupoo::signal::IntentState,
    pub llm_configured: bool,
    pub llm_provider: String,
    pub current_step_info: Option<(usize, usize, String)>,
    pub chat_safe_mode: bool,
    pub stream_buffer: String,
    pub current_tool_status: Option<(String, String)>,
    pub cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub approve_all: bool,
    /// Current CLI layout mode — controls rendering style.
    pub layout_mode: LayoutMode,
    /// Rendering backend. Defaults to the `Ratatui` companion surface; the
    /// legacy `Terminal` printer remains available as a fallback (see `RenderMode`).
    pub render_mode: RenderMode,
}

/// One queued session-persistence write, handed to the background worker.
struct PersistMsg {
    id: String,
    label: String,
    json: String,
}

#[allow(dead_code)]
impl RupooApp {
    pub fn set_repo(mut self, repo: Arc<TaskRepo>) -> Self {
        self.repo = Some(repo);
        self
    }

    /// Spawn the background persistence worker. Idempotent — calling it more
    /// than once is a no-op. Called once after the repo is attached.
    pub fn init_persister(&mut self) {
        if self.persister.is_some() {
            return;
        }
        let (Some(repo), Some(handle)) = (self.repo.clone(), self.rt_handle.clone()) else {
            return;
        };
        let (tx, rx) = crossbeam_channel::unbounded::<PersistMsg>();
        std::thread::spawn(move || {
            // FIFO channel guarantees snapshots are written in send order,
            // so the latest state always wins — stricter than the old
            // spawn-per-call approach where writes could race.
            while let Ok(msg) = rx.recv() {
                let _ = handle.block_on(repo.save_ui_session(&msg.id, &msg.label, &msg.json, true));
            }
        });
        self.persister = Some(tx);
    }

    pub fn persist_sessions(&self) {
        let active_id = self
            .sessions
            .iter()
            .find(|s| s.active)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "default".to_string());
        let active_label = self
            .sessions
            .iter()
            .find(|s| s.active)
            .map(|s| s.label.clone())
            .unwrap_or_else(|| "default".to_string());
        let json = serde_json::to_string(&self.messages).unwrap_or_else(|_| "[]".to_string());

        // Fast path: hand the snapshot to the background writer.
        if let Some(tx) = &self.persister {
            let _ = tx.send(PersistMsg {
                id: active_id,
                label: active_label,
                json,
            });
            return;
        }

        // Fallback (e.g. tests without a persister): original spawn-per-call.
        let (Some(repo), Some(handle)) = (self.repo.clone(), self.rt_handle.clone()) else {
            return;
        };
        std::thread::spawn(move || {
            let _ = handle.block_on(repo.save_ui_session(&active_id, &active_label, &json, true));
        });
    }

    pub fn new(agent_tx: Option<Sender<TuiToAgent>>, rt_handle: tokio::runtime::Handle) -> Self {
        let available_commands = vec![
            CommandDef::new("help", "Show help", "general"),
            CommandDef::new("new", "New session", "session"),
            CommandDef::new("sessions", "List sessions", "session"),
            CommandDef::new("switch", "Switch session", "session"),
            CommandDef::new("model", "Show model", "general"),
            CommandDef::new("clear", "Clear screen", "general"),
            CommandDef::new("quit", "Quit rupoo", "general"),
            CommandDef::new("plan", "Plan mode", "mode"),
        ];

        Self {
            overlay: OverlayState::None,
            sessions: vec![SessionTab::new("default", "New Chat")],
            messages: Vec::new(),
            token_in: 0,
            token_out: 0,
            ctx_tokens: 0,
            ctx_budget: 60000,
            hybrid_search: false,
            pending_tool: None,
            cmd_query: String::new(),
            cmd_selected: 0,
            available_commands,
            status: "Ready".to_string(),
            agent_tx,
            quit: false,
            repo: None,
            plan: None,
            thinking: false,
            input_history: Vec::new(),
            input_history_index: 0,
            model_label: "not configured".to_string(),
            session_messages: std::collections::HashMap::new(),
            session_raw: std::collections::HashMap::new(),
            persister: None,
            rt_handle: Some(rt_handle),
            scroll_bottom: true,
            conversation_history: ConversationHistory::new(crate::cli::HISTORY_DEFAULT_MAX_TURNS)
                .with_token_budget(crate::cli::DEFAULT_TOKEN_BUDGET),
            intent_state: rupoo::signal::IntentState::new(),
            llm_configured: false,
            llm_provider: String::new(),
            current_step_info: None,
            chat_safe_mode: true,
            stream_buffer: String::new(),
            current_tool_status: None,
            cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            approve_all: false,
            layout_mode: LayoutMode::Chat,
            render_mode: RenderMode::Terminal,
        }
    }

    pub fn current_session_id(&self) -> String {
        self.sessions
            .iter()
            .find(|s| s.active)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    /// Return the parsed messages for a session, parsing lazily from the
    /// stored raw JSON on first access (and caching the result).
    pub fn get_session_messages(&mut self, id: &str) -> Vec<ChatMessage> {
        if let Some(m) = self.session_messages.get(id) {
            return m.clone();
        }
        let parsed = self
            .session_raw
            .get(id)
            .and_then(|raw| serde_json::from_str::<Vec<ChatMessage>>(raw).ok())
            .unwrap_or_default();
        self.session_messages.insert(id.to_string(), parsed.clone());
        parsed
    }

    pub fn switch_session(&mut self, session_id: &str) {
        let old_id = self.current_session_id();
        if old_id == session_id {
            return;
        }

        // Save current messages
        self.session_messages.insert(old_id, self.messages.clone());

        // Switch active
        for s in &mut self.sessions {
            s.active = s.id == session_id;
        }
        self.messages = self.get_session_messages(session_id);
        self.persist_sessions();
    }

    pub fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
    }

    pub fn set_quit(&mut self) {
        self.quit = true;
    }

    pub fn set_thinking(&mut self) {
        self.thinking = true;
        self.cancel_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
        if !self.stream_buffer.is_empty() {
            self.stream_buffer.clear();
        }
    }

    pub fn set_idle(&mut self) {
        self.thinking = false;
        self.current_tool_status = None;
        if !self.stream_buffer.is_empty() {
            let partial = std::mem::take(&mut self.stream_buffer);
            self.push_message(ChatMessage::assistant(partial));
            self.persist_sessions();
        }
        self.cancel_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn cancel_thinking(&mut self) {
        self.cancel_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if !self.stream_buffer.is_empty() {
            let partial = std::mem::take(&mut self.stream_buffer);
            if !partial.trim().is_empty() {
                self.push_message(ChatMessage::system(format!(
                    "⚠ Interrupted (partial output):\n{}",
                    partial
                )));
            }
        }
    }

    pub fn show_tool_approval(&mut self, tool: PendingTool) {
        self.pending_tool = Some(tool.clone());
        self.overlay = OverlayState::Approval {
            tool_name: tool.tool_name,
            args: tool.args,
            approved: false,
        };
    }

    pub fn apply_agent_event(&mut self, msg: AgentToTui) {
        match msg {
            AgentToTui::Message(m) => {
                self.push_message(m);
                self.persist_sessions();
                self.scroll_bottom = true;
            }
            AgentToTui::Thinking => self.set_thinking(),
            AgentToTui::Idle => self.set_idle(),
            AgentToTui::ContextUsage { .. } => {
                // Context gauge is a ratatui-only feature; ignored on the legacy path.
            }
            AgentToTui::TokenUpdate {
                in_count,
                out_count,
            } => {
                self.token_in = self.token_in.saturating_add(in_count);
                self.token_out = self.token_out.saturating_add(out_count);
            }
            AgentToTui::RequestApproval(t) => self.show_tool_approval(t),
            AgentToTui::StreamChunk { text } => {
                self.stream_buffer.push_str(&text);
                self.scroll_bottom = true;
            }
            AgentToTui::LlmStatus {
                configured,
                provider,
                model_label,
            } => {
                self.llm_configured = configured;
                self.llm_provider = provider.clone();
                self.model_label = model_label.clone();
                self.status = if configured {
                    format!("Connected: {}", model_label)
                } else {
                    "LLM not configured".to_string()
                };
            }
            AgentToTui::StepProgress {
                step_index,
                total,
                step_name,
            } => {
                self.current_step_info = Some((step_index, total, step_name.clone()));
                self.status = format!("Step {}/{}: {}", step_index + 1, total, step_name);
            }
            AgentToTui::ToolStatus { tool_name, phase } => {
                let phase_str = match phase {
                    rupoo::ToolPhase::Calling => "calling",
                    rupoo::ToolPhase::Completed => "completed",
                };
                self.current_tool_status = Some((tool_name.clone(), phase_str.to_string()));
            }
            AgentToTui::PlanTaskList { .. } => {
                // Handled by the CLI output layer
            }
            AgentToTui::HybridSearchUpdate { enabled } => {
                self.hybrid_search = enabled;
            }
            // 方案 C 新增事件 — 渲染层处理，不需要 app 状态变更
            AgentToTui::ThinkingSummary { .. }
            | AgentToTui::PhaseProgress { .. }
            | AgentToTui::LayoutModeHint(..)
            | AgentToTui::FileChanges { .. } => {}
        }
    }

    pub fn close_overlay(&mut self) {
        self.overlay = OverlayState::None;
    }

    #[allow(dead_code)]
    pub fn filtered_commands(&self) -> Vec<CommandDef> {
        let q = self.cmd_query.to_lowercase();
        if q.is_empty() {
            return self.available_commands.clone();
        }
        self.available_commands
            .iter()
            .filter(|c| c.name.contains(&q) || c.description.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }
}
