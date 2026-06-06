//! Enhanced UI components for Rupoo CLI
//! 
//! Features:
//! - Status bars (Header/Footer)
//! - Progress bars
//! - Markdown code highlighting
//! - Tool execution frames
//! - Real-time status updates
//! 
//! Note: Some components are reserved for future use and may not be currently active.
#![allow(dead_code)]

use owo_colors::OwoColorize;
use std::io::Write;
use unicode_width::UnicodeWidthStr;
use console::Term;
use std::sync::{Arc, Mutex};

use super::theme;

// ═══════════════════════════════════════════════════════════════════════════
// Terminal helpers
// ═══════════════════════════════════════════════════════════════════════════

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

fn terminal_height() -> usize {
    Term::stdout().size().0 as usize
}

// ═══════════════════════════════════════════════════════════════════════════
// Status Bar Components
// ═══════════════════════════════════════════════════════════════════════════

/// Print a horizontal line separator with optional text
pub fn hbar(width: Option<usize>) {
    let t = theme::current();
    let w = width.unwrap_or(terminal_width().max(40));
    println!("{}", "─".repeat(w).color(t.border));
}

/// Print a thick separator
pub fn thick_hbar(width: Option<usize>) {
    let t = theme::current();
    let w = width.unwrap_or(terminal_width().max(40));
    println!("{}", "━".repeat(w).color(t.border));
}

// ═══════════════════════════════════════════════════════════════════════════
// Header Status Bar
// ═══════════════════════════════════════════════════════════════════════════

/// Print header status bar (like Trae)
/// Example:
/// ┌──────────────────────────────────────────────────────┐
/// │ 🎡 rupoo v0.3.1  │ Faster, Steadier, Lighter, Your Trusted Sidekick. │
/// └──────────────────────────────────────────────────────────────────────────────┘
pub fn header_bar(
    version: &str,
    _model: Option<&str>,
    memory_mb: Option<f64>,
    show_help: bool,
) {
    let _t = theme::current(); // Reserved for future theme-based styling
    let width = terminal_width().max(40);
    
    // Official slogan
    const SLOGAN: &str = "Faster, Steadier, Lighter, Your Trusted Sidekick.";
    
    // Build content line first to calculate actual width
    let mut content = format!(" 🎡 rupoo {}", version);
    
    // Show slogan instead of model
    content.push_str(&format!("  │  {}", SLOGAN));
    
    if let Some(mem) = memory_mb {
        content.push_str(&format!("  │  内存: {:.0}MB ", mem));
    }
    
    if show_help {
        content.push_str("  │  [?] ");
    }
    
    // Calculate padding (subtract 4 for borders and spacing)
    let content_width = content.chars().count();  // Use char count, not unicode width
    let _padding = width.saturating_sub(content_width + 4); // Reserved for future alignment
    
    // Top border
    println!("┌{}┐", "─".repeat(width - 2));
    
    // Content line
    println!("│{} │", content);
    
    // Bottom border  
    println!("└{}┘", "─".repeat(width - 2));
}

