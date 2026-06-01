//! Markdown → ANSI renderer with syntect code highlighting.
//!
//! Uses "base16-ocean.dark" theme for dark-terminal code highlighting.
//! Inline colors align with the output.rs palette.
//!
//! Supports: headers, bold, inline code, code blocks, lists, tables,
//! blockquotes, task lists, horizontal rules, links.

use owo_colors::OwoColorize;
use std::io::Write;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthStr;

use std::sync::OnceLock;

use super::theme;

/// Get the current terminal width. Falls back to 80 if detection fails.
fn terminal_width() -> usize {
    console::Term::stdout().size().1 as usize
}

struct Highlighter {
    ss: SyntaxSet,
    ts: ThemeSet,
}

impl Highlighter {
    fn new() -> Self {
        let ss = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        Self { ss, ts }
    }
}

static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();

fn get_highlighter() -> &'static Highlighter {
    HIGHLIGHTER.get_or_init(Highlighter::new)
}

/// Get the current syntect theme name from the active theme.
fn current_code_theme() -> &'static str {
    theme::current().code_theme
}

// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════

/// Render a complete markdown string.
#[allow(dead_code)]
pub fn render_markdown(text: &str) {
    let mut ctx = RenderContext::default();
    let mut dummy_stream_lines = 0;
    for line in text.lines() {
        render_line(line, &mut ctx, &mut dummy_stream_lines);
    }
    flush_table(&mut ctx);
    if ctx.in_code && !ctx.code_buffer.is_empty() {
        erase_streaming_code_lines(dummy_stream_lines);
        flush_code_block(&ctx.code_buffer, &ctx.code_lang);
    }
    let _ = std::io::stdout().flush();
}

/// Streaming state tracker.
pub struct StreamState {
    buffer: String,
    ctx: RenderContext,
    /// Track how many streaming code lines we've printed (for erase-on-complete)
    stream_code_lines: usize,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            ctx: RenderContext::default(),
            stream_code_lines: 0,
        }
    }
}

/// Render a streaming chunk incrementally.
pub fn render_stream_chunk(chunk: &str, state: &mut StreamState) {
    state.buffer.push_str(chunk);

    while let Some(pos) = state.buffer.find('\n') {
        let line = state.buffer[..pos].to_string();
        state.buffer = state.buffer[pos + 1..].to_string();
        process_stream_line(&line, &mut state.ctx, &mut state.stream_code_lines);
    }
}

/// Flush remaining buffer.
pub fn flush_stream(state: &mut StreamState) {
    if !state.buffer.is_empty() {
        let line = std::mem::take(&mut state.buffer);
        process_stream_line(&line, &mut state.ctx, &mut state.stream_code_lines);
    }
    flush_table(&mut state.ctx);
    if state.ctx.in_code && !state.ctx.code_buffer.is_empty() {
        // Erase streaming code lines and rewrite with full highlighting
        erase_streaming_code_lines(state.stream_code_lines);
        state.stream_code_lines = 0;
        flush_code_block(&state.ctx.code_buffer, &state.ctx.code_lang);
        state.ctx.code_buffer.clear();
        state.ctx.code_lang.clear();
        state.ctx.in_code = false;
    }
    let _ = std::io::stdout().flush();
}

