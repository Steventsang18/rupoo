use std::sync::Arc;

use tracing::info;

use crate::db::TaskRepo;
use crate::error::AgentResult;
use crate::task::MemoryEntry;

/// High-level memory operations for the long-term memory system.
/// Wraps TaskRepo's FTS5-backed memory methods with context-aware helpers.
pub struct MemoryStore {
    repo: Arc<TaskRepo>,
    /// Default source tag applied when none is provided.
    default_source: String,
}

impl MemoryStore {
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            repo,
            default_source: "agent".to_string(),
        }
    }

    /// Store a memory entry with automatic timestamp.
    pub async fn remember(&self, content: &str, tags: &[&str]) -> AgentResult<String> {
        let id = self
            .repo
            .store_memory(content, tags, &self.default_source)
            .await?;
        info!(
            memory_id = %id,
            content_preview = &content[..content.len().min(60)],
            "memory stored"
        );
        Ok(id)
    }

    /// Store a memory with an explicit source.
    pub async fn remember_from(
        &self,
        content: &str,
        tags: &[&str],
        source: &str,
    ) -> AgentResult<String> {
        let id = self
            .repo
            .store_memory(content, tags, source)
            .await?;
        info!(
            memory_id = %id,
            source = %source,
            "memory stored from source"
        );
        Ok(id)
    }

    /// Search memories by full-text relevance to a query.
    pub async fn recall(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.repo.search_memories(query, limit).await
    }

    /// Get the most recent memories.
    pub async fn recent(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.repo.recent_memories(limit).await
    }

    /// Format memories into a context string for prompt injection.
    pub fn format_context(memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("--- Relevant Memories ---\n");
        for mem in memories {
            ctx.push_str(&format!(
                "- [{}] {} (tags: {})\n",
                mem.created_at,
                mem.content,
                mem.tags.join(", ")
            ));
        }
        ctx.push_str("--- End Memories ---");
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskRepo;

    fn setup() -> (Arc<TaskRepo>, MemoryStore) {
        let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
        let store = MemoryStore::new(Arc::clone(&repo));
        (repo, store)
    }

    #[tokio::test]
    async fn test_store_and_search_memory() {
        let (_, store) = setup();
        store
            .remember("The user prefers Rust over Python for system programming", &["language", "preference"])
            .await
            .unwrap();

        let results = store.recall("Rust programming", 10).await.unwrap();
        assert!(!results.is_empty(), "should find the memory");
        assert!(results[0].content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_recent_memories() {
        let (_, store) = setup();
        store.remember("memory one", &["a"]).await.unwrap();
        store.remember("memory two", &["b"]).await.unwrap();

        let recent = store.recent(5).await.unwrap();
        assert_eq!(recent.len(), 2);
        // Most recent first
        assert!(recent[0].content.contains("two"));
    }

    #[tokio::test]
    async fn test_format_context_empty() {
        let ctx = MemoryStore::format_context(&[]);
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn test_format_context_with_memories() {
        let mems = vec![MemoryEntry {
            id: "1".into(),
            content: "user likes Rust".into(),
            tags: vec!["lang".into()],
            source: "test".into(),
            created_at: "2025-01-01".into(),
            updated_at: "2025-01-01".into(),
        }];
        let ctx = MemoryStore::format_context(&mems);
        assert!(ctx.contains("Rust"));
        assert!(ctx.contains("Relevant Memories"));
    }
}
