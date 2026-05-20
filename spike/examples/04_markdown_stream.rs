//! Spike 04: Markdown rendering with streaming output
//!
//! 验证目标:
//! 1. Markdown 解析（代码块、inline code、粗体）
//! 2. 流式追加内容时 render 是否无闪烁
//! 3. 预渲染行缓存：避免每帧重新解析
//! 4. thinking 进度条动画
//!
//! 运行: cd spike && cargo run --example 04_markdown_stream

use std::io::{self, stdout};
use std::time::{Duration, Instant};

use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

/// 预渲染的消息行
/// 存储 render 时直接使用的 Vec<Line>，避免每帧重复解析
#[derive(Debug, Clone)]
pub struct PreRenderedLine {
    pub spans: Vec<Span<'static>>,
    pub is_code: bool,
}

impl PreRenderedLine {
    fn text(content: String, fg: Color) -> Self {
        Self {
            spans: vec![Span::styled(content, Style::default().fg(fg))],
            is_code: false,
        }
    }

    fn code_block(lines: Vec<String>) -> Self {
        // Spike: 暂用 monospace，待集成 syntect 时替换此处
        let mut all_spans = Vec::new();
        all_spans.push(Span::styled(
            "─ rust ─────────────────────────────────\n".to_string(),
            Style::default().fg(Color::Magenta),
        ));
        for line in lines {
            all_spans.push(Span::styled(line, Style::default().fg(Color::White)));
            all_spans.push(Span::raw("\n"));
        }
        Self {
            spans: all_spans,
            is_code: true,
        }
    }

    fn to_line(&self) -> Line<'static> {
        Line::from(self.spans.clone())
    }
}

/// 模拟 Markdown 解析器（spike 用简化版）
fn parse_markdown_chunk(content: &str) -> Vec<PreRenderedLine> {
    let mut lines = Vec::new();

    for line in content.lines() {
        if line.starts_with("```") {
            if line.len() > 3 {
                let lang = &line[3..];
                lines.push(PreRenderedLine::text(
                    format!(" ── {} ──", lang),
                    Color::Magenta,
                ));
            }
        } else if line.starts_with('#') {
            lines.push(PreRenderedLine::text(line.to_string(), Color::Cyan));
        } else if line.starts_with("**") && line.ends_with("**") && line.len() > 4 {
            let inner = &line[2..line.len() - 2];
            lines.push(PreRenderedLine::text(inner.to_string(), Color::White));
        } else if line.starts_with('-') || line.starts_with('*') {
            lines.push(PreRenderedLine::text(
                format!("  • {}", &line[1..]),
                Color::White,
            ));
        } else if line.starts_with('`') && line.ends_with('`') && line.len() > 2 {
            let code = &line[1..line.len() - 1];
            lines.push(PreRenderedLine::text(
                format!("`{}`", code),
                Color::Yellow,
            ));
        } else if line.is_empty() {
            lines.push(PreRenderedLine::text(" ".to_string(), Color::White));
        } else {
            // 普通文本，检测 inline code
            let mut span_line = Vec::new();
            let mut remaining = line;
            while let Some(start) = remaining.find('`') {
                if start > 0 {
                    span_line.push(Span::raw(&remaining[..start]));
                }
                if let Some(end) = remaining[start + 1..].find('`') {
                    let code = &remaining[start + 1..start + 1 + end];
                    span_line.push(Span::styled(
                        code.to_string(),
                        Style::default().fg(Color::Yellow),
                    ));
                    remaining = &remaining[start + 1 + end + 1..];
                } else {
                    span_line.push(Span::raw(&remaining[start..]));
                    break;
                }
            }
            if !remaining.is_empty() {
                span_line.push(Span::raw(remaining));
            }
            lines.push(PreRenderedLine {
                spans: span_line,
                is_code: false,
            });
        }
    }

    lines
}

/// 模拟 LLM 流式输出（逐步追加内容）
struct StreamSimulator {
    chunks: Vec<&'static str>,
    chunk_idx: usize,
}

