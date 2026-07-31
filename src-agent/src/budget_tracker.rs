//! Budget tracking for the loop engine.
//!
//! Tracks token consumption and elapsed time across agent loop iterations,
//! and signals when either budget is exceeded via [`BudgetStatus`].
//!
//! # Key Types
//!
//! - [`BudgetTracker`] — accumulates token count and tracks start time
//! - [`BudgetStatus`] — result of budget check: `Ok`, `TokenExceeded`, or
//!   `TimeExceeded`
//!
//! # Usage
//!
//! ```rust,ignore
//! let mut tracker = BudgetTracker::new();
//! tracker.add_tokens(500);
//! match tracker.check(Some(1000), Some(300)) {
//!     BudgetStatus::Ok => { /* continue */ }
//!     BudgetStatus::TokenExceeded { .. } => { /* stop */ }
//!     BudgetStatus::TimeExceeded { .. } => { /* stop */ }
//! }
//! ```
//!
//! Extracted from [`crate::loop_engine`] to reduce module coupling.

#[derive(Debug, Clone)]
pub enum BudgetStatus {
    Ok,
    TokenExceeded { used: u64, limit: u64 },
    TimeExceeded { elapsed_secs: u64, limit_secs: u64 },
}

/// Tracks accumulated resource consumption across iterations.
#[derive(Debug, Clone, Default)]
pub struct BudgetTracker {
    pub total_tokens: u64,
    pub started_at: i64,
}

impl BudgetTracker {
    pub fn new() -> Self {
        Self {
            total_tokens: 0,
            started_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn add_tokens(&mut self, tokens: u64) {
        self.total_tokens += tokens;
    }

    pub fn check(&self, token_budget: Option<u64>, time_budget_secs: Option<u64>) -> BudgetStatus {
        let now = chrono::Utc::now().timestamp();
        let elapsed = (now - self.started_at).max(0) as u64;

        if let Some(limit) = token_budget {
            if self.total_tokens >= limit {
                return BudgetStatus::TokenExceeded {
                    used: self.total_tokens,
                    limit,
                };
            }
        }

        if let Some(limit) = time_budget_secs {
            if elapsed >= limit {
                return BudgetStatus::TimeExceeded {
                    elapsed_secs: elapsed,
                    limit_secs: limit,
                };
            }
        }

        BudgetStatus::Ok
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker() {
        let tracker = BudgetTracker::new();
        assert_eq!(tracker.total_tokens, 0);
        assert!(tracker.started_at > 0);
    }

    #[test]
    fn test_default_tracker() {
        let tracker = BudgetTracker::default();
        assert_eq!(tracker.total_tokens, 0);
    }

    #[test]
    fn test_add_tokens() {
        let mut tracker = BudgetTracker::new();
        tracker.add_tokens(100);
        assert_eq!(tracker.total_tokens, 100);
        tracker.add_tokens(50);
        assert_eq!(tracker.total_tokens, 150);
    }

    #[test]
    fn test_check_no_budgets_always_ok() {
        let tracker = BudgetTracker::new();
        let status = tracker.check(None, None);
        assert!(matches!(status, BudgetStatus::Ok));
    }

    #[test]
    fn test_check_token_budget_exceeded() {
        let mut tracker = BudgetTracker::new();
        tracker.add_tokens(500);

        let status = tracker.check(Some(100), None);
        assert!(matches!(
            status,
            BudgetStatus::TokenExceeded {
                used: 500,
                limit: 100
            }
        ));
    }

    #[test]
    fn test_check_token_budget_ok() {
        let mut tracker = BudgetTracker::new();
        tracker.add_tokens(50);

        let status = tracker.check(Some(100), None);
        assert!(matches!(status, BudgetStatus::Ok));
    }

    #[test]
    fn test_check_time_budget_exceeded() {
        // Create a tracker with a started_at in the past
        let tracker = BudgetTracker {
            total_tokens: 0,
            started_at: chrono::Utc::now().timestamp() - 100, // 100 seconds ago
        };

        let status = tracker.check(None, Some(10)); // budget is 10 seconds
        assert!(matches!(status, BudgetStatus::TimeExceeded { .. }));
    }

    #[test]
    fn test_check_time_budget_ok() {
        let tracker = BudgetTracker::new(); // just started
        let status = tracker.check(None, Some(3600)); // 1 hour budget
        assert!(matches!(status, BudgetStatus::Ok));
    }

    #[test]
    fn test_check_both_budgets_token_first() {
        let mut tracker = BudgetTracker::new();
        tracker.add_tokens(1000);

        // Both exceeded, token check comes first
        let status = tracker.check(Some(100), Some(1));
        assert!(
            matches!(status, BudgetStatus::TokenExceeded { .. }),
            "token exceeded should be checked before time"
        );
    }
}
