use async_trait::async_trait;
use crate::error::AgentResult;
use crate::task::MemoryEntry;
use crate::memory::traits::MemoryStorage;

/// 长期记忆——使用 SQLite FTS5（Phase 3 填充）
pub struct LongTermMemory;

#[async_trait]
impl MemoryStorage for LongTermMemory {
    async fn store(&self, _entry: MemoryEntry) -> AgentResult<()> {
        Ok(())
    }
    async fn retrieve(&self, _query: &str, _limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }
    async fn delete(&self, _id: &str) -> AgentResult<()> {
        Ok(())
    }
    async fn count(&self) -> AgentResult<usize> {
        Ok(0)
    }
}
