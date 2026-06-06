//! Keyboard shortcuts for Rupoo CLI
//! 
//! Provides keyboard shortcuts for common operations.

use rustyline::{KeyEvent, Modifiers};
use std::collections::HashMap;

/// Shortcut action type
pub type ShortcutAction = fn(&mut ShortcutContext) -> bool;

/// Context for shortcut execution
pub struct ShortcutContext {
    pub should_quit: bool,
    pub should_clear: bool,
    pub should_new_session: bool,
    pub should_save_session: bool,
    pub should_search: bool,
    pub should_switch_session: Option<usize>,
    pub search_query: String,
}

impl Default for ShortcutContext {
    fn default() -> Self {
        Self {
            should_quit: false,
            should_clear: false,
            should_new_session: false,
            should_save_session: false,
            should_search: false,
            should_switch_session: None,
            search_query: String::new(),
        }
    }
}

/// Registered keyboard shortcuts
pub struct ShortcutRegistry {
    shortcuts: HashMap<KeyEvent, ShortcutAction>,
}

impl ShortcutRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            shortcuts: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// Register default shortcuts
    fn register_defaults(&mut self) {
        // Ctrl+N - New session
        self.register(KeyEvent::ctrl('n'), |ctx| {
            ctx.should_new_session = true;
            true
        });

        // Ctrl+S - Save session
        self.register(KeyEvent::ctrl('s'), |ctx| {
            ctx.should_save_session = true;
            true
        });

        // Ctrl+Q - Quit
        self.register(KeyEvent::ctrl('q'), |ctx| {
            ctx.should_quit = true;
            true
        });

        // Ctrl+L - Clear screen (this is usually handled by terminal)
        self.register(KeyEvent::ctrl('l'), |ctx| {
            ctx.should_clear = true;
            true
        });

        // Ctrl+F - Search mode
        self.register(KeyEvent::ctrl('f'), |ctx| {
            ctx.should_search = true;
            true
        });

        // Alt+1 through Alt+9 - Switch to session 1-9
        for i in 1..=9 {
            let session_idx = i;
            self.register(KeyEvent {
                code: rustyline::KeyCode::Char(b'0' + i as u8),
                modifiers: Modifiers::ALT,
            }, move |ctx| {
                ctx.should_switch_session = Some(session_idx);
                true
            });
        }

        // Alt+0 - Switch to session 10
        self.register(KeyEvent {
            code: rustyline::KeyCode::Char(b'0'),
            modifiers: Modifiers::ALT,
        }, |ctx| {
            ctx.should_switch_session = Some(10);
            true
        });

        // Ctrl+P - Plan mode
        self.register(KeyEvent::ctrl('p'), |ctx| {
            ctx.search_query = "/plan ".to_string();
            ctx.should_search = true;
            true
        });
    }

    /// Register a custom shortcut
    pub fn register(&mut self, key: KeyEvent, action: ShortcutAction) {
        self.shortcuts.insert(key, action);
    }

    /// Try to handle a key event
    pub fn handle(&self, key: KeyEvent, ctx: &mut ShortcutContext) -> bool {
        if let Some(action) = self.shortcuts.get(&key) {
            action(ctx)
        } else {
            false
        }
    }

    /// Get list of registered shortcuts for help display
    pub fn list_shortcuts(&self) -> Vec<(String, &'static str)> {
        vec![
            ("Ctrl+N", "新建会话"),
            ("Ctrl+S", "保存会话"),
            ("Ctrl+Q", "退出程序"),
            ("Ctrl+L", "清屏"),
            ("Ctrl+F", "搜索模式"),
            ("Ctrl+P", "计划模式"),
            ("Alt+1-9", "切换到第 n 个会话"),
            ("Alt+0", "切换到第 10 个会话"),
            ("Ctrl+C", "中断当前任务"),
            ("Ctrl+R", "搜索历史"),
            ("Tab", "自动补全"),
            ("↑/↓", "浏览历史"),
            ("Esc", "取消输入"),
        ]
    }
}

/// Helper function to check if a key event matches a shortcut
pub fn matches_shortcut(event: &KeyEvent, ctrl: bool, shift: bool, alt: bool, code: rustyline::KeyCode) -> bool {
    event.modifiers.contains(Modifiers::CTRL) == ctrl &&
    event.modifiers.contains(Modifiers::SHIFT) == shift &&
    event.modifiers.contains(Modifiers::ALT) == alt &&
    event.code == code
}

/// Parse key event to human-readable string
pub fn key_to_string(key: KeyEvent) -> String {
    let mut parts = Vec::new();
    
    if key.modifiers.contains(Modifiers::CTRL) {
        parts.push("Ctrl".to_string());
    }
    if key.modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    if key.modifiers.contains(Modifiers::ALT) {
        parts.push("Alt".to_string());
    }
    
    let key_str = match key.code {
        rustyline::KeyCode::Char(c) => {
            if c.is_ascii_lowercase() {
                c.to_ascii_uppercase().to_string()
            } else {
                c.to_string()
            }
        }
        rustyline::KeyCode::Up => "Up".to_string(),
        rustyline::KeyCode::Down => "Down".to_string(),
        rustyline::KeyCode::Left => "Left".to_string(),
        rustyline::KeyCode::Right => "Right".to_string(),
        rustyline::KeyCode::Home => "Home".to_string(),
        rustyline::KeyCode::End => "End".to_string(),
        rustyline::KeyCode::PageUp => "PageUp".to_string(),
        rustyline::KeyCode::PageDown => "PageDown".to_string(),
        rustyline::KeyCode::Delete => "Delete".to_string(),
        rustyline::KeyCode::Backspace => "Backspace".to_string(),
        rustyline::KeyCode::Enter => "Enter".to_string(),
        rustyline::KeyCode::Tab => "Tab".to_string(),
        rustyline::KeyCode::Esc => "Esc".to_string(),
        rustyline::KeyCode::F(n) => format!("F{}", n),
        _ => "Unknown".to_string(),
    };
    
    parts.push(key_str);
    
    parts.join("+")
}