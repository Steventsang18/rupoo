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
use tracing::{error, info};

use crate::agent::Agent;
use crate::build_engine;
use crate::channel::base::ChannelRuntime;
use crate::channel::dingtalk::DingTalkChannel;
use crate::channel::feishu::FeishuChannel;
use crate::config::RupooConfig;

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
    let (_repo, mut agent, _tool_exe) = build_engine::build_engine(&db_path)
        .await
        .context("build agent engine for channel daemon")?;
    // Tag memories stored from channel daemon as "channel" (not "agent"/CLI)
    agent.memory_source = "channel".to_string();
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
    // Start Feishu channel if configured
    if let Some(feishu_cfg) = &channel_cfg.feishu {
        // Read feishu agent profile for role-specific system prompt
        let feishu_prompt = config
            .agents
            .get("feishu")
            .and_then(|a| a.system_prompt.clone());
        let channel = FeishuChannel::with_prompt(feishu_cfg.clone(), feishu_prompt)
            .context("initialize feishu channel")?;
        let agent = Arc::clone(&agent);

        info!(
            app_id = %feishu_cfg.app_id,
            lark_mode = %feishu_cfg.lark_mode,
            "starting feishu channel listener"
        );

        let shutdown_f = shutdown.clone();
        tokio::spawn(async move {
            run_supervised_feishu(channel, agent, shutdown_f).await;
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
            "dingtalk",
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
    info!("all channels stopped");
    Ok(())
}

// ── Supervised listener ──────────────────────────────────────────────

/// Run a Feishu channel with exponential-backoff reconnection.
async fn run_supervised_feishu(
    channel: FeishuChannel,
    agent: Arc<Agent>,
    shutdown: Arc<AtomicBool>,
) {
    let mut backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
    let max_backoff = Duration::from_secs(MAX_BACKOFF_SECS);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("feishu channel: shutting down");
            return;
        }
        info!("feishu channel: connecting...");

        match channel.run_listener(&agent).await {
            Ok(()) => {
                info!("feishu channel: disconnected (clean), reconnecting");
                backoff = Duration::from_secs(INITIAL_BACKOFF_SECS);
            }
            Err(e) => {
                error!(error = %e, "feishu channel: connection error");

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
