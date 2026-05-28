//! ANSI output layer — direct terminal rendering without TUI framework.
//!
//! Color palette (dark-terminal optimized):
//!   ┌──────────────┬────────────┬──────────────────────────────┐
//!   │ Element      │ Color      │ Usage                        │
//!   ├──────────────┼────────────┼──────────────────────────────┤
//!   │ User text    │ #7ee787    │ Bright green — right-aligned │
//!   │ User accent  │ #3fb950    │ Medium green — separators    │
//!   │ User dim     │ #238636    │ Dark green — faded lines     │
//!   │ AI header    │ #58a6ff    │ Soft blue — assistant label  │
//!   │ AI accent    │ #79c0ff    │ Light blue — inline code     │
//!   │ Tool card    │ #d2a8ff    │ Purple — tool borders/name   │
//!   │ Tool result  │ #8b949e    │ Silver — tool output         │
//!   │ Thinking     │ #e3b341    │ Amber — spinner/text         │
//!   │ Error        │ #f85149    │ Red — errors                 │
//!   │ Dim text     │ #484f58    │ Muted gray — footer/line no  │
//!   │ Separator    │ #30363d    │ Border gray — lines          │
//!   └──────────────┴────────────┴──────────────────────────────┘
//!
//! Reference: GitHub Dark Dimmed + Catppuccin Mocha fusion

use owo_colors::OwoColorize;
use std::io::Write;
use unicode_width::UnicodeWidthStr;
use console::Term;

// ═══════════════════════════════════════════════════════════════════════════
// Custom color constants (RGB)
// ═══════════════════════════════════════════════════════════════════════════

use owo_colors::Rgb;

const USER_BRIGHT: Rgb = Rgb(0x7E, 0xE7, 0x87);   // #7ee787
const USER_MED: Rgb   = Rgb(0x3F, 0xB9, 0x50);     // #3fb950
const USER_DIM: Rgb   = Rgb(0x23, 0x86, 0x36);     // #238636
const AI_HEADER: Rgb  = Rgb(0x58, 0xA6, 0xFF);     // #58a6ff
const AI_ACCENT: Rgb  = Rgb(0x79, 0xC0, 0xFF);     // #79c0ff
const TOOL_PURPLE: Rgb = Rgb(0xD2, 0xA8, 0xFF);    // #d2a8ff
const TOOL_DIM: Rgb   = Rgb(0x8B, 0x95, 0x9E);     // #8b949e
const THINK_AMBER: Rgb = Rgb(0xE3, 0xB3, 0x41);    // #e3b341
const ERR_RED: Rgb    = Rgb(0xF8, 0x51, 0x49);     // #f85149
const DIM_GRAY: Rgb   = Rgb(0x48, 0x4F, 0x58);     // #484f58
const BORDER_GRAY: Rgb = Rgb(0x30, 0x36, 0x3D);    // #30363d

// ═══════════════════════════════════════════════════════════════════════════
// Terminal helpers
// ═══════════════════════════════════════════════════════════════════════════

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

// ═══════════════════════════════════════════════════════════════════════════
// Cursor style
// ═══════════════════════════════════════════════════════════════════════════

/// Set cursor style: green blinking bar (thin vertical line).
pub fn set_cursor_style_bar() {
    // DECSCUSR 5 = blinking bar cursor
    // OSC 12    = set cursor color to green
    print!("\x1b[5 q\x1b]12;#3fb950\x1b\\");
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
    println!("{}", "─".repeat(60).color(BORDER_GRAY));
}

pub fn thick_separator() {
    println!("{}", "━".repeat(60).color(BORDER_GRAY));
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
    let marker = "▸ ".color(USER_MED).bold().to_string();
    let content = line.color(USER_BRIGHT).bold().to_string();
    let plain = format!("▸ {}", line);
    let vw = plain.width();
    if vw >= width {
        println!("{}{}", marker, content);
    } else {
        let padding = width - vw;
        print!("{}{}{}", " ".repeat(padding), marker, content);
        println!();
    }
}

/// Print a right-aligned separator line.
fn print_right_separator(text: &str, width: usize) {
    let max_w = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.width())
        .max()
        .unwrap_or(10);
    let sep_len = (max_w + 4).min(width);
    let sep_pad = width.saturating_sub(sep_len);
    println!("{}{}", " ".repeat(sep_pad), "─".repeat(sep_len).color(USER_DIM));
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
    println!("{} {}", "◂".color(AI_HEADER), "Rupoo".color(AI_HEADER).bold());
    thick_separator();
}

