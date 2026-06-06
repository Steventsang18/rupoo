//! String utilities for efficient string operations.
//!
//! This module provides optimized string handling utilities to improve performance
//! by minimizing allocations and copies.

use std::borrow::Cow;
use std::fmt;

/// Efficiently concatenate multiple string slices into a single String.
///
/// Pre-allocates the exact required capacity to avoid reallocations.
///
/// # Examples
///
/// ```rust
/// use rupoo::strings::concat;
///
/// let result = concat(&["Hello", " ", "World", "!"]);
/// assert_eq!(result, "Hello World!");
/// ```
pub fn concat(slices: &[&str]) -> String {
    let total_len: usize = slices.iter().map(|s| s.len()).sum();
    let mut result = String::with_capacity(total_len);
    for s in slices {
        result.push_str(s);
    }
    result
}

/// Efficiently format a string with minimal allocations.
///
/// Uses a pre-allocated buffer when the expected length is known.
pub fn format_with_capacity(capacity: usize, args: fmt::Arguments<'_>) -> String {
    let mut result = String::with_capacity(capacity);
    fmt::write(&mut result, args).unwrap_or_default();
    result
}

/// Trim whitespace and normalize newlines in a string.
///
/// Converts multiple newlines to single newlines and trims leading/trailing whitespace.
pub fn normalize_whitespace(s: &str) -> Cow<'_, str> {
    let trimmed = s.trim();
    if trimmed.contains("\n\n") || trimmed.contains("\r\n") {
        let mut result = String::with_capacity(trimmed.len());
        let mut last_was_newline = false;
        
        for c in trimmed.chars() {
            if c == '\r' {
                continue;
            }
            if c == '\n' {
                if !last_was_newline {
                    result.push('\n');
                    last_was_newline = true;
                }
            } else {
                result.push(c);
                last_was_newline = false;
            }
        }
        Cow::Owned(result)
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// Truncate a string to a maximum length, adding an ellipsis if truncated.
///
/// The `max_len` parameter specifies the maximum number of characters from the
/// original string to include before adding an ellipsis. The ellipsis is only
/// added when truncation actually occurs.
///
/// # Examples
///
/// ```rust
/// use rupoo::strings::truncate;
///
/// assert_eq!(truncate("Hello World", 5), "Hello…");
/// assert_eq!(truncate("Hello World", 15), "Hello World");
/// assert_eq!(truncate("Hello World", 8), "Hello Wo…");
/// ```
pub fn truncate(s: &str, max_len: usize) -> Cow<'_, str> {
    let char_count = s.chars().count();
    if char_count <= max_len {
        return Cow::Borrowed(s);
    }
    
    if max_len == 0 {
        return Cow::Borrowed("");
    }
    
    if max_len == 1 {
        return Cow::Owned("…".to_string());
    }
    
    let mut result: String = s.chars().take(max_len).collect();
    result.push('…');
    Cow::Owned(result)
}

/// Split a string into lines, efficiently handling different line endings.
///
/// Returns an iterator over the lines.
pub fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    s.split_inclusive(&['\n', '\r']).filter(|line| !line.is_empty())
}

/// Escape special characters in a string for use in JSON or similar contexts.
///
/// # Examples
///
/// ```rust
/// use rupoo::strings::escape_json;
///
/// assert_eq!(escape_json(r#"Hello "World""#), r#"Hello \"World\""#);
/// assert_eq!(escape_json("Line1\nLine2"), "Line1\\nLine2");
/// ```
pub fn escape_json(s: &str) -> Cow<'_, str> {
    let needs_escape = s.contains(|c| matches!(c, '"' | '\\' | '\n' | '\r' | '\t'));
    
    if !needs_escape {
        return Cow::Borrowed(s);
    }
    
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    Cow::Owned(result)
}

/// Remove ANSI escape codes from a string.
///
/// # Examples
///
/// ```rust
/// use rupoo::strings::strip_ansi;
///
/// assert_eq!(strip_ansi("\x1b[31mRed Text\x1b[0m"), "Red Text");
/// ```
pub fn strip_ansi(s: &str) -> Cow<'_, str> {
    const ESC: u8 = 0x1b;
    
    if !s.as_bytes().contains(&ESC) {
        return Cow::Borrowed(s);
    }
    
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    
    for c in s.chars() {
        if c == ESC as char {
            in_escape = true;
            continue;
        }
        
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        
        result.push(c);
    }
    
    Cow::Owned(result)
}

/// Efficiently replace all occurrences of a substring.
///
/// Uses Cow to avoid unnecessary allocations when no replacements are needed.
pub fn replace_all<'a>(s: &'a str, from: &str, to: &str) -> Cow<'a, str> {
    if !s.contains(from) {
        return Cow::Borrowed(s);
    }
    
    let from_len = from.len();
    let to_len = to.len();
    
    // Estimate capacity: worst case if every character is replaced
    let estimated_capacity = if to_len > from_len {
        s.len() + (s.len() / from_len) * (to_len - from_len)
    } else {
        s.len()
    };
    let mut result = String::with_capacity(estimated_capacity.max(s.len()));
    
    let mut start = 0;
    while let Some(pos) = s[start..].find(from) {
        let abs_pos = start + pos;
        result.push_str(&s[start..abs_pos]);
        result.push_str(to);
        start = abs_pos + from_len;
    }
    result.push_str(&s[start..]);
    
    Cow::Owned(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_concat() {
        assert_eq!(concat(&["a", "b", "c"]), "abc");
        assert_eq!(concat(&["Hello", " ", "World"]), "Hello World");
        assert_eq!(concat(&[]), "");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("Hello", 10), "Hello");
        assert_eq!(truncate("Hello World", 5), "Hello…");
        assert_eq!(truncate("Hello World", 8), "Hello Wo…");
        assert_eq!(truncate("Hi", 1), "…");
        assert_eq!(truncate("Hello", 5), "Hello");
        assert_eq!(truncate("Short", 10), "Short");
        assert_eq!(truncate("Test", 3), "Tes…");
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("  Hello   "), "Hello");
        assert_eq!(normalize_whitespace("Line1\n\nLine2"), "Line1\nLine2");
        assert_eq!(normalize_whitespace("Line1\r\nLine2"), "Line1\nLine2");
    }

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("Hello"), "Hello");
        assert_eq!(escape_json(r#"Hello "World""#), r#"Hello \"World\""#);
        assert_eq!(escape_json("Line1\nLine2"), "Line1\\nLine2");
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("Plain text"), "Plain text");
        assert_eq!(strip_ansi("\x1b[31mRed\x1b[0m"), "Red");
        assert_eq!(strip_ansi("\x1b[1;32mBold Green\x1b[0m"), "Bold Green");
    }

    #[test]
    fn test_replace_all() {
        assert_eq!(replace_all("Hello World", "World", "Rust"), "Hello Rust");
        assert_eq!(replace_all("ababa", "aba", "x"), "xba");
        assert_eq!(replace_all("ababa", "ab", "x"), "xxa");
        assert_eq!(replace_all("ababab", "ab", "x"), "xxx");
        assert_eq!(replace_all("No match", "xyz", "abc"), "No match");
    }
}
