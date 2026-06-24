use async_trait::async_trait;
use crate::error::AgentResult;
use crate::task::MemoryEntry;
use crate::memory::traits::MemoryStorage;

/// 情景记忆——向量检索案例（Phase 3 填充）
pub struct EpisodicMemory;

#[async_trait]
impl MemoryStorage for EpisodicMemory {
    async fn store(&self, _entry: MemoryEntry) -> AgentResult<()> { Ok(()) }
    async fn retrieve(&self, _query: &str, _limit: usize) -> AgentResult<Vec<MemoryEntry>> { Ok(Vec::new()) }
    async fn delete(&self, _id: &str) -> AgentResult<()> { Ok(()) }
    async fn count(&self) -> AgentResult<usize> { Ok(0) }
}
