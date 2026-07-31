use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::db::TaskRepo;
use crate::error::AgentResult;
use crate::task::MemoryEntry;
use tracing::warn;

pub struct MemoryCache {
    repo: Arc<TaskRepo>,
    cache: tokio::sync::Mutex<lru::LruCache<String, (Vec<MemoryEntry>, Instant)>>,
    ttl: Duration,
}

impl MemoryCache {
    pub fn new(repo: Arc<TaskRepo>, capacity: usize) -> Self {
        Self {
            repo,
            cache: tokio::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(capacity).unwrap_or(std::num::NonZeroUsize::MIN),
            )),
            ttl: Duration::from_secs(300),
        }
    }

    pub async fn search(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let cache_key = format!("{}:{}", query, limit);

        // Try to get from cache with timeout to prevent deadlock
        match tokio::time::timeout(Duration::from_millis(500), self.cache.lock()).await {
            Ok(mut guard) => {
                if let Some((entries, timestamp)) = guard.get(&cache_key) {
                    if timestamp.elapsed() < self.ttl {
                        return Ok(entries.clone());
                    }
                    guard.pop(&cache_key);
                }
            }
            Err(_) => {
                // Lock timeout - skip cache, go directly to database
                warn!("Memory cache lock timeout, falling back to database");
            }
        }

        let results = self.repo.search_memories(query, limit).await?;

        // Try to update cache with timeout
        match tokio::time::timeout(Duration::from_millis(500), self.cache.lock()).await {
            Ok(mut guard) => {
                guard.put(cache_key, (results.clone(), Instant::now()));
            }
            Err(_) => {
                warn!("Memory cache lock timeout, skipping cache update");
            }
        }

        Ok(results)
    }

    pub async fn recent(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.repo.recent_memories(limit).await
    }

    pub async fn invalidate(&self) {
        match tokio::time::timeout(Duration::from_millis(500), self.cache.lock()).await {
            Ok(mut guard) => {
                guard.clear();
            }
            Err(_) => {
                warn!("Memory cache lock timeout, skip invalidate");
            }
        }
    }
}
