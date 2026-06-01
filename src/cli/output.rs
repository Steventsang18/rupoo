//! ANSI output layer — direct terminal rendering without TUI framework.
//! Colors come from the runtime-switchable Theme (cli::theme).

use owo_colors::OwoColorize;
use std::io::Write;
use unicode_width::UnicodeWidthStr;
use console::Term;

use super::theme;

// ═══════════════════════════════════════════════════════════════════════════
// Terminal helpers
// ═══════════════════════════════════════════════════════════════════════════

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
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

pub fn separator() {
    let t = theme::current();
    println!("{}", "─".repeat(60).color(t.border));
}

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
    println!("{}{}", " ".repeat(sep_pad), "─".repeat(sep_len).color(t.user_dim));
}

/// Erase the rustyline input line(s) and replace with a right-aligned user message.
pub fn replace_readline_with_user_message(text: &str) {
    let width = terminal_width().max(40);

    // Calculate how many terminal lines the readline input occupied
    let prompt_w = 3; // "❯ " (2 chars + space, but unicode width of ❯ is 2)
    let input_w = text.width();
    let total_w = prompt_w + input_w;
    let lines_taken = (total_w.max(1) + width - 1) / width;

    // Erase readline lines: move up and clear each
    for _ in 0..lines_taken {
        print!("\x1b[1A\x1b[2K");
    }
    let _ = std::io::stdout().flush();

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

pub fn assistant_header() {
    let t = theme::current();
    println!("{} {}", "◂".color(t.ai_header), "Rupoo".color(t.ai_header).bold());
    thick_separator();
}

pub fn assistant_footer(duration_s: f64, token_in: u64, token_out: u64, ctx_tokens: usize, ctx_budget: usize) {
    let t = theme::current();
    println!();
    let ctx_pct = if ctx_budget > 0 { ctx_tokens * 100 / ctx_budget } else { 0 };
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
// Tool cards (Claude Code style)
// ═══════════════════════════════════════════════════════════════════════════

pub fn tool_call_start(tool_name: &str, args: &str) {
    let t = theme::current();
    println!();
    let display_args = if args.len() > 80 {
        format!("{}…", &args[..77])
    } else {
        args.to_string()
    };
    println!(
        "{} {} {}({})",
        "╭─".color(t.tool_accent),
        "🔧".to_string().color(t.tool_accent),
        tool_name.color(t.tool_accent).bold(),
        display_args.color(t.tool_accent),
    );
}

pub fn tool_result(result: &str, truncated: bool) {
    let t = theme::current();
    let lines: Vec<&str> = result.lines().collect();
    let max_lines = 8;
    let display_lines: Vec<&str> = lines.iter().take(max_lines).copied().collect();

    for line in &display_lines {
                let display = if line.len() > 200 {
                    format!("{}…", line.chars().take(197).collect::<String>())
                } else {
                    line.to_string()
                };
        println!("{} {}", "│".color(t.tool_dim), display.color(t.tool_dim));
    }

    if truncated || lines.len() > max_lines {
        let extra = if lines.len() > max_lines { lines.len() - max_lines } else { 0 };
        println!("{} {}", "│".color(t.dim), format!("... ({} more lines)", extra).color(t.dim));
    }
}

pub fn tool_call_end(done: bool, duration_s: Option<f64>) {
    let t = theme::current();
    let status = if done { "✅ done" } else { "⏳ running" };
    let duration_str = duration_s.map(|d| format!(" ({:.1}s)", d)).unwrap_or_default();
    if done {
        println!(
            "{} {}{} {}",
            "╰─".color(t.border),
            status.color(t.user_med),
            duration_str.color(t.user_med),
            "─".repeat(30).color(t.border),
        );
    } else {
        println!(
            "{} {}{} {}",
            "╰─".color(t.border),
            status.color(t.think),
            duration_str.color(t.think),
            "─".repeat(30).color(t.border),
        );
    }
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Thinking (Coze style)
// ═══════════════════════════════════════════════════════════════════════════

pub fn thinking_spinner(frame: usize, tool_name: Option<&str>) {
    let t = theme::current();
    let spinner = match frame % 10 {
        0 => "⠋", 1 => "⠙", 2 => "⠹", 3 => "⠸",
        4 => "⠼", 5 => "⠴", 6 => "⠦", 7 => "⠧",
        8 => "⠇", _ => "⠏",
    };
    let dots = match frame % 4 {
        0 => "○ ○ ○", 1 => "● ○ ○", 2 => "● ● ○", _ => "● ● ●",
    };

    let msg = match tool_name {
        Some(name) => format!("Calling {}…", name),
        None => "Thinking…".to_string(),
    };

    eprint!("\r  {} {} {}   ", spinner.color(t.think).bold(), msg.color(t.think), dots.color(t.ai_header));
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
    println!("{} {}", "✗ Error:".color(t.error).bold(), msg.color(t.error));
    println!();
}

pub fn system(msg: &str) {
    let t = theme::current();
    println!("{} {}", "│".color(t.dim), msg.color(t.dim));
}

// ═══════════════════════════════════════════════════════════════════════════
// Welcome
// ═══════════════════════════════════════════════════════════════════════════

pub fn welcome(version: &str, model: &str) {
    let t = theme::current();
    println!();
    println!("  {} {}", "Rupoo".color(t.ai_header).bold(), format!("v{}", version).color(t.dim));
    if model == "not configured" {
        println!("  {} {} Run: {} or {}",
            "⚠".to_string().yellow(),
            "LLM not configured.".to_string().yellow(),
            "rupoo config set api_key.anthropic <key>".color(t.ai_accent),
            "rupoo doctor".color(t.ai_accent),
        );
    } else {
        println!("  {} {}", "Model:".color(t.dim), model.color(t.ai_accent));
    }
    println!("  {} Theme: {}", "│".color(t.dim), t.name.color(t.ai_accent));
    println!();
    println!("  {} /help for commands │ /new for new session │ /theme <name> to switch", "›".color(t.dim));
    println!();
    separator();
}
