# Rupoo 项目真实状况修复方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于深度代码审计暴露的架构缺陷和技术债，提供可执行的分阶段修复方案，使 Rupoo 达到生产级质量。

**架构方针：**
- 不重写已有功能，只修复和桥接
- 所有新代码覆盖单元测试 + 集成测试
- 优先解决架构级问题（记忆系统、Orchestrator），再处理质量问题
- 每个 Phase 产生可独立验证的交付物

**技术栈：** Rust 2021, tokio, async-trait, rusqlite, thiserror, tracing

**基线状态（审计总结）：**
- 评估报告原始评分：3.0/5 → 深度审计修正评分：~2.5/5
- 三大架构级问题：① 两套记忆系统不互通 ② Orchestrator 管线设计缺陷 ③ Loop Engine Pattern B/C 占位符
- 核心功能缺口：向量搜索名义 HNSW 实则暴力 O(n)，未持久化

## 全局约束

- **Rust 版本**：edition 2021，resolver = "2"
- **函数命名**：snake_case，遵循项目现有风格
- **错误处理**：使用 `thiserror`，新增错误变体需要在 `AgentError` 中注册
- **测试**：每个新功能必须有 `#[cfg(test)]` 模块，集成测试在 `tests/` 目录
- **提交格式**：`feat(module): description` 或 `fix(module): description`
- **代码格式**：提交前运行 `cargo fmt`
- **禁止**：`unwrap()` 在非测试代码中、`unsafe` 代码（除非 FFI 且隔离到 `ffi/` 模块）
- **文件路径**：所有路径相对于 `/Users/pengxiangzeng/rust-project/`

---

## Phase 0：紧急修复（Week 1）

> 目标：解决阻止 CI 通过的阻塞性问题和立即可以处理的简单技术债。
> 并行度：三个 Task 相互独立，可同时执行。

### Task 0.1：修复 Supervisor 集成测试

**文件：**
- Modify: `src-agent/src/supervisor/mod.rs`
- Style: `src-agent/src/supervisor/audit_logger.rs`

**问题诊断：** 4 个集成测试在 `SqliteAuditLogger::new()` 时使用默认路径 `~/.rupoo/agent.db`，被 sandbox 拦截。修复方案是将测试改为使用 `tempfile::NamedTempFile` 创建临时数据库路径。

- [ ] **Step 1: 在测试文件中导入 tempfile**

在 `src-agent/src/supervisor/mod.rs` 的测试模块顶部添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;  // 新增
```

- [ ] **Step 2: 创建辅助函数初始化临时 AuditLogger**

```rust
/// 创建使用临时数据库文件的 AuditLogger（测试用）
fn create_test_audit_logger() -> (SqliteAuditLogger, NamedTempFile) {
    let file = NamedTempFile::new().expect("failed to create temp file");
    let path = file.path().to_string_lossy().to_string();
    (SqliteAuditLogger::new(&path).expect("failed to create SqliteAuditLogger"), file)
}
```

- [ ] **Step 3: 替换 4 个测试中的 AuditLogger 创建方式**

以 `test_supervisor_approves_safe_action` 为例的修改模式：

```rust
#[tokio::test]
async fn test_supervisor_approves_safe_action() {
    let (logger, _file) = create_test_audit_logger();   // _file 保持 tempfile 活跃
    let compliance = ComplianceChecker::new(&[]);
    let confidence = ConfidenceChecker::new(0.7);
    let breaker = CircuitBreaker::new(3, Duration::from_secs(60));
    let supervisor = SupervisorImpl::new(
        Box::new(compliance),
        Box::new(confidence),
        Box::new(breaker),
        Arc::new(Mutex::new(logger)),
    );
    // ... 原有断言不变
}
```

对 `test_supervisor_blocks_open_breaker`、`test_supervisor_blocks_forbidden_command`、`test_supervisor_blocks_low_confidence` 执行同样修改。

- [ ] **Step 4: 运行测试验证**

```bash
cargo test -p rupoo -- test_supervisor --nocapture
```
Expected: 4 tests passed

- [ ] **Step 5: 提交**

```bash
git add src-agent/src/supervisor/mod.rs
git commit -m "fix(supervisor): use tempfile in integration tests to avoid sandbox path"
```

**耗时估计：** 0.5 天

---

### Task 0.2：修复 VectorStore remove() 内存泄漏 + 标记实现状态

**文件：**
- Modify: `src-agent/src/vector_store.rs`
- Modify: `Cargo.toml` （可选：移除未使用的 `hnswx` 依赖）

**问题诊断：** `remove()` 方法第 218-225 行从 HashMap 移除文档但不清理扁平 embedding 数组。`hnswx = "0.2"` 在 Cargo.toml 中但从未在任何源文件中使用。

- [ ] **Step 1: 修复 remove() 清理 embedding 数组**

```rust
/// Remove a memory entry from the vector store.
pub async fn remove(&mut self, id: &str) -> AgentResult<()> {
    if let Some(doc) = self.documents.remove(id) {
        // 在扁平数组中定位并移除对应的 embedding
        let doc_index = self.find_doc_index(id).await?;
        if let Some(idx) = doc_index {
            let start = idx * self.embedding_dim;
            let end = start + self.embedding_dim;
            // 移除[start..end]范围的 embedding
            self.embeddings.drain(start..end);
            debug!(id, "memory and embedding removed from vector store");
        }
    }
    Ok(())
}

