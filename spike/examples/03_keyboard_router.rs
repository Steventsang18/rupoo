//! Spike 03: Keyboard event routing
//!
//! 验证目标:
//! 1. Ctrl+P / Esc / Arrow / Enter 正确路由到对应 InputMode
//! 2. 方向键在命令面板中选择，上下越界保护
//! 3. Tab 切换在 Chat 内触发审批，阻断输入是否正确
//! 4. 输入历史 ↑↓ 遍历
//!
//! 运行: cd spike && cargo run --example 03_keyboard_router

use std::io::{self, stdout};

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    Frame,
};
use ratatui::widgets::{Block, Borders, Paragraph};
use tui_textarea::TextArea;

/// 所有可能的输入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Chat,           // 正常聊天
    CommandPalette, // Ctrl+P 命令面板
    Approval,       // 审批弹窗（阻断）
    TabSwitch,      // Tab 切换
}

/// 命令面板条目
#[derive(Debug, Clone)]
struct CmdEntry {
    name: &'static str,
    desc: &'static str,
}

fn all_commands() -> Vec<CmdEntry> {
    vec![
        CmdEntry { name: "session list", desc: "List all plans with status" },
        CmdEntry { name: "session show <id>", desc: "Show plan details and steps" },
        CmdEntry { name: "status", desc: "System status overview" },
        CmdEntry { name: "model list", desc: "Show available providers and models" },
        CmdEntry { name: "model set <target>", desc: "Switch LLM provider and model" },
        CmdEntry { name: "logs --follow", desc: "Tail agent logs in real-time" },
        CmdEntry { name: "skills list", desc: "List installed skills" },
        CmdEntry { name: "doctor", desc: "Diagnose configuration issues" },
        CmdEntry { name: "session resume <id>", desc: "Resume a paused plan" },
        CmdEntry { name: "session delete <id>", desc: "Delete a plan" },
        CmdEntry { name: "skills run <name>", desc: "Run a skill by name" },
        CmdEntry { name: "config get <key>", desc: "Get configuration value" },
    ]
}

fn filter_commands(query: &str) -> Vec<CmdEntry> {
    let q = query.to_lowercase();
    all_commands()
        .into_iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q)
        })
        .collect()
}

/// 全局 App 状态（spike 验证用）
struct App {
    input_mode: InputMode,
    /// 命令面板
    cmd_query: String,
    cmd_cursor: usize,
    cmd_filtered: Vec<CmdEntry>,
    /// 聊天输入
    chat_input: TextArea<'static>,
    /// 历史
    history: Vec<String>,
    history_idx: isize,
    /// 审批
    approval_focus: ApprovalFocus,
    /// Tab
    active_tab: usize,
    tab_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalFocus {
    ApproveOnce,
    ApproveAll,
    Deny,
    DenyBlock,
}

impl App {
    fn new() -> Self {
        let mut chat_input = TextArea::default();
        chat_input.set_placeholder_text("Type a message...");
        chat_input.set_max_histories(100);

        Self {
            input_mode: InputMode::Chat,
            cmd_query: String::new(),
            cmd_cursor: 0,
            cmd_filtered: all_commands(),
            chat_input,
            history: vec![
                "分析 src/agent.rs 架构".to_string(),
                "帮我写一个冒泡排序".to_string(),
                "运行 cargo test".to_string(),
            ],
            history_idx: -1,
            approval_focus: ApprovalFocus::ApproveOnce,
            active_tab: 0,
            tab_count: 3,
        }
    }

