//! Spike 02: Overlay / Modal rendering in ratatui
//!
//! 验证目标:
//! 1. Overlay（如命令面板）如何正确覆盖在 Frame 之上
//! 2. 半透明背景遮罩如何实现
//! 3. 浮层矩形如何居中计算
//! 4. Ctrl+P / Esc 切换是否流畅无闪烁
//!
//! 运行: cd spike && cargo run --example 02_overlay_modal

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

/// 计算 Overlay 居中矩形
/// overlay_w / overlay_h 为内容尺寸，实际渲染会稍大（背景遮罩）
fn compute_overlay_area(area: Rect, content_w: u16, content_h: u16) -> OverlayRects {
    let overlay_w = content_w.min(area.width.saturating_sub(4));
    let overlay_h = content_h.min(area.height.saturating_sub(8));
    let overlay_area = Rect::new(
        area.x + (area.width.saturating_sub(overlay_w)) / 2,
        area.y + (area.height.saturating_sub(overlay_h)) / 2,
        overlay_w,
        overlay_h,
    );
    // 遮罩覆盖整个屏幕
    let backdrop = area;

    OverlayRects { backdrop, overlay_area }
}

struct OverlayRects {
    backdrop: Rect,
    overlay_area: Rect,
}

/// 渲染 Overlay 命令面板
fn render_command_palette(frame: &mut Frame, area: Rect) {
    use ratatui::widgets::Borders;

    // 1. 半透明背景遮罩（覆盖整个屏幕）
    frame.render_widget(
        ratatui::widgets::Clear,
        area,
    );

    // 2. 暗色背景（覆盖整个屏幕，乘以透明度）
    // ratatui 不直接支持透明度，用 background + Clear 模拟
    let backdrop_style = Style::default().bg(Color::Black).fg(Color::Black);
    frame.render_widget(
        ratatui::widgets::Paragraph::new("")
            .style(backdrop_style),
        area,
    );

    // 3. 居中浮层面板
    let rects = compute_overlay_area(area, 56, 18);

    // 浮层背景
    let panel_style = Style::default().bg(Color::Black).fg(Color::White);
    frame.render_widget(
        ratatui::widgets::Paragraph::new("").style(panel_style),
        rects.overlay_area,
    );

    // 面板边框
    let border_style = Style::default()
        .fg(Color::Cyan)
        .bg(Color::Black);
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(
                Line::from(vec![
                    Span::raw("▸ "),
                    Span::styled("Command Palette", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw("   "),
                    Span::styled("[ESC: Close]", Style::default().fg(Color::DarkGray)),
                ])
            ),
        rects.overlay_area,
    );

    // 输入框区域（面板内部第2行开始）
    let input_area = Rect::new(
        rects.overlay_area.x + 1,
        rects.overlay_area.y + 2,
        rects.overlay_area.width.saturating_sub(2),
        3,
    );
    frame.render_widget(
        Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
        input_area,
    );

    // 搜索结果
    let commands = vec![
        ("session list", "List all plans with status"),
        ("session show <id>", "Show plan details and steps"),
        ("status", "System status overview"),
        ("model list", "Show available providers and models"),
        ("model set <target>", "Switch LLM provider and model"),
        ("logs --follow", "Tail agent logs in real-time"),
        ("skills list", "List installed skills"),
        ("doctor", "Diagnose configuration issues"),
    ];

    let mut y = rects.overlay_area.y + 4;
    for (i, (name, desc)) in commands.iter().enumerate() {
        let row_area = Rect::new(
            rects.overlay_area.x + 1,
            y,
            rects.overlay_area.width.saturating_sub(2),
            1,
        );

        let row_style = if i == 0 {
            // 选中行：左侧 Cyan 边线 + 浅背景
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
        } else {
            Style::default().fg(Color::White)
        };

        let row_text = Text::from(vec![Line::from(vec![
            Span::raw(format!("{:24}", *name)),
            Span::raw("  "),
            Span::styled(*desc, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("↵", Style::default().fg(Color::DarkGray)),
        ])]);

        frame.render_widget(
            ratatui::widgets::Paragraph::new(row_text)
                .style(row_style)
                .block(Block::new().borders(Borders::NONE)),
            row_area,
        );

        y += 1;
        if y >= rects.overlay_area.y + rects.overlay_area.height.saturating_sub(1) {
            break;
        }
    }
}

fn render_approval_dialog(frame: &mut Frame, area: Rect) {
    let rects = compute_overlay_area(area, 50, 14);

    // 背景遮罩
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        ratatui::widgets::Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    // 面板
    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(
                Line::from(vec![
                    Span::styled("⚠ Tool Approval Required", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ])
            )
            .title_style(Style::default().bg(Color::Black).fg(Color::Yellow)),
        rects.overlay_area,
    );

    let body_top = rects.overlay_area.y + 2;
    let cols = |col: u16| -> Rect {
        Rect::new(rects.overlay_area.x + 2, body_top, rects.overlay_area.width - 4, 1)
    };
    let label_style = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);

    let rows = vec![
        (cols(0), Line::from(vec![Span::styled("Tool:    ", label_style), Span::styled("task_shell_start", value_style)])),
        (cols(1), Line::from(vec![Span::styled("Command: ", label_style), Span::styled("cargo build --release", Style::default().fg(Color::Yellow))])),
        (cols(2), Line::from(vec![Span::styled("Sandbox: ", label_style), Span::styled("[L2 Block]", Style::default().fg(Color::Red))])),
    ];

    for (area, line) in rows {
        frame.render_widget(ratatui::widgets::Paragraph::new(Text::from(line)), area);
    }

    // 按钮行
    let btn_y = rects.overlay_area.y + rects.overlay_area.height - 3;
    let btn_w = (rects.overlay_area.width - 4) / 4;
    let btn_style_approve = Style::default().fg(Color::Green).bg(Color::Black);
    let btn_style_deny = Style::default().fg(Color::Red).bg(Color::Black);

    let buttons = vec![
        ("[Approve Once]", btn_style_approve, rects.overlay_area.x + 1),
        ("[Approve All]", btn_style_approve, rects.overlay_area.x + 1 + btn_w),
        ("[Deny]", btn_style_deny, rects.overlay_area.x + 1 + btn_w * 2),
        ("[Deny+Block]", btn_style_deny, rects.overlay_area.x + 1 + btn_w * 3),
    ];

    for (label, style, x) in buttons {
        let btn_area = Rect::new(x, btn_y, btn_w - 1, 1);
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(Span::styled(label, style))).style(Style::default().bg(Color::Black)),
            btn_area,
        );
    }
}

