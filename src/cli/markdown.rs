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
#![allow(dead_code)]

use owo_colors::OwoColorize;
use console::Term;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::ThemeSet;
use syntect::easy::HighlightLines;

use super::theme;

fn terminal_width() -> usize {
    Term::stdout().size().1 as usize
}

// ═══════════════════════════════════════════════════════════════════════════
// Syntax Highlighting
// ═══════════════════════════════════════════════════════════════════════════

lazy_static::lazy_static! {
    static ref SYNTAX_SET: SyntaxSet = SyntaxSet::load_defaults_newlines();
    static ref THEME_SET: ThemeSet = ThemeSet::load_defaults();
}

/// Highlight code and return ANSI-colored string
pub fn highlight_code(code: &str, language: Option<&str>, theme_name: &str) -> String {
    // Theme is reserved for future styling customization
    let _t = theme::current();
    
    // Find syntax definition
    let syntax = if let Some(lang) = language {
        SYNTAX_SET
            .find_syntax_by_token(lang)
            .or_else(|| SYNTAX_SET.find_syntax_by_extension(lang))
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text())
    } else {
        SYNTAX_SET.find_syntax_plain_text()
    };
    
    // Get theme
    let highlight_theme = THEME_SET
        .themes
        .get(theme_name)
        .or_else(|| THEME_SET.themes.get("base16-ocean.dark"))
        .unwrap_or_else(|| &THEME_SET.themes.values().next().unwrap());
    
    // Parse and highlight
    let mut highlighter = HighlightLines::new(syntax, highlight_theme);
    let mut result = String::new();
    
    #[allow(deprecated)]
    for (style, text) in highlighter.highlight(code, &SYNTAX_SET) {
        let r = style.foreground.r;
        let g = style.foreground.g;
        let b = style.foreground.b;
        
        // Apply color if not default
        if r != 0 || g != 0 || b != 0 {
            result.push_str(&format!("\x1b[38;2;{};{};{}m", r, g, b));
        }
        result.push_str(text);
        
        // Reset color
        if r != 0 || g != 0 || b != 0 {
            result.push_str("\x1b[0m");
        }
    }
    
    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Markdown Parser
// ═══════════════════════════════════════════════════════════════════════════

/// Render a markdown string to terminal with styling
pub fn render_markdown(markdown: &str) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    let mut in_code_block = false;
    let mut code_block_content = String::new();
    let mut code_block_lang = String::new();
    
    for line in markdown.lines() {
        // Check for code fence
        if line.starts_with("```") {
            if in_code_block {
                // End code block - render it
                render_code_block(&code_block_content, &code_block_lang, width);
                code_block_content.clear();
                code_block_lang.clear();
                in_code_block = false;
            } else {
                // Start code block
                in_code_block = true;
                code_block_lang = line.trim_start_matches("```").to_string();
            }
            continue;
        }
        
        if in_code_block {
            if !code_block_content.is_empty() {
                code_block_content.push('\n');
            }
            code_block_content.push_str(line);
            continue;
        }
        
        // Regular markdown line
        render_markdown_line(line, width, &t);
    }
    
    // Handle unclosed code block
    if in_code_block && !code_block_content.is_empty() {
        render_code_block(&code_block_content, &code_block_lang, width);
    }
}

/// Render a single markdown line
fn render_markdown_line(line: &str, width: usize, t: &theme::Theme) {
    // Headers
    if line.starts_with("# ") {
        let text = &line[2..];
        println!("\n{} {}", "█".color(t.ai_header), text.color(t.ai_header).bold());
        return;
    }
    if line.starts_with("## ") {
        let text = &line[3..];
        println!("\n{} {}", "▓".color(t.ai_header), text.color(t.ai_header));
        return;
    }
    if line.starts_with("### ") {
        let text = &line[4..];
        println!("\n{} {}", "▒".color(t.ai_header), text.color(t.ai_header));
        return;
    }
    
    // List items
    if line.starts_with("- ") || line.starts_with("* ") {
        let text = &line[2..];
        println!("  {} {}", "•".color(t.user_med), render_inline(text, t));
        return;
    }
    if line.starts_with("1. ") || line.starts_with("1) ") {
        println!("  {} {}", "▸".color(t.user_med), render_inline(&line[3..], t));
        return;
    }
    
    // Blockquotes
    if line.starts_with("> ") {
        let text = &line[2..];
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
                code.push(chars.next().unwrap());
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
                bold.push(chars.next().unwrap());
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
                link_text.push(chars.next().unwrap());
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

/// Render a code block with border and syntax highlighting
fn render_code_block(code: &str, language: &str, width: usize) {
    let t = theme::current();
    
    // Header with language label
    let lang_display = if language.is_empty() {
        "code".to_string()
    } else {
        language.to_string()
    };
    
    println!();
    print!("{}", "┌─ ".color(t.border));
    println!("{} {} {}", "rust".color(t.tool_accent), "─".repeat(3).color(t.border), "─".repeat(width.saturating_sub(15)).color(t.border));
    
    // Highlighted code lines
    let highlighted = highlight_code(code, Some(&language), t.code_theme);
    
    for line in highlighted.lines() {
        let display = if line.len() > width - 6 {
            format!("{}…", &line[..width.saturating_sub(9)])
        } else {
            line.to_string()
        };
        print!("{}", "│ ".color(t.border));
        println!("{}{}", display, " ".repeat(width.saturating_sub(display.len() + 3)));
    }
    
    // Footer
    print!("{}", "└".color(t.border));
    println!("{}", "─".repeat(width.saturating_sub(2)).color(t.border));
    println!("{} {} {}", "📄".color(t.dim), lang_display.color(t.dim), "copied".color(t.dim));
    println!();
}

// ═══════════════════════════════════════════════════════════════════════════
// Stream State for Progressive Rendering
// ═══════════════════════════════════════════════════════════════════════════

/// Stream state for rendering markdown progressively
#[derive(Debug, Clone)]
pub struct StreamState {
    pub buffer: String,
    pub in_code_block: bool,
    pub code_block_lang: String,
    pub code_block_buffer: String,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            in_code_block: false,
            code_block_lang: String::new(),
            code_block_buffer: String::new(),
        }
    }
}