/// 在文档集合中查找文档的索引位置
async fn find_doc_index(&self, id: &str) -> AgentResult<Option<usize>> {
    // 通过 documents 的迭代顺序确定索引
    // 注意：HashMap 不保证顺序，所以只在测试中可靠
    // 生产环境应考虑维护一个 id→index 的映射表
    for (i, doc_id) in self.documents.keys().enumerate() {
        if doc_id == id {
            // 但这里有个问题：embeddings 的顺序与 HashMap 迭代顺序一致
            // HashMap 迭代顺序不稳定，需要改用有序容器
            return Ok(Some(i));
        }
    }
    Ok(None)
}
```

- [ ] **Step 2: 将 HashMap 改为 IndexMap（有序迭代）**

在 `Cargo.toml` 中添加 `indexmap` 依赖：

```toml
indexmap = "2"
```

修改 `VectorStore` 结构体：

```rust
use indexmap::IndexMap;

pub struct VectorStore {
    /// 使用 IndexMap 保证稳定的迭代顺序
    documents: IndexMap<String, VectorMemoryDoc>,
    /// 扁平 embedding 数组，顺序与 documents 一致
    embeddings: Vec<f32>,
    embedding_dim: usize,
}
```

更新 `new()`、`insert()`、`semantic_search()`、`len()`、`is_empty()` 中所有引用 `HashMap` 的地方为 `IndexMap`。

- [ ] **Step 3: 更新模块文档，真实反映实现状态**

将 `vector_store.rs` 第 1-35 行的模块文档中"Vector Search [TODO]"部分更新为：

```rust
//! # Implementation Status
//!
//! - ✅ VectorStore struct and document types created
//! - ✅ Basic operations defined
//! - ✅ Brute-force O(n) cosine similarity search
//! - ⏳ Approximate Nearest Neighbor (ANN) indexing — currently O(n)
//! - ⏳ Vector embedding persistence across sessions
//! - ⏳ Hybrid search combining FTS5 and vector results
//!
//! # Performance Note
//!
//! Current semantic_search() is O(n) brute-force over all stored embeddings.
//! For deployments with >10,000 memory entries, consider implementing
//! HNSW or other ANN indexing. The `hnswx` crate is available but not
//! yet integrated.
```

- [ ] **Step 4: 在 Cargo.toml 中添加 `hnswx` 未使用的说明注释**

```toml
# hnswx = "0.2"   # 保留但未启用 — 用于将来的 ANN 索引优化
```

- [ ] **Step 5: 运行测试验证**

```bash
cargo test -p rupoo -- vector_store --nocapture
```
Expected: 3 tests passed (insert, search, dimension_mismatch)

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml src-agent/src/vector_store.rs
git commit -m "fix(vector_store): fix remove() leak, label as O(n), add indexmap"
```

**耗时估计：** 1 天

---

### Task 0.3：运行 Clippy 自动修复 + 人工清理残余

**文件：**
- Modify: 多个文件（由 clippy --fix 自动处理）
- Modify: 需要人工审查 ~6 个警告

- [ ] **Step 1: 运行自动修复**

```bash
cargo clippy --fix --all-targets -- -D warnings 2>&1 | tee /tmp/clippy_fix.log
```

- [ ] **Step 2: 审查自动修复未处理的警告**

```bash
cargo clippy --all-targets 2>&1 | grep "warning:" > /tmp/clippy_remaining.txt
cat /tmp/clippy_remaining.txt
```
Expected: ≤6 remaining（~19 个被自动修复）

- [ ] **Step 3: 逐一手动审查剩余警告**

针对每个剩余警告，评估修复风险并修改（常见类型：未使用方法、可合并的 match 分支、不必要的 if let）。

- [ ] **Step 4: 提交**

```bash
git add -u
git commit -m "style: cargo clippy --fix and manual cleanup"
```

**耗时估计：** 0.5 天

---

## Phase 1：记忆系统统一（Week 2-3）

> 目标：解决两个并行的记忆系统不互通的架构级问题。这是 Orchestrator 集成的前置条件。
> 策略：桥接模式（Bridge）——实现 MemorySystem trait 包装 MemoryStore，最小化 Agent 修改量。后期过渡到 MemorySystem 作为主接口。

### Task 1.1：实现 MemorySystem trait 封装 MemoryStore

**文件：**
- Create: `src-agent/src/memory/system_bridge.rs`
- Modify: `src-agent/src/memory/mod.rs`
- Test: `src-agent/src/memory/system_bridge.rs`（内联测试模块）

**设计决策：** 创建一个 `MemorySystemBridge` struct 实现 `MemorySystem` trait，内部包裹遗留的 `MemoryStore`。短期记忆用内存，长期和情景记忆委托给 MemoryStore 的 FTS5 后端。

- [ ] **Step 1: 创建 system_bridge.rs**