/// Erase previously printed streaming code lines using ANSI escape sequences.
fn erase_streaming_code_lines(count: usize) {
    if count == 0 {
        return;
    }
    // Move up N lines and clear from cursor to end of screen
    for _ in 0..count {
        print!("\x1b[1A\x1b[2K");
    }
    let _ = std::io::stdout().flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Render context — tracks table/code/quote state across lines
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct RenderContext {
    in_code: bool,
    code_lang: String,
    code_buffer: Vec<String>,
    needs_header: bool,
    /// Accumulated table rows (before rendering)
    table_rows: Vec<Vec<String>>,
    /// Inside a table (accumulating rows)
    in_table: bool,
    /// Blockquote depth (number of > prefixes)
    quote_depth: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// Line rendering — shared by both batch and streaming
// ═══════════════════════════════════════════════════════════════════════════

fn process_stream_line(line: &str, ctx: &mut RenderContext, stream_code_lines: &mut usize) {
    if ctx.needs_header {
        super::output::assistant_header();
        ctx.needs_header = false;
    }
    render_line(line, ctx, stream_code_lines);
}

fn render_line(line: &str, ctx: &mut RenderContext, stream_code_lines: &mut usize) {
    // Code block toggle
    if line.starts_with("```") {
        if ctx.in_code {
            // Erase streaming placeholder lines, then render final highlighted block
            erase_streaming_code_lines(*stream_code_lines);
            *stream_code_lines = 0;
            flush_code_block(&ctx.code_buffer, &ctx.code_lang);
            ctx.code_buffer.clear();
            ctx.code_lang.clear();
            ctx.in_code = false;
        } else {
            // Flush any pending table before entering code
            flush_table(ctx);
            ctx.in_code = true;
            ctx.code_lang = line.trim_start_matches('`').trim().to_string();
        }
        return;
    }

    // Inside code block — print streaming placeholder (no line numbers)
    if ctx.in_code {
        ctx.code_buffer.push(line.to_string());
        let t = theme::current();
        // Simple placeholder: dim │ prefix + plain text (fast, no syntect)
        let display = if line.len() > 200 { format!("{}…", &line[..197]) } else { line.to_string() };
        println!("  {} {}", "│".color(t.border), display.color(t.dim));
        *stream_code_lines += 1;
        let _ = std::io::stdout().flush();
        return;
    }

    // Horizontal rule: ---, ***, ___ (3+ chars, only that on the line)
    let trimmed = line.trim();
    if trimmed.len() >= 3
        && (trimmed.chars().all(|c| c == '-') || trimmed.chars().all(|c| c == '*') || trimmed.chars().all(|c| c == '_'))
    {
        flush_table(ctx);
        println!("  {}", "─".repeat(50).color(theme::current().border));
        return;
    }

    // Empty line — flush table, reset quote
    if line.is_empty() {
        flush_table(ctx);
        ctx.quote_depth = 0;
        println!();
        return;
    }

    // Blockquote: > text
    if trimmed.starts_with('>') {
        flush_table(ctx);
        let content = trimmed.trim_start_matches('>').trim_start();
        if content.is_empty() {
            println!("  {}", "▎".color(theme::current().dim));
        } else {
            println!("  {} {}", "▎".color(theme::current().ai_accent), render_inline(content));
        }
        ctx.quote_depth = 1;
        return;
    }

    // Table detection: line contains | and has at least 2 pipes
    if trimmed.contains('|') && trimmed.matches('|').count() >= 2 {
        // Is this a separator row? (|---|---|)
        let cleaned = trimmed.replace('|', "").replace('-', "").replace(' ', "").replace(':', "");
        if cleaned.is_empty() {
            // Separator — skip it, we'll compute column widths from data rows
            ctx.in_table = true;
            return;
        }
        // Data row
        let cells: Vec<String> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if !ctx.in_table {
            ctx.in_table = true;
        }
        ctx.table_rows.push(cells);
        return;
    } else if ctx.in_table {
        // End of table
        flush_table(ctx);
    }

    // Headers
    if line.starts_with("### ") {
        println!("  {}", line.trim_start_matches('#').trim().color(theme::current().ai_header).bold());
        return;
    }
    if line.starts_with("## ") {
        println!("  {}", line.trim_start_matches('#').trim().color(theme::current().ai_header).bold());
        return;
    }
    if line.starts_with("# ") {
        println!("{}", line.trim_start_matches('#').trim().color(theme::current().ai_header).bold());
        return;
    }

    // Task list: - [ ] or - [x]
    if trimmed.starts_with("- [ ] ") || trimmed.starts_with("* [ ] ") {
        let content = &trimmed[6..];
        println!("  {} {}", "☐".color(theme::current().dim), render_inline(content));
        return;
    }
    if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") || trimmed.starts_with("* [x] ") || trimmed.starts_with("* [X] ") {
        let content = &trimmed[6..];
        println!("  {} {}", "☑".color(theme::current().user_bright), render_inline(content));
        return;
    }

    // Unordered list: - or * or •
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
        let content = &trimmed[2..];
        println!("  {} {}", "•".color(theme::current().ai_accent), render_inline(content));
        return;
    }

    // Ordered list: 1. 2. etc
    if let Some(dot_pos) = trimmed.find(". ") {
        let prefix = &trimmed[..dot_pos];
        if prefix.chars().all(|c| c.is_ascii_digit()) {
            let content = &trimmed[dot_pos + 2..];
            println!("  {}{}", format!("{}.", prefix).color(theme::current().ai_accent), render_inline(&format!(" {}", content)));
            return;
        }
    }

    // Regular text
    println!("  {}", render_inline(line));
    let _ = std::io::stdout().flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Table rendering
// ═══════════════════════════════════════════════════════════════════════════

fn flush_table(ctx: &mut RenderContext) {
    if ctx.table_rows.is_empty() {
        ctx.in_table = false;
        return;
    }

    let rows = &ctx.table_rows;
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        ctx.table_rows.clear();
        ctx.in_table = false;
        return;
    }

    // Compute column widths (unicode-aware, strip ANSI for measurement)
    let mut col_widths = vec![0usize; col_count];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                let w = strip_ansi(cell).width();
                col_widths[i] = col_widths[i].max(w);
            }
        }
    }

    // Cap column widths so total doesn't exceed terminal width
    let total: usize = col_widths.iter().sum::<usize>() + col_count * 3 + 1;
    let max_width = terminal_width().saturating_sub(4).max(40);
    if total > max_width {
        let scale = (max_width as f64) / (total as f64);
        for w in &mut col_widths {
            *w = (*w as f64 * scale).floor() as usize;
        }
    }

    // Top border: ┌─────┬─────┐
    print!("  ");
    for (i, w) in col_widths.iter().enumerate() {
        if i == 0 {
            print!("{}", format!("┌{}┬", "─".repeat(*w + 2)).color(theme::current().border));
        } else if i == col_count - 1 {
            print!("{}", format!("{}┐", "─".repeat(*w + 2)).color(theme::current().border));
        } else {
            print!("{}", format!("{}┬", "─".repeat(*w + 2)).color(theme::current().border));
        }
    }
    println!();

    for (row_idx, row) in rows.iter().enumerate() {
        // Data row: │ cell │ cell │
        print!("  ");
        for (i, w) in col_widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let cell_visible = strip_ansi(cell);
            let cell_width = cell_visible.width();
            let padding = w.saturating_sub(cell_width);
            let rendered = render_inline(cell);
            print!("{} {}{} {}", "│".color(theme::current().border), rendered, " ".repeat(padding), "");
        }
        println!("{}", "│".color(theme::current().border));

        // Separator after header row
        if row_idx == 0 {
            print!("  ");
            for (i, w) in col_widths.iter().enumerate() {
                if i == 0 {
                    print!("{}", format!("├{}┼", "─".repeat(*w + 2)).color(theme::current().border));
                } else if i == col_count - 1 {
                    print!("{}", format!("{}┤", "─".repeat(*w + 2)).color(theme::current().border));
                } else {
                    print!("{}", format!("{}┼", "─".repeat(*w + 2)).color(theme::current().border));
                }
            }
            println!();
        }
    }

    // Bottom border: └─────┴─────┘
    print!("  ");
    for (i, w) in col_widths.iter().enumerate() {
        if i == 0 {
            print!("{}", format!("└{}┴", "─".repeat(*w + 2)).color(theme::current().border));
        } else if i == col_count - 1 {
            print!("{}", format!("{}┘", "─".repeat(*w + 2)).color(theme::current().border));
        } else {
            print!("{}", format!("{}┴", "─".repeat(*w + 2)).color(theme::current().border));
        }
    }
    println!();

    ctx.table_rows.clear();
    ctx.in_table = false;
}

