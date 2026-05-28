//! Product‑grade TUI rendering — bubbles, code blocks, spinner, status bar.
//!
//! Layout contract (one frame, before any widget):
//!   Left 20 cols | Center min 1 | Right 22 cols
//! Overlay (approval) draws on top of everything.
//!
//! Every frame reads directly from app state — no cache.
//! Lines are pre-wrapped to the terminal width at build time so that
//! Paragraph::scroll() operates on display‑accurate line counts.
//! This avoids the ratatui Wrap + Scroll interaction bug where content
//! at the bottom of long wrapped lines gets clipped.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::{
    FocusTarget, InputMode, OverlayState,
    RupooApp,
};
use rupoo::MessageRole;

// ═══════════════════════════════════════════════════════════════════════════
// Public entry point
// ═══════════════════════════════════════════════════════════════════════════

pub fn render(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let cols = compute_three_column(area);
    render_left(frame, cols.left, app);
    render_center(frame, cols.center, app);
    render_right(frame, cols.right, app);

    // Overlays always on top.
    if matches!(app.overlay, OverlayState::Approval { .. }) {
        render_approval_dialog(frame, area, app);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Layout
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
struct ThreeColumnRects {
    left: Rect,
    center: Rect,
    right: Rect,
}

fn compute_three_column(area: Rect) -> ThreeColumnRects {
    let [left, center, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(1),
            Constraint::Length(22),
        ])
        .areas(area);
    ThreeColumnRects { left, center, right }
}

// ═══════════════════════════════════════════════════════════════════════════
// Left sidebar — session tabs
// ═══════════════════════════════════════════════════════════════════════════

fn render_left(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let block = Block::new()
        .title(" Sessions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let active_id = app.current_session_id();
    let mut lines: Vec<Line> = Vec::new();
    for (i, tab) in app.sessions.iter().enumerate() {
        let is_active = tab.id == active_id;
        let marker = if is_active { "▸" } else { " " };
        let fg = if is_active {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let label = if tab.label.is_empty() {
            format!("Session {}", i + 1)
        } else {
            tab.label.clone()
        };
        // Truncate label to fit in sidebar width
        let max_len = 14;
        let display = if label.len() > max_len {
            format!("{}…", &label[..max_len.saturating_sub(1)])
        } else {
            label
        };
        lines.push(Line::from(Span::styled(
            format!("{} {}", marker, display),
            Style::default().fg(fg),
        )));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), inner);
}

// ═══════════════════════════════════════════════════════════════════════════
// Center column — chat + input + status
// ═══════════════════════════════════════════════════════════════════════════

fn render_center(frame: &mut Frame, area: Rect, app: &RupooApp) {
    // Calculate dynamic input height: min 5, max 8, grows with content
    let input_lines = app.input.lines().len().max(1);
    let input_h = (input_lines as u16 + 2).clamp(5, 8); // +2 for borders

    let [title, chat, input, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .areas(area);
    render_center_title(frame, title, app);
    render_chat_area(frame, chat, app);
    render_input_area(frame, input, app);
    render_center_status(frame, status, app);
}

fn render_center_title(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let mode_label = match app.input_mode {
        InputMode::Chat => " Chat ",
        InputMode::CommandPalette => " Cmd [Ctrl+P] ",
        InputMode::Approval => " ⚠ Approval ",
        InputMode::Thinking => " ⏳ Thinking… ",
        InputMode::Rename => " Rename ",
        InputMode::Disabled => " [disabled] ",
    };
    let fg = match app.input_mode {
        InputMode::Approval | InputMode::Thinking => Color::Yellow,
        _ => Color::Cyan,
    };
    frame.render_widget(
        Paragraph::new(Span::styled(mode_label, Style::default().fg(fg).add_modifier(Modifier::BOLD))),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Chat area — bubble-style messages
// ═══════════════════════════════════════════════════════════════════════════

fn render_chat_area(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let inner = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
        .inner(area);
    let max_w = inner.width as usize;
    let view_h = inner.height as usize;

    let mut display_lines = build_chat_lines(app, max_w);

    // ── Thinking indicator ─────────────────────────────────────────────
    if app.thinking {
        let spinner_char = match app.spinner_frame % 4 {
            0 => "⠋",
            1 => "⠙",
            2 => "⠹",
            _ => "⠸",
        };
        let status_text = if let Some((ref tool, ref phase)) = app.current_tool_status {
            match phase.as_str() {
                "calling" => format!(" {} Calling {}… ", spinner_char, tool),
                "completed" => format!(" {} Processing… ", spinner_char),
                _ => format!(" {} Thinking… ", spinner_char),
            }
        } else if app.stream_buffer.is_empty() {
            format!(" {} Thinking… ", spinner_char)
        } else {
            format!(" {} Generating… ", spinner_char)
        };

        // Typing dots animation
        let dots = match app.spinner_frame % 3 {
            0 => "● ○ ○",
            1 => "● ● ○",
            _ => "● ● ●",
        };

        display_lines.push(Line::from(""));
        display_lines.push(Line::from(vec![
            Span::styled(" 🤖 ", Style::default().fg(Color::Cyan)),
            Span::styled(status_text, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        display_lines.push(Line::from(Span::styled(
            format!("   {}", dots),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )));
    }

    // ── Compute scroll ─────────────────────────────────────────────────
    let max_scroll = display_lines.len().saturating_sub(view_h);
    app.max_scroll_cache.set(max_scroll);
    let scroll = if app.scroll_bottom {
        max_scroll
    } else {
        app.scroll_offset.min(max_scroll)
    };

    // ── "↑ more" hint when scrolled up ────────────────────────────────
    if !app.scroll_bottom && display_lines.len() > view_h {
        display_lines.push(Line::from(Span::styled(
            format!(" ↑ {} more — PgDn to go bottom", scroll),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        )));
    }

    // ── Render ─────────────────────────────────────────────────────────
    let chat_para = Paragraph::new(Text::from(display_lines))
        .scroll((scroll as u16, 0))
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    frame.render_widget(chat_para, area);
}

/// Build chat lines from app.messages with bubble-style rendering.
fn build_chat_lines(app: &RupooApp, max_w: usize) -> Vec<Line<'static>> {
    let mut all_lines: Vec<Line> = Vec::new();

    // ── Welcome screen when empty ─────────────────────────────────────
    if app.messages.is_empty() && !app.thinking {
        all_lines.push(Line::from(Span::styled(
            " Welcome to Rupoo!",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(Span::raw(" Type a message to start.")));
        all_lines.push(Line::from(Span::raw(" /help for commands.")));
        all_lines.push(Line::from(Span::raw(" Ctrl+P for command palette.")));
        all_lines.push(Line::from(Span::raw(" Shift+Enter for multi-line input.")));
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(Span::styled(
            " Ctrl+C × 2 to quit.",
            Style::default().fg(Color::DarkGray),
        )));
        return all_lines;
    }

    // ── Build message lines with bubbles ──────────────────────────────
    for msg in &app.messages {
        let is_user = msg.role == MessageRole::User;
        let is_error = msg.role == MessageRole::System && msg.content.contains("Error");
        let is_tool_call = msg.role == MessageRole::System && msg.content.starts_with("🔧");
        let is_tool_result = msg.role == MessageRole::System && msg.content.starts_with("✅");
        let is_system = msg.role == MessageRole::System && !is_error && !is_tool_call && !is_tool_result;

        // Tool call/result messages — compact card
        if is_tool_call || is_tool_result {
            let icon = if is_tool_call { "🔧" } else { "✅" };
            let fg_color = if is_tool_call { Color::Magenta } else { Color::Green };
            for line in msg.content.lines() {
                for wrapped in wrap_to(line, max_w.saturating_sub(4)) {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {} {}", icon, wrapped),
                        Style::default().fg(fg_color),
                    )));
                }
            }
            continue;
        }

        // System messages — centered, gray
        if is_system {
            for line in msg.content.lines() {
                for wrapped in wrap_to(line, max_w.saturating_sub(2)) {
                    all_lines.push(Line::from(Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            continue;
        }

        // Error messages — centered, red
        if is_error {
            all_lines.push(Line::from(Span::styled(
                " ── Error ──".to_string(),
                Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
            )));
            for line in msg.content.lines() {
                for wrapped in wrap_to(line, max_w.saturating_sub(2)) {
                    all_lines.push(Line::from(Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
            continue;
        }

        // ── Chat bubbles ──────────────────────────────────────────────
        if is_user {
            // User bubble: right-aligned, green accent
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(Span::styled(
                right_align_label("You ▾", max_w),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )));
            let bubble_w = (max_w * 3 / 4).max(20);
            // Top border
            all_lines.push(Line::from(Span::styled(
                right_align_str(&format!("┌{}┐", "─".repeat(bubble_w.saturating_sub(2))), max_w),
                Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
            )));
            // Content
            let content = &msg.content;
            let mut in_code = false;
            let mut code_buffer: Vec<String> = Vec::new();
            for line in content.lines() {
                if line.starts_with("```") {
                    if in_code {
                        flush_code_block_right(&mut all_lines, &code_buffer, bubble_w, Color::Green);
                        code_buffer.clear();
                        in_code = false;
                    } else {
                        in_code = true;
                    }
                    continue;
                }
                if in_code {
                    code_buffer.push(line.to_string());
                    continue;
                }
                for wrapped in wrap_to(line, bubble_w.saturating_sub(4)) {
                    let padded = format!("│ {} │", wrapped);
                    all_lines.push(Line::from(Span::styled(
                        right_align_str(&padded, max_w),
                        Style::default().fg(Color::White),
                    )));
                }
            }
            if in_code && !code_buffer.is_empty() {
                flush_code_block_right(&mut all_lines, &code_buffer, bubble_w, Color::Green);
            }
            // Bottom border
            all_lines.push(Line::from(Span::styled(
                right_align_str(&format!("└{}┘", "─".repeat(bubble_w.saturating_sub(2))), max_w),
                Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
            )));
        } else {
            // Assistant bubble: left-aligned, cyan accent
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(vec![
                Span::styled(" 🤖 ", Style::default().fg(Color::Cyan)),
                Span::styled("Rupoo", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            let bubble_w = (max_w * 3 / 4).max(20);
            // Top border
            all_lines.push(Line::from(Span::styled(
                format!("┌{}┐", "─".repeat(bubble_w.saturating_sub(2))),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
            )));
            // Content
            let content = &msg.content;
            let mut in_code = false;
            let mut code_buffer: Vec<String> = Vec::new();
            for line in content.lines() {
                if line.starts_with("```") {
                    if in_code {
                        flush_code_block_left(&mut all_lines, &code_buffer, bubble_w, Color::Cyan);
                        code_buffer.clear();
                        in_code = false;
                    } else {
                        in_code = true;
                    }
                    continue;
                }
                if in_code {
                    code_buffer.push(line.to_string());
                    continue;
                }
                for wrapped in wrap_to(line, bubble_w.saturating_sub(4)) {
                    all_lines.push(Line::from(Span::styled(
                        format!("│ {} │", wrapped),
                        Style::default().fg(Color::White),
                    )));
                }
            }
            if in_code && !code_buffer.is_empty() {
                flush_code_block_left(&mut all_lines, &code_buffer, bubble_w, Color::Cyan);
            }
            // Bottom border
            all_lines.push(Line::from(Span::styled(
                format!("└{}┘", "─".repeat(bubble_w.saturating_sub(2))),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
            )));
        }
    }

    all_lines
}

/// Right-align a label within max_w columns.
fn right_align_label(label: &str, max_w: usize) -> String {
    let w = unicode_width::UnicodeWidthStr::width(label);
    if w >= max_w {
        label.to_string()
    } else {
        format!("{}{}", " ".repeat(max_w - w), label)
    }
}

/// Right-align a string within max_w columns (using unicode display width).
fn right_align_str(s: &str, max_w: usize) -> String {
    let w = unicode_width::UnicodeWidthStr::width(s);
    if w >= max_w {
        s.to_string()
    } else {
        format!("{}{}", " ".repeat(max_w - w), s)
    }
}

/// Flush code block inside a right-aligned bubble.
fn flush_code_block_left(lines: &mut Vec<Line<'static>>, code: &[String], bubble_w: usize, _border_color: Color) {
    let inner_w = bubble_w.saturating_sub(4);
    lines.push(Line::from(Span::styled(
        format!("│ ┌─{}─┐", "─".repeat(inner_w.saturating_sub(2).min(200))),
        Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
    )));
    for cb_line in code {
        for wrapped in wrap_to(cb_line, inner_w.saturating_sub(2)) {
            lines.push(Line::from(Span::styled(
                format!("│ │ {} │", wrapped),
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        format!("│ └─{}─┘", "─".repeat(inner_w.saturating_sub(2).min(200))),
        Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
    )));
}

/// Flush code block inside a right-aligned bubble.
fn flush_code_block_right(lines: &mut Vec<Line<'static>>, code: &[String], bubble_w: usize, border_color: Color) {
    // Simplify: use left-style code blocks inside right bubbles too
    flush_code_block_left(lines, code, bubble_w, border_color);
}

// ═══════════════════════════════════════════════════════════════════════════
// Input area — with proper cursor tracking (unicode-width aware)
// ═══════════════════════════════════════════════════════════════════════════

fn render_input_area(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let disabled = app.input_mode == InputMode::Approval || app.input_mode == InputMode::Rename;
    let title = if app.input_mode == InputMode::Rename {
        " Rename: Enter to confirm / Esc to cancel "
    } else if disabled {
        " Input [blocked] "
    } else {
        " Input [Enter: send, Shift+Enter: newline] "
    };
    let block = Block::new()
        .borders(Borders::TOP)
        .title(title)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.input_mode == InputMode::Rename {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("[rename]", Style::default().fg(Color::Yellow))))
                .scroll((0, 0)),
            inner,
        );
    } else if disabled {
        frame.render_widget(
            Paragraph::new(Span::styled("(waiting for approval)", Style::default().fg(Color::DarkGray))),
            inner,
        );
    } else {
        let input_text = app.input.lines().join("\n");
        if input_text.is_empty() && app.focus != FocusTarget::Input {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    " Type a message…",
                    Style::default().fg(Color::DarkGray),
                )),
                inner,
            );
        } else {
            let max_w = inner.width as usize;
            let view_h = inner.height as usize;

            // Build display lines with unicode-width-aware wrapping
            let raw_lines = app.input.lines();
            let mut display_lines: Vec<Line> = Vec::new();
            // Track (display_row, display_col) for each raw (row, col)
            // We'll compute cursor display position separately
            for line in raw_lines.iter() {
                if line.is_empty() {
                    display_lines.push(Line::from(Span::styled(
                        String::new(),
                        Style::default().fg(Color::White),
                    )));
                } else {
                    for wrapped in wrap_to_unicode(line, max_w) {
                        display_lines.push(Line::from(Span::styled(
                            wrapped,
                            Style::default().fg(Color::White),
                        )));
                    }
                }
            }

            // ── Compute cursor display position (unicode-width aware) ───
            let (cursor_row, cursor_col) = app.input.cursor();
            let mut cursor_display_row: usize = 0;
            for (i, line) in raw_lines.iter().enumerate() {
                if i == cursor_row {
                    break;
                }
                // Count how many display rows this logical line occupies
                let line_w = unicode_width::UnicodeWidthStr::width(line.as_str());
                cursor_display_row += if max_w > 0 && line_w > max_w {
                    (line_w + max_w - 1) / max_w
                } else {
                    1
                };
            }
            // Within the cursor's logical line, count display rows up to cursor column
            let cursor_line = raw_lines.get(cursor_row).map(|s| s.as_str()).unwrap_or("");
            let prefix = if cursor_col <= cursor_line.len() {
                &cursor_line[..cursor_line.floor_char_boundary(cursor_col)]
            } else {
                cursor_line
            };
            let prefix_w = unicode_width::UnicodeWidthStr::width(prefix);
            if max_w > 0 {
                cursor_display_row += prefix_w / max_w;
            }

            // Cursor x: the column within the wrapped segment
            let cursor_x = if max_w > 0 { prefix_w % max_w } else { prefix_w };

            // ── Scroll to keep cursor visible ──────────────────────────
            let max_scroll = display_lines.len().saturating_sub(view_h);
            let scroll = if cursor_display_row >= view_h {
                (cursor_display_row - view_h + 1).min(max_scroll)
            } else {
                0
            };

            let para = Paragraph::new(Text::from(display_lines))
                .scroll((scroll as u16, 0));
            frame.render_widget(para, inner);

            // Position cursor — unicode-width corrected
            let cursor_display_row = cursor_display_row.saturating_sub(scroll);
            frame.set_cursor_position(ratatui::layout::Position {
                x: inner.x + cursor_x as u16,
                y: inner.y + cursor_display_row.min(view_h - 1) as u16,
            });
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Center status bar
// ═══════════════════════════════════════════════════════════════════════════

fn render_center_status(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let (mode_str, fg) = if app.thinking {
        if let Some((ref tool, ref phase)) = app.current_tool_status {
            match phase.as_str() {
                "calling" => (format!("⏳ calling {}", tool), Color::Yellow),
                "completed" => ("⏳ processing".to_string(), Color::Yellow),
                _ => ("⏳ thinking".to_string(), Color::Yellow),
            }
        } else if !app.stream_buffer.is_empty() {
            ("⏳ generating".to_string(), Color::Yellow)
        } else {
            ("⏳ thinking".to_string(), Color::Yellow)
        }
    } else if app.input_mode == InputMode::Approval {
        ("⚠ approval".to_string(), Color::Yellow)
    } else {
        ("● ready".to_string(), Color::DarkGray)
    };
    let text = format!(
        "{} {} msgs | {} sess",
        mode_str,
        app.messages.len(),
        app.sessions.len(),
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(fg)))),
        area,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Right sidebar — tokens / model / safety / plan progress
// ═══════════════════════════════════════════════════════════════════════════

fn render_right(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let has_tokens = app.token_in > 0 || app.token_out > 0;

    let top_h = 16u16;
    let (top_area, plan_area) = if area.height > top_h + 3 {
        (
            Rect::new(area.x, area.y, area.width, top_h.min(area.height)),
            Rect::new(area.x, area.y + top_h, area.width, area.height - top_h),
        )
    } else {
        (area, Rect::new(area.x, area.y, 0, 0))
    };

    let top_content = vec![
        Line::from(vec![Span::styled(
            " Tokens ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("In: ", Style::default().fg(Color::DarkGray)),
            Span::raw(if has_tokens {
                format!("{}", app.token_in)
            } else {
                "—".to_string()
            }),
        ]),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("Out:", Style::default().fg(Color::DarkGray)),
            Span::raw(if has_tokens {
                format!("{}", app.token_out)
            } else {
                "—".to_string()
            }),
        ]),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("Ctx:", Style::default().fg(Color::DarkGray)),
            {
                let est = app.conversation_history.estimated_tokens();
                let budget = app.conversation_history.token_budget();
                let pct = if budget > 0 { est * 100 / budget } else { 0 };
                let color = if pct > 80 { Color::Red } else if pct > 50 { Color::Yellow } else { Color::Green };
                Span::styled(
                    if budget > 0 { format!("{}k/{}k", est / 1000, budget / 1000) } else { format!("{}k", est / 1000) },
                    Style::default().fg(color),
                )
            },
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            " Model ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(Span::styled(
            format!(" {}", app.model_label),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![Span::styled(
            " Safety ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(Span::styled(
            " ✅ Active",
            Style::default().fg(Color::Green),
        )),
    ];

    let top_block = Block::new()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let top_inner = top_block.inner(top_area);
    frame.render_widget(top_block, top_area);
    frame.render_widget(
        Paragraph::new(Text::from(top_content)).wrap(Wrap { trim: false }),
        top_inner,
    );

    // ── Plan progress (bottom half) ───────────────────────────────────
    if plan_area.width > 0 && plan_area.height > 0 {
        let plan_block = Block::new()
            .title(" Plan ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let plan_inner = plan_block.inner(plan_area);
        frame.render_widget(plan_block, plan_area);

        if let Some(ref plan) = app.plan {
            let mut plan_lines: Vec<Line> = Vec::new();
            plan_lines.push(Line::from(Span::styled(
                format!(" {} steps", plan.steps.len()),
                Style::default().fg(Color::White),
            )));
            plan_lines.push(Line::from(""));
            for (i, step) in plan.steps.iter().enumerate() {
                let kind = match step {
                    rupoo::task::Step::Think { .. } => "Think",
                    rupoo::task::Step::ToolCall { .. } => "Tool",
                    rupoo::task::Step::WaitForInput { .. } => "Input",
                    rupoo::task::Step::Exec { .. } => "Exec",
                    rupoo::task::Step::HttpRequest { .. } => "HTTP",
                    rupoo::task::Step::BrowserAction { .. } => "Browser",
                    rupoo::task::Step::Finish { .. } => "Finish",
                };
                let marker = if i == plan.current_step_index {
                    "▸"
                } else if i < plan.current_step_index {
                    "✓"
                } else {
                    " "
                };
                let fg = if i == plan.current_step_index {
                    Color::Yellow
                } else if i < plan.current_step_index {
                    Color::Green
                } else {
                    Color::DarkGray
                };
                plan_lines.push(Line::from(Span::styled(
                    format!(" {} [{}] {}", marker, i + 1, kind),
                    Style::default().fg(fg),
                )));
            }
            frame.render_widget(
                Paragraph::new(Text::from(plan_lines)).wrap(Wrap { trim: false }),
                plan_inner,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(" No active plan", Style::default().fg(Color::DarkGray))),
                plan_inner,
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Approval dialog overlay
// ═══════════════════════════════════════════════════════════════════════════

fn render_approval_dialog(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let dialog_w = 60u16;
    let dialog_h = 10u16;
    let x = area.x.saturating_add(area.width.saturating_sub(dialog_w) / 2);
    let y = area.y.saturating_add(area.height.saturating_sub(dialog_h) / 2);
    let dialog_area = Rect::new(x, y, dialog_w.min(area.width), dialog_h.min(area.height));

    frame.render_widget(Clear, dialog_area);

    let block = Block::new()
        .title(" ⚠ Approval Required ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(block, dialog_area);

    let inner = Rect::new(
        dialog_area.x + 2,
        dialog_area.y + 1,
        dialog_area.width.saturating_sub(4),
        dialog_area.height.saturating_sub(2),
    );

    let (tool_name, args, _approved) = match &app.overlay {
        OverlayState::Approval {
            tool_name,
            args,
            approved,
        } => (tool_name.clone(), args.clone(), *approved),
        _ => return,
    };

    let lines = vec![
        Line::from(Span::styled(
            format!(" Tool: {}", tool_name),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(" Args: {}", if args.len() > 40 { &args[..40] } else { &args }),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            " Press 'a' to approve, 'd' to deny",
            Style::default().fg(Color::Yellow),
        )),
    ];

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Text wrapping utilities — unicode-width aware
// ═══════════════════════════════════════════════════════════════════════════

/// Split a line into multiple sub-lines each no wider than max_w display columns.
/// Uses unicode display width (CJK=2, emoji=2) for accurate wrapping.
fn wrap_to_unicode(line: &str, max_w: usize) -> Vec<String> {
    if max_w == 0 || line.is_empty() {
        return vec![line.to_string()];
    }

    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_w = 0;

    for ch in line.chars() {
        let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_w + ch_w > max_w && !current.is_empty() {
            result.push(current);
            current = String::new();
            current_w = 0;
        }
        current.push(ch);
        current_w += ch_w;
    }

    if !current.is_empty() {
        result.push(current);
    }

    if result.is_empty() {
        result.push(line.to_string());
    }
    result
}

/// Legacy byte-based wrap (used for message content where we pre-wrap by char count).
/// Kept for backward compatibility with code blocks and system messages.
fn wrap_to(line: &str, max_w: usize) -> Vec<&str> {
    if max_w == 0 || line.len() <= max_w {
        return vec![line];
    }
    let mut result = Vec::new();
    let mut start = 0;
    let bytes = line.len();
    while start < bytes {
        let mut end = (start + max_w).min(bytes);
        end = line.floor_char_boundary(end);
        if end <= start {
            break;
        }
        result.push(&line[start..end]);
        start = end;
    }
    if result.is_empty() {
        result.push(line);
    }
    result
}
