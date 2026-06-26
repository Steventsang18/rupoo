//! 长期记忆——基于 SQLite FTS5 的跨会话持久化知识存储。
//!
//! 与 ShortTermMemory（内存中的会话级缓存）不同，
//! LongTermMemory 将所有数据持久化到 SQLite，
//! 支持跨会话的全文本搜索（FTS5）。
//!
//! 用于存储：用户偏好、项目知识、学习到的模式。

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::db::TaskRepo;
use crate::error::AgentResult;
use crate::memory::traits::MemoryStorage;
use crate::task::MemoryEntry;

/// 长期记忆——跨会话持久化，SQLite FTS5 后端。
pub struct LongTermMemory {
    repo: Arc<TaskRepo>,
    /// 默认来源标签
    default_source: String,
}

impl LongTermMemory {
    /// 创建一个新的长期记忆存储。
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            repo,
            default_source: "agent".to_string(),
        }
    }

    /// 设置默认来源标签。
    pub fn with_source(mut self, source: &str) -> Self {
        self.default_source = source.to_string();
        self
    }
}

#[async_trait]
impl MemoryStorage for LongTermMemory {
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()> {
        // 复用 TaskRepo 的 FTS5 存储
        let tags: Vec<&str> = entry.tags.iter().map(|s| s.as_str()).collect();
        let id = self
            .repo
            .store_memory(&entry.content, &tags, &entry.source)
            .await?;

        if !id.is_empty() {
            info!(
                memory_id = %id,
                content_len = entry.content.len(),
                "long-term memory stored"
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
        info!(memory_id = %id, "long-term memory deleted");
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

    fn test_repo() -> Arc<TaskRepo> {
        Arc::new(TaskRepo::new(":memory:").unwrap())
    }

    #[tokio::test]
    async fn test_long_term_store_and_retrieve() {
        let memory = LongTermMemory::new(test_repo());
        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            content: "User prefers dark theme color scheme".to_string(),
            tags: vec!["preference".to_string(), "theme".to_string()],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        memory.store(entry).await.unwrap();
        assert_eq!(memory.count().await.unwrap(), 1);

        let results = memory.retrieve("dark theme", 5).await.unwrap();
        assert!(!results.is_empty(), "should find memory by FTS5 match");
        assert!(results[0].content.contains("dark theme"));
    }

    #[tokio::test]
    async fn test_long_term_search_across_multiple_entries() {
        let memory = LongTermMemory::new(test_repo());

        let entries = vec![
            ("Rust 异步编程使用 Tokio", "rust"),
            ("Python 数据分析用 Pandas", "python"),
            ("JavaScript 前端用 React", "js"),
        ];

        for (content, tag) in &entries {
            let entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                content: content.to_string(),
                tags: vec![tag.to_string()],
                source: "test".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            memory.store(entry).await.unwrap();
        }

        // FTS5 keyword search
        let results = memory.retrieve("Tokio", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Tokio"));
    }

    #[tokio::test]
    async fn test_long_term_delete() {
        // store_memory() auto-generates a UUID, so we store then search
        // to verify deletion works by content matching
        let memory = LongTermMemory::new(test_repo());
        let entry = MemoryEntry {
            id: "delete-me".to_string(),
            content: "Delete this specific memory".to_string(),
            tags: vec![],
            source: "test".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        memory.store(entry).await.unwrap();
        assert_eq!(memory.count().await.unwrap(), 1);

        // Find the auto-generated ID via search, then delete by exact content
        let results = memory
            .retrieve("Delete this specific memory", 5)
            .await
            .unwrap();
        assert!(!results.is_empty(), "should find just-stored memory");
        let stored_id = results[0].id.clone();
        memory.delete(&stored_id).await.unwrap();
        assert_eq!(memory.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_long_term_empty_search() {
        let memory = LongTermMemory::new(test_repo());
        let results = memory.retrieve("nonexistent", 5).await.unwrap();
        assert!(results.is_empty());
    }
}
