//! Bridge between the new trait-based MemorySystem and the legacy SQLite FTS5 store.
//!
//! This allows the Orchestrator and other new-architecture components to
//! use the same memory backend as the Agent, while the codebase transitions
//! from the concrete MemoryStore to the trait-based MemorySystem.
//!
//! # Design
//!
//! - Short-term memory: in-memory `ShortTermMemory` (ephemeral session cache)
//! - Long-term memory: delegates to `TaskRepo`'s FTS5-backed methods
//! - Episodic memory: same backend as long-term, separate logical partition
//!
//! # Future
//!
//! Once the Agent main loop migrates to `MemorySystem`, this bridge can be
//! replaced with a single unified implementation.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use super::short_term::ShortTermMemory;
use super::traits::{MemoryStorage, MemorySystem};
use crate::db::TaskRepo;
use crate::error::AgentResult;
use crate::task::MemoryEntry;

/// Bridge that exposes TaskRepo-backed memory behind the MemorySystem trait.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use rupoo::db::TaskRepo;
/// use rupoo::memory::MemorySystemBridge;
/// use rupoo::memory::MemorySystem;
///
/// #[tokio::main]
/// async fn main() {
///     let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
///     let bridge = MemorySystemBridge::new(repo);
///     assert_eq!(bridge.hybrid_recall("test", 10).await.unwrap().len(), 0);
/// }
/// ```
pub struct MemorySystemBridge {
    short_term: ShortTermMemory,
    long_term: TaskRepoStorageAdapter,
    episodic: TaskRepoStorageAdapter,
    repo: Arc<TaskRepo>,
}

impl MemorySystemBridge {
    /// Create a new bridge with a shared TaskRepo backend.
    ///
    /// Short-term memory is allocated with a 100-entry ring buffer.
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            short_term: ShortTermMemory::new(100),
            long_term: TaskRepoStorageAdapter::new(Arc::clone(&repo), "long_term"),
            episodic: TaskRepoStorageAdapter::new(Arc::clone(&repo), "episodic"),
            repo,
        }
    }

    /// Access the underlying TaskRepo (for Agent compatibility).
    pub fn repo(&self) -> &Arc<TaskRepo> {
        &self.repo
    }
}

#[async_trait]
impl MemorySystem for MemorySystemBridge {
    fn short_term(&self) -> &dyn MemoryStorage {
        &self.short_term
    }

    fn long_term(&self) -> &dyn MemoryStorage {
        &self.long_term
    }

    fn episodic(&self) -> &dyn MemoryStorage {
        &self.episodic
    }

    async fn hybrid_recall(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. Short-term: in-memory session cache (fastest)
        let short_results = self.short_term.retrieve(query, limit).await?;
        for entry in short_results {
            if seen.insert(entry.id.clone()) {
                results.push(entry);
            }
        }

        // 2. Long-term: SQLite FTS5 (persistent)
        let long_results = self.long_term.retrieve(query, limit).await?;
        for entry in long_results {
            if seen.insert(entry.id.clone()) {
                results.push(entry);
            }
        }

        // 3. Episodic: same SQLite backend, but queried separately
        //    (in a full implementation this would query a separate FTS5 table)
        let epi_results = self.episodic.retrieve(query, limit).await?;
        for entry in epi_results {
            if seen.insert(entry.id.clone()) {
                results.push(entry);
            }
        }

        results.truncate(limit);
        debug!(count = results.len(), "hybrid_recall completed");
        Ok(results)
    }
}

/// Adapter that implements MemoryStorage by delegating to TaskRepo.
///
/// This mirrors the pattern used by LongTermMemory and EpisodicMemory
/// in the codebase, providing a consistent FTS5-backed storage interface.
struct TaskRepoStorageAdapter {
    repo: Arc<TaskRepo>,
    _kind: &'static str,
}

impl TaskRepoStorageAdapter {
    fn new(repo: Arc<TaskRepo>, kind: &'static str) -> Self {
        Self { repo, _kind: kind }
    }
}