impl StreamSimulator {
    fn new() -> Self {
        let chunks: Vec<&'static str> = vec![
            "分析", "完成", "。该", "代码", "库采", "用**", "微内", "核架", "构**", "，核", "心模",
            "块如", "下：", "\n\n", "- `", "src/", "agent", ".rs", "` —", " 代", "理引", "擎核",
            "心\n", "- `", "src/", "cli/", "` —", " TU", "I 界", "面\n", "- `", "src/", "db.r",
            "s` ", "— S", "QL", "ite ", "持久", "化\n", "- `", "src/", "safet", "y.rs", "` —",
            " 沙", "箱执", "行层", "\n\n核", "心入", "口在", " `fn", " mai", "n()`", "：\n\n",
            "```", "rust\n", "pub ", "stru", "ct A", "gent", " {\n", "    r", "epo:", " Ar", "c<T",
            "ask", "Re", "po", ">,", "\n   ", " ll", "m: ", " Ll", "mC", "lie", "nt", ">,\n",
            "    e", "xec", "uto", "r: ", " Mc", "pTo", "olE", "xec", "uto", "r,\n", "}\n\n",
            "impl", " Ag", "ent", " {\n", "    p", "ub ", "asy", "nc", " fn", " r", "un", "_p",
            "la", "n(", "&se", "lf", ", p", "la", "n_", "id", ": ", "&st", "r)", " {\n ",
            "   ", " le", "t ", "pl", "an", " =", " s", "el", "f", ".re", "po", ".", "lo", "ad",
            "_p", "la", "n(", "pl", "an", "_i", "d)", ".", "aw", "ai", "t;", "\n", "    ", "fo",
            "r ", "st", "ep", " i", "n", " &", "pl", "an", ".", "st", "ep", "s", " {\n",
            "        ", "self", ".", "exe", "cut", "e_", "ste", "p(", "st", "ep", ")", ".",
            "aw", "ai", "t;", "\n", "    }", "\n", " }\n", "}\n", "```",
        ];
        Self {
            chunks,
            chunk_idx: 0,
        }
    }

    fn next_chunk(&mut self) -> Option<&'static str> {
        if self.chunk_idx < self.chunks.len() {
            let chunk = self.chunks[self.chunk_idx];
            self.chunk_idx += 1;
            Some(chunk)
        } else {
            None
        }
    }

    fn is_done(&self) -> bool {
        self.chunk_idx >= self.chunks.len()
    }
}

/// App 状态
#[derive(Clone)]
struct App {
    rendered_lines: Vec<PreRenderedLine>,
    pending_text: String,
    stream: StreamSimulator,
    thinking_progress: u8,
    thinking_text: String,
}

impl App {
    fn new() -> Self {
        Self {
            rendered_lines: Vec::new(),
            pending_text: String::new(),
            stream: StreamSimulator::new(),
            thinking_progress: 0,
            thinking_text: "Rupoo is thinking...".to_string(),
        }
    }

    fn append_chunk(&mut self, chunk: &str) {
        self.pending_text.push_str(chunk);

        if self.pending_text.contains('\n') || chunk.ends_with("```") {
            let parsed = parse_markdown_chunk(&self.pending_text);
            self.rendered_lines.extend(parsed);
            self.pending_text.clear();
        }
    }
}

