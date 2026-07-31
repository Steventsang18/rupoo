//! Channel daemon — IM platform integration for rupoo.
//!
//! Provides a supervised async loop that connects to messaging platforms
//! (Feishu, DingTalk, WeCom, etc.) and bridges messages to the rupoo
//! agent kernel.

pub mod base;
pub mod dingtalk;
pub mod feishu;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::build_engine;
use crate::channel::base::ChannelRuntime;
use crate::channel::dingtalk::DingTalkChannel;
use crate::channel::feishu::FeishuChannel;
use crate::config::RupooConfig;
use crate::ops_server::{spawn_ops_server, OpsServerConfig};

// ── Constants ────────────────────────────────────────────────────────

/// Initial backoff delay on reconnect (seconds).
const INITIAL_BACKOFF_SECS: u64 = 2;
/// Maximum backoff delay on reconnect (seconds).
const MAX_BACKOFF_SECS: u64 = 60;

// ── Public API ───────────────────────────────────────────────────────

/// Start all configured channel listeners.
///
/// Loads the agent engine, reads channel config, and runs each enabled
/// channel in a supervised reconnect loop. Blocks until a fatal error
/// or shutdown signal.
pub async fn run_channel_daemon(config_override: Option<RupooConfig>) -> Result<()> {
    info!("starting channel daemon");

    // Load config
    let config = match config_override {
        Some(cfg) => cfg,
        None => RupooConfig::load().context("load rupoo config for channel daemon")?,
    };

    let channel_cfg = &config.channel;

    // Resolve DB path
    let db_path = channel_cfg.db_path.clone().unwrap_or_else(|| {
        crate::config::rupoo_home()
            .join("agent.db")
            .to_string_lossy()
            .to_string()
    });

    // Build agent engine
    info!(db = %db_path, "building agent engine for channel daemon");
    let (repo, mut agent, _tool_exe) = build_engine::build_engine(&db_path)
        .await
        .context("build agent engine for channel daemon")?;
    // Tag memories stored from channel daemon as "channel" (not "agent"/CLI)
    agent.memory_source = "channel".to_string();
    if let Some(profile) = config.agents.get("feishu") {
        if profile.allowed_tools.is_some() || profile.excluded_tools.is_some() {
            agent.tool_executor =
                std::sync::Arc::new(crate::channel::base::FilteredToolExecutor::new(
                    agent.tool_executor.clone(),
                    profile.allowed_tools.clone(),
                    profile.excluded_tools.clone(),
                ));
        }
    }

    // Start Feishu channel if configured
    let agent = Arc::new(agent);

    // Check if agent has LLM configured
    if !agent.has_llm() {
        anyhow::bail!(
            "No LLM configured. Set an API key in {}credentials.toml or \
             set the {}_API_KEY environment variable before starting the channel daemon.",
            crate::config::rupoo_home().display(),
            "LLM_PROVIDER"
        );
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let sig = shutdown.clone();

    // Apply tool filter from agent profile (allowed_tools / excluded_tools)
    if let Some(feishu_cfg) = &channel_cfg.feishu {
        // 飞书走共享运行时：会话管理 / slash 命令 / agent_chat 全部收敛到 ChannelRuntime。
        let runtime = Arc::new(ChannelRuntime::new(
            Arc::clone(&agent),
            config.clone(),
            "feishu",
            Some(repo.clone()),
        ));
        let channel = FeishuChannel::with_runtime(feishu_cfg.clone(), runtime)
            .context("initialize feishu channel")?;

        info!(
            app_id = %feishu_cfg.app_id,
            lark_mode = %feishu_cfg.lark_mode,
            "starting feishu channel listener"
        );

        let shutdown_f = shutdown.clone();
        tokio::spawn(async move {
            run_supervised_feishu(channel, shutdown_f).await;
        });
    } else {
        info!("no feishu channel configured, skipping");
    }

    // Start DingTalk channel if configured
    if let Some(dd_cfg) = &channel_cfg.dingtalk {
        let runtime = Arc::new(ChannelRuntime::new(
            Arc::clone(&agent),
            config.clone(),
            "dingtalk",
            Some(repo.clone()),
        ));
        let channel = DingTalkChannel::new(
            dd_cfg.client_id.clone(),
            dd_cfg.client_secret.clone(),
            runtime,
        )
        .context("initialize dingtalk channel")?;

        info!(
            client_id = %dd_cfg.client_id,
            "starting dingtalk channel listener"
        );

        let shutdown_dd = shutdown.clone();
        tokio::spawn(async move {
            run_supervised_dingtalk(channel, shutdown_dd).await;
        });
    } else {
        info!("no dingtalk channel configured, skipping");
    }

    // Ops server: /healthz + /metrics for process supervisors.
    // Optional by design — a bind failure must never take down the daemon.
    let _ = crate::telemetry::init();
    let ops_handle = if config.server.enabled {
        spawn_ops_server(OpsServerConfig {
            listen: config.server.listen.clone(),
            max_concurrency: config.server.max_concurrency,
        })
        .await
    } else {
        info!("ops server disabled by config");
        None
    };

    // Config hot-reload: watches config.toml and applies `[logging] level`
    // live. Optional by design — a watch failure must not stop the daemon.
    if let Some(path) = config.source_path.clone() {
        if let Some(level_ctrl) = crate::tracing_setup::level_controller() {
            match crate::config_watch::ConfigWatcher::start(path) {
                Ok(watcher) => {
                    tokio::spawn(watcher.run(level_ctrl.clone()));
                }
                Err(e) => warn!(
                    error = %e,
                    "config hot-reload disabled: cannot watch config.toml"
                ),
            }
        }
    }

    // Wait for shutdown signal

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("shutdown signal received, stopping channel daemon");
        sig.store(true, Ordering::SeqCst);
    });

    // Also listen on unix for SIGTERM
    #[cfg(unix)]
    {
        let sig_flag = shutdown.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut term = signal(SignalKind::terminate()).ok();
            if let Some(ref mut sig) = term {
                sig.recv().await;
                info!("SIGTERM received, stopping channel daemon");
                sig_flag.store(true, Ordering::SeqCst);
            }
        });
    }

    // Wait for shutdown, polling every second
    while !shutdown.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if let Some(handle) = ops_handle {
        handle.abort();
    }
    info!("all channels stopped");
    Ok(())
}

