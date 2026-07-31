//! Retry logic with exponential backoff for transient failures.
//!
//! # Key Types
//!
//! - [`RetryConfig`] — configurable retry policy (max retries, delays,
//!   network/rate-limit gating)
//! - [`RetryConfigBuilder`] — fluent builder for [`RetryConfig`]
//! - [`RetryExt`] — extension trait for chaining `.retry()` on closures
//!
//! # Core Functions
//!
//! - [`retry()`] — synchronous operation with retry
//! - [`retry_async()`] — async operation with retry
//!
//! # Retry Decision
//!
//! Retries are only attempted for [`AgentError::Network`],
//! [`AgentError::ConnectionTimeout`], [`AgentError::DnsResolutionFailed`],
//! and [`AgentError::LlmRateLimited`] — and only when the corresponding flags
//! in [`RetryConfig`] are enabled.
//!
//! # Example
//!
//! ```rust,no_run
//! use rupoo::retry::{retry, RetryConfig};
//! use rupoo::error::AgentResult;
//!
//! async fn example() -> AgentResult<String> {
//!     let config = RetryConfig::builder()
//!         .max_retries(3)
//!         .build();
//!     retry(|| Ok("success".into()), config).await
//! }
//! ```

use std::time::Duration;

use tokio::time::sleep;

use crate::error::AgentError;

/// Configuration for automatic retry with exponential backoff.
///
/// # Defaults
///
/// - `max_retries`: 3
/// - `initial_delay`: 1 second
/// - `backoff_multiplier`: 2.0× per attempt
/// - `max_delay`: 30 seconds
/// - `retry_network_errors`: true
/// - `retry_rate_limits`: true
///
/// Use [`RetryConfig::builder()`] for fluent customization.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries before giving up.
    pub max_retries: usize,
    /// Initial delay between retries.
    pub initial_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Whether to retry on network errors.
    pub retry_network_errors: bool,
    /// Whether to retry on rate limiting errors.
    pub retry_rate_limits: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(30),
            retry_network_errors: true,
            retry_rate_limits: true,
        }
    }
}

impl RetryConfig {
    /// Create a builder-style config for quick customization.
    pub fn builder() -> RetryConfigBuilder {
        RetryConfigBuilder(Self::default())
    }
}

/// Builder for RetryConfig.
pub struct RetryConfigBuilder(RetryConfig);

impl RetryConfigBuilder {
    pub fn max_retries(mut self, max_retries: usize) -> Self {
        self.0.max_retries = max_retries;
        self
    }

    pub fn initial_delay(mut self, initial_delay: Duration) -> Self {
        self.0.initial_delay = initial_delay;
        self
    }

    pub fn backoff_multiplier(mut self, backoff_multiplier: f64) -> Self {
        self.0.backoff_multiplier = backoff_multiplier;
        self
    }

    pub fn max_delay(mut self, max_delay: Duration) -> Self {
        self.0.max_delay = max_delay;
        self
    }

    pub fn retry_network_errors(mut self, retry_network_errors: bool) -> Self {
        self.0.retry_network_errors = retry_network_errors;
        self
    }

    pub fn retry_rate_limits(mut self, retry_rate_limits: bool) -> Self {
        self.0.retry_rate_limits = retry_rate_limits;
        self
    }

    pub fn build(self) -> RetryConfig {
        self.0
    }
}

/// Check if an error is retryable based on the configuration.
fn is_retryable_error(err: &AgentError, config: &RetryConfig) -> bool {
    match err {
        AgentError::Network(_) => config.retry_network_errors,
        AgentError::ConnectionTimeout => config.retry_network_errors,
        AgentError::DnsResolutionFailed { .. } => config.retry_network_errors,
        AgentError::LlmRateLimited { .. } => config.retry_rate_limits,
        _ => false,
    }
}

/// Execute a synchronous closure with retry on transient failures.
///
/// Retries are governed by [`RetryConfig`]. Only network errors and rate-limit
/// errors trigger retries (when enabled). Non-retryable errors are returned
/// immediately without waiting.
///
/// # Example
///
/// ```rust,no_run
/// use rupoo::retry::{retry, RetryConfig};
///
/// async fn example() -> rupoo::error::AgentResult<String> {
///     retry(|| Ok("done".into()), RetryConfig::default()).await
/// }
/// ```
pub async fn retry<T, F>(operation: F, config: RetryConfig) -> crate::error::AgentResult<T>
where
    F: Fn() -> crate::error::AgentResult<T>,
{
    let mut delay = config.initial_delay;

    for attempt in 0..=config.max_retries {
        match operation() {
            Ok(result) => return Ok(result),
            Err(err) => {
                if attempt == config.max_retries || !is_retryable_error(&err, &config) {
                    return Err(err);
                }

                tracing::warn!(
                    attempt = attempt,
                    max_retries = config.max_retries,
                    delay_ms = delay.as_millis(),
                    error = %err,
                    "Retrying operation"
                );

                sleep(delay).await;

                delay = Duration::from_millis(
                    (delay.as_millis() as f64 * config.backoff_multiplier) as u64,
                )
                .min(config.max_delay);
            }
        }
    }

    unreachable!("Should have returned before reaching here")
}

