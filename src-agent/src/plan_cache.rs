//! Plan Cache — LRU cache for storing generated LLM plans.
//!
//! Caches [`StepSpec`] vectors keyed by a hash of the task description and
//! optional context. Eviction is LRU with configurable capacity and TTL.
//!
//! # Key Types
//!
//! - [`PlanCache`] — thread-safe LRU cache (RwLock + LruCache)
//! - [`CachedPlan`] — cache entry with creation timestamp
//! - [`PlanCacheConfig`] — capacity and TTL settings
//!
//! # Usage
//!
//! ```rust,ignore
//! let cache = PlanCache::new(PlanCacheConfig::default());
//! cache.put("build a CLI tool", None, generated_steps);
//! if let Some(steps) = cache.get("build a CLI tool", None) {
//!     // cache hit — skip LLM call
//! }
//! ```
//!
//! Extracted from [`crate::agent::Agent`] to reduce module coupling.

use lru::LruCache;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use tracing::{debug, info};

/// Cache entry for a generated plan.
#[derive(Debug, Clone)]
pub struct CachedPlan {
    pub steps: Vec<crate::llm::StepSpec>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub task_hash: String,
}

/// Plan cache configuration.
#[derive(Debug, Clone)]
pub struct PlanCacheConfig {
    pub capacity: usize,
    pub ttl_seconds: u64,
}

impl Default for PlanCacheConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            ttl_seconds: 3600, // 1 hour default TTL
        }
    }
}

/// Thread-safe LRU cache for storing generated plans.
pub struct PlanCache {
    cache: std::sync::RwLock<LruCache<String, CachedPlan>>,
    config: PlanCacheConfig,
}

impl PlanCache {
    pub fn new(config: PlanCacheConfig) -> Self {
        Self {
            cache: std::sync::RwLock::new(LruCache::new(
                std::num::NonZeroUsize::new(config.capacity).unwrap_or(std::num::NonZeroUsize::MIN),
            )),
            config,
        }
    }

    /// Generate a cache key from task input using simple hashing.
    fn generate_key(task: &str, context: Option<&str>) -> String {
        let mut hasher = DefaultHasher::new();
        task.hash(&mut hasher);
        if let Some(ctx) = context {
            ctx.hash(&mut hasher);
        }
        let hash = hasher.finish();
        format!("{:016x}", hash)
    }

    /// Check if a plan exists in cache and is valid.
    pub fn get(&self, task: &str, context: Option<&str>) -> Option<Vec<crate::llm::StepSpec>> {
        let key = Self::generate_key(task, context);
        let mut cache = self.cache.write().ok()?;

        if let Some(cached) = cache.get(&key) {
            // Check TTL
            let now = chrono::Utc::now();
            let age = now.signed_duration_since(cached.created_at).num_seconds() as u64;
            if age < self.config.ttl_seconds {
                debug!(key = %key, age_secs = age, "plan cache hit");
                return Some(cached.steps.clone());
            } else {
                debug!(key = %key, age_secs = age, "plan cache expired");
            }
        }
        None
    }

    /// Store a plan in cache.
    pub fn put(&self, task: &str, context: Option<&str>, steps: Vec<crate::llm::StepSpec>) {
        let key = Self::generate_key(task, context);
        let entry = CachedPlan {
            steps,
            created_at: chrono::Utc::now(),
            task_hash: key.clone(),
        };

        if let Ok(mut cache) = self.cache.write() {
            cache.put(key, entry);
            debug!("plan cached");
        }
    }

    /// Clear the entire cache.
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
            info!("plan cache cleared");
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> (usize, usize) {
        let cache = self.cache.read().ok();
        let len = cache.as_ref().map(|c| c.len()).unwrap_or(0);
        (len, self.config.capacity)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::StepSpec;

    fn make_steps(count: usize) -> Vec<StepSpec> {
        (0..count)
            .map(|i| StepSpec {
                step_type: "think".into(),
                instruction: format!("step {}", i),
                tool_name: String::new(),
                params: serde_json::json!({}),
                prompt: String::new(),
                summary: String::new(),
            })
            .collect()
    }

    #[test]
    fn test_config_defaults() {
        let config = PlanCacheConfig::default();
        assert_eq!(config.capacity, 100);
        assert_eq!(config.ttl_seconds, 3600);
    }

    #[test]
    fn test_put_and_get() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        let steps = make_steps(3);

        cache.put("build a web app", None, steps.clone());

        let result = cache.get("build a web app", None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_get_miss() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        let result = cache.get("nonexistent task", None);
        assert!(result.is_none());
    }

    #[test]
    fn test_context_changes_key() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        let steps = make_steps(1);

        // Same task, different context → different cache key
        cache.put("task", Some("ctx_a"), steps.clone());
        cache.put("task", Some("ctx_b"), make_steps(2));

        let result_a = cache.get("task", Some("ctx_a"));
        let result_b = cache.get("task", Some("ctx_b"));

        assert_eq!(result_a.unwrap().len(), 1);
        assert_eq!(result_b.unwrap().len(), 2);
    }

    #[test]
    fn test_ttl_expiration() {
        let config = PlanCacheConfig {
            capacity: 10,
            ttl_seconds: 0, // immediate expiry
        };
        let cache = PlanCache::new(config);
        cache.put("task", None, make_steps(1));

        let result = cache.get("task", None);
        assert!(result.is_none(), "should expire with TTL 0");
    }

    #[test]
    fn test_clear() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        cache.put("task1", None, make_steps(1));
        cache.put("task2", None, make_steps(2));

        let (len, _) = cache.stats();
        assert_eq!(len, 2);

        cache.clear();
        let (len, _) = cache.stats();
        assert_eq!(len, 0);
    }

    #[test]
    fn test_stats() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        let (len, cap) = cache.stats();
        assert_eq!(len, 0);
        assert_eq!(cap, 100);

        cache.put("t", None, make_steps(1));
        let (len, cap) = cache.stats();
        assert_eq!(len, 1);
        assert_eq!(cap, 100);
    }

    #[test]
    fn test_overwrite_existing_key() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        cache.put("task", None, make_steps(1));
        cache.put("task", None, make_steps(3));

        let result = cache.get("task", None);
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_different_tasks_different_keys() {
        let cache = PlanCache::new(PlanCacheConfig::default());
        cache.put("task_a", None, make_steps(1));
        cache.put("task_b", None, make_steps(2));

        assert_eq!(cache.get("task_a", None).unwrap().len(), 1);
        assert_eq!(cache.get("task_b", None).unwrap().len(), 2);
    }
}
