//! Markdown → ANSI renderer with syntect code highlighting.
//!
//! Uses "base16-ocean.dark" theme for dark-terminal code highlighting.
//! Inline colors align with the output.rs palette.

use owo_colors::OwoColorize;
use std::io::Write;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use std::sync::OnceLock;

use owo_colors::Rgb;

// Palette — must match output.rs
const AI_HEADER: Rgb  = Rgb(0x58, 0xA6, 0xFF);  // #58a6ff
const AI_ACCENT: Rgb  = Rgb(0x79, 0xC0, 0xFF);  // #79c0ff
const USER_BRIGHT: Rgb = Rgb(0x7E, 0xE7, 0x87);  // #7ee787
const TOOL_PURPLE: Rgb = Rgb(0xD2, 0xA8, 0xFF);  // #d2a8ff
const DIM_GRAY: Rgb   = Rgb(0x48, 0x4F, 0x58);   // #484f58
const BORDER_GRAY: Rgb = Rgb(0x30, 0x36, 0x3D);  // #30363d

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

/// Name of the syntect theme to use (dark-terminal optimized).
const CODE_THEME: &str = "base16-ocean.dark";

// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════

/// Render a complete markdown string.
pub fn render_markdown(text: &str) {
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buffer: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.starts_with("```") {
            if in_code {
                flush_code_block(&code_buffer, &code_lang);
                code_buffer.clear();
                code_lang.clear();
                in_code = false;
            } else {
                in_code = true;
                code_lang = line.trim_start_matches('`').trim().to_string();
            }
            continue;
        }

        if in_code {
            code_buffer.push(line.to_string());
            continue;
        }

        if line.is_empty() {
            println!();
            continue;
        }

        if line.starts_with("### ") {
            println!("  {}", line.trim_start_matches('#').trim().color(AI_HEADER).bold());
            continue;
        }
        if line.starts_with("## ") {
            println!("  {}", line.trim_start_matches('#').trim().color(AI_HEADER).bold());
            continue;
        }
        if line.starts_with("# ") {
            println!("{}", line.trim_start_matches('#').trim().color(AI_HEADER).bold());
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
            let content = &trimmed[2..];
            println!("  {} {}", "•".color(AI_ACCENT), render_inline(content));
            continue;
        }

        println!("  {}", render_inline(line));
    }

    if in_code && !code_buffer.is_empty() {
        flush_code_block(&code_buffer, &code_lang);
    }
    let _ = std::io::stdout().flush();
}

/// Streaming state tracker.
pub struct StreamState {
    buffer: String,
    in_code: bool,
    code_lang: String,
    code_buffer: Vec<String>,
    needs_header: bool,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_code: false,
            code_lang: String::new(),
            code_buffer: Vec::new(),
            needs_header: true,
        }
    }
}

/// Render a streaming chunk incrementally.
pub fn render_stream_chunk(chunk: &str, state: &mut StreamState) {
    state.buffer.push_str(chunk);

    while let Some(pos) = state.buffer.find('\n') {
        let line = state.buffer[..pos].to_string();
        state.buffer = state.buffer[pos + 1..].to_string();
        process_stream_line(&line, state);
    }
}

/// Flush remaining buffer.
pub fn flush_stream(state: &mut StreamState) {
    if !state.buffer.is_empty() {
        let line = std::mem::take(&mut state.buffer);
        process_stream_line(&line, state);
    }
    if state.in_code && !state.code_buffer.is_empty() {
        flush_code_block(&state.code_buffer, &state.code_lang);
        state.code_buffer.clear();
        state.code_lang.clear();
        state.in_code = false;
    }
    let _ = std::io::stdout().flush();
}

// ═══════════════════════════════════════════════════════════════════════════
// Internal
// ═══════════════════════════════════════════════════════════════════════════

fn process_stream_line(line: &str, state: &mut StreamState) {
    if state.needs_header {
        super::output::assistant_header();
        state.needs_header = false;
    }

    if line.starts_with("```") {
        if state.in_code {
            flush_code_block(&state.code_buffer, &state.code_lang);
            state.code_buffer.clear();
            state.code_lang.clear();
            state.in_code = false;
        } else {
            state.in_code = true;
            state.code_lang = line.trim_start_matches('`').trim().to_string();
        }
        return;
    }

    if state.in_code {
        state.code_buffer.push(line.to_string());
        let line_no = state.code_buffer.len();
        let highlighted = highlight_single_line(line, &state.code_lang);
        println!("  {} {}", format!("{:>3}", line_no).color(DIM_GRAY), highlighted);
        let _ = std::io::stdout().flush();
        return;
    }

    if line.is_empty() {
        println!();
    } else if line.starts_with("### ") || line.starts_with("## ") || line.starts_with("# ") {
        println!("  {}", line.trim_start_matches('#').trim().color(AI_HEADER).bold());
    } else if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
        let content = &line.trim_start()[2..];
        println!("  {} {}", "•".color(AI_ACCENT), render_inline(content));
    } else {
        println!("  {}", render_inline(line));
    }
    let _ = std::io::stdout().flush();
}

/// Render inline formatting.
fn render_inline(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut in_bold = false;

    while let Some(ch) = chars.next() {
        if ch == '`' {
            let mut code_content = String::new();
            while let Some(&next) = chars.peek() {
                if next == '`' {
                    chars.next();
                    break;
                }
                code_content.push(chars.next().unwrap());
            }
            result.push_str(&code_content.color(USER_BRIGHT).to_string());
            continue;
        }

        if ch == '*' {
            if chars.peek() == Some(&'*') {
                chars.next();
                in_bold = !in_bold;
                continue;
            }
        }

        if in_bold {
            result.push_str(&ch.to_string().bold().to_string());
        } else {
            result.push(ch);
        }
    }

    result
}

/// Highlight a single code line using syntect (dark theme).
fn highlight_single_line(line: &str, lang: &str) -> String {
    let hl = get_highlighter();
    let syntax = if lang.is_empty() {
        hl.ss.find_syntax_by_first_line(line)
            .unwrap_or_else(|| hl.ss.find_syntax_plain_text())
    } else {
        hl.ss.find_syntax_by_token(lang)
            .unwrap_or_else(|| hl.ss.find_syntax_plain_text())
    };

    let mut highlighter = HighlightLines::new(syntax, &hl.ts.themes[CODE_THEME]);
    let ranges = highlighter.highlight_line(line, &hl.ss).unwrap_or_default();

    let mut output = String::new();
    for (style, text) in ranges {
        let fg = style.foreground;
        output.push_str(&format!(
            "\x1b[38;2;{};{};{}m{}\x1b[0m",
            fg.r, fg.g, fg.b, text
        ));
    }
    output
}

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
        "┌─ ".color(TOOL_PURPLE),
        lang_label.color(TOOL_PURPLE).bold(),
        " ",
        "─".repeat(width.saturating_sub(lang_label.len() + 4)).color(BORDER_GRAY),
    );

    let mut highlighter = HighlightLines::new(syntax, &hl.ts.themes[CODE_THEME]);

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
        println!("  {} {}", line_no.color(DIM_GRAY), highlighted);
    }

    // Bottom border: └──────
    println!("  {}", format!("└{}", "─".repeat(width.saturating_sub(1).min(78))).color(BORDER_GRAY));
    println!();
}