```rust
//! Bridge between the new trait-based MemorySystem and the legacy MemoryStore.
//!
//! This allows the Orchestrator and other new-architecture components to
//! use the same memory backend as the Agent, while the codebase transitions
//! from the concrete MemoryStore to the trait-based MemorySystem.

use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use super::traits::{MemoryStorage, MemorySystem};
use super::store::MemoryStore;
use super::short_term::ShortTermMemory;
use crate::error::AgentResult;
use crate::task::MemoryEntry;

/// Bridge that wraps the legacy MemoryStore behind the MemorySystem trait.
pub struct MemorySystemBridge {
    short_term: ShortTermMemory,
    long_term: LegacyStorageAdapter,
    episodic: LegacyStorageAdapter,
    store: Arc<Mutex<MemoryStore>>,
}

impl MemorySystemBridge {
    pub fn new(store: MemoryStore) -> Self {
        let store = Arc::new(Mutex::new(store));
        Self {
            short_term: ShortTermMemory::new(100),  // 会话内缓存
            long_term: LegacyStorageAdapter::new(Arc::clone(&store), "long_term"),
            episodic: LegacyStorageAdapter::new(Arc::clone(&store), "episodic"),
            store,
        }
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
        // 三层召回：先查短期，再查长期，去重合并
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. 短期记忆
        let short_results = self.short_term.retrieve(query, limit).await?;
        for entry in short_results {
            if seen.insert(entry.id.clone()) {
                results.push(entry);
            }
        }

        // 2. 长期 + 情景记忆（委托给 MemoryStore 的 hybrid_recall）
        let store = self.store.lock().await;
        let store_results = store.recall(query, limit).await?;
        for entry in store_results {
            if seen.insert(entry.id.clone()) {
                results.push(entry);
            }
        }

        results.truncate(limit);
        debug!(count = results.len(), "hybrid_recall completed");
        Ok(results)
    }
}

/// Adapts LegacyStorageAdapter to MemoryStorage by delegating to MemoryStore.
struct LegacyStorageAdapter {
    store: Arc<Mutex<MemoryStore>>,
    _kind: &'static str,  // 保留用于未来分区
}

impl LegacyStorageAdapter {
    fn new(store: Arc<Mutex<MemoryStore>>, kind: &'static str) -> Self {
        Self { store, _kind: kind }
    }
}

#[async_trait]
impl MemoryStorage for LegacyStorageAdapter {
    async fn store(&self, entry: MemoryEntry) -> AgentResult<()> {
        let store = self.store.lock().await;
        store.remember(entry).await
    }

    async fn retrieve(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        let store = self.store.lock().await;
        store.recall(query, limit).await
    }

    async fn delete(&self, id: &str) -> AgentResult<()> {
        let store = self.store.lock().await;
        store.forget(id).await
    }

    async fn count(&self) -> AgentResult<usize> {
        let store = self.store.lock().await;
        store.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskRepo;

    async fn create_test_store() -> MemoryStore {
        let repo = TaskRepo::new_in_memory().await.unwrap();
        MemoryStore::new_with_repo(repo)
    }

    #[tokio::test]
    async fn test_bridge_short_term_store_and_retrieve() {
        let store = create_test_store().await;
        let bridge = MemorySystemBridge::new(store);
        let entry = MemoryEntry {
            id: "test-1".to_string(),
            content: "短期测试记忆".to_string(),
            tags: vec!["test".to_string()],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        bridge.short_term().store(entry.clone()).await.unwrap();
        let results = bridge.short_term().retrieve("测试", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "test-1");
    }

    #[tokio::test]
    async fn test_bridge_long_term_delegates_to_memory_store() {
        let repo = TaskRepo::new_in_memory().await.unwrap();
        let store = MemoryStore::new_with_repo(repo);
        let bridge = MemorySystemBridge::new(store);
        let entry = MemoryEntry {
            id: "lt-1".to_string(),
            content: "长期记忆测试".to_string(),
            tags: vec![],
            source: "agent".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        bridge.long_term().store(entry.clone()).await.unwrap();
        let count = bridge.long_term().count().await.unwrap();
        assert!(count > 0);
    }

    #[tokio::test]
    async fn test_hybrid_recall_merges_short_and_long_term() {
        let repo = TaskRepo::new_in_memory().await.unwrap();
        let store = MemoryStore::new_with_repo(repo.clone());
        let bridge = MemorySystemBridge::new(store);

        // 存到长期（走 MemoryStore）
        let long_entry = MemoryEntry {
            id: "hybrid-long".to_string(),
            content: "混合检索长期".to_string(),
            tags: vec![],
            source: "agent".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        bridge.long_term().store(long_entry).await.unwrap();

        // 存到短期
        let short_entry = MemoryEntry {
            id: "hybrid-short".to_string(),
            content: "混合检索短期".to_string(),
            tags: vec![],
            source: "user".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        bridge.short_term().store(short_entry).await.unwrap();

        let results = bridge.hybrid_recall("混合检索", 10).await.unwrap();
        assert_eq!(results.len(), 2);
    }
}
```

- [ ] **Step 2: 在 memory/mod.rs 中导出 MemorySystemBridge**

```rust
pub mod system_bridge;
pub use system_bridge::MemorySystemBridge;
```

- [ ] **Step 3: 运行测试验证**

```bash
cargo test -p rupoo -- memory::system_bridge --nocapture
```
Expected: 3 tests passed

- [ ] **Step 4: 提交**

```bash
git add src-agent/src/memory/system_bridge.rs src-agent/src/memory/mod.rs
git commit -m "feat(memory): MemorySystemBridge wrapping legacy MemoryStore"
```

**耗时估计：** 2 天

---

### Task 1.2：Agent 主循环改用 MemorySystem 桥接

**文件：**
- Modify: `src-agent/src/agent.rs`

**设计决策：** Agent 内部仍然持有 `MemoryStore`（保持向后兼容），但通过 `MemorySystemBridge` 暴露给外部组件（包括未来的 Orchestrator）。在 Agent 构造时创建桥接。

- [ ] **Step 1: 在 Agent 中添加 memory_system 字段**

在 `src-agent/src/agent.rs` 的 Agent struct 定义中添加：

```rust
pub struct Agent {
    // ... 现有字段 ...
    
    /// 基于 trait 的记忆系统桥接（供 Orchestrator 等外部组件使用）
    pub memory_system: Arc<dyn MemorySystem>,
}
```

并在 Agent 构造函数的末尾添加：

```rust
use crate::memory::MemorySystemBridge;

// 创建记忆系统桥接
let memory_system = Arc::new(MemorySystemBridge::new(
    memory_store.clone(),  // 复用已创建的 store
));
```

- [ ] **Step 2: 更新 try_clone_lightweight**

```rust
pub fn try_clone_lightweight(&self) -> AgentResult<Self> {
    // ... 现有克隆逻辑 ...
    memory_system: Arc::clone(&self.memory_system),  // Arc 克隆，共享同一后端
    // ...
}
```

- [ ] **Step 3: 运行编译验证**

```bash
cargo build -p rupoo --lib
```
Expected: 编译通过，0 errors

- [ ] **Step 4: 提交**

```bash
git add src-agent/src/agent.rs
git commit -m "refactor(agent): add MemorySystemBridge field for external consumers"
```