/// Execute an async closure with retry on transient failures.
///
/// Like [`retry()`] but the operation closure returns a `Pin<Box<dyn Future>>`.
/// Useful when the operation itself is async (e.g., HTTP calls, LLM API requests).
pub async fn retry_async<T, F>(operation: F, config: RetryConfig) -> crate::error::AgentResult<T>
where
    F: Fn() -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::error::AgentResult<T>> + Send>,
    >,
{
    let mut delay = config.initial_delay;

    for attempt in 0..=config.max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                if attempt == config.max_retries || !is_retryable_error(&err, &config) {
                    return Err(err);
                }

                tracing::warn!(
                    attempt = attempt,
                    max_retries = config.max_retries,
                    delay_ms = delay.as_millis(),
                    error = %err,
                    "Retrying async operation"
                );

                sleep(delay).await;

                delay = Duration::from_millis(
                    (delay.as_millis() as f64 * config.backoff_multiplier) as u64,
                )
                .min(config.max_delay);
            }
        }
    }

    unreachable!("Should have returned before reaching here")
}

/// Helper trait for chaining retry logic on results.
pub trait RetryExt<T> {
    /// Retry this operation with default configuration.
    fn retry(self) -> crate::error::AgentResult<T>;

    /// Retry this operation with custom configuration.
    fn retry_with(self, config: RetryConfig) -> crate::error::AgentResult<T>;
}

impl<T, F> RetryExt<T> for F
where
    F: Fn() -> crate::error::AgentResult<T>,
{
    fn retry(self) -> crate::error::AgentResult<T> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| AgentError::Other(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(retry(self, RetryConfig::default()))
    }

    fn retry_with(self, config: RetryConfig) -> crate::error::AgentResult<T> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| AgentError::Other(format!("Failed to create runtime: {}", e)))?;
        rt.block_on(retry(self, config))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, Duration::from_secs(1));
        assert_eq!(config.backoff_multiplier, 2.0);
        assert_eq!(config.max_delay, Duration::from_secs(30));
        assert!(config.retry_network_errors);
        assert!(config.retry_rate_limits);
    }

    #[test]
    fn test_retry_config_builder() {
        let config = RetryConfig::builder()
            .max_retries(5)
            .initial_delay(Duration::from_millis(100))
            .backoff_multiplier(3.0)
            .max_delay(Duration::from_secs(10))
            .retry_network_errors(false)
            .retry_rate_limits(false)
            .build();
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_delay, Duration::from_millis(100));
        assert_eq!(config.backoff_multiplier, 3.0);
        assert_eq!(config.max_delay, Duration::from_secs(10));
        assert!(!config.retry_network_errors);
        assert!(!config.retry_rate_limits);
    }

    #[tokio::test]
    async fn test_retry_success_first_try() {
        let result = retry(|| Ok(42), RetryConfig::default()).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let a = attempts.clone();

        let config = RetryConfig {
            max_retries: 3,
            initial_delay: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_millis(5),
            retry_network_errors: true,
            retry_rate_limits: true,
        };

        let result = retry(
            || {
                let n = a.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    Err(AgentError::Network("transient".into()))
                } else {
                    Ok(99)
                }
            },
            config,
        )
        .await;

        assert_eq!(result.unwrap(), 99);
        assert_eq!(attempts.load(Ordering::SeqCst), 3); // 2 failures + 1 success
    }

    #[tokio::test]
    async fn test_retry_non_retryable_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let a = attempts.clone();

        let config = RetryConfig::default();

        let result: crate::error::AgentResult<()> = retry(
            || {
                a.fetch_add(1, Ordering::SeqCst);
                Err(AgentError::Config("fatal".into()))
            },
            config,
        )
        .await;

        assert!(result.is_err());
        // Should only try once (no retries for non-retryable)
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_max_retries_exceeded() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let a = attempts.clone();

        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_millis(5),
            retry_network_errors: true,
            retry_rate_limits: false,
        };

        let result: crate::error::AgentResult<()> = retry(
            || {
                a.fetch_add(1, Ordering::SeqCst);
                Err(AgentError::ConnectionTimeout)
            },
            config,
        )
        .await;

        assert!(result.is_err());
        // 1 initial + 2 retries = 3 attempts total
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_async_success() {
        let result = retry_async(
            || Box::pin(async { Ok("async ok".to_string()) }),
            RetryConfig::default(),
        )
        .await;
        assert_eq!(result.unwrap(), "async ok");
    }

    #[tokio::test]
    async fn test_retry_async_with_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let a = attempts.clone();

        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_millis(5),
            retry_network_errors: true,
            retry_rate_limits: true,
        };

        let result = retry_async(
            || {
                let a = a.clone();
                Box::pin(async move {
                    let n = a.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Err(AgentError::Network("flaky".into()))
                    } else {
                        Ok("eventually ok".to_string())
                    }
                })
            },
            config,
        )
        .await;

        assert_eq!(result.unwrap(), "eventually ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_respects_network_setting() {
        let config = RetryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_millis(5),
            retry_network_errors: false, // disabled
            retry_rate_limits: true,
        };

        let attempts = Arc::new(AtomicUsize::new(0));
        let a = attempts.clone();

        let result: crate::error::AgentResult<()> = retry(
            || {
                a.fetch_add(1, Ordering::SeqCst);
                Err(AgentError::Network("no retry".into()))
            },
            config,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1); // No retries
    }

    #[test]
    fn test_retry_ext_sync_success() {
        let result: crate::error::AgentResult<i32> = (|| Ok(10)).retry();
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_retry_ext_sync_non_retryable() {
        let result: crate::error::AgentResult<i32> =
            (|| Err::<i32, AgentError>(AgentError::Config("fail".into()))).retry();
        assert!(result.is_err());
    }
}
