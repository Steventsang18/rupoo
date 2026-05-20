//! Spike 01: Three-column layout validation
//!
//! 验证目标:
//! 1. ratatui 三栏 Layout 约束是否正确（左20列固定，右22列固定，中间自适应）
//! 2. 各区域边界是否精确
//! 3. 终端 resize 时布局是否正确响应
//!
//! 运行: cd spike && cargo run --example 01_layout_3column

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

/// 三栏布局验证
/// - Left:  固定 20 列
/// - Center: 自适应 (Min(1))
/// - Right: 固定 22 列
pub fn compute_three_column(area: Rect) -> ThreeColumnRects {
    let left_w = 20u16;
    let right_w = 22u16;

    let [left, center, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_w),
            Constraint::Min(1),
            Constraint::Length(right_w),
        ])
        .areas(area);

    // Center 内部: title(1) + chat(Min) + input(3) + status(1)
    let [title, chat, input, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(center);

    ThreeColumnRects {
        left,
        title,
        chat,
        input,
        status,
        right,
    }
}

#[derive(Debug)]
pub struct ThreeColumnRects {
    pub left: Rect,
    pub title: Rect,
    pub chat: Rect,
    pub input: Rect,
    pub status: Rect,
    pub right: Rect,
}

fn render(frame: &mut Frame, rects: &ThreeColumnRects, app: &App) {
    use ratatui::{
        style::{Color, Style},
        text::{Line, Text},
        widgets::{Block, Borders, Paragraph},
    };

    // — Left sidebar —
    let left_content = Text::from(vec![
        Line::from("Sessions"),
        Line::from(""),
        Line::from("● plan_abc [Run]"),
        Line::from("○ plan_def [Done]"),
        Line::from("○ plan_ghi [Pend]"),
        Line::from(""),
        Line::from("Skills"),
        Line::from(""),
        Line::from("↻ code-review"),
        Line::from("↻ git-integrate"),
    ]);
    frame.render_widget(
        Paragraph::new(left_content)
            .block(Block::new().title("Left 20col").borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan)),
        rects.left,
    );

    // — Title bar —
    let title_style = Style::default().fg(Color::Cyan);
    frame.render_widget(
        Paragraph::new(Line::from("⚡ Rupoo — AI Terminal Assistant")).style(title_style),
        rects.title,
    );

    // — Chat area —
    frame.render_widget(
        Paragraph::new(app.chat_content.clone())
            .block(Block::new().title("Chat (Min)").borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)))
            .style(Style::default().fg(Color::White)),
        rects.chat,
    );

    // — Input area —
    frame.render_widget(
        Block::new()
            .title("Input")
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
        rects.input,
    );

    // — Status bar —
    let status_style = if app.loading {
        Style::default().fg(Color::Yellow).bg(Color::Black)
    } else {
        Style::default().fg(Color::DarkGray).bg(Color::Black)
    };
    frame.render_widget(
        Paragraph::new(Line::from(format!(" {} | db: ~/.rupoo/agent.db", if app.loading { "⏳ Loading..." } else { "Ready" }))).style(status_style),
        rects.status,
    );

    // — Right sidebar —
    let right_content = Text::from(vec![
        Line::from("Tokens"),
        Line::from(""),
        Line::from(format!("↑ {}", app.token_in)),
        Line::from(format!("↓ {}", app.token_out)),
        Line::from(""),
        Line::from("Model"),
        Line::from(""),
        Line::from("deepseek-v4"),
        Line::from(""),
        Line::from("Safety"),
        Line::from(""),
        Line::from("[L1] Audit"),
        Line::from("[L2] Block"),
    ]);
    frame.render_widget(
        Paragraph::new(right_content)
            .block(Block::new().title("Right 22col").borders(Borders::ALL))
            .style(Style::default().fg(Color::Green)),
        rects.right,
    );
}

#[derive(Debug, Clone)]
pub struct App {
    pub chat_content: String,
    pub token_in: u32,
    pub token_out: u32,
    pub loading: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            chat_content: "Hello, this is a long chat message that should wrap properly within the center column when the terminal is narrow enough...".to_string(),
            token_in: 2341,
            token_out: 1847,
            loading: true,
        }
    }
}

fn main() {
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io::{self, stdout};

    let stdout = stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut app = App::default();

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let rects = compute_three_column(area);

            // 打印布局诊断
            eprintln!("[LAYOUT] area={:?} left={:?} right={:?}", area, rects.left, rects.right);

            render(f, &rects, &app);
        }).unwrap();

        use crossterm::event::{read, Event, KeyCode, KeyEventKind};
        match read().unwrap() {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('l') => app.loading = !app.loading,
                    KeyCode::Char('+') => { app.token_in += 100; app.token_out += 50; }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