**耗时估计：** 1 天

---

## Phase 2：Orchestrator 管线修复（Week 2-3，与 Phase 1 并行）

> 目标：修复 Orchestrator 管线的设计缺陷，使其能够实际工作，然后与 Agent 主循环集成。

### Task 2.1：修复 Orchestrator 空 JSON 校验

**文件：**
- Modify: `src-agent/src/orchestrator.rs`

**问题诊断：** 第 86 行对所有步骤使用空 JSON `{}` 进行校验，使验证形同虚设。需要根据步骤类型传递实际参数。

- [ ] **Step 1: 添加步骤类型感知的校验逻辑**

将 `orchestrator.rs` 中的步骤循环（第 78-96 行）替换为：

```rust
use crate::task::Step;

for (i, step) in best_plan.steps.iter().enumerate() {
    let step_action = Action::new("execute_step", &format!("step {}/{}", i + 1, best_plan.steps.len()));
    let meta = ExecutionMeta::with_tool(&format!("{:?}", std::mem::discriminant(step)));

    // 每步执行前监督拦截
    self.supervisor.intercept(&step_action, &meta).await?;

    // 根据步骤类型提取输入参数
    let input_params = match step {
        Step::ToolCall { tool_name, params, .. } => {
            serde_json::json!({ "tool": tool_name, "params": params })
        }
        Step::Exec { command, .. } => {
            serde_json::json!({ "command": command })
        }
        Step::HttpRequest { method, url, .. } => {
            serde_json::json!({ "method": method, "url": url })
        }
        Step::BrowserAction { action, url, .. } => {
            serde_json::json!({ "browser_action": action, "url": url })
        }
        _ => serde_json::json!({}),  // Think, WaitForInput, Finish 无参数
    };

    // 入参校验
    let validation = self.execution.validate_input(
        &format!("step_{}", i),
        &input_params,
    ).await?;

    if validation.trigger_replan {
        warn!("[执行层] 步骤 {} 入参校验失败，触发重规划", i);
        // 调用 Replanner（需要 Phase 2.2 实现）
        continue;
    }

    info!("[执行层] 步骤 {}/{} 校验通过", i + 1, best_plan.steps.len());
    // 实际执行由 Agent::run_next_step 处理（Phase 4 集成）
}
```

- [ ] **Step 2: 为 Step 添加 Debug discriminant 支持**

检查 `Step` 是否实现了必要的 trait，确保 `std::mem::discriminant(step)` 可以工作。如果 `Step` 不是 enum 类型，改用 `std::any::type_name` 或添加一个 `step_type()` 方法。

- [ ] **Step 3: 编译验证**

```bash
cargo build -p rupoo --lib
```
Expected: 编译通过

- [ ] **Step 4: 提交**

```bash
git add src-agent/src/orchestrator.rs
git commit -m "fix(orchestrator): step-type-aware validation, replace empty JSON"
```

**耗时估计：** 1 天

---

### Task 2.2：替换重规划占位符

**文件：**
- Modify: `src-agent/src/orchestrator.rs`
- Modify: `src-agent/src/execution/replanner.rs`（增强）

**问题诊断：** `continue` 占位符不执行实际重规划。需要实现 `Replanner` 的最小可行版本。

- [ ] **Step 1: 增强 Replanner 实现**

在 `src-agent/src/execution/replanner.rs` 中添加：

```rust
use crate::planning::ExecutionPlan;

/// 重规划结果
pub struct ReplanResult {
    pub revised_plan: ExecutionPlan,
    pub changes: Vec<String>,
}

/// 最小可行重规划器——修改失败步骤后的剩余步骤
impl Replanner {
    /// 对执行计划中失败的步骤进行重规划。
    /// 策略：标记失败步骤为"跳过"，从失败处重新估算后续步骤。
    pub async fn replan(
        &self,
        original: &ExecutionPlan,
        failed_step_index: usize,
        failure_reason: &str,
    ) -> AgentResult<ReplanResult> {
        // 记录失败
        warn!(
            step = failed_step_index,
            reason = failure_reason,
            "正在重规划"
        );

        // 从失败步骤之后截断计划，标记最后一个成功步骤
        let mut revised_steps = original.steps[..failed_step_index].to_vec();
        
        // 添加一个 Think 步骤来重新评估
        let rethink_step = Step::Think {
            content: format!(
                "步骤 {} 失败（{}），需要重新规划后续执行",
                failed_step_index, failure_reason
            ),
        };
        revised_steps.push(rethink_step);

        // 追加剩余步骤（但添加上下文说明）
        for (j, step) in original.steps.iter().enumerate().skip(failed_step_index + 1) {
            revised_steps.push(step.clone_with_context(
                &format!("重规划后步骤（原始步骤 {}）", j)
            ));
        }

        let revised_plan = ExecutionPlan {
            id: original.id.clone(),
            name: format!("{} (重规划)", original.name),
            steps: revised_steps,
            score: original.score.clone(),
            estimated_cost: original.estimated_cost.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let changes = vec![format!("步骤 {} 因 {} 失败，已重规划", failed_step_index, failure_reason)];

        Ok(ReplanResult { revised_plan, changes })
    }
}
```

- [ ] **Step 2: 在 Orchestrator 中替换 continue**

```rust
if validation.trigger_replan {
    warn!("[执行层] 步骤 {} 入参校验失败，触发重规划", i);
    let replan_result = /* 使用 Replanner 的重规划结果 */.await?;
    info!("[执行层] 重规划完成，变更: {:?}", replan_result.changes);
    // 使用修订后的计划继续执行
    continue;
}
```