/// Simple header without fancy formatting
pub fn simple_header(version: &str, _model: &str) {
    let t = theme::current();
    const SLOGAN: &str = "Faster, Steadier, Lighter, Your Trusted Sidekick.";
    println!();
    println!(
        "{} {} {} {}",
        "🎡".color(t.ai_header),
        "rupoo".color(t.ai_header).bold(),
        format!("v{}", version).color(t.dim),
        format!("│ {}", SLOGAN).color(t.dim),
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Footer Status Bar
// ═══════════════════════════════════════════════════════════════════════════

/// Print footer status bar with token usage and settings
/// Example:
/// Tokens: 2,340 │ 输入: 1.2k │ 输出: 2.5k │ 缓存: ♻️ 890 │ 模型: deepseek-chat
pub fn footer_bar(
    token_in: u64,
    token_out: u64,
    ctx_tokens: usize,
    ctx_budget: usize,
    model: &str,
    hybrid_search: bool,
) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    // Top border
    print!("{}", "┌".color(t.border));
    print!("{}", "─".repeat(width - 2).color(t.border));
    println!("{}", "┐".color(t.border));
    
    // Content line
    print!("{}", "│".color(t.border));
    
    // Format token info
    let in_str = format_tokens(token_in);
    let out_str = format_tokens(token_out);
    let total = format_tokens(token_in + token_out);
    
    // Context usage
    let ctx_pct = if ctx_budget > 0 {
        (ctx_tokens * 100) / ctx_budget
    } else {
        0
    };
    
    let _ctx_color = if ctx_pct > 80 { // Reserved for future color-coded display
        t.error
    } else if ctx_pct > 50 {
        t.think
    } else {
        t.dim
    };
    
    let parts = vec![
        format!(" Tokens: {}", total.color(t.dim)),
        format!(" │ 输入: {} ", in_str.color(t.user_med)),
        format!(" │ 输出: {} ", out_str.color(t.ai_accent)),
        format!(" │ ctx: {:.0}% ", ctx_pct as f64 / 100.0),
        format!(" │ 深度搜索: {} ", if hybrid_search { "ON".color(t.user_med) } else { "OFF".color(t.dim) }),
        format!(" │ {} ", model.color(t.ai_header)),
    ];
    
    let content = parts.join("");
    print!("{}", content);
    
    // Padding
    let padding = width.saturating_sub(content.width() + 2);
    print!("{}", " ".repeat(padding));
    println!("{}", "│".color(t.border));
    
    // Bottom border
    println!("{}", format!("└{}┘", "─".repeat(width - 2)).color(t.border));
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        tokens.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Progress Bar
// ═══════════════════════════════════════════════════════════════════════════

/// Animated progress bar component
/// Example:
/// [████████████░░░░░░░░░░░░░░] 45%  分析中...
pub struct ProgressBar {
    width: usize,
    prefix: String,
}

impl ProgressBar {
    pub fn new(prefix: &str) -> Self {
        let width = terminal_width().max(40).saturating_sub(20);
        Self {
            width: width.max(20),
            prefix: prefix.to_string(),
        }
    }
    
    pub fn print(&self, progress: f32, message: &str) {
        let t = theme::current();
        let filled = ((progress / 100.0) * self.width as f32) as usize;
        let empty = self.width - filled;
        
        let bar = format!(
            "{}[{}{}] {:.0}%  {}",
            format!("\x1b[38;2;{};{};{}m", t.border.0, t.border.1, t.border.2),
            "█".repeat(filled).color(t.user_med),
            "░".repeat(empty).color(t.dim),
            progress,
            message.color(t.dim),
        );
        
        print!("\r{}", bar);
        let _ = std::io::stdout().flush();
    }
    
    pub fn finish(self, message: &str) {
        let t = theme::current();
        let bar = format!(
            "{}[{}{}] 100%  {}",
            format!("\x1b[38;2;{};{};{}m", t.border.0, t.border.1, t.border.2),
            "█".repeat(self.width).color(t.user_med),
            "░".repeat(0).color(t.dim),
            message.color(t.user_med),
        );
        println!("\r{}", bar);
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new("")
    }
}

/// Simple inline progress indicator
/// Example:
/// ████████░░░░░░░░░░░░░░░░░ 40%
pub fn inline_progress(progress: f32, width: usize) -> String {
    let t = theme::current();
    let filled = ((progress / 100.0) * width as f32) as usize;
    let empty = width - filled;
    
    format!(
        "{}[{}{}] {:.0}%",
        "".color(t.border),
        "█".repeat(filled).color(t.user_med),
        "░".repeat(empty).color(t.dim),
        progress,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool Execution Frame
// ═══════════════════════════════════════════════════════════════════════════

/// Tool execution frame with border
/// Example:
/// ┌─ 🔧 exec ────────────────────────────────────────┐
/// │ $ cargo build                                     │
/// │ Compiling rupoo v0.3.1                           │
/// │ ████████████████░░░░░░░░░░░░  45%               │
/// └───────────────────────────────────────────────────┘
pub struct ToolFrame {
    tool_name: String,
    width: usize,
}

impl ToolFrame {
    pub fn new(tool_name: &str) -> Self {
        let width = terminal_width().max(40);
        Self {
            tool_name: tool_name.to_string(),
            width,
        }
    }
    
    /// Start the tool frame
    pub fn start(&self, args: &str) {
        let t = theme::current();
        let top = format!("┌─ {} {} ─{} ─┐", 
            "🔧".to_string().color(t.tool_accent),
            self.tool_name.color(t.tool_accent).bold(),
            "─".repeat(self.width.saturating_sub(self.tool_name.len() + 8))
        );
        println!("{}", top.color(t.border));
        
        // Print args if provided
        if !args.is_empty() {
            let display_args = if args.chars().count() > self.width - 6 {
                // UTF-8 safe truncation by character count
                let truncated: String = args.chars().take(self.width - 10).collect();
                format!("{}…", truncated)
            } else {
                args.to_string()
            };
            println!("{} {} {}", "│".color(t.border), display_args.color(t.tool_dim), " ".repeat(self.width.saturating_sub(display_args.len() + 3)));
            println!("{} {}", "│".color(t.border), " ".repeat(self.width - 2));
        }
    }
    
    /// Print a line inside the frame
    pub fn println(&self, line: &str) {
        let t = theme::current();
        let display = if line.chars().count() > self.width - 4 {
            // UTF-8 safe truncation by character count
            let truncated: String = line.chars().take(self.width - 7).collect();
            format!("{}…", truncated)
        } else {
            line.to_string()
        };
        println!("{} {} {}", "│".color(t.border), display.color(t.tool_dim), " ".repeat(self.width.saturating_sub(display.len() + 3)));
    }
    
    /// Print progress bar inside the frame
    pub fn progress(&self, progress: f32, message: &str) {
        let t = theme::current();
        let bar = inline_progress(progress, self.width.saturating_sub(8));
        let msg = format!("{} {}", bar, message);
        println!("{} {}", "│".color(t.border), msg.color(t.tool_dim));
    }
    
    /// End the tool frame
    pub fn end(&self, success: bool, duration_s: Option<f64>) {
        let t = theme::current();
        let status = if success { "✅ done" } else { "⚠️ failed" };
        let duration = duration_s.map(|d| format!(" ({:.1}s)", d)).unwrap_or_default();
        let status_line = format!("{} {} {}", status.color(if success { t.user_med } else { t.error }), duration.color(t.dim), " ".repeat(self.width.saturating_sub(status.len() + duration.len() + 1)));
        
        let bottom = format!("└{}──{}┘", "─".repeat(3), status_line);
        println!("{}", bottom.color(t.border));
        println!();
    }
}

/// Convenience function for quick tool output
pub fn tool_block<F>(tool_name: &str, args: &str, duration_s: Option<f64>, f: F)
where
    F: FnOnce(&ToolFrame)
{
    let frame = ToolFrame::new(tool_name);
    frame.start(args);
    f(&frame);
    frame.end(true, duration_s);
}

// ═══════════════════════════════════════════════════════════════════════════
// Message Bubbles
// ═══════════════════════════════════════════════════════════════════════════

/// Print a styled message bubble for assistant
pub fn assistant_bubble(content: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    for (i, line) in content.lines().enumerate() {
        let prefix = if i == 0 { "💬" } else { " " };
        let display = if line.len() > width - 6 {
            format!("{}…", &line[..width - 9])
        } else {
            line.to_string()
        };
        println!("{} {}", prefix.color(t.ai_header), display.color(t.ai_accent));
    }
}

/// Print a styled message bubble for user
pub fn user_bubble(content: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    for line in content.lines() {
        let display = if line.len() > width - 6 {
            format!("{}…", &line[..width - 9])
        } else {
            line.to_string()
        };
        // Right-aligned
        let padding = width.saturating_sub(display.len() + 3);
        println!("{}{} {}", " ".repeat(padding), "▸".color(t.user_med), display.color(t.user_bright).bold());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Task List
// ═══════════════════════════════════════════════════════════════════════════

/// Task status for display
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "○",
            TaskStatus::Running => "▶",
            TaskStatus::Completed => "✓",
            TaskStatus::Failed => "✗",
        }
    }
    
    pub fn color(&self, t: &theme::Theme) -> owo_colors::Rgb {
        match self {
            TaskStatus::Pending => t.dim,
            TaskStatus::Running => t.think,
            TaskStatus::Completed => t.user_med,
            TaskStatus::Failed => t.error,
        }
    }
}

/// Convert StepStatus to TaskStatus
pub fn step_status_to_task_status(status: &rupoo::task::StepStatus) -> TaskStatus {
    match status {
        rupoo::task::StepStatus::Pending => TaskStatus::Pending,
        rupoo::task::StepStatus::Running => TaskStatus::Running,
        rupoo::task::StepStatus::Completed => TaskStatus::Completed,
        rupoo::task::StepStatus::Failed => TaskStatus::Failed,
        rupoo::task::StepStatus::WaitingForInput => TaskStatus::Pending,
    }
}

/// Print a task list
pub fn task_list(tasks: &[(String, TaskStatus)]) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    println!("{} {}", "📋".color(t.ai_header), "任务列表".color(t.ai_header).bold());
    thick_hbar(Some(width.min(50)));
    
    for (i, (task, status)) in tasks.iter().enumerate() {
        let icon = status.icon();
        let color = status.color(&t);
        let line = format!("{} {}. {}", icon.color(color), i + 1, task);
        
        if line.len() < width - 4 {
            println!("{} {}", "│".color(t.border), line.color(color));
        } else {
            println!("{} {}", "│".color(t.border), format!("{} {}... ", icon.color(color), i + 1).color(color));
        }
    }
    
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Welcome Screen
// ═══════════════════════════════════════════════════════════════════════════

/// Enhanced welcome screen with status bar
pub fn welcome_enhanced(version: &str, model: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    println!();
    header_bar(version, Some(model), None, true);
    println!();
    
    println!("  {} Commands: /help │ /new │ /theme │ /config", "›".color(t.dim));
    
    thick_hbar(Some(width));
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Keyboard Shortcuts Help
// ═══════════════════════════════════════════════════════════════════════════

/// Print keyboard shortcuts help
pub fn shortcuts_help() {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    println!();
    println!("{} {}", "⌨️".color(t.ai_header), "快捷键".color(t.ai_header).bold());
    thick_hbar(Some(width.min(50)));
    
    let shortcuts = vec![
        ("Ctrl+C", "中断当前任务"),
        ("Ctrl+L", "清屏"),
        ("Ctrl+R", "搜索历史"),
        ("Tab", "自动补全"),
        ("↑/↓", "切换历史命令"),
        ("Esc", "取消输入"),
        ("#", "引用上下文文件"),
    ];
    
    for (key, desc) in shortcuts {
        let line = format!("  {}  {}", key.color(t.user_med).bold(), desc.color(t.dim));
        if line.len() < width - 4 {
            println!("{}", line);
        }
    }
    
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Status Bar System
// ═══════════════════════════════════════════════════════════════════════════

/// LLM connection status
#[derive(Debug, Clone, Copy)]
pub enum LlmStatus {
    Online,
    Offline,
    Connecting,
    Error,
}

impl LlmStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            LlmStatus::Online => "✓",
            LlmStatus::Offline => "✗",
            LlmStatus::Connecting => "◐",
            LlmStatus::Error => "⚠",
        }
    }
    
    pub fn color(&self) -> owo_colors::Rgb {
        let t = theme::current();
        match self {
            LlmStatus::Online => t.user_med,
            LlmStatus::Offline => t.error,
            LlmStatus::Connecting => t.think,
            LlmStatus::Error => t.error,
        }
    }
}

/// Status information for the bottom status bar
#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub session_name: String,
    pub llm_status: LlmStatus,
    pub model: String,
    pub tokens_used: usize,
    pub tokens_budget: usize,
    pub network_latency_ms: Option<u64>,
    pub thinking: bool,
    pub current_tool: Option<String>,
}

impl Default for StatusInfo {
    fn default() -> Self {
        Self {
            session_name: "default".to_string(),
            llm_status: LlmStatus::Offline,
            model: "not configured".to_string(),
            tokens_used: 0,
            tokens_budget: 60000,
            network_latency_ms: None,
            thinking: false,
            current_tool: None,
        }
    }
}

/// Thread-safe status holder for real-time updates
pub type SharedStatus = Arc<Mutex<StatusInfo>>;

/// Print the bottom status bar
pub fn status_bar(status: &StatusInfo) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    // Build status items
    let mut parts = Vec::new();
    
    // Session name
    parts.push(format!("📝 {}", status.session_name.color(t.user_med)));
    
    // LLM Status
    let llm_icon = status.llm_status.icon();
    let llm_color = status.llm_status.color();
    parts.push(format!("│ {} {}", llm_icon.color(llm_color), status.model.color(llm_color)));
    
    // Token usage
    let token_pct = if status.tokens_budget > 0 {
        (status.tokens_used * 100) / status.tokens_budget
    } else {
        0
    };
    
    let token_color = if token_pct > 80 {
        t.error
    } else if token_pct > 50 {
        t.think
    } else {
        t.dim
    };
    
    parts.push(format!("│ 令牌: {}/{}", 
        status.tokens_used.color(token_color), 
        status.tokens_budget.color(t.dim)
    ));
    
    // Network latency
    if let Some(latency) = status.network_latency_ms {
        let latency_color = if latency < 100 {
            t.user_med
        } else if latency < 500 {
            t.think
        } else {
            t.error
        };
        parts.push(format!("│ ⚡ {}ms", latency.color(latency_color)));
    }
    
    // Thinking indicator
    if status.thinking {
        let tool_str = status.current_tool.as_ref()
            .map(|t| format!(" {}", t))
            .unwrap_or_default();
        parts.push(format!("│ 🧠 思考中{}", tool_str.color(t.think)));
    }
    
    // Join all parts
    let content = parts.join(" ");
    let content_width = content.width();
    let padding = width.saturating_sub(content_width);
    
    // Save cursor position
    print!("\x1b[s");
    
    // Move to bottom line
    let height = terminal_height();
    print!("\x1b[{};1H", height);
    
    // Print status bar with reverse video
    print!("\x1b[7m"); // Reverse video on
    print!("{}{}", content, " ".repeat(padding));
    print!("\x1b[27m"); // Reverse video off
    
    // Restore cursor position
    print!("\x1b[u");
    
    let _ = std::io::stdout().flush();
}

/// Clear the status bar area
pub fn clear_status_bar() {
    let width = terminal_width().max(40);
    
    // Save cursor position
    print!("\x1b[s");
    
    // Move to bottom line
    let height = terminal_height();
    print!("\x1b[{};1H", height);
    
    // Clear line
    print!("{}", " ".repeat(width));
    
    // Restore cursor position
    print!("\x1b[u");
    
    let _ = std::io::stdout().flush();
}

/// Print a temporary status message
pub fn temporary_status(message: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    // Save cursor position
    print!("\x1b[s");
    
    // Move to bottom line
    let height = terminal_height();
    print!("\x1b[{};1H", height);
    
    // Print message with reverse video
    print!("\x1b[7m");
    print!("{} {}", "⏳".color(t.think), message.color(t.think));
    print!("{}", " ".repeat(width.saturating_sub(message.len() + 3)));
    print!("\x1b[27m");
    
    let _ = std::io::stdout().flush();
    
    // Small delay to show the message
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    // Clear and restore
    clear_status_bar();
}