pub fn assistant_footer(duration_s: f64, token_in: u64, token_out: u64, ctx_tokens: usize, ctx_budget: usize) {
    println!();
    let ctx_pct = if ctx_budget > 0 { ctx_tokens * 100 / ctx_budget } else { 0 };
    let ctx_str = format!("{:.1}k/{}k", ctx_tokens as f64 / 1000.0, ctx_budget / 1000);

    let ctx_display = if ctx_pct > 80 {
        ctx_str.color(ERR_RED).to_string()
    } else if ctx_pct > 50 {
        ctx_str.color(THINK_AMBER).to_string()
    } else {
        ctx_str.color(USER_MED).to_string()
    };

    println!(
        "{} {:.1}s │ {} in │ {} out │ ctx {}",
        "⏱".color(DIM_GRAY),
        duration_s,
        token_in.to_string().color(DIM_GRAY),
        token_out.to_string().color(DIM_GRAY),
        ctx_display,
    );
    thick_separator();
}

// ═══════════════════════════════════════════════════════════════════════════
// Tool cards (Claude Code style)
// ═══════════════════════════════════════════════════════════════════════════

pub fn tool_call_start(tool_name: &str, args: &str) {
    println!();
    let display_args = if args.len() > 80 {
        format!("{}…", &args[..77])
    } else {
        args.to_string()
    };
    println!(
        "{} {} {}({})",
        "╭─".color(TOOL_PURPLE),
        "🔧".to_string().color(TOOL_PURPLE),
        tool_name.color(TOOL_PURPLE).bold(),
        display_args.color(TOOL_PURPLE),
    );
}

pub fn tool_result(result: &str, truncated: bool) {
    let lines: Vec<&str> = result.lines().collect();
    let max_lines = 8;
    let display_lines: Vec<&str> = lines.iter().take(max_lines).copied().collect();

    for line in &display_lines {
        let display = if line.len() > 200 {
            format!("{}…", &line[..197])
        } else {
            line.to_string()
        };
        println!("{} {}", "│".color(TOOL_DIM), display.color(TOOL_DIM));
    }

    if truncated || lines.len() > max_lines {
        let extra = if lines.len() > max_lines { lines.len() - max_lines } else { 0 };
        println!("{} {}", "│".color(DIM_GRAY), format!("... ({} more lines)", extra).color(DIM_GRAY));
    }
}

pub fn tool_call_end(done: bool, duration_s: Option<f64>) {
    let status = if done { "✅ done" } else { "⏳ running" };
    let duration_str = duration_s.map(|d| format!(" ({:.1}s)", d)).unwrap_or_default();
    if done {
        println!(
            "{} {}{} {}",
            "╰─".color(BORDER_GRAY),
            status.color(USER_MED),
            duration_str.color(USER_MED),
            "─".repeat(30).color(BORDER_GRAY),
        );
    } else {
        println!(
            "{} {}{} {}",
            "╰─".color(BORDER_GRAY),
            status.color(THINK_AMBER),
            duration_str.color(THINK_AMBER),
            "─".repeat(30).color(BORDER_GRAY),
        );
    }
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Thinking (Coze style)
// ═══════════════════════════════════════════════════════════════════════════

pub fn thinking_spinner(frame: usize, tool_name: Option<&str>) {
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

    eprint!("\r  {} {} {}   ", spinner.color(THINK_AMBER).bold(), msg.color(THINK_AMBER), dots.color(AI_HEADER));
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
    println!();
    println!("{} {}", "✗ Error:".color(ERR_RED).bold(), msg.color(ERR_RED));
    println!();
}

pub fn system(msg: &str) {
    println!("{} {}", "│".color(DIM_GRAY), msg.color(DIM_GRAY));
}

// ═══════════════════════════════════════════════════════════════════════════
// Welcome
// ═══════════════════════════════════════════════════════════════════════════

pub fn welcome(version: &str, model: &str) {
    println!();
    println!("  {} {}", "Rupoo".color(AI_HEADER).bold(), format!("v{}", version).color(DIM_GRAY));
    println!("  {} {}", "Model:".color(DIM_GRAY), model.color(AI_ACCENT));
    println!();
    println!("  {} /help for commands │ /new for new session │ Ctrl+C to interrupt", "›".color(DIM_GRAY));
    println!();
    separator();
}
