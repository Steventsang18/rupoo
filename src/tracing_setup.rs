//! Logging setup for Rupoo.
//!
//! By default, all tracing output (INFO, DEBUG) goes to a log file at
//! `~/.rupoo/rupoo.log` and is suppressed from the terminal.  Pass
//! `--verbose` to also emit logs on stderr (useful for debugging).

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;

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
