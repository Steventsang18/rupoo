//! Markdown rendering with syntax highlighting for CLI
//!
//! Features:
//! - Code block syntax highlighting
//! - Inline code styling
//! - List formatting
//! - Link formatting
//! - Fenced code blocks
//!
//! Note: Some functions are reserved for future use and may not be currently active.

use super::output::wrap_content;
use super::theme;
use console::Term;
use owo_colors::OwoColorize;

// ═══════════════════════════════════════════════════════════════════════════
// Lightweight syntax highlighting — keyword-based, no external parser.
// Covers common languages seen in agent output (Rust/JSON/YAML/TOML/Python/JS/Go).
// ═══════════════════════════════════════════════════════════════════════════

/// Very lightweight keyword-based syntax highlighting for code blocks.
/// Uses simple line-level patterns rather than a full parser.
fn syntax_highlight_code(code: &str, lang: &str) -> String {
    let t = theme::current();
    let lang = lang.to_lowercase();

    // Define keyword sets per language family
    let keywords: &[&str] = match lang.as_str() {
        "rust" | "rs" => &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "match", "if",
            "else", "for", "while", "loop", "return", "async", "await", "trait", "where", "type",
            "const", "static", "ref", "move", "self", "super", "crate", "in", "as", "break",
            "continue", "unsafe", "dyn", "true", "false", "Some", "None", "Ok", "Err",
        ],
        "json" => &["null", "true", "false"],
        "python" | "py" => &[
            "def", "class", "import", "from", "if", "elif", "else", "for", "while", "return",
            "yield", "async", "await", "with", "as", "try", "except", "raise", "pass", "None",
            "True", "False", "self", "in", "not", "and", "or",
        ],
        "javascript" | "js" | "typescript" | "ts" => &[
            "function",
            "const",
            "let",
            "var",
            "if",
            "else",
            "for",
            "while",
            "return",
            "async",
            "await",
            "import",
            "export",
            "from",
            "class",
            "new",
            "this",
            "null",
            "undefined",
            "true",
            "false",
            "try",
            "catch",
            "throw",
            "switch",
            "case",
            "break",
            "continue",
        ],
        "go" | "golang" => &[
            "func",
            "package",
            "import",
            "if",
            "else",
            "for",
            "range",
            "return",
            "var",
            "const",
            "type",
            "struct",
            "interface",
            "map",
            "chan",
            "go",
            "defer",
            "select",
            "case",
            "default",
            "break",
            "continue",
            "nil",
            "true",
            "false",
        ],
        "toml" => &[],
        "yaml" | "yml" => &[],
        "shell" | "bash" | "sh" | "zsh" => &[
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "exit", "export", "local", "source", "set", "echo", "cd",
        ],
        "diff" | "patch" => &[],
        _ => &[], // unknown language — no keyword highlighting
    };

    let mut result = String::new();

    for line in code.lines() {
        // Diff/patch: color the prefix
        if lang == "diff" || lang == "patch" {
            if line.starts_with("+") && !line.starts_with("+++") {
                result.push_str(&format!("{}\n", line.green()));
                continue;
            } else if line.starts_with("-") && !line.starts_with("---") {
                result.push_str(&format!("{}\n", line.red()));
                continue;
            } else if line.starts_with("@") {
                result.push_str(&format!("{}\n", line.color(t.think)));
                continue;
            }
        }

        // Comments: highlight entire line
        let comment_style = if lang == "rust" || lang == "rs" || lang == "go" {
            line.starts_with("//")
        } else if lang == "python" || lang == "py" {
            line.trim_start().starts_with('#')
        } else {
            false
        };
        if comment_style {
            result.push_str(&format!("{}\n", line.color(t.dim)));
            continue;
        }

        if line.trim().is_empty() {
            result.push('\n');
            continue;
        }

        let mut highlighted = String::new();
        let mut word = String::new();
        let mut in_string = false;
        let mut string_char = '"';
        let mut chars_iter = line.chars().peekable();

        while let Some(ch) = chars_iter.next() {
            if in_string {
                highlighted.push(ch);
                if ch == string_char {
                    in_string = false;
                }
                continue;
            }

            // String literals
            if ch == '"' || ch == '\'' || ch == '`' {
                if !word.is_empty() {
                    if keywords.contains(&word.as_str()) {
                        highlighted.push_str(&word.color(t.think).to_string());
                    } else {
                        highlighted.push_str(&word);
                    }
                    word.clear();
                }
                highlighted.push(ch);
                in_string = true;
                string_char = ch;
                continue;
            }

            // // line comments (Rust, Go, JS, TS)
            if ch == '/' && chars_iter.peek() == Some(&'/') {
                if !word.is_empty() {
                    if keywords.contains(&word.as_str()) {
                        highlighted.push_str(&word.color(t.think).to_string());
                    } else {
                        highlighted.push_str(&word);
                    }
                    word.clear();
                }
                // Rest of line is a comment — dim it
                let rest: String = std::iter::once('/').chain(chars_iter).collect();
                highlighted.push_str(&rest.color(t.dim).to_string());
                break;
            }

            if ch.is_alphanumeric() || ch == '_' {
                word.push(ch);
            } else {
                if !word.is_empty() {
                    if keywords.contains(&word.as_str()) {
                        highlighted.push_str(&word.color(t.think).to_string());
                    } else {
                        highlighted.push_str(&word);
                    }
                    word.clear();
                }
                highlighted.push(ch);
            }
        }

        // Flush remaining word
        if !word.is_empty() {
            if keywords.contains(&word.as_str()) {
                highlighted.push_str(&word.color(t.think).to_string());
            } else {
                highlighted.push_str(&word);
            }
        }

        result.push_str(&highlighted);
        result.push('\n');
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

/// Render a single markdown line, wrapping long content to the terminal width
/// with hanging indents so lists/quotes stay aligned.
fn render_markdown_line(line: &str, width: usize, t: &theme::Theme) {
    // Headers (marker is 2 columns: "█ "/"▓ "/"▒ ")
    if let Some(text) = line.strip_prefix("# ") {
        let cw = width.saturating_sub(2);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!(
                    "\n{} {}",
                    "█".color(t.ai_header),
                    seg.color(t.ai_header).bold()
                );
            } else {
                println!("  {}", seg.color(t.ai_header).bold());
            }
        }
        return;
    }
    if let Some(text) = line.strip_prefix("## ") {
        let cw = width.saturating_sub(2);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!("\n{} {}", "▓".color(t.ai_header), seg.color(t.ai_header));
            } else {
                println!("  {}", seg.color(t.ai_header));
            }
        }
        return;
    }
    if let Some(text) = line.strip_prefix("### ") {
        let cw = width.saturating_sub(2);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!("\n{} {}", "▒".color(t.ai_header), seg.color(t.ai_header));
            } else {
                println!("  {}", seg.color(t.ai_header));
            }
        }
        return;
    }

    // List items (prefix "  • "/"  ▸ " is 4 columns → 4-space hanging indent)
    if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        let cw = width.saturating_sub(4);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!("  {} {}", "•".color(t.user_med), render_inline(seg, t));
            } else {
                println!("    {}", render_inline(seg, t));
            }
        }
        return;
    }
    if let Some(text) = line
        .strip_prefix("1. ")
        .or_else(|| line.strip_prefix("1) "))
    {
        let cw = width.saturating_sub(4);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!("  {} {}", "▸".color(t.user_med), render_inline(seg, t));
            } else {
                println!("    {}", render_inline(seg, t));
            }
        }
        return;
    }

    // Blockquotes (prefix "  │ " is 4 columns → 4-space hanging indent)
    if let Some(text) = line.strip_prefix("> ") {
        let cw = width.saturating_sub(4);
        for (i, seg) in wrap_content(text, cw).iter().enumerate() {
            if i == 0 {
                println!("  │ {}", seg.color(t.dim));
            } else {
                println!("    {}", seg.color(t.dim));
            }
        }
        return;
    }

    // Horizontal rule
    if line == "---" || line == "***" || line == "___" {
        println!("{}", "─".repeat(width.min(50)).color(t.border));
        return;
    }

    // Empty line
    if line.trim().is_empty() {
        println!();
        return;
    }

    // Regular paragraph (prefix "  " is 2 columns → 2-space hanging indent)
    let cw = width.saturating_sub(2);
    for seg in wrap_content(line, cw) {
        println!("  {}", render_inline(&seg, t));
    }
}

