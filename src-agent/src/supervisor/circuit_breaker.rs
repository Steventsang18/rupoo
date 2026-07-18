// src-agent/src/supervisor/circuit_breaker.rs
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::{AgentError, AgentResult};

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BreakerState {
    /// 正常工作
    Closed,
    /// 熔断开启——拒绝所有请求
    Open,
    /// 半开——允许一个试探请求
    HalfOpen,
}

/// 熔断器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerConfig {
    /// 触发开启的连续失败次数
    pub failure_threshold: u32,
    /// 熔断开启持续时长（秒）
    pub open_duration_secs: u64,
    /// 半开状态容许的试探请求数
    pub half_open_max_requests: u32,
    /// 最大调用频率（每秒），超过则拒绝
    pub max_rate_per_sec: u64,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 20,
        }
    }
}

/// 熔断器内部状态
struct BreakerInner {
    state: BreakerState,
    failure_count: u32,
    last_state_change: Instant,
    half_open_requests: u32,
    /// Sliding window: per-call timestamps (front = oldest).
    /// Choice: VecDeque over Vec because pop_front is O(1) for eviction.
    call_timestamps: VecDeque<Instant>,
}

/// 熔断器——防止系统雪崩
pub struct CircuitBreaker {
    config: BreakerConfig,
    inner: Arc<Mutex<BreakerInner>>,
}

impl CircuitBreaker {
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            inner: Arc::new(Mutex::new(BreakerInner {
                state: BreakerState::Closed,
                failure_count: 0,
                last_state_change: Instant::now(),
                half_open_requests: 0,
                call_timestamps: VecDeque::new(),
            })),
        }
    }

    /// Evict timestamps older than 1 second from the front of the deque.
    fn evict_stale(inner: &mut BreakerInner) {
        let now = Instant::now();
        while let Some(front) = inner.call_timestamps.front() {
            if now.duration_since(*front) >= Duration::from_secs(1) {
                inner.call_timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    /// 检查是否允许通过
    pub fn check(&self) -> AgentResult<()> {
        let mut inner = self.inner.lock();
        Self::evict_stale(&mut inner);
        let now = Instant::now();

        // 频率限制
        if inner.call_timestamps.len() as u64 >= self.config.max_rate_per_sec {
            return Err(AgentError::CircuitBreakerOpen {
                reason: "调用频率超限".to_string(),
                retry_after_secs: 1,
            });
        }
        inner.call_timestamps.push_back(now);

        match inner.state {
            BreakerState::Closed => Ok(()),
            BreakerState::Open => {
                let elapsed = now.duration_since(inner.last_state_change);
                if elapsed >= Duration::from_secs(self.config.open_duration_secs) {
                    // 冷却时间到，进入半开
                    inner.state = BreakerState::HalfOpen;
                    inner.failure_count = 0;
                    inner.half_open_requests = 0;
                    inner.last_state_change = now;
                    info!("circuit breaker: Closed -> HalfOpen after cooldown");
                    Ok(())
                } else {
                    let remaining = self.config.open_duration_secs - elapsed.as_secs();
                    Err(AgentError::CircuitBreakerOpen {
                        reason: "熔断器已开启".to_string(),
                        retry_after_secs: remaining,
                    })
                }
            }
            BreakerState::HalfOpen => {
                if inner.half_open_requests < self.config.half_open_max_requests {
                    inner.half_open_requests += 1;
                    Ok(())
                } else {
                    Err(AgentError::CircuitBreakerOpen {
                        reason: "熔断器半开状态，超过试探请求数".to_string(),
                        retry_after_secs: self.config.open_duration_secs,
                    })
                }
            }
        }
    }

    /// 记录一次成功——重置失败计数
    pub fn record_success(&self) {
        let mut inner = self.inner.lock();
        Self::evict_stale(&mut inner);
        inner.failure_count = 0;
        if inner.state == BreakerState::HalfOpen {
            inner.state = BreakerState::Closed;
            inner.last_state_change = Instant::now();
            info!("circuit breaker: HalfOpen -> Closed (success)");
        }
    }

    /// 记录一次失败——可能触发熔断
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock();
        Self::evict_stale(&mut inner);
        inner.failure_count += 1;

        match inner.state {
            BreakerState::Closed if inner.failure_count >= self.config.failure_threshold => {
                inner.state = BreakerState::Open;
                inner.last_state_change = Instant::now();
                warn!(
                    failures = inner.failure_count,
                    threshold = self.config.failure_threshold,
                    "circuit breaker: Closed -> Open (failure threshold exceeded)"
                );
            }
            BreakerState::HalfOpen => {
                inner.state = BreakerState::Open;
                inner.last_state_change = Instant::now();
                warn!("circuit breaker: HalfOpen -> Open (probe failed)");
            }
            _ => {}
        }
    }

    /// 当前状态（用于监控）
    pub fn state(&self) -> BreakerState {
        self.inner.lock().state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_closed_state_allows_calls() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 5,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        let result = breaker.check();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_opens_after_failure_threshold() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        // 前3次：触发熔断
        for _ in 0..3 {
            breaker.record_failure();
        }
        // 调用check应被拒绝
        let result = breaker.check();
        assert!(result.is_err());
        assert_eq!(breaker.state(), BreakerState::Open);
    }

    #[tokio::test]
    async fn test_half_open_recovers_on_success() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 0, // 立即进入半开
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        // 触发熔断
        for _ in 0..3 {
            breaker.record_failure();
        }
        // 因为 open_duration=0，check 应该立即进入半开并放行
        let _ = breaker.check();
        // 记录成功
        breaker.record_success();
        assert_eq!(breaker.state(), BreakerState::Closed);
    }

    #[tokio::test]
    async fn test_rate_limiting_rejects_excessive_calls() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 100,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 2, // 每秒最多2次
        });
        // 前2次通过
        assert!(breaker.check().is_ok());
        assert!(breaker.check().is_ok());
        // 第三次被频率限制
        let result = breaker.check();
        assert!(result.is_err());
    }
}