// ── Supervised listener ──────────────────────────────────────────────

/// Run a Feishu channel with exponential-backoff reconnection.
async fn run_supervised_feishu(channel: FeishuChannel, shutdown: Arc<AtomicBool>) {
    let mut backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
    let max_backoff = Duration::from_secs(MAX_BACKOFF_SECS);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("feishu channel: shutting down");
            return;
        }
        info!("feishu channel: connecting...");

        match channel.run_listener().await {
            Ok(()) => {
                info!("feishu channel: disconnected (clean), reconnecting");
                backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
            }
            Err(e) => {
                error!(error = %e, "feishu channel: connection error");
                crate::telemetry::record_channel_error("feishu");

                // Check for unrecoverable errors
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("401")
                    || err_str.contains("unauthorized")
                    || err_str.contains("forbidden")
                {
                    error!("feishu channel: unrecoverable auth error, waiting for config reload");
                    std::future::pending::<()>().await;
                }

                error!(
                    error = %e,
                    retry_in_secs = backoff.as_secs(),
                    "feishu channel: will reconnect"
                );
            }
        }

        // Wait with backoff
        sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}
/// Run a DingTalk channel with exponential-backoff reconnection.
async fn run_supervised_dingtalk(channel: DingTalkChannel, shutdown: Arc<AtomicBool>) {
    let mut backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
    let max_backoff = Duration::from_secs(MAX_BACKOFF_SECS);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("dingtalk channel: shutting down");
            return;
        }
        info!("dingtalk channel: connecting...");

        match channel.run_listener().await {
            Ok(()) => {
                info!("dingtalk channel: disconnected (clean), reconnecting");
                backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
            }
            Err(e) => {
                error!(error = %e, "dingtalk channel: connection error");
                crate::telemetry::record_channel_error("dingtalk");
                if e.to_string().contains("401") || e.to_string().contains("unauthorized") {
                    error!("dingtalk channel: unrecoverable auth error");
                    std::future::pending::<()>().await;
                }
                error!(
                    retry_in_secs = backoff.as_secs(),
                    "dingtalk channel: will reconnect"
                );
            }
        }

        sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Configuration for a Feishu channel.
/// Re-exported for convenience.
pub use crate::config::FeishuConfig as ChannelFeishuConfig;
