//! ANSI output layer — direct terminal rendering without TUI framework.
//! Colors come from the runtime-switchable Theme (cli::theme).
//!
//! Note: Some functions are reserved for future UI enhancements.

use chrono::{DateTime, Utc};
use console::Term;
use owo_colors::OwoColorize;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use super::enhanced_ui;
use super::theme;
use rupoo::{FileAction, FileChangeInfo, LayoutMode};

// Thread-local storage for the active tool frame
thread_local! {
    static TOOL_FRAME: std::cell::RefCell<Option<enhanced_ui::ToolFrame>> =
        const { std::cell::RefCell::new(None) };
}

// ═══════════════════════════════════════════════════════════════════════════
// Terminal helpers
// ═══════════════════════════════════════════════════════════════════════════

// Cached terminal width. Re-queried lazily on first use and whenever
// `refresh_terminal_width` is called (e.g. on SIGWINCH / Event::Resize),
// so the streaming render path doesn't issue a TIOCGWINSZ ioctl per line.
static CACHED_WIDTH: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn terminal_width() -> usize {
    let w = CACHED_WIDTH.load(Ordering::Relaxed);
    if w != 0 {
        return w;
    }
    let live = Term::stdout().size().1 as usize;
    if live != 0 {
        CACHED_WIDTH.store(live, Ordering::Relaxed);
        live
    } else {
        0 // caller applies .max(40) for piped/non-tty output
    }
}

/// Re-query the terminal width (call after a resize event).
pub(crate) fn refresh_terminal_width() {
    let live = Term::stdout().size().1 as usize;
    if live != 0 {
        CACHED_WIDTH.store(live, Ordering::Relaxed);
    }
}