/// Strip ANSI escape sequences to measure visible width.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c.is_ascii_alphabetic() || c == 'm' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\x07' || c == '\x1b' {
                        break;
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Inline formatting
// ═══════════════════════════════════════════════════════════════════════════

/// Render inline formatting: bold, inline code, links.
fn render_inline(text: &str) -> String {
    let mut result = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    let mut in_bold = false;

    while i < bytes.len() {
        let Some(ch) = text[i..].chars().next() else {
            break;
        };

        // Inline code: `code`
        if ch == '`' {
            let mut code_content = String::new();
            i += 1; // skip opening `
            while i < bytes.len() {
                let Some(c) = text[i..].chars().next() else {
                    break;
                };
                if c == '`' {
                    i += c.len_utf8();
                    break;
                }
                code_content.push(c);
                i += c.len_utf8();
            }
            result.push_str(&code_content.color(theme::current().user_bright).to_string());
            continue;
        }

        // Bold: **text**
        if ch == '*' && i + 1 < bytes.len() && text[i..].starts_with("**") {
            i += 2;
            in_bold = !in_bold;
            continue;
        }

        // Link: [text](url)
        if ch == '[' {
            if let Some(end_bracket) = text[i..].find(']') {
                let link_text = &text[i + 1..i + end_bracket];
                let after_bracket = i + end_bracket + 1;
                if after_bracket < bytes.len() && text[after_bracket..].starts_with('(') {
                    if let Some(end_paren) = text[after_bracket..].find(')') {
                        let url = &text[after_bracket + 1..after_bracket + end_paren];
                        result.push_str(&format!(
                            "{}{}",
                            link_text.underline().color(theme::current().ai_accent).to_string(),
                            format!("({})", url).color(theme::current().dim).to_string(),
                        ));
                        i = after_bracket + end_paren + 1;
                        continue;
                    }
                }
            }
            // Not a valid link — output [
            result.push('[');
            i += 1;
            continue;
        }

        if in_bold {
            result.push_str(&ch.to_string().bold().to_string());
        } else {
            result.push(ch);
        }
        i += ch.len_utf8();
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Code highlighting
// ═══════════════════════════════════════════════════════════════════════════

/// Flush a complete code block with syntax highlighting and line numbers.
fn flush_code_block(lines: &[String], lang: &str) {
    let hl = get_highlighter();
    let syntax = if lang.is_empty() {
        hl.ss.find_syntax_by_first_line(&lines.first().unwrap_or(&String::new()))
            .unwrap_or_else(|| hl.ss.find_syntax_plain_text())
    } else {
        hl.ss.find_syntax_by_token(lang)
            .unwrap_or_else(|| hl.ss.find_syntax_plain_text())
    };

    let lang_label = if lang.is_empty() { "code" } else { lang };
    let width = lines.iter().map(|l| l.len()).max().unwrap_or(20).min(80);

    // Top border: ┌─ rust ─────
    println!(
        "  {}{}{}{}",
        "┌─ ".color(theme::current().tool_accent),
        lang_label.color(theme::current().tool_accent).bold(),
        " ",
        "─".repeat(width.saturating_sub(lang_label.len() + 4)).color(theme::current().border),
    );

    let mut highlighter = HighlightLines::new(syntax, &hl.ts.themes[current_code_theme()]);

    for (i, line) in lines.iter().enumerate() {
        let line_no = format!("{:>3}", i + 1);
        let ranges = highlighter.highlight_line(line, &hl.ss).unwrap_or_default();
        let mut highlighted = String::new();
        for (style, text) in ranges {
            let fg = style.foreground;
            highlighted.push_str(&format!(
                "\x1b[38;2;{};{};{}m{}\x1b[0m",
                fg.r, fg.g, fg.b, text
            ));
        }
        println!("  {} {}", line_no.color(theme::current().dim), highlighted);
    }

    // Bottom border: └──────
    println!("  {}", format!("└{}", "─".repeat(width.saturating_sub(1).min(78))).color(theme::current().border));
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_width_returns_value() {
        let w = terminal_width();
        assert!(w >= 40, "terminal_width returned {w}, expected >= 40");
    }
}