impl StreamState {
    /// Create a new stream state
    pub fn new() -> Self {
        Self::default()
    }
    /// Process incoming text chunk
    pub fn push_chunk(&mut self, chunk: &str) -> Option<RenderedChunk> {
        self.buffer.push_str(chunk);
        
        // Check for complete line
        if let Some(pos) = self.buffer.rfind('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 1..].to_string();
            
            // Handle code fences
            if line.starts_with("```") {
                if self.in_code_block {
                    // End code block
                    self.in_code_block = false;
                    let content = std::mem::take(&mut self.code_block_buffer);
                    let lang = std::mem::take(&mut self.code_block_lang);
                    return Some(RenderedChunk::CodeBlockEnd { content, language: lang });
                } else {
                    // Start code block
                    self.in_code_block = true;
                    self.code_block_lang = line.trim_start_matches("```").to_string();
                    return Some(RenderedChunk::CodeBlockStart { language: self.code_block_lang.clone() });
                }
            }
            
            if self.in_code_block {
                if !self.code_block_buffer.is_empty() {
                    self.code_block_buffer.push('\n');
                }
                self.code_block_buffer.push_str(&line);
                return None;
            }
            
            return Some(RenderedChunk::Line(line));
        }
        
        None
    }
    
    /// Flush remaining buffer
    pub fn flush(&mut self) -> Option<RenderedChunk> {
        if self.in_code_block && !self.code_block_buffer.is_empty() {
            let content = std::mem::take(&mut self.code_block_buffer);
            let lang = std::mem::take(&mut self.code_block_lang);
            self.in_code_block = false;
            return Some(RenderedChunk::CodeBlockEnd { content, language: lang });
        }
        
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            return Some(RenderedChunk::Line(line));
        }
        
        None
    }
}

/// Types of rendered chunks
#[derive(Debug, Clone)]
pub enum RenderedChunk {
    Line(String),
    CodeBlockStart { language: String },
    CodeBlockEnd { content: String, language: String },
    CodeLine(String),
}

// ═══════════════════════════════════════════════════════════════════════════
// Convenience Functions
// ═══════════════════════════════════════════════════════════════════════════

/// Print a code snippet with highlighting
pub fn print_code(code: &str, language: Option<&str>) {
    let t = theme::current();
    let width = terminal_width().max(40);
    
    let lang = language.unwrap_or("text");
    
    // Top border
    println!();
    print!("{}", "┌─ ".color(t.border));
    println!("{} {} {}", lang.color(t.tool_accent), "─".repeat(3).color(t.border), "─".repeat(width.saturating_sub(15)).color(t.border));
    
    // Highlighted code
    let highlighted = highlight_code(code, language, t.code_theme);
    
    for line in highlighted.lines() {
        let display = if line.len() > width - 6 {
            format!("{}…", &line[..width.saturating_sub(9)])
        } else {
            line.to_string()
        };
        print!("{}", "│ ".color(t.border));
        println!("{}{}", display, " ".repeat(width.saturating_sub(display.len() + 3)));
    }
    
    // Bottom border
    print!("{}", "└".color(t.border));
    println!("{}", "─".repeat(width.saturating_sub(2)).color(t.border));
    println!();
}

/// Print styled checklist
pub fn print_checklist(items: &[(String, bool)]) {
    let t = theme::current();
    
    for (text, checked) in items {
        let icon = if *checked { "✓" } else { "○" };
        let color = if *checked { t.user_med } else { t.dim };
        println!("  {} {}", icon.color(color), text.color(if *checked { t.user_med } else { t.dim }));
    }
}

/// Print styled bullets
pub fn print_bullets(items: &[String]) {
    let t = theme::current();
    
    for item in items {
        println!("  {} {}", "•".color(t.user_med), item.color(t.ai_accent));
    }
}

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