/// Greedy word-wrap `text` to fit `content_width` display columns.
///
/// Breaks on ASCII spaces; for a run longer than `content_width` (e.g. CJK
/// text with no spaces) it falls back to character-level breaking so nothing
/// overflows the terminal. Returns plain-text content lines (no prefix);
/// the caller prepends the per-line prefix/indent.
///
/// Uses `UnicodeWidthStr::width()` so double-width (CJK) characters are
/// counted correctly.
pub(crate) fn wrap_content(text: &str, content_width: usize) -> Vec<String> {
    let cw = content_width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;

    for raw in text.split(' ') {
        // Skip empty tokens produced by runs of spaces.
        if raw.is_empty() {
            continue;
        }
        let word_w = raw.width();
        if cur.is_empty() {
            // First word on the line — break it internally if too long.
            if word_w > cw {
                push_wrapped_word(&mut lines, &mut cur, &mut cur_w, raw, cw);
            } else {
                cur.push_str(raw);
                cur_w = word_w;
            }
        } else {
            let sep = 1usize; // the joining space
            if cur_w + sep + word_w > cw {
                // Word doesn't fit — flush current line and start a new one.
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
                if word_w > cw {
                    push_wrapped_word(&mut lines, &mut cur, &mut cur_w, raw, cw);
                } else {
                    cur.push_str(raw);
                    cur_w = word_w;
                }
            } else {
                cur.push(' ');
                cur.push_str(raw);
                cur_w += sep + word_w;
            }
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Break a single over-long word (no internal spaces) into display-width
/// bounded pieces, pushing all but the last piece as complete lines and
/// leaving the last piece in `cur`.
fn push_wrapped_word(
    lines: &mut Vec<String>,
    cur: &mut String,
    cur_w: &mut usize,
    word: &str,
    cw: usize,
) {
    let mut piece = String::new();
    let mut piece_w = 0usize;
    for ch in word.chars() {
        let c = ch.width().unwrap_or(0);
        if piece_w + c > cw && !piece.is_empty() {
            lines.push(std::mem::take(&mut piece));
            piece_w = 0;
        }
        piece.push(ch);
        piece_w += c;
    }
    if !piece.is_empty() {
        *cur = piece;
        *cur_w = piece_w;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cursor style
// ═══════════════════════════════════════════════════════════════════════════

/// Set cursor style: blinking bar with theme cursor color.
pub fn set_cursor_style_bar() {
    let t = theme::current();
    let c = t.cursor;
    // DECSCUSR 5 = blinking bar cursor
    // OSC 12    = set cursor color
    print!("\x1b[5 q\x1b]12;#{:02x}{:02x}{:02x}\x1b\\", c.0, c.1, c.2);
    let _ = std::io::stdout().flush();
}

/// Reset cursor to terminal default (blinking block, default color).
pub fn reset_cursor_style() {
    // DECSCUSR 0 = default cursor shape
    // OSC 112   = reset cursor color
    print!("\x1b[0 q\x1b]112\x1b\\");
    let _ = std::io::stdout().flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Primitives
// ═══════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub fn separator() {
    let t = theme::current();
    println!("{}", "─".repeat(60).color(t.border));
}

#[allow(dead_code)]
pub fn thick_separator() {
    let t = theme::current();
    println!("{}", "━".repeat(60).color(t.border));
}

// ═══════════════════════════════════════════════════════════════════════════
// Messages — User (right-aligned) / Assistant (left-aligned)
// ═══════════════════════════════════════════════════════════════════════════

/// Print a single line right-aligned with `▸` prefix.
fn print_right_aligned(line: &str, width: usize) {
    if line.is_empty() {
        println!();
        return;
    }
    let t = theme::current();
    let marker = "▸ ".color(t.user_med).bold().to_string();
    let content = line.color(t.user_bright).bold().to_string();
    let plain = format!("▸ {}", line);
    let vw = plain.width();
    if vw >= width {
        println!("{}{}", marker, content);
    } else {
        let padding = width - vw;
        println!("{}{}{}", " ".repeat(padding), marker, content);
    }
}

/// Print a right-aligned separator line.
fn print_right_separator(text: &str, width: usize) {
    let t = theme::current();
    let max_w = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.width())
        .max()
        .unwrap_or(10);
    let sep_len = (max_w + 4).min(width);
    let sep_pad = width.saturating_sub(sep_len);
    println!(
        "{}{}",
        " ".repeat(sep_pad),
        "─".repeat(sep_len).color(t.user_dim)
    );
}

/// Erase the input prompt line and replace with a right-aligned user message.
pub fn replace_readline_with_user_message(text: &str, ts: Option<DateTime<Utc>>) {
    let width = terminal_width().max(40);

    // Calculate how many lines the user's input wraps across
    let prompt_w = 2; // "> " width
    let input_w = text.width();
    let total_w = prompt_w + input_w;
    let input_lines = total_w.max(1).div_ceil(width);

    // Erase just the input line(s) — "> " line
    for _ in 0..input_lines {
        print!("\x1b[1A\x1b[2K");
    }
    let _ = std::io::stdout().flush();

    // Print a dimmed timestamp line above the right-aligned user message
    if let Some(t) = ts {
        println!("{}", timestamp_prefix(Some(t)));
    }

    // Print right-aligned user message
    for line in text.lines() {
        print_right_aligned(line, width);
    }

    // Right-aligned thin separator
    print_right_separator(text, width);
    println!();
    let _ = std::io::stdout().flush();
}

/// Print a right-aligned user message (without erasing readline).
pub fn user_message(text: &str) {
    let width = terminal_width().max(40);
    println!();
    for line in text.lines() {
        print_right_aligned(line, width);
    }
    print_right_separator(text, width);
    println!();
}

#[allow(dead_code)]
pub fn assistant_footer(
    duration_s: f64,
    token_in: u64,
    token_out: u64,
    ctx_tokens: usize,
    ctx_budget: usize,
) {
    let t = theme::current();
    println!();
    let ctx_pct = (ctx_tokens * 100).checked_div(ctx_budget).unwrap_or(0);
    let ctx_str = format!("{:.1}k/{}k", ctx_tokens as f64 / 1000.0, ctx_budget / 1000);

    let ctx_display = if ctx_pct > 80 {
        ctx_str.color(t.error).to_string()
    } else if ctx_pct > 50 {
        ctx_str.color(t.think).to_string()
    } else {
        ctx_str.color(t.user_med).to_string()
    };

    println!(
        "{} {:.1}s │ {} in │ {} out │ ctx {}",
        "⏱".color(t.dim),
        duration_s,
        token_in.to_string().color(t.dim),
        token_out.to_string().color(t.dim),
        ctx_display,
    );
    thick_separator();
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool cards — reserved for future plan-mode tool panels
// ═══════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
pub fn tool_call_start(tool_name: &str, args: &str) {
    let frame = enhanced_ui::ToolFrame::new(tool_name);
    frame.start(args);

    // Store the frame for later use
    TOOL_FRAME.with(|f| {
        *f.borrow_mut() = Some(frame);
    });
}

#[allow(dead_code)]
pub fn tool_result(result: &str, truncated: bool) {
    TOOL_FRAME.with(|f| {
        if let Some(frame) = &mut *f.borrow_mut() {
            let lines: Vec<&str> = result.lines().collect();
            let max_lines = 8;
            let display_lines: Vec<&str> = lines.iter().take(max_lines).copied().collect();

            for line in &display_lines {
                frame.println(line);
            }

            if truncated || lines.len() > max_lines {
                let extra = if lines.len() > max_lines {
                    lines.len() - max_lines
                } else {
                    0
                };
                frame.println(&format!("... ({} more lines)", extra));
            }
        }
    });
}

#[allow(dead_code)]
pub fn tool_call_end(done: bool, duration_s: Option<f64>) {
    TOOL_FRAME.with(|f| {
        if let Some(frame) = f.borrow_mut().take() {
            frame.end(done, duration_s);
        }
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Thinking (Coze style)
// ═══════════════════════════════════════════════════════════════════════════

pub fn thinking_spinner(frame: usize, tool_name: Option<&str>) {
    let t = theme::current();
    let spinner = match frame % 10 {
        0 => "⠋",
        1 => "⠙",
        2 => "⠹",
        3 => "⠸",
        4 => "⠼",
        5 => "⠴",
        6 => "⠦",
        7 => "⠧",
        8 => "⠇",
        _ => "⠏",
    };
    let dots = match frame % 4 {
        0 => "○ ○ ○",
        1 => "● ○ ○",
        2 => "● ● ○",
        _ => "● ● ●",
    };

    let msg = match tool_name {
        Some(name) => format!("Calling {}…", name),
        None => "Thinking…".to_string(),
    };

    eprint!(
        "\r  {} {} {}   ",
        spinner.color(t.think).bold(),
        msg.color(t.think),
        dots.color(t.ai_header)
    );
    let _ = std::io::stderr().flush();
}

pub fn clear_spinner() {
    eprint!("\r{}\r", " ".repeat(70));
    let _ = std::io::stderr().flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Errors
// ═══════════════════════════════════════════════════════════════════════════

pub fn error(msg: &str) {
    let t = theme::current();
    println!();
    println!(
        "{} {}",
        "✗ Error:".color(t.error).bold(),
        msg.color(t.error)
    );
    println!();
}

pub fn system(msg: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    let border = "│".color(t.dim).to_string();
    let cw = width.saturating_sub(2); // "│ " prefix (same for all lines)
    for seg in wrap_content(msg, cw) {
        println!("{} {}", border, seg.color(t.dim));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 方案 C — 混合自适应布局渲染函数
// ═══════════════════════════════════════════════════════════════════════════

/// Display LLM reasoning summary (Work mode).
/// Single line: "⟳ 正在分析 src/error.rs..."
pub fn thinking_summary(text: &str) {
    let t = theme::current();
    eprint!(
        "\r{} {}          ",
        "⟳".color(t.think),
        text.color(t.think).italic()
    );
    let _ = std::io::stderr().flush();
}

/// Clear the current thinking summary line.
pub fn clear_thinking_summary() {
    eprint!("\r{}\r", " ".repeat(80));
    let _ = std::io::stderr().flush();
}

/// Display phase progress bar (Work mode).
/// Example: "████░░░░░░ 重构错误处理  40%"
pub fn phase_progress(phase_name: &str, percentage: u8) {
    let t = theme::current();
    if percentage == 0 {
        // 0% = indeterminate progress (no plan structure, just showing current step)
        println!("  {} {}", "⟳".color(t.think), phase_name.color(t.ai_header),);
    } else {
        let bar_width = 20;
        let filled = ((percentage as usize) * bar_width) / 100;
        let bar = format!(
            "{}{}",
            "█".repeat(filled.clamp(0, bar_width)),
            "░".repeat(bar_width.saturating_sub(filled))
        );
        println!(
            "  {}  {}  {}%",
            bar.color(t.think),
            phase_name.color(t.ai_header),
            percentage
        );
    }
}

/// Display a single file change (Work mode).
/// Example: "~ src/error.rs  ++--"
pub fn file_change(info: &FileChangeInfo) {
    let t = theme::current();
    let (icon, color) = match info.action {
        FileAction::Modified => ("~", t.think),
        FileAction::Created => ("+", t.user_med),
        FileAction::Deleted => ("-", t.error),
    };
    let line_info = format!(" +{}/-{}", info.lines_added, info.lines_removed);
    println!(
        "  {} {} {}",
        icon.color(color).bold(),
        info.path.color(t.ai_accent),
        line_info.color(t.dim),
    );
}

/// Render a dimmed `[HH:MM:SS]` prefix for a message timestamp.
/// Returns an empty string when no timestamp is available (e.g. messages
/// loaded from older history that predate the field).
fn timestamp_prefix(ts: Option<DateTime<Utc>>) -> String {
    match ts {
        Some(t) => format!("\x1b[2m[{}]\x1b[0m ", t.format("%H:%M:%S")),
        None => String::new(),
    }
}

/// Display a chat bubble (Chat mode).
/// Role=User: right-aligned with ▸ marker
/// Role=Assistant: left-aligned with subtle border
pub fn chat_bubble(text: &str, role: rupoo::MessageRole, ts: Option<DateTime<Utc>>) {
    let t = theme::current();
    let width = terminal_width().max(40);
    let stamp = timestamp_prefix(ts);
    match role {
        rupoo::MessageRole::User => {
            for line in text.lines() {
                print_right_aligned(line, width);
            }
            print_right_separator(text, width);
            println!();
        }
        rupoo::MessageRole::Assistant => {
            // Print with subtle border — just a thin left bar
            let border = "╎".color(t.ai_header).dimmed().to_string();
            // First line reserves room for "╎ " + stamp + " "; continuations
            // align under the content with a 2-space indent ("╎  ").
            let cw_first = width.saturating_sub(3 + stamp.width());
            let segs = wrap_content(text, cw_first.max(1));
            for (i, seg) in segs.iter().enumerate() {
                if i == 0 {
                    println!("{} {}{}", border, stamp, seg);
                } else {
                    println!("{}  {}", border, seg);
                }
            }
        }
        _ => {
            let border = "│".color(t.dim).to_string();
            let cw_first = width.saturating_sub(3 + stamp.width());
            let segs = wrap_content(text, cw_first.max(1));
            for (i, seg) in segs.iter().enumerate() {
                if i == 0 {
                    println!("{} {}{}", border, stamp, seg.color(t.dim));
                } else {
                    println!("{}  {}", border, seg.color(t.dim));
                }
            }
        }
    }
}

/// Show layout mode banner when switching modes.
pub fn layout_mode_banner(mode: LayoutMode) {
    let t = theme::current();
    match mode {
        LayoutMode::Chat => {
            println!();
            println!("  {}", "─ ◇ 对话模式 ─".color(t.dim).dimmed());
            println!();
        }
        LayoutMode::Work => {
            println!();
            println!(
                "  {} {}",
                "▣".color(t.think).bold(),
                "开始工作".color(t.think).bold()
            );
            println!(
                "  {} 自动检测到开发需求，进入工作计划模式",
                "⟳".color(t.dim)
            );
            println!();
        }
        LayoutMode::Summary => {
            // No banner for summary — it transitions silently
        }
    }
}

/// Display a compact task completion summary.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn summary_block(
    summary: &str,
    files_changed: u32,
    lines_added: u32,
    lines_removed: u32,
    duration_s: f64,
    token_in: u64,
    token_out: u64,
    passed: bool,
) {
    let t = theme::current();
    let status_icon = if passed { "✅" } else { "⚠️" };
    println!();
    println!("  {} {}", status_icon, summary.color(t.ai_header));
    if files_changed > 0 {
        println!(
            "  {} {} files changed, +{}/-{} lines",
            "│".color(t.dim),
            files_changed,
            lines_added,
            lines_removed,
        );
    }
    println!(
        "  {} {:.1}s · {} in · {} out",
        "│".color(t.dim),
        duration_s,
        token_in,
        token_out,
    );
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Welcome
// ═══════════════════════════════════════════════════════════════════════════

pub fn welcome(version: &str, model: &str) {
    let t = theme::current();
    println!();

    // Use enhanced header bar (slogan is shown in header now)
    enhanced_ui::header_bar(version, Some(model), None, false);

    if model == "not configured" {
        println!(
            "  {} {} Run: {} or {}",
            "⚠".to_string().yellow(),
            "LLM not configured.".to_string().yellow(),
            "rupoo config set api_key.anthropic <key>".color(t.ai_accent),
            "rupoo doctor".color(t.ai_accent),
        );
    }
    println!(
        "  {} Theme: {}",
        "│".color(t.dim),
        t.name.color(t.ai_accent)
    );
    println!();
    println!("  {} Quick Actions:", "›".color(t.dim));
    println!("     /read <path>   - Read file (e.g., /read ./src/main.rs)");
    println!("     /cmd <cmd>     - Execute command (e.g., /cmd ls -la)");
    println!("     /search <query> - Web search (e.g., /search Rust async)");
    println!("     /ls [path]    - List directory");
    println!();
    println!(
        "  {} /help for full commands │ /tools to list tools",
        "›".color(t.dim)
    );
    println!();
}

/// Print footer status bar with token usage
#[allow(dead_code)]
pub fn footer(
    token_in: u64,
    token_out: u64,
    ctx_tokens: usize,
    ctx_budget: usize,
    model: &str,
    hybrid_search: bool,
) {
    enhanced_ui::footer_bar(
        token_in,
        token_out,
        ctx_tokens,
        ctx_budget,
        model,
        hybrid_search,
    );
}

/// Print plan task list
pub fn plan_task_list(tasks: &[(String, rupoo::task::StepStatus)]) {
    let converted: Vec<(String, enhanced_ui::TaskStatus)> = tasks
        .iter()
        .map(|(name, status)| {
            (
                name.clone(),
                enhanced_ui::step_status_to_task_status(status),
            )
        })
        .collect();
    enhanced_ui::task_list(&converted);
}

#[cfg(test)]
mod tests {
    use super::wrap_content;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn wraps_on_word_boundary() {
        let lines = wrap_content("the quick brown fox jumps", 10);
        assert_eq!(lines, vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn cjk_wraps_by_char_width() {
        // Each CJK char is 2 display columns; width 8 → 4 chars per line.
        let lines = wrap_content("中文中文中文中文", 8);
        assert_eq!(lines, vec!["中文中文", "中文中文"]);
        // No single line may exceed the content width.
        for l in &lines {
            assert!(l.width() <= 8);
        }
    }

    #[test]
    fn long_word_breaks_char_by_char() {
        let lines = wrap_content("abcdefghijklmnop", 5);
        assert_eq!(lines, vec!["abcde", "fghij", "klmno", "p"]);
        for l in &lines {
            assert!(l.width() <= 5);
        }
    }

    #[test]
    fn empty_text_yields_one_empty_line() {
        assert_eq!(wrap_content("", 20), vec![""]);
    }

    #[test]
    fn respects_content_width_exactly() {
        let lines = wrap_content("hello world foo bar", 11);
        assert_eq!(lines, vec!["hello world", "foo bar"]);
        for l in &lines {
            assert!(l.width() <= 11);
        }
    }
}
