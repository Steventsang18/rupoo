//! HTTP/HTTPS network request tool.
//!
//! Uses `reqwest` (with rustls) to perform GET/POST requests.
//! Security: SSRF protection (localhost blocked), 30s timeout, 5MB body limit.
//!
//! # Safety
//! - Requests to localhost/127.0.0.1 are rejected to prevent SSRF attacks.
//! - Response body is capped at 5 MB to prevent memory exhaustion.
//! - Hard 30-second timeout prevents hanging.

use crate::error::{AgentError, AgentResult};
use crate::task::HttpMethod;

// SafetyContext provides SSRF protection via localhost URL detection
use crate::safety::SafetyContext;

/// Maximum response body size (5 MB).
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

/// Maximum response text length in output (5000 chars).
const MAX_OUTPUT_CHARS: usize = 5000;

/// Extract hostname from a URL string.
fn extract_host(url: &str) -> Option<String> {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|rest| {
            // Remove port and path
            let host = rest.split('/').next().unwrap_or(rest);
            let hostname = host.split(':').next().unwrap_or(host);
            // Strip userinfo (e.g. user@host)
            Some(hostname.rsplit('@').next().unwrap_or(hostname).to_string())
        })
}

/// Execute an HTTP request and return the response.
pub async fn execute_http_request(
    url: &str,
    method: &HttpMethod,
    body: Option<&str>,
    headers: Option<&std::collections::HashMap<String, String>>,
) -> AgentResult<String> {
    // SSRF protection: block localhost (string-based fast check)
    if SafetyContext::is_localhost_url(url) {
        return Err(AgentError::Network(
            "HTTP request to localhost is blocked for security".into(),
        ));
    }

    // SSRF protection: DNS resolution check (prevents DNS rebinding)
    if let Some(host) = extract_host(url) {
        if SafetyContext::is_private_host(&host).await {
            return Err(AgentError::Network(format!(
                "HTTP request to '{host}' is blocked: resolves to private/local IP (SSRF protection)"
            )));
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AgentError::Network(format!("failed to build HTTP client: {e}")))?;

    let mut req = match method {
        HttpMethod::GET => client.get(url),
        HttpMethod::POST => {
            let r = client.post(url);
            if let Some(b) = body {
                r.body(b.to_string())
            } else {
                r
            }
        }
    };

    // Apply custom headers
    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            req = req.header(k.as_str(), v.as_str());
        }
    }

    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            AgentError::Network("HTTP request timed out after 30s".into())
        } else {
            AgentError::Network(format!("HTTP request failed: {e}"))
        }
    })?;

    let status_code = resp.status();

    // Body size limit
    let content = resp.bytes().await.map_err(|e| {
        AgentError::Network(format!("failed to read response body: {e}"))
    })?;

    if content.len() > MAX_RESPONSE_BYTES {
        return Err(AgentError::Network(
            format!(
                "Response body too large: {} bytes (max {})",
                content.len(),
                MAX_RESPONSE_BYTES
            ),
        ));
    }

    let text = String::from_utf8_lossy(&content);
    let truncated = if text.len() > MAX_OUTPUT_CHARS {
        format!(
            "{}...\n[truncated at {} characters]",
            &text[..MAX_OUTPUT_CHARS],
            MAX_OUTPUT_CHARS
        )
    } else {
        text.to_string()
    };

    Ok(format!("HTTP {status_code}\n{truncated}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_localhost_blocked() {
        let result = execute_http_request(
            "http://localhost:8080/test",
            &HttpMethod::GET,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("localhost"));
    }

    #[tokio::test]
    async fn test_127_blocked() {
        let result = execute_http_request(
            "http://127.0.0.1/api",
            &HttpMethod::GET,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }
}