/// 渲染普通 Frame（无 Overlay）
fn render_normal(frame: &mut Frame, area: Rect) {
    use ratatui::widgets::{Borders, Block, Paragraph};
    use ratatui::style::Color;

    let [left, center, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(1),
            Constraint::Length(22),
        ])
        .areas(area);

    frame.render_widget(
        Paragraph::new("Sessions\n\n● plan_abc\n○ plan_def\n○ plan_ghi\n\nSkills\n\n↻ code-review\n↻ git-integrate")
            .block(Block::new().title("Left").borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan)),
        left,
    );

    frame.render_widget(
        Paragraph::new("Chat area — normal mode\n\nType /help or Ctrl+P for commands")
            .block(Block::new().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray))),
        center,
    );

    frame.render_widget(
        Paragraph::new("Tokens\n↑ 2,341\n↓ 1,847\n\nModel\ndeepseek-v4")
            .block(Block::new().title("Right").borders(Borders::ALL))
            .style(Style::default().fg(Color::Green)),
        right,
    );
}

fn main() {
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io::{self, stdout};

    let stdout = stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Overlay {
        None,
        CommandPalette,
        Approval,
    }

    let mut overlay: Overlay = Overlay::None;

    loop {
        terminal.draw(|f| {
            let area = f.area();
            match overlay {
                Overlay::None => render_normal(f, area),
                Overlay::CommandPalette => render_command_palette(f, area),
                Overlay::Approval => render_approval_dialog(f, area),
            }
        }).unwrap();

        use crossterm::event::{read, Event, KeyCode, KeyEventKind};
        match read().unwrap() {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        overlay = Overlay::None;
                    }
                    KeyCode::Char('p') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                        overlay = match overlay {
                            Overlay::CommandPalette => Overlay::None,
                            _ => Overlay::CommandPalette,
                        };
                    }
                    KeyCode::Char('a') => {
                        overlay = match overlay {
                            Overlay::Approval => Overlay::None,
                            _ => Overlay::Approval,
                        };
                    }
                    _ => {}
                }
                if key.code == KeyCode::Char('q') && overlay == Overlay::None {
                    break;
                }
            }
            _ => {}
        }
    }
}