/// Render inline elements (bold, italic, code, links)
fn render_inline(text: &str, t: &theme::Theme) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            // Inline code
            let mut code = String::new();
            while let Some(&next) = chars.peek() {
                if next == '`' {
                    chars.next();
                    break;
                }
                code.push(chars.next().unwrap_or('`'));
            }
            result.push_str(&format!(" `{}` ", code.color(t.tool_dim)));
        } else if ch == '*' && chars.peek() == Some(&'*') {
            // Bold
            chars.next();
            let mut bold = String::new();
            while let Some(&next) = chars.peek() {
                if next == '*' {
                    chars.next();
                    break;
                }
                bold.push(chars.next().unwrap_or('*'));
            }
            result.push_str(&bold.color(t.user_bright).bold().to_string());
        } else if ch == '[' {
            // Link [text](url)
            let mut link_text = String::new();
            while let Some(&next) = chars.peek() {
                if next == ']' {
                    chars.next();
                    break;
                }
                link_text.push(chars.next().unwrap_or(']'));
            }
            // Skip url part
            if chars.peek() == Some(&'(') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    if next == ')' {
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            }
            result.push_str(&format!("{} ", link_text.color(t.ai_accent).underline()));
        } else {
            result.push(ch);
        }
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream State for Progressive Rendering
// ═══════════════════════════════════════════════════════════════════════════

