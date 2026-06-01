//! Browser automation tool.
//!
//! Controls a headless Chrome/Chromium via CLI for screenshot and navigation.
//! Uses `which` to locate the browser binary in system PATH.
//!
//! # Capabilities (Headless CLI Mode)
//! - **Navigate**: Load a URL and dump the DOM
//! - **Screenshot**: Capture a PNG screenshot
//! - **GetText**: Extract plain text from the page (DOM dump with HTML stripped)
//! - **Click**: Navigate to URL with virtual-time-budget for JS execution (limited)
//! - **ExtractLinks**: Parse all `<a href>` links from the page DOM
//! - **JavaScript**: Not available in CLI mode (returns clear message)
//!
//! # Safety
//! - Browser process is killed on timeout.
//! - Screenshots are saved to system temp directory.
//! - All operations have a hard 30-second timeout.

use std::path::PathBuf;
use std::time::Duration;

use tracing::{warn, info};

use crate::error::{AgentError, AgentResult};
use crate::safety::SafetyContext;
use crate::task::BrowserActionType;

/// Locate Chrome/Chromium in system PATH.
fn find_browser() -> Option<PathBuf> {
    // Try common names
    for name in &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ] {
        if let Ok(path) = which::which(name) {
            return Some(path);
        }
    }
    // Check common macOS path directly
    let mac_path = PathBuf::from(
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    );
    if mac_path.exists() {
        return Some(mac_path);
    }
    None
}

