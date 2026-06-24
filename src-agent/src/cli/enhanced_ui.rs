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

use console::Term;
use owo_colors::OwoColorize;
use unicode_width::UnicodeWidthStr;

use super::theme;

// Magic number constants
/// Default separator width for horizontal rules.
const SEPARATOR_WIDTH: usize = 60;

// ═══════════════════════════════════════════════════════════════════════════
// Terminal helpers
// ═══════════════════════════════════════════════════════════════════════════

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

/// Print a thick separator
pub fn thick_hbar(width: Option<usize>) {
    let t = theme::current();
    let w = width.unwrap_or(terminal_width().max(SEPARATOR_WIDTH));
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
pub fn header_bar(version: &str, _model: Option<&str>, memory_mb: Option<f64>, show_help: bool) {
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
    let content_width = content.chars().count(); // Use char count, not unicode width
    let _padding = width.saturating_sub(content_width + 4); // Reserved for future alignment

    // Top border
    println!("┌{}┐", "─".repeat(width - 2));

    // Content line
    println!("│{} │", content);

    // Bottom border
    println!("└{}┘", "─".repeat(width - 2));
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
    let ctx_pct = (ctx_tokens * 100).checked_div(ctx_budget).unwrap_or(0);

    let parts = [
        format!(" Tokens: {}", total.color(t.dim)),
        format!(" │ in: {} ", in_str.color(t.user_med)),
        format!(" │ out: {} ", out_str.color(t.ai_accent)),
        format!(" │ ctx: {ctx_pct}% "),
        format!(
            " │ 深度搜索: {} ",
            if hybrid_search {
                "ON".color(t.user_med)
            } else {
                "OFF".color(t.dim)
            }
        ),
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
        let top = format!(
            "┌─ {} {} ─{} ─┐",
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
            println!(
                "{} {} {}",
                "│".color(t.border),
                display_args.color(t.tool_dim),
                " ".repeat(self.width.saturating_sub(display_args.len() + 3))
            );
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
        println!(
            "{} {} {}",
            "│".color(t.border),
            display.color(t.tool_dim),
            " ".repeat(self.width.saturating_sub(display.len() + 3))
        );
    }

    /// End the tool frame
    pub fn end(&self, success: bool, duration_s: Option<f64>) {
        let t = theme::current();
        let status = if success { "✅ done" } else { "⚠️ failed" };
        let duration = duration_s
            .map(|d| format!(" ({:.1}s)", d))
            .unwrap_or_default();
        let status_line = format!(
            "{} {} {}",
            status.color(if success { t.user_med } else { t.error }),
            duration.color(t.dim),
            " ".repeat(self.width.saturating_sub(status.len() + duration.len() + 1))
        );

        let bottom = format!("└{}──{}┘", "─".repeat(3), status_line);
        println!("{}", bottom.color(t.border));
        println!();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Message Bubbles
// ═══════════════════════════════════════════════════════════════════════════

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

    println!(
        "{} {}",
        "📋".color(t.ai_header),
        "任务列表".color(t.ai_header).bold()
    );
    thick_hbar(Some(width.min(SEPARATOR_WIDTH)));

    for (i, (task, status)) in tasks.iter().enumerate() {
        let icon = status.icon();
        let color = status.color(&t);
        let line = format!("{} {}. {}", icon.color(color), i + 1, task);

        if line.len() < width - 4 {
            println!("{} {}", "│".color(t.border), line.color(color));
        } else {
            println!(
                "{} {}",
                "│".color(t.border),
                format!("{} {}... ", icon.color(color), i + 1).color(color)
            );
        }
    }

    println!();
}