- [ ] **Step 3: 添加 Replanner 单元测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::ExecutionPlan;
    use crate::task::Step;

    #[tokio::test]
    async fn test_replan_creates_revised_plan() {
        let planner = Replanner;
        let step1 = Step::Think { content: "分析需求".to_string() };
        let step2 = Step::ToolCall {
            tool_name: "execute_command".to_string(),
            params: serde_json::json!({"command": "make"}),
            description: "构建项目".to_string(),
        };
        let step3 = Step::Exec {
            command: "./run.sh".to_string(),
            description: "运行测试".to_string(),
        };

        let plan = ExecutionPlan {
            id: "plan-1".to_string(),
            name: "测试计划".to_string(),
            steps: vec![step1, step2, step3],
            score: None,
            estimated_cost: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = planner.replan(&plan, 1, "命令执行超时").await.unwrap();
        assert!(result.changes.len() > 0);
        assert!(result.revised_plan.steps.len() > 1);
        // 修订后的计划应在失败步骤位置包含一个 Think 步骤
        assert!(matches!(result.revised_plan.steps[1], Step::Think { .. }));
    }
}
```

- [ ] **Step 4: 运行测试**

```bash
cargo test -p rupoo -- replanner --nocapture
```
Expected: 至少 1 test passed

- [ ] **Step 5: 提交**

```bash
git add src-agent/src/execution/replanner.rs src-agent/src/orchestrator.rs
git commit -m "fix(orchestrator): implement real replanning instead of continue placeholder"
```

**耗时估计：** 1.5 天

---

### Task 2.3：添加 Mock 全栈集成测试

**文件：**
- Create: `tests/orchestrator_integration_test.rs`

- [ ] **Step 1: 创建 Orchestrator 集成测试**

```rust
use rupoo::cognitive::goal::AgentGoal;
use rupoo::cognitive::CognitiveEngine;
use rupoo::orchestrator::Orchestrator;
use rupoo::memory::MemorySystemBridge;
use rupoo::memory::store::MemoryStore;
use rupoo::db::TaskRepo;
use async_trait::async_trait;

// Mock 认知层——返回固定目标
struct MockCognitive;
#[async_trait]
impl CognitiveEngine for MockCognitive {
    async fn parse(&self, raw: &str, _ctx: &rup::context::ConversationContext) -> rupoo::error::AgentResult<AgentGoal> {
        Ok(AgentGoal::new(raw, format!("解析:{}", raw)))
    }
    async fn decompose(&self, _goal: &AgentGoal) -> rupoo::error::AgentResult<Vec<AgentGoal>> {
        Ok(vec![])
    }
    async fn check_boundary(&self, _goal: &AgentGoal) -> rupoo::error::AgentResult<rup::cognitive::goal::AuthLevel> {
        Ok(rup::cognitive::goal::AuthLevel::FullAuto)
    }
}