fn main() {
    let stdout = stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut app = App::new();
    let start = Instant::now();

    loop {
        terminal
            .draw(|f| {
                let area = f.area();
                render(&mut app.clone(), f, area, start.elapsed());
            })
            .unwrap();

        use crossterm::event::{poll, read, Event, KeyCode, KeyEventKind};

        if poll(Duration::from_millis(16)).unwrap() {
            if let Ok(Event::Key(key)) = read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char(' ') => {
                            while !app.stream.is_done() {
                                if let Some(chunk) = app.stream.next_chunk() {
                                    app.append_chunk(chunk);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 自动推进流式输出
        if start.elapsed() >= Duration::from_millis(60) * app.stream.chunk_idx as u32 {
            if let Some(chunk) = app.stream.next_chunk() {
                app.append_chunk(chunk);
            }
        }

        // thinking 进度
        if !app.stream.is_done() {
            app.thinking_progress =
                ((app.stream.chunk_idx as f32 / app.stream.chunks.len() as f32) * 100.0) as u8;
            app.thinking_text = format!(
                "Analyzing... {}% ({}/{})",
                app.thinking_progress,
                app.stream.chunk_idx,
                app.stream.chunks.len()
            );
        } else {
            app.thinking_progress = 100;
            app.thinking_text = "Complete".to_string();
        }
    }
}

fn render(app: &mut App, frame: &mut Frame, area: Rect, elapsed: Duration) {
    let [left, center, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(1),
            Constraint::Length(22),
        ])
        .areas(area);

    // Left
    frame.render_widget(
        Paragraph::new("Sessions\n\n● plan_abc\n○ plan_def")
            .block(Block::new().title("Left").borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan)),
        left,
    );

    // Center: Chat
    let [title, chat, input, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(center);

    let title_text = format!(
        " Rupoo — Stream Demo [elapsed: {:.1}s]",
        elapsed.as_secs_f32()
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title_text,
            Style::default().fg(Color::Cyan),
        ))),
        title,
    );

    // Chat 内容
    let mut all_lines: Vec<Line> = vec![Line::from(Span::styled(
        "You",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ))];
    all_lines.push(Line::from(Span::raw(" 分析这个代码库的架构")));
    all_lines.push(Line::from(""));
    all_lines.push(Line::from(Span::styled(
        "Rupoo",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    // Thinking 进度
    if !app.stream.is_done() {
        let progress_blocks = (app.thinking_progress as usize / 5).min(20);
        let progress_bar = format!(
            "[{}{}] {}%",
            "=".repeat(progress_blocks),
            "-".repeat(20 - progress_blocks),
            app.thinking_progress
        );
        all_lines.push(Line::from(Span::styled(
            progress_bar,
            Style::default().fg(Color::Cyan),
        )));
        all_lines.push(Line::from(Span::styled(
            app.thinking_text.clone(),
            Style::default().fg(Color::DarkGray),
        )));
        all_lines.push(Line::from(""));
    }

    // 已渲染的行
    for line in &app.rendered_lines {
        all_lines.push(line.to_line());
    }

    // Pending 行（正在输入）
    if !app.pending_text.is_empty() {
        let cursor_char = if (elapsed.as_millis() / 300) % 2 == 0 { "|" } else { " " };
        all_lines.push(Line::from(vec![
            Span::raw(app.pending_text.clone()),
            Span::styled(cursor_char, Style::default().fg(Color::Cyan)),
        ]));
    }

    let chat_para = Paragraph::new(Text::from(all_lines))
        .block(
            Block::new()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });

    frame.render_widget(chat_para, chat);

    // Input
    frame.render_widget(
        Block::new()
            .borders(Borders::TOP)
            .title(if app.stream.is_done() {
                "Input"
            } else {
                "Input (disabled during stream)"
            })
            .border_style(Style::default().fg(Color::DarkGray)),
        input,
    );

    // Status
    let token_text = format!(
        "Tokens: {} in / {} out  [Stream: {}/{}]",
        1024 + app.rendered_lines.len() * 8,
        app.rendered_lines.len() * 12,
        app.stream.chunk_idx,
        app.stream.chunks.len()
    );
    frame.render_widget(Paragraph::new(Line::from(token_text)), status);

    // Right
    frame.render_widget(
        Paragraph::new(format!(
            "Tokens\n\nIn:  {}\nOut: {}\n\nModel\ndeepseek-v4\n\nSafety\n\n[L1] Audit\n[L2] Block",
            1024 + app.rendered_lines.len() * 8,
            app.rendered_lines.len() * 12
        ))
        .block(Block::new().title("Right").borders(Borders::ALL))
        .style(Style::default().fg(Color::Green)),
        right,
    );
}
