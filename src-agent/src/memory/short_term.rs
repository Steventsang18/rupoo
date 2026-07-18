use crate::error::AgentResult;
use crate::memory::traits::MemoryStorage;
use crate::task::MemoryEntry;
use async_trait::async_trait;
use std::collections::VecDeque;
// Choice: parking_lot::Mutex over std::sync::Mutex because:
// - parking_lot::Mutex never poisons (no unwrap_or_else needed)
// - Lock hold time is minimal (VecDeque ops only), never held across .await
// - Better performance under contention (fair queuing, no syscalls on Linux)
use parking_lot::Mutex;

/// 短期记忆——会话内高速缓存
pub struct ShortTermMemory {
    entries: Mutex<VecDeque<MemoryEntry>>,
    capacity: usize,
}

impl ShortTermMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }
}

#[async_trait]
impl MemoryStorage for ShortTermMemory {
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()> {
        let mut entries = self.entries.lock();
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        Ok(())
    }

    async fn retrieve(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let entries = self.entries.lock();
        let query_lower = query.to_lowercase();
        let mut results: Vec<MemoryEntry> = entries
            .iter()
            .filter(|e| {
                e.content.to_lowercase().contains(&query_lower)
                    || e.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .take(limit)
            .cloned()
            .collect();
        results.reverse(); // 最新的优先
        Ok(results)
    }

    async fn delete(&self, id: &str) -> AgentResult<()> {
        let mut entries = self.entries.lock();
        entries.retain(|e| e.id != id);
        Ok(())
    }

    async fn count(&self) -> AgentResult<usize> {
        Ok(self.entries.lock().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_short_term_store_and_count() {
        let mem = ShortTermMemory::new(3);
        let entry = MemoryEntry {
            id: "1".to_string(),
            content: "test memory".to_string(),
            tags: vec!["test".to_string()],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        mem.store(entry).await.unwrap();
        assert_eq!(mem.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_short_term_capacity_eviction() {
        let mem = ShortTermMemory::new(2);
        for i in 0..3 {
            let entry = MemoryEntry {
                id: format!("{}", i),
                content: format!("content {}", i),
                tags: vec![],
                source: "user".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            mem.store(entry).await.unwrap();
        }
        assert_eq!(mem.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_short_term_retrieve() {
        let mem = ShortTermMemory::new(10);
        let entry = MemoryEntry {
            id: "1".to_string(),
            content: "用户喜欢 Rust 语言".to_string(),
            tags: vec!["preference".to_string()],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        mem.store(entry).await.unwrap();
        let results = mem.retrieve("Rust", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "用户喜欢 Rust 语言");
    }
}