#[async_trait]
impl MemoryStorage for TaskRepoStorageAdapter {
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()> {
        let tags: Vec<&str> = entry.tags.iter().map(|s| s.as_str()).collect();
        let id = self
            .repo
            .store_memory(&entry.content, &tags, &entry.source)
            .await?;

        if !id.is_empty() {
            debug!(
                memory_id = %id,
                kind = self._kind,
                "memory stored via bridge"
            );
        }
        Ok(())
    }

    async fn retrieve(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let results = self.repo.search_memories(query, limit).await?;
        Ok(results)
    }

    async fn delete(&self, id: &str) -> AgentResult<()> {
        self.repo.delete_memory(id).await?;
        debug!(memory_id = %id, kind = self._kind, "memory deleted via bridge");
        Ok(())
    }

    async fn count(&self) -> AgentResult<usize> {
        let count = self.repo.count_memories().await?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_repo() -> Arc<TaskRepo> {
        Arc::new(TaskRepo::new(":memory:").unwrap())
    }

    fn make_entry(id: &str, content: &str) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            content: content.to_string(),
            tags: vec!["test".to_string()],
            source: "test".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_bridge_short_term_in_memory() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        let e = make_entry("st-1", "hello world");
        bridge.short_term().store(e).await.unwrap();
        assert_eq!(bridge.short_term().count().await.unwrap(), 1);
        let r = bridge.short_term().retrieve("hello", 10).await.unwrap();
        assert_eq!(r.len(), 1);
    }

    #[tokio::test]
    async fn test_bridge_long_term_persists() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        let e = make_entry("lt-1", "persistent memory");
        bridge.long_term().store(e).await.unwrap();
        let c = bridge.long_term().count().await.unwrap();
        assert!(c > 0);
    }

    #[tokio::test]
    async fn test_bridge_long_term_store_and_count() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        let e = make_entry("lt-cnt", "countable memory");
        bridge.long_term().store(e).await.unwrap();
        assert_eq!(bridge.long_term().count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_bridge_episodic_stores() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        let e = make_entry("ep-1", "episodic record");
        bridge.episodic().store(e).await.unwrap();
        let c = bridge.episodic().count().await.unwrap();
        assert!(c > 0);
    }

    #[tokio::test]
    async fn test_hybrid_recall_merges_short_and_long() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        bridge
            .long_term()
            .store(make_entry("hl", "cross layer memory"))
            .await
            .unwrap();
        bridge
            .short_term()
            .store(make_entry("hs", "cross layer short"))
            .await
            .unwrap();
        let results = bridge.hybrid_recall("cross", 10).await.unwrap();
        let ids: std::collections::HashSet<String> = results.into_iter().map(|e| e.id).collect();
        assert!(ids.contains("hs"), "short-term entry must be present");
    }

    #[tokio::test]
    async fn test_hybrid_recall_dedup_by_id() {
        let bridge = MemorySystemBridge::new(create_test_repo());
        let entry = make_entry("dup", "dedup test content");
        bridge.short_term().store(entry.clone()).await.unwrap();
        bridge.long_term().store(entry).await.unwrap();
        // Short-term should find it (in-memory contains search), long-term via FTS5
        let results = bridge.hybrid_recall("test", 10).await.unwrap();
        let ids: std::collections::HashSet<&str> = results.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), results.len(), "no duplicate IDs in results");
    }

    #[tokio::test]
    async fn test_bridge_delete() {
        // Use TaskRepo directly so we can capture the stored memory's ID
        let repo = create_test_repo();
        let e = make_entry("del-manual", "delete me test");
        let tags: Vec<&str> = e.tags.iter().map(|s| s.as_str()).collect();
        let stored_id = repo
            .store_memory(&e.content, &tags, &e.source)
            .await
            .unwrap();
        assert!(!stored_id.is_empty(), "store_memory should return an ID");

        // Now delete by the returned ID
        let bridge = MemorySystemBridge::new(repo);
        bridge.long_term().delete(&stored_id).await.unwrap();

        // Verify deletion
        let remaining = bridge.long_term().count().await.unwrap();
        assert_eq!(remaining, 0, "count should be 0 after delete");
    }

    #[tokio::test]
    async fn test_bridge_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MemorySystemBridge>();
    }
}