/// Stream state for rendering markdown progressively
#[derive(Debug, Clone, Default)]
pub struct StreamState {
    pub buffer: String,
    pub in_code_block: bool,
    pub code_block_lang: String,
    pub code_block_buffer: String,
}

impl StreamState {
    /// Create a new stream state
    pub fn new() -> Self {
        Self::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Convenience Functions
// ═══════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════
// Stream Processing Helpers (for mod.rs compatibility)
// ═══════════════════════════════════════════════════════════════════════════

/// Flush stream state and render any remaining content
pub fn flush_stream(state: &mut StreamState) {
    // Check if we're in a code block
    if state.in_code_block && !state.code_block_buffer.is_empty() {
        let content = std::mem::take(&mut state.code_block_buffer);
        let lang = std::mem::take(&mut state.code_block_lang);
        state.in_code_block = false;
        let highlighted = syntax_highlight_code(&content, &lang);
        println!("\n```{}\n{}", lang, highlighted);
    }

    // Flush remaining buffer content
    if !state.buffer.is_empty() {
        let line = std::mem::take(&mut state.buffer);
        let t = theme::current();
        let width = terminal_width().max(40);
        render_markdown_line(&line, width, &t);
    }
}

/// Process a stream chunk and render it
pub fn render_stream_chunk(text: &str, state: &mut StreamState) {
    // Push chunk to buffer only once at the beginning
    state.buffer.push_str(text);

    // Process complete lines from buffer until no more complete lines
    while let Some(pos) = state.buffer.rfind('\n') {
        let line = state.buffer[..pos].to_string();
        state.buffer = state.buffer[pos + 1..].to_string();

        // Handle code fences
        if line.starts_with("```") {
            if state.in_code_block {
                // End code block
                state.in_code_block = false;
                let content = std::mem::take(&mut state.code_block_buffer);
                let lang = std::mem::take(&mut state.code_block_lang);
                let highlighted = syntax_highlight_code(&content, &lang);
                println!("\n```{}\n{}", lang, highlighted);
            } else {
                // Start code block
                state.in_code_block = true;
                state.code_block_lang = line.trim_start_matches("```").to_string();
                println!("\n```{}", state.code_block_lang);
            }
            continue;
        }

        if state.in_code_block {
            if !state.code_block_buffer.is_empty() {
                state.code_block_buffer.push('\n');
            }
            state.code_block_buffer.push_str(&line);
            continue;
        }

        // Render regular markdown line
        let t = theme::current();
        let width = terminal_width().max(40);
        render_markdown_line(&line, width, &t);
    }
}
