//! 情景记忆——基于混合检索（FTS5 + 向量）的历史案例存储。
//!
//! 用于存储之前的执行经验、调试记录和交互案例，
//! 支持语义搜索以在类似场景中检索相关经验。
//!
//! 与 LongTermMemory 的区别：
//! - LongTermMemory：存储持久知识（用户偏好、项目事实）
//! - EpisodicMemory：存储执行案例（过去发生了什么、结果如何）

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::db::TaskRepo;
use crate::embedding::EmbeddingService;
use crate::error::AgentResult;
use crate::memory::store::{HybridSearchConfig, MemoryStore};
use crate::memory::traits::MemoryStorage;
use crate::task::MemoryEntry;

/// 情景记忆——带向量搜索的执行案例存储。
///
/// 如果提供了 EmbeddingService，将启用语义搜索（向量相似度），
/// 否则回退到纯 FTS5 关键字搜索。
pub struct EpisodicMemory {
    store: MemoryStore,
    /// 是否启用向量搜索
    vector_enabled: bool,
}

impl EpisodicMemory {
    /// 创建一个情景记忆存储（仅 FTS5）。
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            store: MemoryStore::new(repo),
            vector_enabled: false,
        }
    }

    /// 创建一个带混合搜索的情景记忆存储（FTS5 + 向量）。
    pub fn with_hybrid_search(
        repo: Arc<TaskRepo>,
        embedding: Option<Arc<EmbeddingService>>,
        config: Option<HybridSearchConfig>,
    ) -> Self {
        let cfg = config.unwrap_or_default();
        let vector_enabled = cfg.enable_vector_search;
        let store = MemoryStore::with_hybrid_search(repo, embedding, cfg);
        Self {
            store,
            vector_enabled,
        }
    }
}

#[async_trait]
impl MemoryStorage for EpisodicMemory {
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()> {
        let tags: Vec<&str> = entry.tags.iter().map(|s| s.as_str()).collect();

        if self.vector_enabled {
            // 使用带向量嵌入的存储
            let id = self
                .store
                .remember_from(&entry.content, &tags, &entry.source)
                .await?;
            if !id.is_empty() {
                info!(
                    memory_id = %id,
                    content_len = entry.content.len(),
                    vector_enabled = true,
                    "episodic memory stored with vector embedding"
                );
            }
        } else {
            // 纯 FTS5 存储
            let id = self
                .store
                .remember_from(&entry.content, &tags, &entry.source)
                .await?;
            if !id.is_empty() {
                info!(
                    memory_id = %id,
                    content_len = entry.content.len(),
                    vector_enabled = false,
                    "episodic memory stored (FTS5 only)"
                );
            }
        }
        Ok(())
    }

    async fn retrieve(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let results = self.store.recall(query, limit).await?;
        Ok(results)
    }

    async fn delete(&self, id: &str) -> AgentResult<()> {
        self.store.delete(id).await?;
        info!(memory_id = %id, "episodic memory deleted");
        Ok(())
    }

    async fn count(&self) -> AgentResult<usize> {
        self.store.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_repo() -> Arc<TaskRepo> {
        Arc::new(TaskRepo::new(":memory:").unwrap())
    }

    fn sample_entry(id: &str, content: &str, tags: Vec<&str>) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            content: content.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            source: "test".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_episodic_store_and_retrieve() {
        let memory = EpisodicMemory::new(test_repo());

        let entry = sample_entry(
            "ep-1",
            "Optimized database query: added index, reduced time from 2s to 50ms",
            vec!["optimization", "database"],
        );

        memory.store(entry).await.unwrap();

        let results = memory.retrieve("database index", 5).await.unwrap();
        assert!(!results.is_empty(), "should find memory by FTS5 match");
        let contents: Vec<&str> = results.iter().map(|r| r.content.as_str()).collect();
        assert!(contents.iter().any(|c| c.contains("database")));
    }

    #[tokio::test]
    async fn test_episodic_multiple_entries() {
        let memory = EpisodicMemory::new(test_repo());

        let entries = vec![
            (
                "Fix null pointer: check input parameter for null before use",
                "bugfix",
            ),
            (
                "Refactor config module: migrate hardcoded config to TOML files",
                "refactor",
            ),
            (
                "Add login feature: implement JWT token verification middleware",
                "feature",
            ),
        ];

        for (content, tag) in &entries {
            let entry = sample_entry(&uuid::Uuid::new_v4().to_string(), content, vec![tag]);
            memory.store(entry).await.unwrap();
        }

        let results = memory.retrieve("TOML config", 5).await.unwrap();
        assert_eq!(results.len(), 1, "should find refactor entry by FTS5 match");
        assert!(results[0].content.contains("config"));
    }

    #[tokio::test]
    async fn test_episodic_count() {
        let memory = EpisodicMemory::new(test_repo());
        assert_eq!(memory.count().await.unwrap(), 0);

        let entry = sample_entry("ep-2", "Docker 部署优化", vec!["devops"]);
        memory.store(entry).await.unwrap();
        assert_eq!(memory.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_episodic_no_results() {
        let memory = EpisodicMemory::new(test_repo());
        let results = memory.retrieve("完全不存在的内容", 5).await.unwrap();
        assert!(results.is_empty());
    }

    /// H2 regression: delete must actually remove the entry, not silently no-op.
    #[tokio::test]
    async fn test_episodic_delete_actually_removes() {
        let memory = EpisodicMemory::new(test_repo());
        assert_eq!(memory.count().await.unwrap(), 0);

        let entry = sample_entry("ep-del", "Unique memory to be deleted", vec!["test"]);
        memory.store(entry).await.unwrap();
        assert_eq!(memory.count().await.unwrap(), 1);

        // Retrieve to get the auto-generated content_id
        let results = memory
            .retrieve("Unique memory to be deleted", 5)
            .await
            .unwrap();
        assert!(!results.is_empty(), "should find just-stored memory");
        let stored_id = results[0].id.clone();

        // Delete by content_id
        memory.delete(&stored_id).await.unwrap();

        // Count must decrease — this was the bug: delete was a no-op
        assert_eq!(
            memory.count().await.unwrap(),
            0,
            "count should be 0 after delete"
        );

        // Search should return empty
        let results = memory
            .retrieve("Unique memory to be deleted", 5)
            .await
            .unwrap();
        assert!(
            results.is_empty(),
            "deleted memory should not be retrievable"
        );
    }
}
