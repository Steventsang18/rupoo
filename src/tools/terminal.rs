//! Terminal command execution tool.
//!
//! Uses `tokio::process::Command` for async subprocess management with timeout.
//! Integrates with `SafetyContext` for command validation.
//!
//! # Safety
//! - All commands are validated against the blacklist via `SafetyContext`.
//! - Environment variables that could leak secrets are stripped.
//! - Output is capped at 10,000 characters.
//! - Timeout forces process termination.

use std::time::Duration;

use tokio::process::Command;

use crate::error::{AgentError, AgentResult};
use super::super::safety::SafetyContext;

/// Maximum command output length.
const MAX_OUTPUT_CHARS: usize = 10_000;

/// Execute a command with safety checks.
pub async fn execute_command(
    command: &str,
    args: &[String],
    timeout_secs: Option<u64>,
    safety: &SafetyContext,
) -> AgentResult<String> {
    // Security check: validate command
    safety.validate_command(command)?;

    let timeout = timeout_secs
        .map(Duration::from_secs)
        .unwrap_or(safety.default_timeout);

    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    // Security: strip sensitive environment variables before execution.
    // This prevents child processes from leaking credentials.
    // Only essential safe vars are preserved.
    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd.env("HOME", std::env::var("HOME").unwrap_or_default());
    cmd.env("USER", std::env::var("USER").unwrap_or_default());
    cmd.env("SHELL", std::env::var("SHELL").unwrap_or_default());
    cmd.env("LANG", std::env::var("LANG").unwrap_or_default());
    cmd.env("TERM", std::env::var("TERM").unwrap_or_default());
    // NOTE: The following are explicitly NOT forwarded:
    // AWS_*, GITHUB_*, TOKEN, SECRET, PASSWORD, KEY, DOCKER_AUTH

    let child = cmd.spawn().map_err(|e| {
        AgentError::Other(format!("failed to start '{}': {e}", command))
    })?;

    // Use wait_with_output which captures stdout/stderr and waits for exit
    let result = tokio::time::timeout(timeout, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = if stderr.is_empty() {
                stdout.to_string()
            } else {
                format!("{stdout}\n{stderr}")
            };

            let truncated = if combined.len() > MAX_OUTPUT_CHARS {
                format!(
                    "{}...\n[output truncated at {} characters]",
                    &combined[..MAX_OUTPUT_CHARS],
                    MAX_OUTPUT_CHARS
                )
            } else {
                combined
            };

            if output.status.success() {
                Ok(truncated)
            } else {
                Ok(format!("Exit code: {}\n{}", output.status, truncated))
            }
        }
        Ok(Err(e)) => Err(AgentError::Other(format!("command failed: {e}"))),
        Err(_) => Err(AgentError::Other(format!(
            "Command '{}' timed out after {}s",
            command,
            timeout.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_command() {
        let safety = SafetyContext::default();
        let result = execute_command("echo", &["hello yupoo".into()], Some(5), &safety).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hello yupoo"));
    }

    #[tokio::test]
    async fn test_forbidden_command() {
        let safety = SafetyContext::default();
        let result = execute_command("sudo", &["echo".into(), "test".into()], Some(5), &safety).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_timeout() {
        let safety = SafetyContext::default();
        let result = execute_command("sleep", &["10".into()], Some(2), &safety).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }
}
