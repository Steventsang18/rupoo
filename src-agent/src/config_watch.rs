//! Config hot-reload — watches `config.toml` and applies runtime settings.
//!
//! Only `[logging] level` is hot-applied today (via the live tracing
//! subscriber). Other sections are read once at startup and changing them
//! still requires a restart; this module stays deliberately small so the
//! whitelist is easy to extend.
//!
//! A malformed config file is reported and the previous state is kept —
//! a bad edit must never take down the serve daemon.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::RupooConfig;
use crate::tracing_setup::LogLevelController;

/// Debounce window for filesystem events — editors emit a burst on save.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// Watcher handle; keep alive for the lifetime of the daemon.
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: mpsc::UnboundedReceiver<PathBuf>,
}

impl ConfigWatcher {
    /// Watch the directory containing `config_path`.
    ///
    /// The watcher thread only forwards matching paths into a channel; all
    /// parsing happens on the async side, so a slow disk never stalls the
    /// notify callback.
    pub fn start(config_path: PathBuf) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        let watch_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(crate::config::rupoo_home);

        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let event = match res {
                    Ok(e) => e,
                    Err(_) => return,
                };
                // Match by file name: editors often save via rename, which
                // surfaces as a Create event on the final path.
                let hit = event
                    .paths
                    .iter()
                    .any(|p| p.file_name() == config_path.file_name());
                if hit {
                    let _ = tx.send(config_path.clone());
                }
            })?;

        watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Consume filesystem events with debouncing and apply hot settings.
    pub async fn run(mut self, level_ctrl: LogLevelController) {
        let mut last_level = String::new();
        while let Some(path) = self.rx.recv().await {
            // Wait for the save burst to settle, then drop stale events.
            tokio::time::sleep(DEBOUNCE).await;
            while self.rx.try_recv().is_ok() {}

            if let Err(e) = Self::reload(&path, &level_ctrl, &mut last_level) {
                warn!(
                    error = %e,
                    path = %path.display(),
                    "config reload failed, keeping previous settings"
                );
            }
        }
    }

    /// Re-parse the config and apply hot-reloadable fields.
    fn reload(
        path: &Path,
        level_ctrl: &LogLevelController,
        last_level: &mut String,
    ) -> Result<(), String> {
        let config = RupooConfig::load_from(path).map_err(|e| format!("load: {e}"))?;
        config.validate().map_err(|e| format!("validate: {e}"))?;

        // Whitelist: only logging.level is applied live.
        let level = config.logging.level;
        if level != *last_level {
            if level_ctrl.set_level(&level) {
                info!(level = %level, "log level hot-reloaded from config.toml");
                *last_level = level;
            } else {
                warn!(
                    level = %level,
                    "log level rejected by tracing, keeping current level"
                );
            }
        }
        Ok(())
    }
}
