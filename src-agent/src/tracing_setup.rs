//! Logging setup for Rupoo.
//!
//! By default, all tracing output (INFO, DEBUG) goes to a log file at
//! `$RUPOO_HOME/rupoo.log` and is suppressed from the terminal.  Pass
//! `--verbose` to also emit logs on stderr (useful for debugging).
//!
//! Sensitive fields (api_key, token, secret, password, authorization,
//! system_prompt) are automatically redacted to prevent credential leakage
//! in log files.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::warn;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{reload, EnvFilter};

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
/// Integrated into the subscriber pipeline via `RedactingWriter` to ensure
/// no credentials leak into log files.
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

/// A writer wrapper that redacts sensitive fields before writing to the underlying writer.
struct RedactingWriter<W> {
    inner: Arc<Mutex<W>>,
}

impl<W: Write + 'static> RedactingWriter<W> {
    fn new(inner: Arc<Mutex<W>>) -> Self {
        Self { inner }
    }
}

impl<W: Write + 'static> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut line = String::from_utf8_lossy(buf).into_owned();
        redact_sensitive_fields(&mut line);
        let mut guard = self.inner.lock().unwrap_or_else(|poisoned| {
            eprintln!("[rupoo] log writer mutex poisoned, recovering");
            poisoned.into_inner()
        });
        guard.write(line.as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut guard = self.inner.lock().unwrap_or_else(|poisoned| {
            eprintln!("[rupoo] log writer mutex poisoned, recovering");
            poisoned.into_inner()
        });
        guard.flush()
    }
}

impl<W: Write + 'static> tracing_subscriber::fmt::MakeWriter<'_> for RedactingWriter<W> {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Runtime handle for adjusting the log level of the live subscriber.
///
/// Obtained from [`init_logging`] (or [`level_controller`] afterwards) and
/// used by the config hot-reload watcher to apply `[logging] level` live.
#[derive(Clone)]
pub struct LogLevelController {
    handle: reload::Handle<EnvFilter, tracing_subscriber::Registry>,
}

impl LogLevelController {
    /// Switch the global log level, keeping the `RUST_LOG` base filter.
    /// Returns false if `level` is not a valid tracing level.
    pub fn set_level(&self, level: &str) -> bool {
        let directive = match level.parse() {
            Ok(d) => d,
            Err(_) => return false,
        };
        let filter = EnvFilter::from_default_env().add_directive(directive);
        self.handle.reload(filter).is_ok()
    }
}

/// Controller of the process-wide log level, if logging was initialised.
static LEVEL_CTRL: OnceLock<LogLevelController> = OnceLock::new();

/// Return the live log-level controller, if any.
///
/// `None` before [`init_logging`] runs — callers (e.g. the config watcher)
/// should treat that as "hot-reload unavailable".
pub fn level_controller() -> Option<&'static LogLevelController> {
    LEVEL_CTRL.get()
}

/// Initialise the tracing subscriber.
///
/// When `verbose` is false (default) logs go to a file only.
/// When `verbose` is true logs ≥ DEBUG are additionally shown on stderr.
///
/// Returns a controller that can switch the level at runtime.
pub fn init_logging(verbose: bool) -> LogLevelController {
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
                    if let Err(e) = std::fs::remove_file(&rotated) {
                        warn!(error = %e, path = %rotated.display(), "failed to remove oversized log file");
                    }
                }
            }
        }
        if let Err(e) = std::fs::rename(&log_path, &rotated) {
            warn!(error = %e, from = %log_path.display(), to = %rotated.display(), "failed to rotate log file");
        }
    }

    // Base filter honouring RUST_LOG, with a built-in floor at `verbose`.
    let default_level = if verbose { "debug" } else { "info" };
    let base = EnvFilter::from_default_env().add_directive(
        default_level
            .parse()
            .unwrap_or_else(|_| tracing::Level::INFO.into()),
    );
    let (filter, handle): (
        reload::Layer<EnvFilter, tracing_subscriber::Registry>,
        reload::Handle<EnvFilter, tracing_subscriber::Registry>,
    ) = reload::Layer::new(base);

    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(file) => {
            let file_writer = Arc::new(Mutex::new(file));
            let redacting = RedactingWriter::new(file_writer);
            tracing_subscriber::registry()
                .with(filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(redacting)
                        .with_ansi(false)
                        .with_target(true)
                        .with_thread_ids(true),
                )
                .init();
            if verbose {
                eprintln!("[rupoo] verbose logging enabled");
            }
        }
        Err(e) => {
            eprintln!(
                "[rupoo] warning: cannot create log file at {}: {e}, logging to stderr only",
                log_path.display()
            );
            tracing_subscriber::registry()
                .with(filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_target(true)
                        .with_thread_ids(true),
                )
                .init();
        }
    }

    let ctrl = LogLevelController { handle };
    let _ = LEVEL_CTRL.set(ctrl.clone());
    ctrl
}

/// Return the data directory — see [`rupoo::rupoo_home()`].
///
/// Respects `$RUPOO_HOME` (or falls back to `~/.rupoo`).
/// On Windows, ignores `RUPOO_HOME` and uses `%APPDATA%\rupoo` instead.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("rupoo")
    }

    #[cfg(not(target_os = "windows"))]
    {
        crate::rupoo_home()
    }
}

/// Return the data directory as a `String`, or `Err` if the path is not valid UTF-8.
/// Used by the global panic hook to write crash logs without allocating a `PathBuf`.
pub fn data_dir_str() -> Result<String, std::path::PathBuf> {
    let dir = data_dir();
    dir.into_os_string().into_string().map_err(|_| data_dir())
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
