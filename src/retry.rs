use std::time::Duration;

use tokio::time::sleep;

use crate::error::AgentError;

/// Retry configuration for operations that may fail transiently.
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

/// Execute an operation with retry logic.
///
/// # Examples
///
/// ```rust,no_run
/// use rupoo::retry::{retry, RetryConfig};
///
/// #[tokio::main]
/// async fn main() {
///     // Successful operation
///     let result = retry(|| Ok("success".to_string()), RetryConfig::default()).await;
///     assert!(result.is_ok());
///     assert_eq!(result.unwrap(), "success");
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

/// Execute an async operation with retry logic.
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
        tokio::runtime::Runtime::new()
            .expect("Failed to create runtime")
            .block_on(retry(self, RetryConfig::default()))
    }

    fn retry_with(self, config: RetryConfig) -> crate::error::AgentResult<T> {
        tokio::runtime::Runtime::new()
            .expect("Failed to create runtime")
            .block_on(retry(self, config))
    }
}
