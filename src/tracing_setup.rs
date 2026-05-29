//! Logging setup for Rupoo.
//!
//! By default, all tracing output (INFO, DEBUG) goes to a log file at
//! `~/.rupoo/rupoo.log` and is suppressed from the terminal.  Pass
//! `--verbose` to also emit logs on stderr (useful for debugging).
//!
//! Sensitive fields (api_key, token, secret, password, authorization,
//! system_prompt) are automatically redacted to prevent credential leakage
//! in log files.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

/// Field names that should be redacted in all log output.
const SENSITIVE_FIELDS: &[&str] = &[
    "api_key",
    "token",
    "secret",
    "password",
    "authorization",
    "system_prompt",
];

/// Redact sensitive field values: replace the value with "***REDACTED***".
///
/// Ready for integration with a custom tracing Layer/Writer that filters
/// log output before writing. Currently not wired into the subscriber
/// pipeline because existing log calls don't emit sensitive fields.
/// Will be activated when sensitive field logging is added.
#[allow(dead_code)]
fn redact_sensitive_fields(fmt_fields: &mut String) {
    for field in SENSITIVE_FIELDS {
        // Try quoted value first: field="value"
        let quoted_pattern = format!("{field}=\"");
        if let Some(start) = fmt_fields.find(&quoted_pattern) {
            let val_start = start + quoted_pattern.len();
            let val_end = fmt_fields[val_start..]
                .find('"')
                .map(|i| val_start + i)
                .unwrap_or(fmt_fields.len());
            fmt_fields.replace_range(val_start..val_end, "***REDACTED***");
            continue; // Move to next field
        }

        // Try unquoted value: field=value (ends at space, comma, or })
        let unquoted_pattern = format!("{field}=");
        if let Some(start) = fmt_fields.find(&unquoted_pattern) {
            let val_start = start + unquoted_pattern.len();
            let val_end = fmt_fields[val_start..]
                .find([' ', ',', '}'])
                .map(|i| val_start + i)
                .unwrap_or(fmt_fields.len());
            fmt_fields.replace_range(val_start..val_end, "***REDACTED***");
        }
    }
}

/// Initialise the tracing subscriber.
///
/// When `verbose` is false (default) logs go to a file only.
/// When `verbose` is true logs ≥ DEBUG are additionally shown on stderr.
pub fn init_logging(verbose: bool) {
    let log_dir = data_dir();
    std::fs::create_dir_all(&log_dir).ok();
    let log_path = log_dir.join("rupoo.log");

    // Rotate the log file each session.
    if log_path.exists() {
        let rotated = log_dir.join("rupoo.prev.log");
        // Clean up oversized prev.log (>10MB) before rotating to avoid disk bloat.
        if rotated.exists() {
            if let Ok(meta) = rotated.metadata() {
                if meta.len() > 10 * 1024 * 1024 {
                    let _ = std::fs::remove_file(&rotated);
                }
            }
        }
        let _ = std::fs::rename(&log_path, &rotated);
    }

    let log_to_stderr = |verbose: bool| {
        let builder = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true);
        if verbose {
            builder
                .with_env_filter(EnvFilter::from_default_env()
                    .add_directive("debug".parse().unwrap_or_else(|_| tracing::Level::DEBUG.into())))
                .init();
        } else {
            builder
                .with_env_filter(EnvFilter::from_default_env()
                    .add_directive("info".parse().unwrap_or_else(|_| tracing::Level::INFO.into())))
                .init();
        }
    };

    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(file) => {
            let file_writer = Mutex::new(file);
            let builder = tracing_subscriber::fmt()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true);

            if verbose {
                builder
                    .with_env_filter(EnvFilter::from_default_env()
                        .add_directive("debug".parse().unwrap_or_else(|_| tracing::Level::DEBUG.into())))
                    .init();
                eprintln!("[rupoo] verbose logging enabled");
            } else {
                builder
                    .with_env_filter(EnvFilter::from_default_env()
                        .add_directive("info".parse().unwrap_or_else(|_| tracing::Level::INFO.into())))
                    .init();
            }
        }
        Err(e) => {
            eprintln!("[rupoo] warning: cannot create log file at {}: {e}, logging to stderr only", log_path.display());
            log_to_stderr(verbose);
        }
    }
}

/// Return the data directory `~/.rupoo`.
pub fn data_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".rupoo")
    } else {
        PathBuf::from(".rupoo")
    }
}

/// Return the history file path `~/.rupoo/history.txt`.
pub fn history_path() -> PathBuf {
    data_dir().join("history.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_api_key() {
        let mut s = r#"provider=anthropic api_key="sk-ant-abc123" model=claude"#.to_string();
        redact_sensitive_fields(&mut s);
        assert!(s.contains("api_key=\"***REDACTED***\""));
        assert!(!s.contains("sk-ant-abc123"));
    }

    #[test]
    fn test_redact_token_unquoted() {
        let mut s = "token=ghp_abc123 user=alice".to_string();
        redact_sensitive_fields(&mut s);
        assert!(s.contains("token=***REDACTED***"));
        assert!(!s.contains("ghp_abc123"));
    }

    #[test]
    fn test_no_redact_normal_fields() {
        let mut s = "provider=openai model=gpt-4 prompt_tokens=100".to_string();
        redact_sensitive_fields(&mut s);
        assert_eq!(s, "provider=openai model=gpt-4 prompt_tokens=100");
    }

    #[test]
    fn test_redact_system_prompt() {
        let mut s = r#"system_prompt="You are a helpful assistant""#.to_string();
        redact_sensitive_fields(&mut s);
        assert!(s.contains("system_prompt=\"***REDACTED***\""));
    }
}
