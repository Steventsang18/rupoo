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

use console::Term;
use owo_colors::OwoColorize;
use super::theme;

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

/// Render a single markdown line
fn render_markdown_line(line: &str, width: usize, t: &theme::Theme) {
    // Headers
    if let Some(text) = line.strip_prefix("# ") {
        println!(
            "\n{} {}",
            "█".color(t.ai_header),
            text.color(t.ai_header).bold()
        );
        return;
    }
    if let Some(text) = line.strip_prefix("## ") {
        println!("\n{} {}", "▓".color(t.ai_header), text.color(t.ai_header));
        return;
    }
    if let Some(text) = line.strip_prefix("### ") {
        println!("\n{} {}", "▒".color(t.ai_header), text.color(t.ai_header));
        return;
    }

    // List items
    if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        println!("  {} {}", "•".color(t.user_med), render_inline(text, t));
        return;
    }
    if let Some(text) = line
        .strip_prefix("1. ")
        .or_else(|| line.strip_prefix("1) "))
    {
        println!("  {} {}", "▸".color(t.user_med), render_inline(text, t));
        return;
    }

    // Blockquotes
    if let Some(text) = line.strip_prefix("> ") {
        println!("  │ {}", text.color(t.dim));
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

    // Regular paragraph
    println!("  {}", render_inline(line, t));
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
        let _lang = std::mem::take(&mut state.code_block_lang); // Reserved for syntax highlighting
        state.in_code_block = false;
        println!("\n```\n{}", content);
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
                let _lang = std::mem::take(&mut state.code_block_lang); // Reserved for syntax highlighting
                println!("\n```\n{}", content);
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
