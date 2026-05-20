//! E2: InputHandler strategy pattern — one handler per InputMode.
//!
//! Each mode's key handling is isolated into its own struct implementing
//! `InputHandler`. The `Handler` enum provides zero-cost dispatch without
//! heap allocation or vtables.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::app::{InputMode, RupooApp};
use crate::cli::{ApprovalChoice, ChatMessage, TuiToAgent};

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Strategy for mode-specific key event handling.
pub trait InputHandler {
    /// Handle a key event for the current mode.
    /// Return `true` if the key was consumed, `false` to fall through.
    fn handle_key(&mut self, app: &mut RupooApp, key: &KeyEvent) -> bool;
}

// ---------------------------------------------------------------------------
// Concrete handlers — one per InputMode variant
// ---------------------------------------------------------------------------

/// Chat mode: text input + Ctrl shortcuts + scroll keys + Enter to send.
pub struct ChatHandler;
impl InputHandler for ChatHandler {
    fn handle_key(&mut self, app: &mut RupooApp, key: &KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.set_quit();
                true
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.input_mode = InputMode::CommandPalette;
                app.cmd_query.clear();
                app.cmd_selected = 0;
                true
            }
            KeyCode::Enter => {
                app.submit_message();
                true
            }
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if app.scroll_bottom {
                    app.scroll_bottom = false;
                    app.scroll_offset = app.max_scroll_cache.get();
                }
                app.scroll_offset = app.scroll_offset.saturating_sub(1);
                true
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                if !app.scroll_bottom {
                    app.scroll_offset = app.scroll_offset.saturating_add(1);
                }
                // when scroll_bottom, Shift+Down does nothing
                true
            }
            KeyCode::PageUp => {
                if app.scroll_bottom {
                    app.scroll_bottom = false;
                    app.scroll_offset = app.max_scroll_cache.get();
                }
                app.scroll_offset = app.scroll_offset.saturating_sub(10);
                true
            }
            KeyCode::PageDown => {
                if !app.scroll_bottom {
                    app.scroll_offset = app.scroll_offset.saturating_add(10);
                }
                // when scroll_bottom, PageDown does nothing
                true
            }
            _ => false,
        }
    }
}

/// Thinking mode: blocks all input except Esc/q to cancel.
pub struct ThinkingHandler;
impl InputHandler for ThinkingHandler {
    fn handle_key(&mut self, app: &mut RupooApp, key: &KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                app.set_idle();
                app.messages
                    .push(ChatMessage::assistant("Cancelled.".to_string()));
                true
            }
            _ => true, // eat all keys
        }
    }
}

/// Command palette mode: navigate + filter commands.
pub struct PaletteHandler;
impl InputHandler for PaletteHandler {
    fn handle_key(&mut self, app: &mut RupooApp, key: &KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_overlay();
                true
            }
            KeyCode::Enter => {
                app.execute_selected_command();
                true
            }
            KeyCode::Up => {
                let n = app.filtered_commands().len();
                if n > 0 {
                    app.cmd_selected = (app.cmd_selected + n - 1) % n;
                }
                true
            }
            KeyCode::Down => {
                let n = app.filtered_commands().len();
                if n > 0 {
                    app.cmd_selected = (app.cmd_selected + 1) % n;
                }
                true
            }
            KeyCode::Backspace => {
                app.cmd_query.pop();
                app.cmd_selected = 0;
                true
            }
            KeyCode::Char(c) => {
                app.cmd_query.push(c);
                app.cmd_selected = 0;
                true
            }
            _ => false,
        }
    }
}

