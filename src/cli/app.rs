//! TUI application state and event handling for Rupoo.

use tui_textarea::TextArea;

// ---------------------------------------------------------------------------
// Chat message types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum MessageRole {
    User,
    Assistant,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "You"),
            MessageRole::Assistant => write!(f, "Rupoo"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Main App state
// ---------------------------------------------------------------------------

pub struct App {
    pub messages: Vec<ChatMessage>,
    pub input: TextArea<'static>,
    pub status: String,
    pub loading: bool,
    pub show_help: bool,
}

impl App {
    pub fn new() -> Self {
        let mut input = TextArea::default();
        input.set_placeholder_text("Type a message and press Enter to send...");
        input.set_cursor_line_style(ratatui::style::Style::default());
        input.set_max_histories(100);

        Self {
            messages: Vec::new(),
            input,
            status: "Ready".into(),
            loading: false,
            show_help: false,
        }
    }

    pub fn add_user_message(&mut self, text: String) {
        self.messages.push(ChatMessage {
            role: MessageRole::User,
            content: text,
        });
    }

    pub fn add_assistant_message(&mut self, text: String) {
        self.messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: text,
        });
    }
}