    fn apply_key_chat(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_mode = InputMode::CommandPalette;
                self.cmd_query.clear();
                self.cmd_filtered = all_commands();
                self.cmd_cursor = 0;
                return true;
            }
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_mode = InputMode::TabSwitch;
                return true;
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_mode = InputMode::Approval;
                self.approval_focus = ApprovalFocus::ApproveOnce;
                return true;
            }
            KeyCode::Up if key.modifiers == KeyModifiers::NONE && self.history_idx < (self.history.len() as isize - 1) => {
                self.history_idx += 1;
                if let Some(h) = self.history.get(self.history_idx as usize) {
                    self.chat_input.select_all();
                    self.chat_input.insert_str(h);
                }
                return true;
            }
            KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                if self.history_idx > 0 {
                    self.history_idx -= 1;
                    if let Some(h) = self.history.get(self.history_idx as usize) {
                        self.chat_input.select_all();
                        self.chat_input.insert_str(h);
                    }
                } else if self.history_idx == 0 {
                    self.history_idx = -1;
                    self.chat_input.select_all();
                    self.chat_input.select_all(); self.chat_input.delete_str(9999);
                }
                return true;
            }
            _ => false,
        }
    }

    fn apply_key_palette(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Chat;
                return true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_mode = InputMode::Chat;
                return true;
            }
            KeyCode::Down => {
                let max = self.cmd_filtered.len().saturating_sub(1);
                self.cmd_cursor = self.cmd_cursor.saturating_add(1).min(max);
                return true;
            }
            KeyCode::Up => {
                self.cmd_cursor = self.cmd_cursor.saturating_sub(1);
                return true;
            }
            KeyCode::Enter => {
                // 执行命令
                self.input_mode = InputMode::Chat;
                if let Some(cmd) = self.cmd_filtered.get(self.cmd_cursor) {
                    self.chat_input.select_all();
                    self.chat_input.insert_str(&format!("/{}", cmd.name));
                }
                return true;
            }
            KeyCode::Char(c) => {
                self.cmd_query.push(c);
                self.cmd_filtered = filter_commands(&self.cmd_query);
                self.cmd_cursor = 0;
                return true;
            }
            KeyCode::Backspace => {
                self.cmd_query.pop();
                self.cmd_filtered = filter_commands(&self.cmd_query);
                self.cmd_cursor = self.cmd_cursor.min(self.cmd_filtered.len().saturating_sub(1));
                return true;
            }
            _ => false,
        }
    }

    fn apply_key_approval(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Chat;
                return true;
            }
            KeyCode::Tab => {
                // 循环切换焦点
                self.approval_focus = match self.approval_focus {
                    ApprovalFocus::ApproveOnce => ApprovalFocus::ApproveAll,
                    ApprovalFocus::ApproveAll => ApprovalFocus::Deny,
                    ApprovalFocus::Deny => ApprovalFocus::DenyBlock,
                    ApprovalFocus::DenyBlock => ApprovalFocus::ApproveOnce,
                };
                return true;
            }
            KeyCode::Enter => {
                // 确认当前焦点选项
                self.input_mode = InputMode::Chat;
                return true;
            }
            KeyCode::Left | KeyCode::Right => {
                // ← → 切换焦点
                let dirs = &[
                    ApprovalFocus::ApproveOnce,
                    ApprovalFocus::ApproveAll,
                    ApprovalFocus::Deny,
                    ApprovalFocus::DenyBlock,
                ];
                let cur = self.approval_focus;
                let idx = dirs.iter().position(|&x| x == cur).unwrap_or(0);
                let new_idx = if key.code == KeyCode::Left {
                    (idx + dirs.len() - 1) % dirs.len()
                } else {
                    (idx + 1) % dirs.len()
                };
                self.approval_focus = dirs[new_idx];
                return true;
            }
            _ => false,
        }
    }

    fn apply_key_tab_switch(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match key.code {
            KeyCode::Esc | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_mode = InputMode::Chat;
                return true;
            }
            KeyCode::Left => {
                self.active_tab = self.active_tab.saturating_sub(1);
                return true;
            }
            KeyCode::Right => {
                self.active_tab = (self.active_tab + 1) % self.tab_count;
                return true;
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Chat;
                return true;
            }
            _ => false,
        }
    }

    fn apply_key(&mut self, key: crossterm::event::KeyEvent) {
        let consumed = match self.input_mode {
            InputMode::Chat => self.apply_key_chat(key),
            InputMode::CommandPalette => self.apply_key_palette(key),
            InputMode::Approval => self.apply_key_approval(key),
            InputMode::TabSwitch => self.apply_key_tab_switch(key),
        };

        // 未消费的键传给 TextArea（仅在 Chat 模式）
        if !consumed && self.input_mode == InputMode::Chat {
            // tui-textarea 自己处理剩余按键
        }
    }
}

/// 渲染 Chat 模式
fn render_chat(frame: &mut Frame, area: Rect, app: &App) {
    let [title, chat, input, status] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("⚡ Rupoo — Chat Mode", Style::default().fg(Color::Cyan)))),
        title,
    );

    frame.render_widget(
        Paragraph::new("Chat history would appear here...\n\nUse Ctrl+P to open command palette\nUse Ctrl+A to open approval dialog\nUse Ctrl+T to switch tabs")
            .block(Block::new().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray))),
        chat,
    );

    // 渲染 TextArea
    frame.render_widget(&app.chat_input, input);

    frame.render_widget(
        Paragraph::new(Line::from(format!(
            "[{:?}] ⏳ Ready | ↑↓ History | Ctrl+P: Commands | Ctrl+A: Approval | Ctrl+T: Tabs | q: Quit",
            app.input_mode
        ))),
        status,
    );
}

/// 渲染命令面板
fn render_palette(frame: &mut Frame, area: Rect, app: &App) {
    // 全屏遮罩
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    // 居中面板
    let pw = 60u16.min(area.width.saturating_sub(4));
    let ph = 16u16.min(area.height.saturating_sub(6));
    let panel = Rect::new(
        area.x + (area.width.saturating_sub(pw)) / 2,
        area.y + (area.height.saturating_sub(ph)) / 2,
        pw,
        ph,
    );

    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Line::from(vec![
                Span::raw("▸ "),
                Span::styled("Command Palette", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw("   [ESC: Close | ↑↓: Select | Enter: Execute]"),
            ])),
        panel,
    );

    // 搜索框
    let input_rect = Rect::new(panel.x + 1, panel.y + 2, panel.width.saturating_sub(2), 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::raw(format!("> {}", app.cmd_query)))),
        input_rect,
    );
    frame.render_widget(
        Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
        input_rect,
    );

    // 命令列表
    for (i, cmd) in app.cmd_filtered.iter().enumerate() {
        let row_y = panel.y + 3 + i as u16;
        if row_y >= panel.y + panel.height.saturating_sub(1) {
            break;
        }
        let row_rect = Rect::new(panel.x + 1, row_y, panel.width.saturating_sub(2), 1);

        let is_selected = i == app.cmd_cursor;
        let row_style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default().fg(Color::White)
        };

        let line = Line::from(vec![
            if is_selected {
                Span::styled("▶ ", Style::default().fg(Color::Cyan))
            } else {
                Span::raw("  ")
            },
            Span::styled(cmd.name, if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            }),
            Span::raw("  "),
            Span::styled(cmd.desc, Style::default().fg(Color::DarkGray)),
        ]);

        frame.render_widget(
            Paragraph::new(Text::from(line)).style(row_style),
            row_rect,
        );
    }
}