/// Approval mode: select + confirm/deny tool execution.
pub struct ApprovalHandler;
impl InputHandler for ApprovalHandler {
    fn handle_key(&mut self, app: &mut RupooApp, key: &KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_overlay();
                true
            }
            KeyCode::Enter => {
                if let Some(ref tx) = app.agent_tx {
                    let choice = app
                        .approval_choice
                        .unwrap_or(ApprovalChoice::Deny);
                    match choice {
                        ApprovalChoice::Deny | ApprovalChoice::DenyBlock => {
                            let _ = tx.send(TuiToAgent::DenyTool);
                        }
                        ApprovalChoice::ApproveOnce => {
                            let _ = tx.send(TuiToAgent::ApproveTool(String::new()));
                        }
                        ApprovalChoice::ApproveAll => {
                            let _ = tx.send(TuiToAgent::ApproveAll);
                        }
                    }
                }
                true
            }
            KeyCode::Char('1') => {
                app.approval_choice = Some(ApprovalChoice::ApproveOnce);
                true
            }
            KeyCode::Char('2') => {
                app.approval_choice = Some(ApprovalChoice::ApproveAll);
                true
            }
            KeyCode::Char('3') => {
                app.approval_choice = Some(ApprovalChoice::Deny);
                true
            }
            KeyCode::Char('4') => {
                app.approval_choice = Some(ApprovalChoice::DenyBlock);
                true
            }
            KeyCode::Left => {
                let next = app.approval_choice.map(|c| match c {
                    ApprovalChoice::ApproveOnce => ApprovalChoice::DenyBlock,
                    ApprovalChoice::ApproveAll => ApprovalChoice::ApproveOnce,
                    ApprovalChoice::Deny => ApprovalChoice::ApproveAll,
                    ApprovalChoice::DenyBlock => ApprovalChoice::Deny,
                });
                app.approval_choice = next.or(Some(ApprovalChoice::Deny));
                true
            }
            KeyCode::Right => {
                let next = app.approval_choice.map(|c| match c {
                    ApprovalChoice::ApproveOnce => ApprovalChoice::ApproveAll,
                    ApprovalChoice::ApproveAll => ApprovalChoice::Deny,
                    ApprovalChoice::Deny => ApprovalChoice::DenyBlock,
                    ApprovalChoice::DenyBlock => ApprovalChoice::ApproveOnce,
                });
                app.approval_choice = next.or(Some(ApprovalChoice::ApproveOnce));
                true
            }
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch enum — zero-cost dispatch without heap allocation
// ---------------------------------------------------------------------------

/// Dispatch a key event based on the current input mode.
/// This is a pure dispatch — handlers are stateless, so no &mut self needed.
pub fn dispatch(app: &mut RupooApp, key: &KeyEvent) -> bool {
    if key.kind != KeyEventKind::Press {
        return false;
    }
    match app.input_mode {
        InputMode::Chat => ChatHandler.handle_key(app, key),
        InputMode::Thinking => ThinkingHandler.handle_key(app, key),
        InputMode::CommandPalette => PaletteHandler.handle_key(app, key),
        InputMode::Approval => ApprovalHandler.handle_key(app, key),
        InputMode::Rename | InputMode::Disabled => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn test_app() -> RupooApp {
        let rt = tokio::runtime::Runtime::new().unwrap();
        RupooApp::new(None, rt.handle().clone())
    }

    fn press(key: KeyCode) -> KeyEvent {
        KeyEvent::new(key, KeyModifiers::NONE)
    }

    #[test]
    fn test_dispatch_routes_to_chat_handler() {
        let mut app = test_app();
        app.input_mode = InputMode::Chat;
        let mut ctrl_c = press(KeyCode::Char('c'));
        ctrl_c.modifiers = KeyModifiers::CONTROL;
        assert!(dispatch(&mut app, &ctrl_c));
        assert!(app.quit);
    }

    #[test]
    fn test_chat_handler_enter_submits() {
        let mut app = test_app();
        app.input.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        app.input.input(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let enter = press(KeyCode::Enter);
        assert!(ChatHandler.handle_key(&mut app, &enter));
        assert!(app.input.lines().join("").is_empty());
        assert!(!app.messages.is_empty());
    }

    #[test]
    fn test_scroll_up_from_bottom() {
        let mut app = test_app();
        assert!(app.scroll_bottom);
        let shift_up = {
            let mut k = press(KeyCode::Up);
            k.modifiers = KeyModifiers::SHIFT;
            k
        };
        assert!(ChatHandler.handle_key(&mut app, &shift_up));
        assert!(!app.scroll_bottom);
        assert_eq!(app.scroll_offset, app.max_scroll_cache.get().saturating_sub(1));
    }

    #[test]
    fn test_scroll_down_from_bottom_does_nothing() {
        let mut app = test_app();
        assert!(app.scroll_bottom);
        let shift_down = {
            let mut k = press(KeyCode::Down);
            k.modifiers = KeyModifiers::SHIFT;
            k
        };
        assert!(ChatHandler.handle_key(&mut app, &shift_down));
        assert!(app.scroll_bottom);  // stays at bottom
        assert_eq!(app.scroll_offset, 0);  // untouched
    }

    #[test]
    fn test_page_down_from_bottom_does_nothing() {
        let mut app = test_app();
        assert!(app.scroll_bottom);
        assert!(ChatHandler.handle_key(&mut app, &press(KeyCode::PageDown)));
        assert!(app.scroll_bottom);
    }

    #[test]
    fn test_page_up_from_bottom_disables() {
        let mut app = test_app();
        assert!(app.scroll_bottom);
        assert!(ChatHandler.handle_key(&mut app, &press(KeyCode::PageUp)));
        assert!(!app.scroll_bottom);
        assert_eq!(app.scroll_offset, app.max_scroll_cache.get().saturating_sub(10));
    }

    #[test]
    fn test_chat_handler_ctrl_p_opens_palette() {
        let mut app = test_app();
        let mut ctrl_p = press(KeyCode::Char('p'));
        ctrl_p.modifiers = KeyModifiers::CONTROL;
        assert!(ChatHandler.handle_key(&mut app, &ctrl_p));
        assert_eq!(app.input_mode, InputMode::CommandPalette);
    }

    #[test]
    fn test_thinking_handler_blocks_all() {
        let mut app = test_app();
        app.input_mode = InputMode::Thinking;
        assert!(ThinkingHandler.handle_key(&mut app, &press(KeyCode::Char('x'))));
        assert!(ThinkingHandler.handle_key(&mut app, &press(KeyCode::Enter)));
        assert!(ThinkingHandler.handle_key(&mut app, &press(KeyCode::Tab)));
        assert!(ThinkingHandler.handle_key(&mut app, &press(KeyCode::Esc)));
        assert!(!app.thinking);
    }

    #[test]
    fn test_approval_handler_approve_once() {
        let mut app = test_app();
        app.input_mode = InputMode::Approval;
        assert!(ApprovalHandler.handle_key(&mut app, &press(KeyCode::Char('1'))));
        assert_eq!(app.approval_choice, Some(ApprovalChoice::ApproveOnce));
    }

    #[test]
    fn test_approval_handler_deny() {
        let mut app = test_app();
        app.input_mode = InputMode::Approval;
        assert!(ApprovalHandler.handle_key(&mut app, &press(KeyCode::Char('3'))));
        assert_eq!(app.approval_choice, Some(ApprovalChoice::Deny));
    }

    #[test]
    fn test_approval_handler_left_right_navigation() {
        let mut app = test_app();
        app.input_mode = InputMode::Approval;
        app.approval_choice = Some(ApprovalChoice::ApproveOnce);
        assert!(ApprovalHandler.handle_key(&mut app, &press(KeyCode::Right)));
        assert_eq!(app.approval_choice, Some(ApprovalChoice::ApproveAll));
        assert!(ApprovalHandler.handle_key(&mut app, &press(KeyCode::Right)));
        assert_eq!(app.approval_choice, Some(ApprovalChoice::Deny));
        assert!(ApprovalHandler.handle_key(&mut app, &press(KeyCode::Left)));
        assert_eq!(app.approval_choice, Some(ApprovalChoice::ApproveAll));
    }

    #[test]
    fn test_palette_handler_query_and_select() {
        let mut app = test_app();
        app.input_mode = InputMode::CommandPalette;
        assert!(PaletteHandler.handle_key(&mut app, &press(KeyCode::Char('c'))));
        assert_eq!(app.cmd_query, "c");
        assert!(PaletteHandler.handle_key(&mut app, &press(KeyCode::Char('l'))));
        assert_eq!(app.cmd_query, "cl");
        assert!(PaletteHandler.handle_key(&mut app, &press(KeyCode::Backspace)));
        assert_eq!(app.cmd_query, "c");
    }

    #[test]
    fn test_dispatch_filters_non_press_events() {
        let mut app = test_app();
        let release = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: crossterm::event::KeyEventState::NONE,
        };
        assert!(!dispatch(&mut app, &release));
    }

    #[test]
    fn test_chat_handler_non_consumed_keys() {
        let mut app = test_app();
        // Plain Up/Down without Shift are NOT consumed by ChatHandler
        assert!(!ChatHandler.handle_key(&mut app, &press(KeyCode::Up)));
        assert!(!ChatHandler.handle_key(&mut app, &press(KeyCode::Down)));
        // Tab is not consumed
        assert!(!ChatHandler.handle_key(&mut app, &press(KeyCode::Tab)));
        // Shift+Up/Down IS consumed
        let shift_up = {
            let mut k = press(KeyCode::Up);
            k.modifiers = KeyModifiers::SHIFT;
            k
        };
        assert!(ChatHandler.handle_key(&mut app, &shift_up));
    }
}