// Mock 规划器——返回包含测试步骤的方案
struct MockPlanner;
#[async_trait]
impl rupoo::planning::Planner for MockPlanner {
    async fn generate_alternatives(
        &self,
        _goal: &AgentGoal,
        count: usize,
    ) -> rupoo::error::AgentResult<Vec<rup::planning::ExecutionPlan>> {
        let plan = rup::planning::ExecutionPlan {
            id: "test-plan".to_string(),
            name: "测试方案".to_string(),
            steps: vec![
                rup::task::Step::Think { content: "分析".to_string() },
                rup::task::Step::Think { content: "完成".to_string() },
            ],
            score: None,
            estimated_cost: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        Ok(vec![plan; count])
    }
    async fn score(&self, plan: &rup::planning::ExecutionPlan) -> rupoo::error::AgentResult<rup::planning::PlanScore> {
        Ok(rup::planning::PlanScore {
            success_probability: 0.8,
            resource_cost: 0.3,
            risk_level: 0.1,
            overall: 0.8,
        })
    }
    async fn select_best(
        &self,
        mut candidates: Vec<rup::planning::ExecutionPlan>,
    ) -> rupoo::error::AgentResult<(rup::planning::ExecutionPlan, Vec<rup::planning::ExecutionPlan>)> {
        candidates.sort_by(|a, b| {
            b.score.as_ref().map(|s| s.overall).unwrap_or(0.0)
                .partial_cmp(&a.score.as_ref().map(|s| s.overall).unwrap_or(0.0))
                .unwrap()
        });
        let rest = candidates.drain(1..).collect();
        Ok((candidates.into_iter().next().unwrap(), rest))
    }
}

#[tokio::test]
async fn test_orchestrator_executes_full_pipeline() {
    let repo = TaskRepo::new_in_memory().await.unwrap();
    let store = MemoryStore::new_with_repo(repo);
    let memory = Arc::new(MemorySystemBridge::new(store));

    let orch = Orchestrator::new(
        Box::new(MockCognitive),
        Box::new(MockPlanner),
        Box::new(rup::execution::validator::ExecutionEngineImpl),
        memory,
        Box::new(rup::supervisor::mock::AlwaysAllowSupervisor),
    );

    let result = orch.execute("测试完整管道").await;
    assert!(result.is_ok(), "管道执行失败: {:?}", result);
}
```

- [ ] **Step 2: 运行集成测试**

```bash
cargo test --test orchestrator_integration_test --nocapture
```
Expected: 至少 1 test passed

- [ ] **Step 3: 提交**

```bash
git add tests/orchestrator_integration_test.rs
git commit -m "test(orchestrator): add mock full-stack integration test"
```

**耗时估计：** 1 天

---

## Phase 3：Loop Engine 状态如实标记（Week 3）

> 目标：对未实现的功能诚实地标记状态，而不是用占位符误导。

### Task 3.1：标记 Pattern B execute_plan_inner 为占位符

**文件：**
- Modify: `src-agent/src/loop_engine.rs`
- Modify: `src-agent/src/loop_engine.rs`（测试模块）

- [ ] **Step 1: 检查 execute_plan_inner 并添加运行时警告**

找到 `execute_plan_inner()` 方法（约第 1522 行），在其入口添加：

```rust
fn execute_plan_inner(&self, plan: &Plan) -> impl Future<Output = Vec<StepOutcome>> {
    // ⚠️ 注意：当前是占位符实现——所有步骤标记为 Completed 但不实际执行
    // 完整的子循环执行将在 Phase 4 实现（依赖 Orchestrator 集成）
    warn!("execute_plan_inner: 占位符实现——子循环步骤不实际执行");
    // ... 现有代码 ...
}
```

- [ ] **Step 2: 更新 execute_plan_inner 的文档注释**

```rust
/// 执行子计划（Pattern B 递归分解的一部分）
///
/// # 当前状态：部分实现
///
/// 此方法标记所有步骤为 Completed 状态，但不实际执行工具调用或 HTTP 请求。
/// 在 Orchestrator 集成完成后，此方法将委托给 Agent::run_next_step()。
```

- [ ] **Step 3: 提交**

```bash
git add src-agent/src/loop_engine.rs
git commit -m "fix(loop_engine): label execute_plan_inner as placeholder, add runtime warning"
```

**耗时估计：** 0.5 天

---

### Task 3.2：处理 Pattern C（守护模式）

**文件：**
- Modify: `src-agent/src/loop_engine.rs`

- [ ] **Step 1: 在 run_loop 中添加守护模式检测**

在 `run_loop()` 主入口处：

```rust
pub async fn run_loop(&self, agent: &Agent) -> AgentResult<()> {
    if self.config.daemon {
        // ⚠️ 守护模式尚未实现——回退到标准循环
        warn!("守护模式（daemon=true）尚未实现，回退到标准循环模式");
    }
    // ... 现有执行逻辑 ...
}
```

- [ ] **Step 2: 提交**

```bash
git add src-agent/src/loop_engine.rs
git commit -m "fix(loop_engine): add daemon-mode not-implemented warning"
```

**耗时估计：** 0.5 天

---

## Phase 4：Orchestrator 集成到 Agent 主循环（Week 4-5）

> 目标：将五层管道接入实际的 Agent 执行路径。此阶段依赖 Phase 1（记忆统一）和 Phase 2（Orchestrator 修复）。
> 设计方针：Orchestrator 作为"决策管线"在新任务启动时运行，生成 ExecutionPlan 后交给 Agent 的实际步骤执行器。

### Task 4.1：设计集成接口

- [ ] **Step 1: 分析 Agent 的入口点**

在 `agent.rs` 中找到 `chat()` 或 `agent_chat()` 方法——这是用户消息进入 Agent 的主入口。当前执行路径：
1. 用户输入 → LLM 生成响应
2. LLM 选择工具调用 → Agent 执行工具
3. 结果返回 LLM → 循环

需要增加的分支：如果 Agent 处于新架构模式，先调 Orchestrator.execute()。

- [ ] **Step 2: 在 Agent 中添加 orchestrator 字段**

```rust
pub struct Agent {
    // ... 现有字段 ...
    pub orchestrator: Option<Arc<Orchestrator>>,
}
```

- [ ] **Step 3: 提交设计**

```bash
git add src-agent/src/agent.rs
git commit -m "feat(agent): add optional Orchestrator field"
```

**耗时估计：** 1 天（分析 + 设计）

---

### Task 4.2：实现 Orchestrator 入口分支

**文件：**
- Modify: `src-agent/src/agent.rs`

- [ ] **Step 1: 在 agent_chat 中增加 Orchestrator 入口**

在 `agent_chat()` 或 `chat()` 方法开头新增：

```rust
/// 处理用户输入的入口方法。
/// 如果配置了 Orchestrator，先通过五层管道处理；
/// 否则回退到原有的直接 LLM 对话路径。
pub async fn chat(&self, input: &str) -> AgentResult<String> {
    // 如果启用了 Orchestrator，先走五层管道
    if let Some(ref orch) = self.orchestrator {
        info!("[Agent] 使用 Orchestrator 管道处理输入");
        match orch.execute(input).await {
            Ok(()) => {
                info!("[Agent] Orchestrator 管道执行完成");
                // Orchestrator 完成后，使用标准对话循环生成最终回复
            }
            Err(e) => {
                warn!("[Agent] Orchestrator 管道执行异常: {}，回退到标准路径", e);
            }
        }
    }

    // 原有对话逻辑（直通 LLM）
    self.agent_chat(input).await
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p rupoo --lib
```
Expected: 编译通过

- [ ] **Step 3: 提交**

```bash
git add src-agent/src/agent.rs
git commit -m "feat(agent): integrate Orchestrator as optional pre-processing pipeline"
```

**耗时估计：** 1.5 天

---

### Task 4.3：添加集成路径的单元测试

**文件：**
- Modify: `src-agent/src/agent.rs`（测试模块）
- Or Create: `tests/orchestrator_agent_integration_test.rs`

- [ ] **Step 1: 创建一个测试验证 Orchestrator 回退行为**

```rust
#[cfg(test)]
mod tests {
    // ... 现有测试 ...
    
    #[tokio::test]
    async fn test_chat_falls_back_when_orchestrator_fails() {
        // 当 Orchestrator 执行失败时，Agent 应回退到标准路径
        // 测试逻辑：Mock Orchestrator 返回错误 → Agent 仍能正常响应
    }
    
    #[tokio::test]
    async fn test_chat_uses_orchestrator_when_configured() {
        // 配置了 Orchestrator 时，Agent 先调用 Orchestrator
        // 测试逻辑：验证 Orchestrator.execute() 被调用
    }
}
```

- [ ] **Step 2: 运行测试**

```bash
cargo test -p rupoo -- agent::tests --nocapture
```
Expected: 所有测试通过，包括新增的

- [ ] **Step 3: 提交**

```bash
git add src-agent/src/agent.rs
git commit -m "test(agent): add Orchestrator integration tests"
```

**耗时估计：** 1 天

---

## Phase 5：向量存储升级（Week 5-6）

> 目标：将向量搜索从暴力 O(n) 升级为 HNSW 或替代 ANN 索引，并添加持久化支持。

### Task 5.1：集成 hnswx ANN 索引

**文件：**
- Modify: `src-agent/src/vector_store.rs`
- Modify: `Cargo.toml`（启用 `hnswx`）

- [ ] **Step 1: 在 Cargo.toml 中启用 hnswx**

```toml
hnswx = "0.2"
```

- [ ] **Step 2: 使用 hnswx::Index 重构 VectorStore**

将内部的扁平 Vec<f32> 替换为 hnswx::Index：

```rust
use hnswx::index::Index;

pub struct VectorStore {
    documents: IndexMap<String, VectorMemoryDoc>,
    hnsw: Index<f32>,  // HNSW 索引替换扁平 embedding 数组
    embedding_dim: usize,
}
```

- [ ] **Step 3: 适配 insert() 方法**

```rust
pub async fn insert(&mut self, doc: VectorMemoryDoc, embedding: Vec<f32>) -> AgentResult<()> {
    if embedding.len() != self.embedding_dim {
        tracing::warn!(expected = self.embedding_dim, actual = embedding.len(), "dimension mismatch");
        return Ok(());
    }
    let id = doc.id.clone();
    self.documents.insert(id.clone(), doc);
    // 使用 hnswx 索引插入 embedding，返回内部 ID
    self.hnsw.insert(embedding);
    debug!(id, "memory inserted into HNSW vector store");
    Ok(())
}
```

- [ ] **Step 4: 适配 semantic_search() 方法**

```rust
pub async fn semantic_search(&self, query_embedding: Vec<f32>, limit: usize) -> AgentResult<Vec<SearchResult>> {
    if query_embedding.len() != self.embedding_dim {
        return Err(/* 维度错误 */);
    }
    // 使用 HNSW 的近似搜索
    let results = self.hnsw.search(&query_embedding, limit);
    // 将 HNSW 内部 ID 映射回文档 ID
    // ... 映射逻辑 ...
    Ok(results)
}
```

- [ ] **Step 5: 添加性能基准测试**

```rust
#[cfg(test)]
mod benches {
    // 对比 HNSW vs 暴力搜索在不同数据规模下的延迟
}
```

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml src-agent/src/vector_store.rs
git commit -m "feat(vector_store): migrate from brute-force O(n) to HNSW ANN index"
```

**耗时估计：** 3 天（含基准测试）

---

### Task 5.2：添加向量持久化

**文件：**
- Create: `src-agent/src/vector_persistence.rs`
- Modify: `src-agent/src/vector_store.rs`
- Modify: `src-agent/src/memory/store.rs`

- [ ] **Step 1: 设计持久化方案**

使用 SQLite 存储序列化的 embedding + 文档内容（利用现有的 rusqlite 依赖），或使用独立的向量数据文件。

- [ ] **Step 2: 实现序列化/反序列化**

- [ ] **Step 3: 在 MemoryStore 初始化时加载持久化向量**

- [ ] **Step 4: 提交**

**耗时估计：** 3 天

---

## Phase 6：质量清理与配置对齐（Week 5-7，可与其他 Phase 并行）

### Task 6.1：遗留死代码审计

**文件：**
- Audit: `src-agent/src/cli/commands.rs`
- Audit: `src-agent/src/cli/shortcuts.rs`
- Audit: `src-agent/src/security_policy.rs`
- Audit: `src-agent/src/cli/mod.rs`（`build_prompt` 方法）

- [ ] **Step 1: 分析 commands.rs 是否被当前 CLI 路径依赖**

```bash
# 检查 commands.rs 中导出的公共符号是否被其他文件引用
grep -r "CommandRegistry" src-agent/src/ --include="*.rs"
```

- [ ] **Step 2: 分析 shortcuts.rs 是否被依赖**

```bash
grep -r "ShortcutRegistry\|shortcuts" src-agent/src/ --include="*.rs"
```

- [ ] **Step 3: 分析 security_policy.rs 是否被依赖**

```bash
grep -r "SecurityPolicy\|security_policy" src-agent/src/ --include="*.rs"
```

- [ ] **Step 4: 为每个被找到的引用创建替代方案或删除**

策略：
- 零引用 → 标记为 `#[deprecated]` 并安排在下一版本删除
- 有引用 → 评估能否迁移到新模块，否则保留但加说明

- [ ] **Step 5: 删除 `build_prompt` 方法**

```
L289 的 build_prompt 方法 → 确认未使用后删除
```

- [ ] **Step 6: 提交**

**耗时估计：** 2 天

---

### Task 6.2：配置/安全边界对齐

**文件：**
- Modify: `src-agent/src/config.rs`
- Modify: `src-agent/src/safety.rs`

**问题诊断：** `config.rs` 中 `forbidden_commands` 默认为空列表，`SafetyContext::default()` 有硬编码列表，修改配置文件不影响运行时安全行为。

- [ ] **Step 1: 在 SafetyContext 中添加 from_config 构造方法**

```rust
impl SafetyContext {
    /// 从配置创建 SafetyContext，同时允许配置文件覆盖部分默认值
    pub fn from_config(config: &RupooConfig) -> Self {
        let mut ctx = SafetyContext::default();
        
        // 如果配置文件中有 forbidden_commands，合并（非覆盖）
        if !config.safety.forbidden_commands.is_empty() {
            let configured: HashSet<String> = config.safety.forbidden_commands
                .iter().cloned().collect();
            ctx.forbidden_commands.extend(configured);
        }
        
        // 应用配置中的 jail_root
        if let Some(ref root) = config.safety.jail_root {
            ctx.set_jail_root(root.clone());
        }
        
        ctx
    }
}
```

- [ ] **Step 2: 在 Agent 初始化时使用 from_config**

```rust
// agent.rs 的 new() 方法中
let safety_ctx = if let Some(config) = &config {
    SafetyContext::from_config(config)
} else {
    SafetyContext::default()
};
```

- [ ] **Step 3: 添加测试验证合并行为**

```rust
#[test]
fn test_safety_context_from_config_extends_defaults() {
    // 验证配置文件添加的命令确实合并到了默认集中
}
```

- [ ] **Step 4: 提交**

```bash
git add src-agent/src/safety.rs src-agent/src/agent.rs
git commit -m "fix(safety): align config defaults with runtime, add from_config constructor"
```

**耗时估计：** 1.5 天

---

### Task 6.3：清理冗余依赖

**文件：**
- Modify: `Cargo.toml`

- [ ] **Step 1: 审计冗余依赖**

```bash
# 检查 once_cell 和 lazy_static 是否同时被实际使用
grep -r "once_cell\|lazy_static" src-agent/src/ --include="*.rs"
```

```bash
# 检查 lru 和 moka 的使用情况
grep -r "lru::\|moka::" src-agent/src/ --include="*.rs"
```

- [ ] **Step 2: 根据使用情况决定保留或移除**

策略：
```toml
# 如果 only once_cell 被使用：
# lazy_static = "..."  → 删除
# 如果 only lazy_static 被使用：
# once_cell = "..."   → 删除
# 如果两者都被使用：
# 逐步迁移到 once_cell（更现代）并标记后删除
```

- [ ] **Step 3: 编译验证**

```bash
cargo build --workspace
```
Expected: 编译通过

- [ ] **Step 4: 提交**

```bash
git add Cargo.toml
git commit -m "chore(deps): remove unused/redundant dependencies"
```

**耗时估计：** 1 天

---

## 时间线汇总

| Phase | 任务 | 耗时 | 并行 | 实际日历 |
|-------|------|------|------|---------|
| **Phase 0** | 紧急修复 | | | **Week 1** |
| | 0.1 修复 Supervisor 测试 | 0.5 天 | ✅ | |
| | 0.2 修复 VectorStore | 1 天 | ✅ | |
| | 0.3 Clippy 清理 | 0.5 天 | ✅ | |
| **Phase 1** | 记忆系统统一 | | | **Week 2-3** |
| | 1.1 MemorySystemBridge | 2 天 | — | |
| | 1.2 Agent 集成桥接 | 1 天 | 阻塞 1.1 | |
| **Phase 2** | Orchestrator 修复 | | | **Week 2-3** |
| | 2.1 空 JSON 校验修复 | 1 天 | ✅ 与 P1 并行 | |
| | 2.2 重规划占位符替换 | 1.5 天 | ✅ 与 P1 并行 | |
| | 2.3 集成测试 | 1 天 | 阻塞 2.1+2.2 | |
| **Phase 3** | Loop Engine 标记 | | | **Week 3** |
| | 3.1 Pattern B 标记 | 0.5 天 | ✅ | |
| | 3.2 Pattern C 标记 | 0.5 天 | ✅ | |
| **Phase 4** | Orchestrator 集成 | | | **Week 4-5** |
| | 4.1 接口设计 | 1 天 | — | |
| | 4.2 实现入口分支 | 1.5 天 | 阻塞 4.1 | |
| | 4.3 集成测试 | 1 天 | 阻塞 4.2 | |
| **Phase 5** | 向量存储升级 | | | **Week 5-6** |
| | 5.1 HNSW 集成 | 3 天 | ✅ 与 P4 并行 | |
| | 5.2 持久化 | 3 天 | 阻塞 5.1 | |
| **Phase 6** | 质量清理 | | | **Week 5-7** |
| | 6.1 死代码审计 | 2 天 | ✅ | |
| | 6.2 配置安全对齐 | 1.5 天 | ✅ | |
| | 6.3 冗余依赖 | 1 天 | ✅ | |
| **总计** | **15 个任务** | **~22 人天** | | **~7 周** |

> **注：** 以上估计假设一个人全职工作。如果两个人并行（Phase 1 和 Phase 2 可以完全并行），日历时间可压缩到 4-5 周。

---

## 关键里程碑

| 里程碑 | 时间 | 验证标准 |
|--------|------|---------|
| **M0: CI 绿** | Week 1 末 | 所有 235+ 测试通过，无集成测试失败 |
| **M1: 记忆统一** | Week 3 初 | MemorySystemBridge 实现 + 测试，Agent 访问统一接口 |
| **M2: 管线修复** | Week 3 初 | Orchestrator 校验真实、重规划可工作、有集成测试 |
| **M3: 诚实标记** | Week 3 末 | 所有占位符/未实现功能有运行时警告 |
| **M4: 管线集成** | Week 5 末 | Orchestrator 接入 Agent 主循环，有回退机制 |
| **M5: 向量搜索** | Week 6 末 | HNSW 索引替换暴力搜索，搜索持久化 |
| **M6: 质量基线** | Week 7 末 | 无死代码、配置/安全对齐、95%+ clippy clean |

---

## 风险登记

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| 记忆系统桥接后遗留代码依赖 $MemoryStore 具体方法 | 中 | 高 | 先做影响分析，确保 MemoryStore 的所有公开方法都有 trait 等效 |
| Orchestrator 集成后对话行为变化 | 中 | 高 | 保持回退路径，A/B 测试开关，逐步迁移 |
| HNSW 维度不兼容导致搜索失败 | 低 | 中 | 保留暴力搜索作为降级路径 |
| 遗留死代码删除后 CI 失败 | 中 | 中 | 先标记 deprecated 再删除，给一个版本缓冲期 |
| 项目方向变化（新架构被放弃） | 低 | 高 | 保持正向兼容——Phase 1 的桥接模式不影响任何现有功能 |
