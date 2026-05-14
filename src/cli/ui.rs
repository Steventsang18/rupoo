//! TUI rendering — layout, message bubbles, input area, status bar.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::app::{App, MessageRole};

/// Main render entry — called once per frame.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ── Overall layout: title bar | chat | input | status ──
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),           // title
            Constraint::Min(1),              // chat area
            Constraint::Length(3),           // input
            Constraint::Length(1),           // status
        ])
        .split(area);

    render_title(frame, chunks[0]);
    render_chat(frame, chunks[1], app);
    render_input(frame, chunks[2], app);
    render_status(frame, chunks[3], app);
}

// ── Title bar ──

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Line::from(Span::styled(
        " Rupoo — AI Terminal Assistant",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(title, area);
}

// ── Chat area ──

fn render_chat(frame: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    if app.messages.is_empty() {
        let welcome = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                " Welcome to Rupoo!",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "  Type a message below and press Enter to start.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "  /help  for available commands  ·  Ctrl+C to quit",
                Style::default().fg(Color::DarkGray),
            )),
        ]));
        frame.render_widget(welcome, area);
        return;
    }

    let mut text = Text::default();
    for msg in &app.messages {
        let role_style = match msg.role {
            MessageRole::User => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            MessageRole::Assistant => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        };
        text.push_line(Line::from(Span::styled(format!(" {}", msg.role), role_style)));
        text.push_line(Line::from(Span::raw(format!(" {}", msg.content))));
        text.push_line(Line::from(""));
    }

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

// ── Input area ──

fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let input_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));

    // Constrain the inner area for the textarea
    let inner = input_block.inner(area);
    frame.render_widget(input_block, area);
    frame.render_widget(&app.input, inner);
}

// ── Status bar ──

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let style = if app.loading {
        Style::default().fg(Color::Yellow).bg(Color::Black)
    } else {
        Style::default().fg(Color::DarkGray).bg(Color::Black)
    };

    let text = if app.loading {
        format!(" ⏳ {}  (Ctrl+C to cancel)", app.status)
    } else {
        format!(" {}", app.status)
    };

    let status = Paragraph::new(Line::from(Span::styled(text, style)));
    frame.render_widget(status, area);
}
