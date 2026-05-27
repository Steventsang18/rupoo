//! Browser automation tool.
//!
//! Controls a headless Chrome/Chromium via CLI for screenshot and navigation.
//! Uses `which` to locate the browser binary in system PATH.
//!
//! # Limitations
//! - Click and GetText are not yet implemented in headless CLI mode.
//!   For full DOM interaction (click, fill forms, extract text), upgrade to
//!   `chromiumoxide` (pure Rust CDP client) in future iterations.
//!
//! # Safety
//! - Browser process is killed on timeout.
//! - Screenshots are saved to system temp directory.
//! - All operations have a hard 30-second timeout.

use std::path::PathBuf;
use std::time::Duration;

use tracing::warn;

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

/// Execute a browser action (navigate, screenshot, click, get text).
pub async fn execute_browser_action(
    action: &BrowserActionType,
    url: Option<&str>,
    timeout_secs: Option<u64>,
    safety: &SafetyContext,
) -> AgentResult<String> {
    let timeout = timeout_secs.unwrap_or(30);

    // Check unsupported actions early (before browser lookup)
    match action {
        BrowserActionType::Click => {
            return Ok("Click action is not yet supported in headless CLI mode. Use a full browser automation framework.".to_string());
        }
        BrowserActionType::GetText => {
            return Ok("GetText action is not yet supported in headless CLI mode. Use a full browser automation framework.".to_string());
        }
        _ => {}
    }

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
        AgentError::Other(
            "No supported browser found (looked for Chrome/Chromium)".into(),
        )
    })?;

    match action {
        BrowserActionType::Navigate => {
            let url_str = url.ok_or_else(|| {
                AgentError::Other("URL is required for Navigate action".into())
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
            Ok(format!("Page source ({}) chars:\n{}", output_len, truncated))
        }

        BrowserActionType::Screenshot => {
            let url_str = url.ok_or_else(|| {
                AgentError::Other("URL is required for Screenshot action".into())
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
                Err(AgentError::Other(
                    "Screenshot file was not created by browser".into(),
                ))
            }
        }
        _ => unreachable!("handled by early return above"),
    }
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
        .map_err(|e| AgentError::Other(format!("failed to start browser: {e}")))?;

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
        Ok(Err(e)) => Err(AgentError::Other(format!("browser error: {e}"))),
        Err(_) => {
            // Timeout — child was dropped (kill_on_drop handles cleanup).
            Err(AgentError::Other(format!(
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

    #[tokio::test]
    async fn test_unsupported_actions() {
        let safety = SafetyContext::default();
        let result = execute_browser_action(
            &BrowserActionType::Click,
            None,
            Some(5),
            &safety,
        )
        .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("not yet supported"));
    }
}