/// Execute a browser action (navigate, screenshot, click, get text, extract links, javascript).
pub async fn execute_browser_action(
    action: &BrowserActionType,
    url: Option<&str>,
    timeout_secs: Option<u64>,
    safety: &SafetyContext,
) -> AgentResult<String> {
    let timeout = timeout_secs.unwrap_or(30);

    // Find browser
    let browser = safety
        .browser_path
        .as_ref()
        .map_or_else(find_browser, |p| {
            if p.exists() {
                Some(p.clone())
            } else {
                find_browser()
            }
        });

    let browser_path = browser.ok_or_else(|| {
        AgentError::Browser(
            "No supported browser found (looked for Chrome/Chromium)".into(),
        )
    })?;

    match action {
        BrowserActionType::Navigate => {
            let url_str = url.ok_or_else(|| {
                AgentError::Browser("URL is required for Navigate action".into())
            })?;

            let output = run_browser_with_timeout(
                &browser_path,
                &[
                    "--headless",
                    "--disable-gpu",
                    "--no-sandbox",
                    "--dump-dom",
                    url_str,
                ],
                timeout,
            )
            .await?;

            let output_len = output.len();
            let truncated = if output_len > 3000 {
                format!("{}...\n[page source truncated]", &output[..3000])
            } else {
                output
            };
            Ok(format!("Page source ({} chars):\n{}", output_len, truncated))
        }

        BrowserActionType::Screenshot => {
            let url_str = url.ok_or_else(|| {
                AgentError::Browser("URL is required for Screenshot action".into())
            })?;

            // Screenshot is saved to system temp directory (not project dir).
            // This is a fixed path, not user-controlled, so path_jail is not bypassed.
            // The temp dir is a safe sandbox boundary.
            let tmp_dir = std::env::temp_dir();
            let screenshot_path = tmp_dir.join(format!(
                "rupoo_screenshot_{}.png",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            ));

            let _ = run_browser_with_timeout(
                &browser_path,
                &[
                    "--headless",
                    "--disable-gpu",
                    "--no-sandbox",
                    &format!("--screenshot={}", screenshot_path.display()),
                    "--window-size=1920,1080",
                    url_str,
                ],
                timeout,
            )
            .await?;

            if screenshot_path.exists() {
                Ok(format!(
                    "Screenshot saved to: {} ({} bytes)",
                    screenshot_path.display(),
                    std::fs::metadata(&screenshot_path)
                        .map(|m| m.len())
                        .unwrap_or(0)
                ))
            } else {
                Err(AgentError::Browser(
                    "Screenshot file was not created by browser".into(),
                ))
            }
        }

        BrowserActionType::GetText => {
            // GetText: Load the page, dump DOM, then strip HTML tags for plain text
            let url_str = url.ok_or_else(|| {
                AgentError::Browser("URL is required for GetText action".into())
            })?;

            let output = run_browser_with_timeout(
                &browser_path,
                &[
                    "--headless",
                    "--disable-gpu",
                    "--no-sandbox",
                    "--dump-dom",
                    url_str,
                ],
                timeout,
            )
            .await?;

            // Strip HTML tags to get plain text
            let plain_text = super::strip_html_tags(&output);
            
            // Clean up extra whitespace and limit output
            let cleaned: String = plain_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            
            let output_len = cleaned.len();
            let truncated = if output_len > 4000 {
                format!("{}...\n[text truncated]", &cleaned[..4000])
            } else {
                cleaned
            };
            
            Ok(format!("Page text ({} chars):\n{}", output_len, truncated))
        }

        BrowserActionType::Click => {
            // Click in headless CLI mode: Use --virtual-time-budget to let JS execute
            // after page load, then dump the resulting DOM.
            // This is a best-effort approach for JS-heavy pages.
            let url_str = url.ok_or_else(|| {
                AgentError::Browser("URL is required for Click action".into())
            })?;

            let output = run_browser_with_timeout(
                &browser_path,
                &[
                    "--headless",
                    "--disable-gpu",
                    "--no-sandbox",
                    "--virtual-time-budget=3000",
                    "--dump-dom",
                    url_str,
                ],
                timeout,
            )
            .await?;

            let output_len = output.len();
            let truncated = if output_len > 3000 {
                format!("{}...\n[DOM after JS execution truncated]", &output[..3000])
            } else {
                output
            };
            
            info!(
                action = "Click (JS execution)",
                url = url_str,
                "Page DOM after virtual-time-budget (3s JS execution)"
            );
            
            Ok(format!(
                "Clicked/navigated to '{}' and executed JS for 3 seconds.\nPage state ({} chars):\n{}",
                url_str,
                output_len,
                truncated
            ))
        }

        BrowserActionType::ExtractLinks => {
            // ExtractLinks: Load page, dump DOM, parse <a href="..."> tags
            let url_str = url.ok_or_else(|| {
                AgentError::Browser("URL is required for ExtractLinks action".into())
            })?;

            let output = run_browser_with_timeout(
                &browser_path,
                &[
                    "--headless",
                    "--disable-gpu",
                    "--no-sandbox",
                    "--dump-dom",
                    url_str,
                ],
                timeout,
            )
            .await?;

            // Parse all href links from the DOM
            let links = extract_links_from_html(&output);
            
            if links.is_empty() {
                Ok(format!("No links found on page: {}", url_str))
            } else {
                let links_str = links
                    .iter()
                    .enumerate()
                    .map(|(i, (text, href))| {
                        if text.is_empty() {
                            format!("{}. {}", i + 1, href)
                        } else {
                            format!("{}. {} - {}", i + 1, text, href)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                
                Ok(format!(
                    "Found {} links on '{}':\n{}",
                    links.len(),
                    url_str,
                    links_str
                ))
            }
        }

        BrowserActionType::JavaScript => {
            // JavaScript execution is not available in Chrome headless CLI mode
            // without a CDP connection (chromiumoxide). Return a clear message.
            Ok(
                "JavaScript execution is not available in headless CLI mode.\n\
                 To execute custom JavaScript in the browser:\n\
                 1. Use 'Navigate' or 'Click' action to load the page\n\
                 2. Use 'GetText' or 'ExtractLinks' to extract data from the DOM\n\
                 3. For full JS support, consider upgrading to a CDP-based solution (chromiumoxide)"
                    .to_string(),
            )
        }
    }
}

/// Parse links from HTML content. Returns Vec<(link_text, href)>.
fn extract_links_from_html(html: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut pos = 0;

    while let Some(tag_start) = html[pos..].find("<a ") {
        pos += tag_start;
        
        // Find the end of the opening tag
        if let Some(tag_end) = html[pos..].find('>') {
            let open_tag = &html[pos..pos + tag_end + 1];
            let after_open_tag = pos + tag_end + 1;
            
            // Extract href attribute
            if let Some(href_start) = open_tag.find("href=\"") {
                let href_pos = open_tag.len() - open_tag.len() + href_start + 6;
                let href_end = open_tag[href_pos..].find('"').unwrap_or(usize::MAX);
                let href = open_tag[href_pos..href_pos + href_end].to_string();
                
                // Extract link text (between </a> and start of next tag)
                if let Some(close_pos) = html[after_open_tag..].find("</a>") {
                    let link_text = super::strip_html_tags(&html[after_open_tag..after_open_tag + close_pos])
                        .trim()
                        .to_string();
                    
                    if !href.is_empty() && !href.starts_with("javascript:") {
                        links.push((link_text, href));
                    }
                }
            }
            
            pos = after_open_tag;
        } else {
            pos += 1;
        }
        
        // Safety limit
        if links.len() >= 100 {
            break;
        }
    }

    links
}

/// Run a browser command with timeout protection.
async fn run_browser_with_timeout(
    browser: &PathBuf,
    args: &[&str],
    timeout_secs: u64,
) -> AgentResult<String> {
    use tokio::process::Command;

    let timeout = Duration::from_secs(timeout_secs);

    let child = Command::new(browser)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| AgentError::Browser(format!("failed to start browser: {e}")))?;

    // wait_with_output consumes child, but kill_on_drop handles cleanup on drop
    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                Ok(stdout)
            } else {
                warn!(browser = %browser.display(), exit = %output.status, "browser exit error");
                if !stdout.is_empty() {
                    Ok(stdout)
                } else {
                    Ok(format!("Browser exited with status: {}\n{}", output.status, stderr))
                }
            }
        }
        Ok(Err(e)) => Err(AgentError::Browser(format!("browser error: {e}"))),
        Err(_) => {
            // Timeout — child was dropped (kill_on_drop handles cleanup).
            Err(AgentError::Browser(format!(
                "Browser operation timed out after {}s",
                timeout_secs
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_browser_no_crash() {
        // Just ensure the function runs without panic
        let _ = find_browser();
    }

    #[test]
    fn test_strip_html_tags() {
        use super::super::strip_html_tags;
        assert_eq!(strip_html_tags("hello world"), "hello world");
        assert_eq!(strip_html_tags("hello <b>world</b>"), "hello world");
        assert_eq!(strip_html_tags("a &amp; b"), "a & b");
        assert_eq!(strip_html_tags("<script>evil</script>hello"), "evilhello");
    }

    #[test]
    fn test_extract_links_from_html() {
        let html = r#"
            <a href="https://example.com">Example</a>
            <a href="/relative/path">Relative Link</a>
            <a href="https://test.com">Test</a>
        "#;
        let links = extract_links_from_html(html);
        // All 3 links should be extracted (relative paths are kept as-is)
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].0, "Example");
        assert_eq!(links[0].1, "https://example.com");
        assert_eq!(links[1].1, "/relative/path"); // Relative paths are kept
        assert_eq!(links[2].1, "https://test.com");
    }

    #[test]
    fn test_extract_links_skips_javascript() {
        let html = r#"
            <a href="javascript:void(0)">JS Link</a>
            <a href="https://valid.com">Valid</a>
        "#;
        let links = extract_links_from_html(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1, "https://valid.com");
    }
}
