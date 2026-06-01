//! Web search tool using DuckDuckGo HTML.
//!
//! Uses DuckDuckGo's HTML search endpoint (no API key required) and parses
//! the results using simple string-based extraction.

use crate::error::AgentResult;
use crate::http_client::HTTP_CLIENT;
use crate::safety::SafetyContext;

/// Maximum number of characters in the output (truncated if exceeded).
const MAX_OUTPUT_CHARS: usize = 5000;

/// Perform a web search using DuckDuckGo HTML.
///
/// # Arguments
/// * `query` - The search query string
/// * `safety` - SafetyContext for SSRF protection
///
/// # Returns
/// Formatted string with up to 10 search results (title, snippet, URL).
pub async fn web_search(query: &str, _safety: &SafetyContext) -> AgentResult<String> {
    let query_encoded = urlencoding::encode(query).to_string();
    let url = format!("https://html.duckduckgo.com/html/?q={}", query_encoded);

    // SSRF protection: block requests to localhost/private networks
    if SafetyContext::is_localhost_url(&url) {
        return Err(crate::error::AgentError::Safety(
            "web search URL resolves to localhost — blocked by SSRF protection".into(),
        ));
    }

    let client = HTTP_CLIENT.as_ref();

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (compatible; Rupoo/1.0)")
        .header("Accept", "text/html")
        .send()
        .await
        .map_err(|e| crate::error::AgentError::Tool(format!("search request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(crate::error::AgentError::Tool(format!(
            "search returned status: {}",
            response.status()
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| crate::error::AgentError::Tool(format!("failed to read response: {e}")))?;

    // Limit response size to 1MB
    let body = if body.len() > 1_048_576 {
        &body[..1_048_576]
    } else {
        &body
    };

    let results = parse_ddg_results(body);
    let output = format_search_results(&results, query);

    // Compress if needed
    let output = crate::signal::compress_output(&output, Some(MAX_OUTPUT_CHARS));

    Ok(output)
}

/// Search result from DuckDuckGo HTML.
struct SearchResult {
    title: String,
    snippet: String,
    url: String,
}

/// Parse DuckDuckGo HTML response to extract search results.
///
/// DuckDuckGo HTML has result blocks like:
/// ```html
/// <div class="result">
///   <a class="result__a" href="...">Title</a>
///   <a class="result__snippet" href="...">Snippet text...</a>
/// </div>
/// ```
fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut pos = 0;

    while results.len() < 10 {
        // Find next result block
        match html[pos..].find("<div class=\"result\">") {
            Some(start_offset) => {
                pos += start_offset;
                // Find end of this result block
                match html[pos..].find("</div>\n</div>") {
                    Some(end_offset) => {
                        let block = &html[pos..pos + end_offset + 13];
                        pos += end_offset + 13;

                        // Extract title: <a class="result__a" href="URL">Title</a>
                        let title = extract_ddg_field(block, "result__a").unwrap_or_default();

                        // Extract snippet: <a class="result__snippet" ...>Snippet</a>
                        let snippet = extract_ddg_field(block, "result__snippet").unwrap_or_default();

                        // Extract URL from the title link href
                        let url = extract_ddg_url(block, "result__a").unwrap_or_default();

                        if !title.is_empty() {
                            results.push(SearchResult {
                                title,
                                snippet,
                                url,
                            });
                        }
                    }
                    None => break,
                }
            }
            None => break,
        }
    }

    results
}

/// Extract text content for a field with the given class name.
fn extract_ddg_field(block: &str, class_name: &str) -> Option<String> {
    let marker = format!("class=\"{}\"", class_name);
    let start_tag = format!("<a {}", marker);
    let end_tag = "</a>";

    // Find the opening <a ...> tag
    let open_tag_start = block.find(&start_tag)?;
    let open_tag_end = block[open_tag_start..].find('>')?;
    let content_start = open_tag_start + open_tag_end + 1;

    // Find closing </a>
    let content_end = block[content_start..].find(end_tag)?;
    let raw = &block[content_start..content_start + content_end];

    // Strip any remaining HTML tags
    Some(super::strip_html_tags(raw).trim().to_string())
}

/// Extract URL from the href attribute of an anchor with the given class.
fn extract_ddg_url(block: &str, class_name: &str) -> Option<String> {
    let marker = format!("class=\"{}\"", class_name);
    let tag_start = format!("<a {}", marker);

    let open_pos = block.find(&tag_start)?;
    let href_start = block[open_pos..].find("href=\"")?;
    let href_start = open_pos + href_start + 6;
    let href_end = block[href_start..].find('"').unwrap_or(usize::MAX);
    let url = &block[href_start..href_start + href_end];

    // DuckDuckGo uses relative URLs for internal links
    if url.starts_with("/") {
        Some(format!("https://duckduckgo.com{}", url))
    } else {
        Some(url.to_string())
    }
}

/// Format search results as a readable string.
fn format_search_results(results: &[SearchResult], query: &str) -> String {
    if results.is_empty() {
        return format!("No search results found for: {}\n", query);
    }

    let mut output = format!("Search results for \"{}\":\n\n", query);
    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!(
            "{}. {}\n   {}\n   {}\n\n",
            i + 1,
            result.title,
            result.snippet,
            result.url
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        use super::super::strip_html_tags;
        assert_eq!(strip_html_tags("hello world"), "hello world");
        assert_eq!(strip_html_tags("hello <b>world</b>"), "hello world");
        assert_eq!(strip_html_tags("a &amp; b"), "a & b");
        assert_eq!(strip_html_tags("<script>evil</script>hello"), "evilhello");
    }

    #[test]
    fn test_format_empty_results() {
        let out = format_search_results(&[], "test query");
        assert!(out.contains("No search results"));
    }

    #[test]
    fn test_format_results() {
        let results = vec![
            SearchResult {
                title: "Rust Programming".to_string(),
                snippet: "A language empowering everyone.".to_string(),
                url: "https://rust-lang.org".to_string(),
            },
        ];
        let out = format_search_results(&results, "rust");
        assert!(out.contains("Rust Programming"));
        assert!(out.contains("language empowering"));
        assert!(out.contains("rust-lang.org"));
    }
}
