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

// Spinner frames for "thinking" animation.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// ── Public entry ────────────────────────────────────────────────────────────

/// Render the full TUI. Called once per `terminal.draw()`.
pub fn render(frame: &mut Frame, app: &RupooApp) {
    let area = frame.area();
    let rects = compute_three_column(area);

    render_left(frame, rects.left, app);
    render_center(frame, rects.center, app);
    render_right(frame, rects.right, app);

    // Overlays always on top.
    if matches!(app.overlay, OverlayState::Approval { .. }) {
        render_approval_dialog(frame, area, app);
    }

    // ── Anchor cursor inside the input area ───────────────────────────
    // Cursor position is managed by TextArea rendering, so we just ensure
    // the cursor is visible within the input bounds. TextArea handles it.
}

// ── Layout ─────────────────────────────────────────────────────────────────

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

// ── Left sidebar — session tabs ────────────────────────────────────────────

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
        let is_focused = app.focus == FocusTarget::Sessions && is_active;
        let style = if is_focused {
            Style::default().fg(fg).bg(Color::Indexed(236))
        } else {
            Style::default().fg(fg)
        };
        lines.push(Line::from(Span::styled(format!(" {} {}{}", marker, i, if i < 10 { " " } else { "" }), style)));
        lines.push(Line::from(Span::styled(format!("  {}", tab.label), style)));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }), inner);
}

// ── Center column ───────────────────────────────────────────────────────────

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
    let text = Line::from(vec![
        Span::styled(" Rupoo ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(mode_label, Style::default().fg(fg)),
    ]);
    frame.render_widget(Paragraph::new(Text::from(vec![text])), area);
}

// ── Chat area — uses cached lines, renders stream buffer, shows tool status ─