/// 渲染审批弹窗
fn render_approval(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    let pw = 50u16.min(area.width.saturating_sub(4));
    let ph = 12u16.min(area.height.saturating_sub(8));
    let panel = Rect::new(
        area.x + (area.width.saturating_sub(pw)) / 2,
        area.y + (area.height.saturating_sub(ph)) / 2,
        pw,
        ph,
    );

    frame.render_widget(
        Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Line::from(Span::styled("⚠ Tool Approval", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)))),
        panel,
    );

    let buttons = vec![
        (ApprovalFocus::ApproveOnce, "[Approve Once]"),
        (ApprovalFocus::ApproveAll, "[Approve All]"),
        (ApprovalFocus::Deny, "[Deny]"),
        (ApprovalFocus::DenyBlock, "[Deny+Block]"),
    ];

    let btn_w = (panel.width - 2) / 4;
    let btn_y = panel.y + panel.height - 3;

    for (i, (focus, label)) in buttons.iter().enumerate() {
        let bx = panel.x + 1 + (i as u16) * btn_w;
        let btn_rect = Rect::new(bx, btn_y, btn_w - 1, 1);
        let style = if *focus == app.approval_focus {
            Style::default().fg(Color::Yellow).bg(Color::DarkGray).add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(*label, style))),
            btn_rect,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Tool: "),
            Span::styled("task_shell_start", Style::default().fg(Color::White)),
        ])),
        Rect::new(panel.x + 2, panel.y + 2, panel.width - 4, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Cmd: "),
            Span::styled("cargo build --release", Style::default().fg(Color::Yellow)),
        ])),
        Rect::new(panel.x + 2, panel.y + 3, panel.width - 4, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from("← → Switch focus  |  Tab: Cycle  |  Enter: Confirm  |  Esc: Cancel")),
        Rect::new(panel.x + 1, panel.y + panel.height - 2, panel.width - 2, 1),
    );
}

/// 渲染 Tab 切换
fn render_tab_switch(frame: &mut Frame, area: Rect, app: &App) {
    let tab_names = ["plan_abc123", "plan_def456", "plan_ghi789"];

    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(Color::Black)),
        area,
    );

    let total_w: u16 = tab_names.iter().map(|n| n.len() as u16 + 4).sum::<u16>() + 4;
    let start_x = area.x + (area.width.saturating_sub(total_w)) / 2;
    let mut x = start_x;

    for (i, name) in tab_names.iter().enumerate() {
        let w = name.len() as u16 + 4;
        let tab_rect = Rect::new(x, area.y + area.height / 2 - 1, w, 3);
        let is_active = i == app.active_tab;

        let border_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        frame.render_widget(
            Block::new()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(if is_active { "●" } else { "○" }),
            tab_rect,
        );

        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(*name))),
            tab_rect,
        );

        x += w;
    }

    frame.render_widget(
        Paragraph::new(Line::from("← → Select tab  |  Enter: Switch  |  Esc: Cancel")),
        Rect::new(area.x + 2, area.y + area.height - 3, area.width - 4, 1),
    );
}

fn main() {
    use ratatui::{backend::CrosstermBackend, Terminal};
    use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};

    let stdout = stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    // 启用 bracketed paste（可选）
    let _ = crossterm::execute!(io::stdout(), EnableBracketedPaste);

    let mut app = App::new();

    loop {
        terminal.draw(|f| {
            let area = f.area();
            match app.input_mode {
                InputMode::Chat => render_chat(f, area, &app),
                InputMode::CommandPalette => render_palette(f, area, &app),
                InputMode::Approval => render_approval(f, area, &app),
                InputMode::TabSwitch => render_tab_switch(f, area, &app),
            }
        }).unwrap();

        use crossterm::event::{read, Event, KeyEventKind};

        match read().unwrap() {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // 全局退出键
                if key.code == crossterm::event::KeyCode::Char('q')
                    && app.input_mode == InputMode::Chat
                {
                    break;
                }

                app.apply_key(key);
            }
            Event::Paste(ref text) if app.input_mode == InputMode::Chat => {
                app.chat_input.insert_str(text);
            }
            _ => {}
        }
    }

    let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
}
