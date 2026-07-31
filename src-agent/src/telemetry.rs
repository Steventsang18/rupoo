//! Runtime metrics for the serve daemon.
//!
//! Uses the `metrics` crate for instrumenting, with a Prometheus text
//! recorder installed once at startup. The ops server (`ops_server`)
//! exposes the rendered snapshot at `/metrics`.
//!
//! Instrumented signals (serve mode):
//! - `rupoo_messages_received_total{channel}` — IM messages entering the daemon
//! - `rupoo_messages_replied_total{channel}`  — replies sent back to IM platforms
//! - `rupoo_channel_errors_total{channel}`    — channel connection/processing errors
//! - `rupoo_llm_call_duration_seconds`        — LLM round-trip latency (histogram)
//!
//! Channel message rates are low (human IM traffic), so call sites use the
//! `metrics` macros directly rather than cached handles.

use std::sync::{Mutex, OnceLock};

use metrics::describe_counter;
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::{error, info};

/// Process-wide Prometheus handle, installed once.
static PROMETHEUS: OnceLock<metrics_exporter_prometheus::PrometheusHandle> = OnceLock::new();

/// Serializes recorder installation. `metrics` owns a process-global recorder
/// slot, so parallel callers (e.g. concurrent tests) must not race it.
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Install the Prometheus recorder and register metric descriptions.
///
/// Idempotent: a second call in the same process is a no-op. Returns
/// `false` only if installation genuinely fails (practically never).
pub fn init() -> bool {
    if PROMETHEUS.get().is_some() {
        return true;
    }
    // Double-checked locking: only one thread may reach install_recorder,
    // the metrics crate rejects a second install in the same process.
    let _guard = INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if PROMETHEUS.get().is_some() {
        return true;
    }
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            describe_metrics();
            let _ = PROMETHEUS.set(handle);
            info!("prometheus metrics recorder installed");
            true
        }
        Err(e) => {
            error!(error = %e, "failed to install metrics recorder");
            false
        }
    }
}

/// Render the current metrics snapshot as Prometheus text format.
///
/// Returns `None` when the recorder was never installed — the caller
/// should respond 503 in that case rather than emit a blank body.
pub fn render_prometheus() -> Option<String> {
    PROMETHEUS.get().map(|handle| handle.render())
}

fn describe_metrics() {
    use metrics::Unit;

    describe_counter!(
        "rupoo_messages_received_total",
        Unit::Count,
        "Messages received from IM platforms"
    );
    describe_counter!(
        "rupoo_messages_replied_total",
        Unit::Count,
        "Replies sent to IM platforms"
    );
    describe_counter!(
        "rupoo_channel_errors_total",
        Unit::Count,
        "Channel connection or processing errors"
    );
    metrics::describe_histogram!(
        "rupoo_llm_call_duration_seconds",
        Unit::Seconds,
        "LLM round-trip latency per call"
    );
}

// ---------------------------------------------------------------------------
// Recording helpers — the only instrumentation entry points used by callers
// ---------------------------------------------------------------------------

/// Record an incoming IM message for `channel` (e.g. "feishu").
pub fn record_message_received(channel: &str) {
    metrics::counter!("rupoo_messages_received_total", "channel" => channel.to_string())
        .increment(1);
}

/// Record a reply sent back to `channel`.
pub fn record_message_replied(channel: &str) {
    metrics::counter!("rupoo_messages_replied_total", "channel" => channel.to_string())
        .increment(1);
}

/// Record a channel-level error (connect/process failure) for `channel`.
pub fn record_channel_error(channel: &str) {
    metrics::counter!("rupoo_channel_errors_total", "channel" => channel.to_string()).increment(1);
}

/// Record an LLM call duration in seconds.
pub fn record_llm_call_duration(secs: f64) {
    metrics::histogram!("rupoo_llm_call_duration_seconds").record(secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_render_after_init() {
        // Install is idempotent — safe even if another test ran first.
        assert!(init());
        record_message_received("feishu");
        record_llm_call_duration(0.42);
        let snapshot = render_prometheus().expect("recorder must be installed");
        assert!(snapshot.contains("rupoo_messages_received_total"));
        assert!(snapshot.contains("rupoo_llm_call_duration_seconds"));
        assert!(snapshot.contains("channel=\"feishu\""));
    }
}
