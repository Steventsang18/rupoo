//! ANSI output layer — direct terminal rendering without TUI framework.

use owo_colors::OwoColorize;
use std::io::Write;

// ═══════════════════════════════════════════════════════════════════════════
// Primitives
// ═══════════════════════════════════════════════════════════════════════════

pub fn separator() {
    println!("{}", "─".repeat(60).dimmed());
}

pub fn thick_separator() {
    println!("{}", "━".repeat(60).dimmed());
}

// ═══════════════════════════════════════════════════════════════════════════
// Messages
// ═══════════════════════════════════════════════════════════════════════════

pub fn user_message(text: &str) {
    println!();
    println!("{} {}", "You:".green().bold(), text);
    println!();
}

pub fn assistant_header() {
    println!("{} {}", "🤖".cyan(), "Rupoo".cyan().bold());
    thick_separator();
}

pub fn assistant_footer(duration_s: f64, token_in: u64, token_out: u64, ctx_tokens: usize, ctx_budget: usize) {
    println!();
    let ctx_pct = if ctx_budget > 0 { ctx_tokens * 100 / ctx_budget } else { 0 };
    let ctx_str = format!("{:.1}k/{}k", ctx_tokens as f64 / 1000.0, ctx_budget / 1000);
    
    let ctx_display = if ctx_pct > 80 {
        ctx_str.red().to_string()
    } else if ctx_pct > 50 {
        ctx_str.yellow().to_string()
    } else {
        ctx_str.green().to_string()
    };

    println!(
        "{} {:.1}s │ {} in │ {} out │ ctx {}",
        "⏱".dimmed(),
        duration_s,
        token_in.to_string().dimmed(),
        token_out.to_string().dimmed(),
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
        "╭─".magenta().dimmed(),
        "🔧".to_string().magenta(),
        tool_name.magenta().bold(),
        display_args.magenta(),
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
        println!("{} {}", "│".green().dimmed(), display.green());
    }

    if truncated || lines.len() > max_lines {
        let extra = if lines.len() > max_lines { lines.len() - max_lines } else { 0 };
        println!("{} {}", "│".dimmed(), format!("... ({} more lines)", extra).dimmed());
    }
}

pub fn tool_call_end(done: bool, duration_s: Option<f64>) {
    let status = if done { "✅ done" } else { "⏳ running" };
    let duration_str = duration_s.map(|d| format!(" ({:.1}s)", d)).unwrap_or_default();
    if done {
        println!(
            "{} {}{} {}",
            "╰─".dimmed(),
            status.green(),
            duration_str.green(),
            "─".repeat(30).dimmed(),
        );
    } else {
        println!(
            "{} {}{} {}",
            "╰─".dimmed(),
            status.yellow(),
            duration_str,
            "─".repeat(30).dimmed(),
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

    eprint!("\r  {} {} {}   ", spinner.yellow().bold(), msg.yellow(), dots.cyan().dimmed());
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
    println!("{} {}", "✗ Error:".red().bold(), msg.red());
    println!();
}

pub fn system(msg: &str) {
    println!("{} {}", "│".dimmed(), msg.dimmed());
}

// ═══════════════════════════════════════════════════════════════════════════
// Welcome
// ═══════════════════════════════════════════════════════════════════════════

pub fn welcome(version: &str, model: &str) {
    println!();
    println!("  {} {}", "Rupoo".cyan().bold(), format!("v{}", version).dimmed());
    println!("  {} {}", "Model:".dimmed(), model.cyan());
    println!();
    println!("  {} /help for commands │ /new for new session │ Ctrl+C to interrupt", "›".dimmed());
    println!();
    separator();
}
