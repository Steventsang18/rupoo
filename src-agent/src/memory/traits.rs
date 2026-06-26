use crate::error::AgentResult;
use crate::task::MemoryEntry;
use async_trait::async_trait;

/// 可插拔存储单元
#[async_trait]
pub trait MemoryStorage: Send + Sync {
    /// 存储一条记忆
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()>;

    /// 检索记忆（按相关性排序）
    async fn retrieve(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>>;

    /// 按 ID 删除
    async fn delete(&self, id: &str) -> AgentResult<()>;

    /// 统计条目数
    async fn count(&self) -> AgentResult<usize>;
}

/// 三层记忆统一接口
#[async_trait]
pub trait MemorySystem: Send + Sync {
    /// 短期记忆（会话内共享上下文，高速缓存）
    fn short_term(&self) -> &dyn MemoryStorage;

    /// 长期记忆（持久化知识）
    fn long_term(&self) -> &dyn MemoryStorage;

    /// 情景记忆（历史案例，向量检索）
    fn episodic(&self) -> &dyn MemoryStorage;

    /// 跨层混合检索（优先短期→情景→长期）
    async fn hybrid_recall(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_short_term_implements_memory_storage() {
        let storage: &dyn MemoryStorage = &crate::memory::short_term::ShortTermMemory::new(10);
        let entry = crate::task::MemoryEntry {
            id: "test".to_string(),
            content: "test".to_string(),
            tags: vec![],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        storage.store(entry).await.unwrap();
        assert_eq!(storage.count().await.unwrap(), 1);
    }
}
