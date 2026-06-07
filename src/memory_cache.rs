//! Memory cache with sharded locking for improved concurrency.
//!
//! This module implements a sharded memory cache that reduces lock contention
//! by dividing the cache into multiple shards, each with its own lock.
//! This allows multiple concurrent operations to proceed without blocking
//! each other, significantly improving throughput under high concurrency.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::db::TaskRepo;
use crate::error::AgentResult;
use crate::task::MemoryEntry;
use tracing::warn;

/// Number of shards for the cache.
/// Increasing this number reduces lock contention but increases memory overhead.
const NUM_SHARDS: usize = 16;

/// Hash function to determine which shard a key belongs to.
fn shard_index(key: &str) -> usize {
    let hash = seahash::hash(key.as_bytes());
    (hash % NUM_SHARDS as u64) as usize
}

/// A single shard of the cache with its own lock.
struct CacheShard {
    cache: lru::LruCache<String, (Vec<MemoryEntry>, Instant)>,
    ttl: Duration,
}

impl CacheShard {
    fn new(capacity_per_shard: usize, ttl: Duration) -> Self {
        Self {
            cache: lru::LruCache::new(
                std::num::NonZeroUsize::new(capacity_per_shard)
                    .unwrap_or(std::num::NonZeroUsize::new(1).unwrap()),
            ),
            ttl,
        }
    }

    fn get(&mut self, key: &str) -> Option<Vec<MemoryEntry>> {
        if let Some((entries, timestamp)) = self.cache.get(key) {
            if timestamp.elapsed() < self.ttl {
                return Some(entries.clone());
            }
            self.cache.pop(key);
        }
        None
    }

    fn put(&mut self, key: String, entries: Vec<MemoryEntry>) {
        self.cache.put(key, (entries, Instant::now()));
    }

    fn clear(&mut self) {
        self.cache.clear();
    }
}

/// Sharded memory cache with reduced lock contention.
///
/// This implementation divides the cache into multiple shards, each with its own lock.
/// This allows concurrent operations on different keys to proceed without blocking
/// each other, significantly improving throughput under high concurrency.
pub struct MemoryCache {
    repo: Arc<TaskRepo>,
    shards: Vec<tokio::sync::Mutex<CacheShard>>,
    ttl: Duration,
}

impl MemoryCache {
    /// Create a new sharded memory cache.
    ///
    /// - `repo`: The underlying task repository for cache misses
    /// - `capacity`: Total cache capacity across all shards
    pub fn new(repo: Arc<TaskRepo>, capacity: usize) -> Self {
        let capacity_per_shard = capacity.div_ceil(NUM_SHARDS);
        let ttl = Duration::from_secs(300);

        let mut shards = Vec::with_capacity(NUM_SHARDS);
        for _ in 0..NUM_SHARDS {
            shards.push(tokio::sync::Mutex::new(CacheShard::new(
                capacity_per_shard,
                ttl,
            )));
        }

        Self { repo, shards, ttl }
    }

    /// Search for memories matching the query.
    ///
    /// This operation only locks the specific shard corresponding to the cache key,
    /// allowing other concurrent operations to proceed on different shards.
    pub async fn search(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let cache_key = format!("{}:{}", query, limit);
        let shard_idx = shard_index(&cache_key);

        // Try to get from cache with timeout to prevent deadlock
        let cached_result =
            match tokio::time::timeout(Duration::from_millis(500), self.shards[shard_idx].lock())
                .await
            {
                Ok(mut guard) => guard.get(&cache_key),
                Err(_) => {
                    warn!("Memory cache shard lock timeout, falling back to database");
                    None
                }
            };

        if let Some(entries) = cached_result {
            return Ok(entries);
        }

        let results = self.repo.search_memories(query, limit).await?;

        // Try to update cache with timeout
        match tokio::time::timeout(Duration::from_millis(500), self.shards[shard_idx].lock()).await
        {
            Ok(mut guard) => {
                guard.put(cache_key, results.clone());
            }
            Err(_) => {
                warn!("Memory cache shard lock timeout, skipping cache update");
            }
        }

        Ok(results)
    }

    /// Get recent memories.
    ///
    /// This bypasses the cache and goes directly to the database.
    pub async fn recent(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.repo.recent_memories(limit).await
    }

    /// Invalidate the entire cache.
    ///
    /// This operation locks all shards sequentially to clear the cache.
    /// Note: This is intentionally not parallelized to avoid overwhelming the system.
    pub async fn invalidate(&self) {
        for (i, shard) in self.shards.iter().enumerate() {
            match tokio::time::timeout(Duration::from_millis(500), shard.lock()).await {
                Ok(mut guard) => {
                    guard.clear();
                }
                Err(_) => {
                    warn!(
                        shard = i,
                        "Memory cache shard lock timeout during invalidate"
                    );
                }
            }
        }
    }

    /// Get the number of shards in the cache.
    pub fn num_shards(&self) -> usize {
        self.shards.len()
    }

    /// Get the TTL duration for cached entries.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_index_distribution() {
        let keys = [
            "query1", "query2", "query3", "query4", "query5", "test", "hello", "world",
        ];

        let mut counts = [0; NUM_SHARDS];
        for key in &keys {
            let idx = shard_index(key);
            counts[idx] += 1;
        }

        assert!(
            counts.iter().all(|&c| c <= 2),
            "Shard distribution should be roughly even"
        );
    }

    #[test]
    fn test_shard_index_deterministic() {
        let key = "test-query";
        let idx1 = shard_index(key);
        let idx2 = shard_index(key);

        assert_eq!(idx1, idx2, "Same key should map to same shard");
    }
}
