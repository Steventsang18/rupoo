//! Rupoo system tool modules.
//!
//! Each tool provides a single `async fn execute_*` entry point that
//! returns `AgentResult<String>`. All tools integrate with SafetyContext
//! for access control and timeout protection.

pub mod browser;
pub mod network;
pub mod schema;
pub mod search;
pub mod terminal;
pub mod verify;

/// Strip HTML tags from a string, leaving only the text content.
/// Also decodes common HTML entities. Used by both search and browser tools.
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Skip to the next '>'
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'>' {
                j += 1;
            }
            i = j.saturating_add(1);
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