fn render_chat_area(frame: &mut Frame, area: Rect, app: &RupooApp) {
    if area.height < 2 || area.width < 2 {
        return;
    }
    let max_w = area.width.saturating_sub(4) as usize;
    let view_h = area.height as usize;

    // ── Use cached lines or rebuild ────────────────────────────────────
    let mut display_lines = match app.cached_lines.take() {
        Some((counter, cached_max_w, lines)) if counter == app.change_counter && cached_max_w == max_w => {
            // Cache hit — store it back and use it
            app.cached_lines.set(Some((counter, cached_max_w, lines.clone())));
            lines
        }
        cache_miss => {
            let lines = build_chat_lines(app, max_w);
            app.cached_lines.set(Some((app.change_counter, max_w, lines.clone())));
            // Drop the old cache value we took
            drop(cache_miss);
            lines
        }
    };

    // ── Streaming buffer: show in-progress text during thinking ───────
    if app.thinking && !app.stream_buffer.is_empty() {
        // Add a streaming assistant "draft" bubble
        let header_span = Span::styled(
            " A ",
            Style::default().fg(Color::Cyan).bg(Color::Indexed(17)).add_modifier(Modifier::BOLD),
        );
        let role_span = Span::styled(
            " Rupoo",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );
        display_lines.push(Line::from(vec![header_span, role_span]));

        let indent = " ";
        for line in app.stream_buffer.lines() {
            for wrapped in wrap_to(line, max_w.saturating_sub(indent.len())) {
                display_lines.push(Line::from(Span::styled(
                    format!("{}{}", indent, wrapped),
                    Style::default().fg(Color::White),
                )));
            }
        }
    }

    // ── Thinking spinner (appended after messages) ─────────────────────
    if app.thinking {
        let frame_idx = app.spinner_frame % SPINNER_FRAMES.len();
        let spinner_char = SPINNER_FRAMES[frame_idx];
        let status_text = if let Some((ref tool_name, ref phase)) = app.current_tool_status {
            match phase.as_str() {
                "calling" => format!(" {} {} is calling {}… ", spinner_char, "Rupoo", tool_name),
                "completed" => format!(" {} Processing result… ", spinner_char),
                _ => format!(" {} Rupoo is thinking… ", spinner_char),
            }
        } else if app.stream_buffer.is_empty() {
            format!(" {} Rupoo is thinking… ", spinner_char)
        } else {
            // Stream buffer has content — spinner shows "generating"
            format!(" {} Generating… ", spinner_char)
        };
        display_lines.push(Line::from(Span::styled(
            status_text,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
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

    // ── Render with wrapping disabled (we pre-wrapped already) ────────
    let chat_para = Paragraph::new(Text::from(display_lines))
        .scroll((scroll as u16, 0))
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    frame.render_widget(chat_para, area);
}

/// Build chat lines from app.messages, pre-wrapping each line to max_w.
/// This ensures Paragraph::scroll() operates on display‑accurate line counts
/// and no content is clipped at the bottom.
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
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(Span::styled(
            " Esc / Ctrl+C to quit.",
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

        // Tool call/result messages get a compact style
        if is_tool_call || is_tool_result {
            let fg_color = if is_tool_call { Color::Magenta } else { Color::Green };
            for line in msg.content.lines() {
                for wrapped in wrap_to(line, max_w.saturating_sub(1)) {
                    all_lines.push(Line::from(Span::styled(
                        format!(" {}", wrapped),
                        Style::default().fg(fg_color),
                    )));
                }
            }
            continue;
        }

        let (bracket, bg, fg_color) = if is_error {
            ("!", Color::Red, Color::White)
        } else if is_user {
            ("U", Color::Indexed(22), Color::Green)
        } else {
            ("A", Color::Indexed(17), Color::Cyan)
        };

        let header_span = Span::styled(
            format!(" {} ", bracket),
            Style::default().fg(fg_color).bg(bg).add_modifier(Modifier::BOLD),
        );
        let role_span = Span::styled(
            if is_user { " You" } else if is_error { " Error" } else { " Rupoo" },
            Style::default().fg(fg_color).add_modifier(Modifier::BOLD),
        );
        all_lines.push(Line::from(vec![header_span, role_span]));

        let content = &msg.content;
        let mut in_code = false;
        let mut code_lang = String::new();
        let mut code_buffer: Vec<String> = Vec::new();

        for line in content.lines() {
            if line.starts_with("```") {
                if in_code {
                    // End code block — flush
                    let border_w = max_w.saturating_sub(4);
                    all_lines.push(Line::from(Span::styled(
                        format!(" ┌─{}", "─".repeat(border_w.min(200))),
                        Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
                    )));
                    for cb_line in &code_buffer {
                        for wrapped in wrap_to(cb_line, max_w.saturating_sub(2)) {
                            all_lines.push(Line::from(Span::styled(
                                format!(" │ {}", wrapped),
                                Style::default().fg(Color::Yellow),
                            )));
                        }
                    }
                    all_lines.push(Line::from(Span::styled(
                        format!(" └─{}", "─".repeat(border_w.min(200))),
                        Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
                    )));
                    code_buffer.clear();
                    code_lang.clear();
                    in_code = false;
                } else {
                    in_code = true;
                    code_lang = line.trim_start_matches("```").trim().to_string();
                }
                continue;
            }
            if in_code {
                code_buffer.push(line.to_string());
                continue;
            }

            // Normal content — pre-wrap to terminal width
            let indent = if is_user { "  " } else { " " };
            for wrapped in wrap_to(line, max_w.saturating_sub(indent.len())) {
                let line_style = if is_error {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::White)
                };
                all_lines.push(Line::from(Span::styled(
                    format!("{}{}", indent, wrapped),
                    line_style,
                )));
            }
        }

        // Flush any unclosed code block
        if in_code && !code_buffer.is_empty() {
            let border_w = max_w.saturating_sub(4);
            all_lines.push(Line::from(Span::styled(
                format!(" ┌─{}", "─".repeat(border_w.min(200))),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
            )));
            for cb_line in &code_buffer {
                for wrapped in wrap_to(cb_line, max_w.saturating_sub(2)) {
                    all_lines.push(Line::from(Span::styled(
                        format!(" │ {}", wrapped),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            all_lines.push(Line::from(Span::styled(
                format!(" └─{}", "─".repeat(border_w.min(200))),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM),
            )));
        }

        all_lines.push(Line::from(""));
    }

    all_lines
}

/// Split a line into multiple sub‑lines each no longer than max_w.
/// Uses floor_char_boundary to avoid splitting multi‑byte UTF‑8 chars.
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

// ── Input area ──────────────────────────────────────────────────────────────

fn render_input_area(frame: &mut Frame, area: Rect, app: &RupooApp) {
    let disabled = app.input_mode == InputMode::Approval || app.input_mode == InputMode::Rename;
    let title = if app.input_mode == InputMode::Rename {
        " Rename: Enter to confirm / Esc to cancel "
    } else if disabled {
        " Input [blocked] "
    } else {
        " Input "
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
            // Custom rendering: wrap input text to fit the inner width
            let max_w = inner.width as usize;
            let mut display_lines: Vec<Line> = Vec::new();
            let raw_lines = app.input.lines();
            for line in raw_lines.iter() {
                if max_w > 0 && line.len() > max_w {
                    for wrapped in wrap_to(line, max_w) {
                        display_lines.push(Line::from(Span::styled(
                            wrapped.to_string(),
                            Style::default().fg(Color::White),
                        )));
                    }
                } else {
                    display_lines.push(Line::from(Span::styled(
                        line.clone(),
                        Style::default().fg(Color::White),
                    )));
                }
            }

            // Calculate scroll to keep cursor visible
            // TextArea cursor position: (row, col)
            let cursor_row = app.input.cursor().0;
            // Count display rows up to cursor (accounting for wrapping)
            let mut display_row = 0;
            for (i, line) in raw_lines.iter().enumerate() {
                if i == cursor_row {
                    break;
                }
                if max_w > 0 && line.len() > max_w {
                    display_row += (line.len() + max_w - 1) / max_w;
                } else {
                    display_row += 1;
                }
            }
            // Add wrapped rows for the cursor line up to the cursor column
            let cursor_col = app.input.cursor().1;
            if max_w > 0 {
                display_row += cursor_col / max_w.max(1);
            }

            let view_h = inner.height as usize;
            let max_scroll = display_lines.len().saturating_sub(view_h);
            let scroll = if display_row >= view_h {
                (display_row - view_h + 1).min(max_scroll)
            } else {
                0
            };

            let para = Paragraph::new(Text::from(display_lines))
                .scroll((scroll as u16, 0));
            frame.render_widget(para, inner);

            // Position cursor within the inner area
            let cursor_display_row = display_row.saturating_sub(scroll);
            frame.set_cursor_position(ratatui::layout::Position {
                x: inner.x + (cursor_col % max_w.max(1)) as u16,
                y: inner.y + cursor_display_row.min(view_h - 1) as u16,
            });
        }
    }
}

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

// ── Right sidebar — tokens / model / safety / plan progress ────────────────

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

// ── Approval dialog overlay ─────────────────────────────────────────────────

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
