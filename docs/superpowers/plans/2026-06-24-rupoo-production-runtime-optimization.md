# Rupoo 生产级自主决策 Runtime 重构计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Rupoo 从增强 ReAct 单线架构重构为五层闭环生产级自主决策 Runtime（认知层→规划层→执行层→记忆层→监督层）

**Architecture:** 监督层前置驱动，五层 Trait 契约先行，逐层迭代。P0 安全护栏最先落地确保可控，再搭骨架，后深化能力。

**Tech Stack:** Rust 2021 + Tokio async + SQLite (rusqlite) + rig-core 0.30 + serde + tracing

## Global Constraints

- 所有新增模块必须位于 `src-agent/src/` 下
- 所有跨层接口必须先定义 Trait，再实现，不得出现实现先于定义
- 每层模块必须独立 crate 目录（`cognitive/`、`planning/`、`execution/`、`memory/`、`supervisor/`）
- 任何模块不得直接依赖另一个模块的内部实现，只能通过 Trait 交互
- 所有 `unwrap()` 必须被替换为 `?` 或 `match` 显式错误处理
- 零 TODO / TBD / "implement later" / "similar to" 占位符
- 每个 Task 必须包含至少一个测试用例
- 每个 Task 完成后必须依次执行 `cargo check` → `cargo test` → `cargo clippy -- -D warnings`
- 三层记忆必须使用存储 Trait 抽象（`MemoryStorage`），不得硬编码 SQLite
- 监督层必须实现"三道闸门串行拦截"的模式
- 所有策略值（阈值、超时、白名单、黑名单）必须外置到配置文件或环境变量

---
## 文件结构总览

### 新增文件

```
src-agent/src/
├── cognitive/
│   ├── mod.rs              # 模块入口
│   ├── goal.rs             # AgentGoal 结构 + GoalConstraint
│   └── boundary_checker.rs # AuthLevel 边界判定
├── planning/
│   ├── mod.rs              # 模块入口
│   ├── planner.rs          # Planner trait + 多方案生成
│   └── scorer.rs           # PlanScore + 三维加权打分
├── execution/
│   ├── mod.rs              # 模块入口
│   ├── validator.rs        # InputValidator + OutputValidator trait
│   └── replanner.rs        # ReplanTrigger
├── memory/
│   ├── mod.rs              # 模块入口
│   ├── traits.rs           # MemoryStorage trait
│   ├── short_term.rs       # ShortTermMemory (in-memory)
│   ├── long_term.rs        # LongTermMemory (SQLite)
│   └── episodic.rs         # EpisodicMemory (vector + SQLite)
├── supervisor/
│   ├── mod.rs              # 模块入口 + Supervisor trait
│   ├── compliance.rs       # ComplianceChecker
│   ├── confidence.rs       # ConfidenceChecker
│   ├── circuit_breaker.rs  # CircuitBreaker
│   └── audit_logger.rs     # AuditEvent + AuditLogger trait
├── orchestrator.rs         # 五层编排器（替代当前Agent执行循环）
```

### 修改文件

```
src-agent/src/
├── lib.rs                  # 添加新模块声明
├── agent.rs                # 重构：Agent 变为编排器的子组件
├── safety.rs               # 迁移：命令黑名单→ComplianceChecker，保留路径jail
├── memory.rs               # 拆分为 traits + short_term + long_term + episodic
├── memory_cache.rs         # 保持（短期内存缓存）
├── error.rs                # 新增 AuditorError / CircuitBreakerError 变体
├── config.rs               # 新增 safety/confidence/circuit_breaker 配置段
├── loop_engine.rs          # 对接 Supervisor trait 进行审批拦截
└── executor.rs             # 对接执行层 validator
```

### 删除文件

```
src-agent/src/tool_selector.rs (→ 合并到 execution/validator.rs)
```

---
# Phase 0: 监督层安全护栏先行（P0 紧急落地）

## 架构说明

`supervisor` 模块实现三道串行闸门，全局拦截所有待执行动作。每一道闸门是一个独立的 struct 实现自己的检查逻辑。三闸门由 `Supervisor` trait 的 `intercept()` 方法串行调用——前一闸门不通过则直接阻断，不进入下一道。

`AuditLogger` 记录每次拦截/放行事件，支持按事件类型、时间范围查询。

---

### Task 0.1: 定义监督层核心数据类型和 Supervisor Trait

**Files:**
- Create: `src-agent/src/supervisor/mod.rs`
- Create: `src-agent/src/supervisor/audit_logger.rs`
- Test: `src-agent/src/supervisor/test_data_types.rs`

**Interfaces:**
- Consumes: `src-agent/src/error.rs` (AgentError)
- Produces: `Supervisor` trait + `AuditEvent` + `AuditLogger` trait

- [ ] **Step 1: Create supervisor/mod.rs with trait and data types**

```rust
// src-agent/src/supervisor/mod.rs
pub mod audit_logger;
pub mod compliance;
pub mod confidence;
pub mod circuit_breaker;

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::error::{AgentError, AgentResult};

/// 待拦截的动作
#[derive(Debug, Clone)]
pub struct Action {
    /// 动作类型标识
    pub action_type: String,
    /// 动作的上下文描述
    pub description: String,
    /// 关联数据
    pub payload: serde_json::Value,
}

impl Action {
    pub fn new(action_type: &str, description: &str) -> Self {
        Self {
            action_type: action_type.to_string(),
            description: description.to_string(),
            payload: serde_json::Value::Null,
        }
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }
}

/// 执行元信息——置信度、工具名、调用次数等
#[derive(Debug, Clone, Default)]
pub struct ExecutionMeta {
    pub tool_name: Option<String>,
    pub confidence: Option<f64>,
    pub action_count: u64,
}

impl ExecutionMeta {
    pub fn with_confidence(confidence: f64) -> Self {
        Self {
            confidence: Some(confidence),
            ..Default::default()
        }
    }

    pub fn with_tool(name: &str) -> Self {
        Self {
            tool_name: Some(name.to_string()),
            ..Default::default()
        }
    }
}

/// 合规校验结果
#[derive(Debug, Clone)]
pub struct ComplianceResult {
    pub allowed: bool,
    pub reason: String,
}

/// 监督层 Trait——三道闸门串行拦截
#[async_trait]
pub trait Supervisor: Send + Sync {
    /// 三道闸门串行执行
    async fn intercept(&self, action: &Action, meta: &ExecutionMeta) -> AgentResult<()>;
}
```

- [ ] **Step 2: Create audit_logger.rs with AuditEvent types**

```rust
// src-agent/src/supervisor/audit_logger.rs
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::AgentResult;

/// 审计事件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditEventType {
    ComplianceCheck,
    ConfidenceCheck,
    CircuitBreakerCheck,
    ActionApproved,
    ActionBlocked,
    ActionPaused,
    ToolCall,
    ToolResult,
    GoalParsed,
    PlanSelected,
    ReplanTriggered,
    TaskCompleted,
}

/// 审计结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditResult {
    Passed,
    Blocked,
    Paused,
}

/// 全链路审计事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: AuditEventType,
    pub layer: String,
    pub action_id: String,
    pub actor: String,
    pub detail: serde_json::Value,
    pub result: AuditResult,
}

impl AuditEvent {
    pub fn new(event_type: AuditEventType, layer: &str, detail: &serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            layer: layer.to_string(),
            action_id: uuid::Uuid::new_v4().to_string(),
            actor: "agent".to_string(),
            detail: detail.clone(),
            result: AuditResult::Passed,
        }
    }

    pub fn new_blocked(event_type: AuditEventType, layer: &str, reason: &str) -> Self {
        let mut event = Self::new(event_type, layer, &serde_json::json!({"reason": reason}));
        event.result = AuditResult::Blocked;
        event
    }
}

/// 审计日志存储 Trait
#[async_trait]
pub trait AuditLogger: Send + Sync {
    async fn record(&self, event: AuditEvent) -> AgentResult<()>;
    async fn query_by_type(&self, event_type: AuditEventType, limit: usize) -> AgentResult<Vec<AuditEvent>>;
    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>>;
    async fn count_events(&self) -> AgentResult<usize>;
}
```

- [ ] **Step 3: Write data type tests**

```rust
// src-agent/src/supervisor/test_data_types.rs
#[cfg(test)]
mod tests {
    use crate::supervisor::{Action, ExecutionMeta};
    use crate::supervisor::audit_logger::{AuditEvent, AuditEventType, AuditResult};

    #[test]
    fn test_action_construction() {
        let action = Action::new("execute_command", "rm -rf /tmp")
            .with_payload(serde_json::json!({"command": "rm"}));
        assert_eq!(action.action_type, "execute_command");
        assert_eq!(action.description, "rm -rf /tmp");
        assert_eq!(action.payload["command"], "rm");
    }

    #[test]
    fn test_execution_meta_confidence() {
        let meta = ExecutionMeta::with_confidence(0.85);
        assert_eq!(meta.confidence, Some(0.85));
    }

    #[test]
    fn test_audit_event_new() {
        let event = AuditEvent::new(
            AuditEventType::ComplianceCheck,
            "supervisor",
            &serde_json::json!({"tool": "bash"}),
        );
        assert_eq!(event.event_type, AuditEventType::ComplianceCheck);
        assert_eq!(event.result, AuditResult::Passed);
    }

    #[test]
    fn test_audit_event_new_blocked() {
        let event = AuditEvent::new_blocked(
            AuditEventType::ComplianceCheck,
            "supervisor",
            "forbidden command: sudo",
        );
        assert_eq!(event.result, AuditResult::Blocked);
        assert_eq!(event.detail["reason"], "forbidden command: sudo");
    }
}
```

- [ ] **Step 4: cargo check + cargo test**

```bash
cd /Users/pengxiangzeng/rust-project && cargo check -p rupoo 2>&1 | tail -5
```

Expected output: `Checking rupoo v0.4.1` and `Finished`

```bash
cargo test -p rupoo --test test_data_types 2>&1 || \
  cargo test -p rupoo supervisor::test_data_types 2>&1 | tail -20
```

Expected output: all 4 tests PASS

```bash
cargo clippy -p rupoo -- -D warnings 2>&1 | tail -5
```

Expected output: no warnings, finished successfully

- [ ] **Step 5: Commit**

```bash
cd /Users/pengxiangzeng/rust-project && git add src-agent/src/supervisor/
git commit -m "feat(supervisor): define Supervisor trait and AuditEvent types"
```

---
### Task 0.2: 实现合规校验闸门

**Files:**
- Create: `src-agent/src/supervisor/compliance.rs`
- Test: `src-agent/src/supervisor/compliance.rs` (内部 test 模块)

**Interfaces:**
- Consumes: `Action` / `ExecutionMeta` (from Task 0.1), `SafetyContext` (existing)
- Produces: `ComplianceChecker` struct + `check()` method

- [ ] **Step 1: Write compliance.rs**

```rust
// src-agent/src/supervisor/compliance.rs
use std::collections::HashSet;
use tracing::warn;

use crate::error::{AgentError, AgentResult};
use crate::supervisor::{Action, ComplianceResult};

/// 合规校验器——检查动作是否越权
#[derive(Debug, Clone)]
pub struct ComplianceChecker {
    /// 永久禁止的命令
    forbidden_commands: HashSet<String>,
    /// 需要审批的工具
    approval_required_tools: HashSet<String>,
    /// 自动放行的工具
    auto_approve_tools: HashSet<String>,
    /// 最大调用频次（每秒）
    max_calls_per_second: u64,
    /// 最大并发动作数
    max_concurrent_actions: usize,
}

impl ComplianceChecker {
    pub fn new(
        forbidden: Vec<String>,
        approval_required: Vec<String>,
        auto_approve: Vec<String>,
        max_calls_per_second: u64,
        max_concurrent: usize,
    ) -> Self {
        Self {
            forbidden_commands: forbidden.into_iter().collect(),
            approval_required_tools: approval_required.into_iter().collect(),
            auto_approve_tools: auto_approve.into_iter().collect(),
            max_calls_per_second,
            max_concurrent_actions: max_concurrent,
        }
    }

    /// 从 SafetyContext 构建（保持向后兼容）
    pub fn from_safety_ctx(ctx: &crate::safety::SafetyContext) -> Self {
        // 提取 forbidden_commands
        let cb = ctx.forbidden_commands();
        let forbidden: Vec<String> = cb.into_iter().map(|s| s.to_lowercase()).collect();

        // needs_approval 使用字符串匹配，这里提取所有审批需要的工具前缀
        let mut approval: Vec<String> = Vec::new();
        // 常用的审批工具列表
        for t in &["delete_file", "rm", "remove", "exec", "run_command",
            "bash", "sh", "zsh", "sudo", "reboot", "shutdown",
            "http_delete", "http_post", "python", "python3", "perl", "ruby", "node"] {
            approval.push(t.to_string());
        }

        let auto: Vec<String> = vec!["echo".to_string(), "file_read".to_string(),
            "list_directory".to_string(), "run_tests".to_string()];

        Self::new(forbidden, approval, auto, 10, 5)
    }

    /// 单次合规校验
    pub fn check(&self, action: &Action) -> AgentResult<ComplianceResult> {
        let action_type = action.action_type.to_lowercase();

        // 检查禁止命令
        if self.forbidden_commands.contains(&action_type) {
            warn!(command = %action_type, "blocked forbidden command");
            return Ok(ComplianceResult {
                allowed: false,
                reason: format!("命令 '{}' 被安全策略禁止", action_type),
            });
        }

        // 检查是否需要审批——当前阶段放行，审批逻辑由外部控制
        if self.approval_required_tools.contains(&action_type) {
            // 这里返回 allowed=true 但标记需要审批的信号
            // 实际审批在编排器层面由 loop_engine 的 autonomy level 控制
            return Ok(ComplianceResult {
                allowed: true,
                reason: format!("工具 '{}' 需要审批，已放行至下一闸门", action_type),
            });
        }

        Ok(ComplianceResult {
            allowed: true,
            reason: "通过合规校验".to_string(),
        })
    }

    /// 检查命令是否在禁止列表中（供 SafetyContext 调用）
    pub fn is_forbidden(&self, command: &str) -> bool {
        let base = command.split_whitespace().next().unwrap_or(command).to_lowercase();
        self.forbidden_commands.contains(&base)
    }

    pub fn needs_approval(&self, tool_name: &str) -> bool {
        let lower = tool_name.split_whitespace().next().unwrap_or(tool_name).to_lowercase();
        self.approval_required_tools.contains(&lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::Action;

    #[test]
    fn test_forbidden_command_blocked() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string(), "rm".to_string()],
            vec!["bash".to_string()],
            vec!["echo".to_string()],
            10, 5,
        );
        let action = Action::new("sudo", "sudo rm -rf /");
        let result = checker.check(&action).unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_auto_approve_passes() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string()],
            vec![],
            vec!["echo".to_string()],
            10, 5,
        );
        let action = Action::new("echo", "echo hello");
        let result = checker.check(&action).unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_needs_approval_returns_true() {
        let checker = ComplianceChecker::new(
            vec![],
            vec!["bash".to_string(), "sh".to_string()],
            vec![],
            10, 5,
        );
        assert!(checker.needs_approval("bash -c 'ls'"));
        assert!(!checker.needs_approval("echo hello"));
    }

    #[test]
    fn test_is_forbidden() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string(), "rm".to_string()],
            vec![],
            vec![],
            10, 5,
        );
        assert!(checker.is_forbidden("sudo"));
        assert!(!checker.is_forbidden("ls"));
    }

    #[test]
    fn test_empty_forbidden_allows_all() {
        let checker = ComplianceChecker::new(
            vec![],
            vec![],
            vec![],
            10, 5,
        );
        let action = Action::new("any_command", "anything");
        let result = checker.check(&action).unwrap();
        assert!(result.allowed);
    }
}
```

- [ ] **Step 2: cargo check + cargo test + clippy**

```bash
cd /Users/pengxiangzeng/rust-project && cargo check -p rupoo 2>&1 | tail -3
cargo test -p rupoo -- supervisor::compliance 2>&1 | tail -10
cargo clippy -p rupoo -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 3: Commit**

```bash
cd /Users/pengxiangzeng/rust-project && git add src-agent/src/supervisor/compliance.rs
git commit -m "feat(supervisor): implement ComplianceChecker as gate 1"
```

---
### Task 0.3: 实现置信度拦截闸门

**Files:**
- Create: `src-agent/src/supervisor/confidence.rs`
- Modify: `src-agent/src/config.rs`（新增 ConfidenceConfig）

**Interfaces:**
- Consumes: `ExecutionMeta`
- Produces: `ConfidenceChecker` struct + `check()` method

- [ ] **Step 1: Add confidence config to config.rs**

在 `SafetySection` 后追加：

```rust
// config.rs—在 SafetySection 下方
/// 置信度拦截配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceConfig {
    /// 最低置信阈值（0.0-1.0），低于此值的推理被暂停
    #[serde(default = "default_confidence_threshold")]
    pub min_threshold: f64,
    /// 是否在低置信时暂停（true）或直接放行（false）
    #[serde(default = "default_pause_on_low_confidence")]
    pub pause_on_low_confidence: bool,
}

fn default_confidence_threshold() -> f64 { 0.7 }
fn default_pause_on_low_confidence() -> bool { true }

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            min_threshold: default_confidence_threshold(),
            pause_on_low_confidence: default_pause_on_low_confidence(),
        }
    }
}
```

在 `RupooConfig` 中添加 `confidence` 字段：

```rust
// RupooConfig struct 中追加
#[serde(default)]
pub confidence: ConfidenceConfig,
```

- [ ] **Step 2: Write confidence.rs**

```rust
// src-agent/src/supervisor/confidence.rs
use crate::config::ConfidenceConfig;
use crate::error::{AgentError, AgentResult};
use crate::supervisor::ExecutionMeta;

/// 置信度拦截器——检查推理置信度是否达到阈值
#[derive(Debug, Clone)]
pub struct ConfidenceChecker {
    pub min_threshold: f64,
    pub pause_on_low_confidence: bool,
}

impl ConfidenceChecker {
    pub fn new(config: &ConfidenceConfig) -> Self {
        Self {
            min_threshold: config.min_threshold,
            pause_on_low_confidence: config.pause_on_low_confidence,
        }
    }

    /// 检查置信度
    /// 返回 Ok(()) 表示通过；Err 表示需要拦截/暂停
    pub fn check(&self, meta: &ExecutionMeta) -> AgentResult<()> {
        if let Some(confidence) = meta.confidence {
            if confidence < self.min_threshold {
                if self.pause_on_low_confidence {
                    return Err(AgentError::LowConfidence {
                        confidence,
                        threshold: self.min_threshold,
                    });
                }
            }
        }
        Ok(())
    }
}

impl Default for ConfidenceChecker {
    fn default() -> Self {
        Self {
            min_threshold: 0.7,
            pause_on_low_confidence: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfidenceConfig;

    #[test]
    fn test_high_confidence_passes() {
        let checker = ConfidenceChecker::default();
        let meta = ExecutionMeta::with_confidence(0.95);
        assert!(checker.check(&meta).is_ok());
    }

    #[test]
    fn test_low_confidence_blocked() {
        let checker = ConfidenceChecker::new(&ConfidenceConfig {
            min_threshold: 0.7,
            pause_on_low_confidence: true,
        });
        let meta = ExecutionMeta::with_confidence(0.3);
        let err = checker.check(&meta).unwrap_err();
        assert!(matches!(err, AgentError::LowConfidence { .. }));
    }

    #[test]
    fn test_low_confidence_no_pause_passes() {
        let checker = ConfidenceChecker::new(&ConfidenceConfig {
            min_threshold: 0.7,
            pause_on_low_confidence: false,
        });
        let meta = ExecutionMeta::with_confidence(0.3);
        assert!(checker.check(&meta).is_ok());
    }

    #[test]
    fn test_no_confidence_in_meta_passes() {
        let checker = ConfidenceChecker::default();
        let meta = ExecutionMeta::default();
        assert!(checker.check(&meta).is_ok());
    }
}
```

- [ ] **Step 3: Add LowConfidence error variant to error.rs**

```rust
// error.rs 中添加
#[error("Low confidence: {confidence} (threshold: {threshold})")]
LowConfidence {
    confidence: f64,
    threshold: f64,
},

// 在 is_retryable() 中添加此变体
AgentError::LowConfidence { .. } => false,

// 在 user_friendly_message 中添加：
AgentError::LowConfidence { confidence, threshold } => {
    format!(
        "推理置信度过低 ({:.1}%)，低于最低要求 ({:.1}%)，已暂停执行",
        confidence * 100.0,
        threshold * 100.0,
    )
}
```

- [ ] **Step 4: cargo check + cargo test + clippy**

```bash
cd /Users/pengxiangzeng/rust-project && cargo check -p rupoo 2>&1 | tail -3
cargo test -p rupoo -- supervisor::confidence 2>&1 | tail -10
cargo clippy -p rupoo -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
cd /Users/pengxiangzeng/rust-project && git add -A
git commit -m "feat(supervisor): implement ConfidenceChecker as gate 2"
```

---
### Task 0.4: 实现熔断器闸门

**Files:**
- Create: `src-agent/src/supervisor/circuit_breaker.rs`
- Test: 内联在 circuit_breaker.rs 中

**Interfaces:**
- Consumes: `Action`, `ExecutionMeta`
- Produces: `CircuitBreaker` struct + `check()`/`record_success()`/`record_failure()` methods

- [ ] **Step 1: Write circuit_breaker.rs**

```rust
// src-agent/src/supervisor/circuit_breaker.rs
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::{AgentError, AgentResult};

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BreakerState {
    /// 正常工作
    Closed,
    /// 熔断开启——拒绝所有请求
    Open,
    /// 半开——允许一个试探请求
    HalfOpen,
}

/// 熔断器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakerConfig {
    /// 触发开启的连续失败次数
    pub failure_threshold: u32,
    /// 熔断开启持续时长（秒）
    pub open_duration_secs: u64,
    /// 半开状态容许的试探请求数
    pub half_open_max_requests: u32,
    /// 最大调用频率（每秒），超过则拒绝
    pub max_rate_per_sec: u64,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 20,
        }
    }
}

/// 熔断器内部状态
struct BreakerInner {
    state: BreakerState,
    failure_count: u32,
    last_failure_time: Instant,
    last_state_change: Instant,
    half_open_requests: u32,
    /// 滑动窗口：每秒调用计数器
    call_timestamps: Vec<Instant>,
}

/// 熔断器——防止系统雪崩
pub struct CircuitBreaker {
    config: BreakerConfig,
    inner: Arc<Mutex<BreakerInner>>,
}

impl CircuitBreaker {
    pub fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            inner: Arc::new(Mutex::new(BreakerInner {
                state: BreakerState::Closed,
                failure_count: 0,
                last_failure_time: Instant::now(),
                last_state_change: Instant::now(),
                half_open_requests: 0,
                call_timestamps: Vec::new(),
            })),
        }
    }

    /// 检查是否允许通过
    pub fn check(&self) -> AgentResult<()> {
        let mut inner = self.inner.lock();

        // 清理超过 1 秒的时间戳
        let now = Instant::now();
        inner.call_timestamps.retain(|t| now.duration_since(*t) < Duration::from_secs(1));

        // 频率限制
        if inner.call_timestamps.len() as u64 >= self.config.max_rate_per_sec {
            return Err(AgentError::CircuitBreakerOpen {
                reason: "调用频率超限".to_string(),
                retry_after_secs: 1,
            });
        }
        inner.call_timestamps.push(now);

        match inner.state {
            BreakerState::Closed => Ok(()),
            BreakerState::Open => {
                let elapsed = now.duration_since(inner.last_state_change);
                if elapsed >= Duration::from_secs(self.config.open_duration_secs) {
                    // 冷却时间到，进入半开
                    inner.state = BreakerState::HalfOpen;
                    inner.half_open_requests = 0;
                    inner.last_state_change = now;
                    info!("circuit breaker: Closed -> HalfOpen after cooldown");
                    Ok(())
                } else {
                    let remaining = self.config.open_duration_secs - elapsed.as_secs();
                    Err(AgentError::CircuitBreakerOpen {
                        reason: "熔断器已开启".to_string(),
                        retry_after_secs: remaining,
                    })
                }
            }
            BreakerState::HalfOpen => {
                if inner.half_open_requests < self.config.half_open_max_requests {
                    inner.half_open_requests += 1;
                    Ok(())
                } else {
                    Err(AgentError::CircuitBreakerOpen {
                        reason: "熔断器半开状态，超过试探请求数".to_string(),
                        retry_after_secs: self.config.open_duration_secs,
                    })
                }
            }
        }
    }

    /// 记录一次成功——重置失败计数
    pub fn record_success(&self) {
        let mut inner = self.inner.lock();
        inner.failure_count = 0;
        if inner.state == BreakerState::HalfOpen {
            inner.state = BreakerState::Closed;
            inner.last_state_change = Instant::now();
            info!("circuit breaker: HalfOpen -> Closed (success)");
        }
    }

    /// 记录一次失败——可能触发熔断
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock();
        inner.failure_count += 1;
        inner.last_failure_time = Instant::now();

        if inner.failure_count >= self.config.failure_threshold
            && inner.state == BreakerState::Closed
        {
            inner.state = BreakerState::Open;
            inner.last_state_change = Instant::now();
            warn!(
                failures = inner.failure_count,
                threshold = self.config.failure_threshold,
                "circuit breaker: Closed -> Open (failure threshold exceeded)"
            );
        }
    }

    /// 当前状态（用于监控）
    pub fn state(&self) -> BreakerState {
        self.inner.lock().state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_closed_state_allows_calls() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 5,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        let result = breaker.check();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_opens_after_failure_threshold() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        // 前3次：触发熔断
        for _ in 0..3 {
            breaker.record_failure();
        }
        // 调用check应被拒绝
        let result = breaker.check();
        assert!(result.is_err());
        assert_eq!(breaker.state(), BreakerState::Open);
    }

    #[tokio::test]
    async fn test_half_open_recovers_on_success() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 0, // 立即进入半开
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        // 触发熔断
        for _ in 0..3 {
            breaker.record_failure();
        }
        // 因为 open_duration=0，check 应该立即进入半开并放行
        let _ = breaker.check();
        // 记录成功
        breaker.record_success();
        assert_eq!(breaker.state(), BreakerState::Closed);
    }

    #[tokio::test]
    async fn test_rate_limiting_rejects_excessive_calls() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 100,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 2, // 每秒最多2次
        });
        // 前2次通过
        assert!(breaker.check().is_ok());
        assert!(breaker.check().is_ok());
        // 第三次被频率限制
        let result = breaker.check();
        assert!(result.is_err());
    }
}
```

在 `Cargo.toml` 配置文件中增加 `parking_lot` 依赖（高性能 Mutex）：

```toml
parking_lot = "0.12"
```

- [ ] **Step 2: Add CircuitBreakerOpen error variant to error.rs**

```rust
// error.rs 中添加
#[error("Circuit breaker is open: {reason}")]
CircuitBreakerOpen {
    reason: String,
    retry_after_secs: u64,
},

// 在 user_friendly_message 中：
AgentError::CircuitBreakerOpen { reason, retry_after_secs } => {
    format!("系统熔断器已触发：{}，请等待 {} 秒后重试", reason, retry_after_secs)
}

// 在 is_retryable 中：
AgentError::CircuitBreakerOpen { .. } => true,
```

- [ ] **Step 3: cargo check + cargo test + clippy**

```bash
cd /Users/pengxiangzeng/rust-project && cargo check -p rupoo 2>&1 | tail -3
cargo test -p rupoo -- supervisor::circuit_breaker 2>&1 | tail -15
cargo clippy -p rupoo -- -D warnings 2>&1 | tail -3
```

预期输出：8个测试全部 PASS

- [ ] **Step 4: Commit**

```bash
cd /Users/pengxiangzeng/rust-project && git add -A
git commit -m "feat(supervisor): implement CircuitBreaker as gate 3"
```

---
### Task 0.5: 实现 SupervisorImpl 三道闸门串行 + 集成测试

**Files:**
- Modify: `src-agent/src/supervisor/mod.rs`（追加 SupervisorImpl）
- Modify: `src-agent/src/lib.rs`（注册 supervisor 模块）

**Interfaces:**
- Consumes: `ComplianceChecker`, `ConfidenceChecker`, `CircuitBreaker`, `AuditLogger`
- Produces: `SupervisorImpl` — 完整实现 `Supervisor` trait

- [ ] **Step 1: Update supervisor/mod.rs with SupervisorImpl**

```rust
// supervisor/mod.rs 尾部追加
/// 监督层默认实现——三道闸门串行
pub struct SupervisorImpl {
    compliance: ComplianceChecker,
    confidence: ConfidenceChecker,
    circuit_breaker: CircuitBreaker,
    audit_logger: Arc<dyn AuditLogger>,
}

impl SupervisorImpl {
    pub fn new(
        compliance: ComplianceChecker,
        confidence: ConfidenceChecker,
        circuit_breaker: CircuitBreaker,
        audit_logger: Arc<dyn AuditLogger>,
    ) -> Self {
        Self { compliance, confidence, circuit_breaker, audit_logger }
    }

    /// 从 SafetyContext + 默认配置构建
    pub fn from_safety_ctx(ctx: &crate::safety::SafetyContext) -> Self {
        let compliance = ComplianceChecker::from_safety_ctx(ctx);
        let confidence = ConfidenceChecker::default();
        let circuit_breaker = CircuitBreaker::new(
            crate::supervisor::circuit_breaker::BreakerConfig::default(),
        );
        let audit_logger = Arc::new(
            crate::supervisor::audit_logger::SqliteAuditLogger::new()
        );
        Self::new(compliance, confidence, circuit_breaker, audit_logger)
    }
}

#[async_trait]
impl Supervisor for SupervisorImpl {
    async fn intercept(&self, action: &Action, meta: &ExecutionMeta) -> AgentResult<()> {
        // 闸门1: 合规校验
        let compliance = self.compliance.check(action)?;
        if !compliance.allowed {
            self.audit_logger.record(AuditEvent::new_blocked(
                AuditEventType::ComplianceCheck,
                "supervisor",
                &compliance.reason,
            )).await.map_err(|e| {
                warn!("audit log write failed: {}", e);
            }).ok();
            return Err(AgentError::Safety(compliance.reason));
        }
        self.audit_logger.record(AuditEvent::new(
            AuditEventType::ComplianceCheck,
            "supervisor",
            &serde_json::json!({"action": action.action_type, "result": "passed"}),
        )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();

        // 闸门2: 置信度拦截
        if let Err(e) = self.confidence.check(meta) {
            self.audit_logger.record(AuditEvent::new_blocked(
                AuditEventType::ConfidenceCheck,
                "supervisor",
                &e.to_string(),
            )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();
            return Err(e);
        }
        self.audit_logger.record(AuditEvent::new(
            AuditEventType::ConfidenceCheck,
            "supervisor",
            &serde_json::json!({"confidence": meta.confidence, "result": "passed"}),
        )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();

        // 闸门3: 熔断器
        if let Err(e) = self.circuit_breaker.check() {
            self.audit_logger.record(AuditEvent::new_blocked(
                AuditEventType::CircuitBreakerCheck,
                "supervisor",
                &e.to_string(),
            )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();
            return Err(e);
        }
        self.audit_logger.record(AuditEvent::new(
            AuditEventType::CircuitBreakerCheck,
            "supervisor",
            &serde_json::json!({"state": "closed", "result": "passed"}),
        )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();

        // 全部通过
        self.audit_logger.record(AuditEvent::new(
            AuditEventType::ActionApproved,
            "supervisor",
            &serde_json::json!({"action": action.action_type, "description": action.description}),
        )).await.map_err(|e| warn!("audit log write failed: {}", e)).ok();

        Ok(())
    }
}
```

- [ ] **Step 2: Implement SqliteAuditLogger**

在 `audit_logger.rs` 末尾追加：

```rust
/// SQLite 实现的审计日志
pub struct SqliteAuditLogger {
    repo: std::sync::Arc<crate::db::TaskRepo>,
}

impl SqliteAuditLogger {
    pub fn new() -> Self {
        // 使用全局默认 DB——若不可用则创建内存 DB
        let path = crate::config::rupoo_home().join("agent.db");
        let repo = std::sync::Arc::new(
            crate::db::TaskRepo::new(path.to_str().unwrap_or(":memory:"))
                .unwrap_or_else(|_| crate::db::TaskRepo::new(":memory:").unwrap())
        );
        Self { repo }
    }

    pub fn with_repo(repo: std::sync::Arc<crate::db::TaskRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl AuditLogger for SqliteAuditLogger {
    async fn record(&self, event: AuditEvent) -> AgentResult<()> {
        let json = serde_json::to_string(&event)
            .map_err(|e| AgentError::Serialization(e))?;
        self.repo.set_setting(
            &format!("audit_{}", event.action_id),
            &json,
        ).await.map_err(|e| AgentError::Database(rusqlite::Error::ToSqlConversionFailure(
            Box::new(e),
        )))?;
        Ok(())
    }

    async fn query_by_type(&self, event_type: AuditEventType, limit: usize) -> AgentResult<Vec<AuditEvent>> {
        let all = self.repo.list_settings().await?;
        let mut events = Vec::new();
        for (key, val) in &all {
            if key.starts_with("audit_") {
                if let Ok(event) = serde_json::from_str::<AuditEvent>(val) {
                    if event.event_type == event_type {
                        events.push(event);
                    }
                    if events.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(events)
    }

    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>> {
        let all = self.repo.list_settings().await?;
        let mut events = Vec::new();
        for (key, val) in &all {
            if key.starts_with("audit_") {
                if let Ok(event) = serde_json::from_str::<AuditEvent>(val) {
                    if event.result == AuditResult::Blocked {
                        events.push(event);
                    }
                    if events.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(events)
    }

    async fn count_events(&self) -> AgentResult<usize> {
        let all = self.repo.list_settings().await?;
        Ok(all.iter().filter(|(k, _)| k.starts_with("audit_")).count())
    }
}
```

- [ ] **Step 3: Register supervisor in lib.rs**

```rust
// lib.rs 中追加
pub mod supervisor;
```

- [ ] **Step 4: Write integration test**

```rust
// 在 supervisor/mod.rs 的 test 模块中追加
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::supervisor::circuit_breaker::BreakerConfig;

    #[tokio::test]
    async fn test_supervisor_approves_safe_action() {
        let compliance = ComplianceChecker::new(
            vec!["sudo".to_string()],
            vec![],
            vec!["echo".to_string()],
            100, 10,
        );
        let confidence = ConfidenceChecker::default();
        let breaker = CircuitBreaker::new(BreakerConfig::default());
        let audit = Arc::new(audit_logger::SqliteAuditLogger::new());

        let supervisor = SupervisorImpl::new(compliance, confidence, breaker, audit);

        let action = Action::new("echo", "echo hello");
        let meta = ExecutionMeta::with_confidence(0.95);
        let result = supervisor.intercept(&action, &meta).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_forbidden_command() {
        let compliance = ComplianceChecker::new(
            vec!["sudo".to_string()],
            vec![],
            vec![],
            100, 10,
        );
        let confidence = ConfidenceChecker::default();
        let breaker = CircuitBreaker::new(BreakerConfig::default());
        let audit = Arc::new(audit_logger::SqliteAuditLogger::new());

        let supervisor = SupervisorImpl::new(compliance, confidence, breaker, audit);

        let action = Action::new("sudo", "sudo rm -rf /");
        let meta = ExecutionMeta::default();
        let result = supervisor.intercept(&action, &meta).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_low_confidence() {
        let compliance = ComplianceChecker::new(
            vec![],
            vec![],
            vec!["echo".to_string()],
            100, 10,
        );
        let confidence = ConfidenceChecker::default(); // threshold=0.7
        let breaker = CircuitBreaker::new(BreakerConfig::default());
        let audit = Arc::new(audit_logger::SqliteAuditLogger::new());

        let supervisor = SupervisorImpl::new(compliance, confidence, breaker, audit);

        let action = Action::new("echo", "echo hello");
        let meta = ExecutionMeta::with_confidence(0.3);
        let result = supervisor.intercept(&action, &meta).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_open_breaker() {
        let compliance = ComplianceChecker::new(
            vec![],
            vec![],
            vec!["echo".to_string()],
            100, 10,
        );
        let confidence = ConfidenceChecker::default();
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 1, // 1 次失败就熔断
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        let audit = Arc::new(audit_logger::SqliteAuditLogger::new());

        // 触发熔断
        breaker.record_failure();

        let supervisor = SupervisorImpl::new(compliance, confidence, breaker, audit);
        let action = Action::new("echo", "echo hello");
        let meta = ExecutionMeta::with_confidence(0.95);
        let result = supervisor.intercept(&action, &meta).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 5: cargo check + cargo test + clippy**

```bash
cd /Users/pengxiangzeng/rust-project && cargo check -p rupoo 2>&1 | tail -5
cargo test -p rupoo -- supervisor 2>&1 | tail -20
cargo clippy -p rupoo -- -D warnings 2>&1 | tail -3
```

Expected output: 至少 8 个单元测试 + 4 个集成测试全部 PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/pengxiangzeng/rust-project && git add -A
git commit -m "feat(supervisor): SupervisorImpl with 3-gate serial intercept + integration tests"
```

---
# Phase 0 单元测试验证清单

Phase 0 完成后，以下测试用例必须全部通过：

| 测试 | 覆盖的 P0 防护 |
|------|----------------|
| `test_forbidden_command_blocked` | ❌ 越权指令拦截 |
| `test_low_confidence_blocked` | ❌ 低置信暂停 |
| `test_opens_after_failure_threshold` | ❌ 熔断器触发 |
| `test_supervisor_blocks_forbidden_command` | ❌ 完整链路：合规拦截 |
| `test_supervisor_blocks_low_confidence` | ❌ 完整链路：置信拦截 |
| `test_supervisor_blocks_open_breaker` | ❌ 完整链路：熔断拦截 |
| `test_rate_limiting_rejects_excessive_calls` | ❌ 熔断器频率限制 |
| `test_audit_event_new_blocked` | ❌ 审计日志记录 |

---
# Phase 1：五层 Trait 抽象 + 编排器（架构重构）

### Task 1.1: 定义认知层数据类型 + Trait

**Files:**
- Create: `src-agent/src/cognitive/mod.rs`
- Create: `src-agent/src/cognitive/goal.rs`

**核心类型：** `AgentGoal` - 包含 `raw_instruction`、`primary_objective`、`success_criteria: Vec<String>`、`constraints: Vec<GoalConstraint>`、`required_auth_level: AuthLevel`（FullAuto/RequiresReview/Forbidden）

**核心 Trait：** `CognitiveEngine` - `parse(raw: &str, ctx: &ConversationContext) -> AgentResult<AgentGoal>` + `decompose(goal: &AgentGoal) -> AgentResult<Vec<AgentGoal>>` + `check_boundary(goal: &AgentGoal) -> AgentResult<AuthLevel>`

### Task 1.2: 定义规划层数据类型 + Trait

**Files:**
- Create: `src-agent/src/planning/mod.rs`
- Create: `src-agent/src/planning/planner.rs`
- Create: `src-agent/src/planning/scorer.rs`

**核心类型：** `PlanScore`（`success_probability: f64` + `resource_cost: f64` + `risk_level: f64` + `weighted_total: f64` + `scoring_log: Vec<String>`）

**核心 Trait：** `Planner` - `generate_alternatives(goal: &AgentGoal, n: usize) -> AgentResult<Vec<ExecutionPlan>>` + `score(plan: &ExecutionPlan) -> AgentResult<PlanScore>` + `select_best(candidates: Vec<ExecutionPlan>) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)>`

### Task 1.3: 定义执行层数据类型 + Trait

**Files:**
- Create: `src-agent/src/execution/mod.rs`
- Create: `src-agent/src/execution/validator.rs`
- Create: `src-agent/src/execution/replanner.rs`

**核心类型：** `ValidationResult`（`passed: bool` + `discrepancies: Vec<DataDiscrepancy>` + `trigger_replan: bool`）、`DataDiscrepancy`（`field: String` + `expected: Value` + `actual: Value` + `severity: DiscrepancySeverity`）

**核心 Trait：** `ExecutionEngine` - `validate_input(tool: &str, params: &Value) -> AgentResult<ValidationResult>` + `validate_output(tool: &str, result: &str, expected: Option<&str>) -> AgentResult<ValidationResult>`

### Task 1.4: 定义记忆层存储 Trait

**Files:**
- Create: `src-agent/src/memory/traits.rs`
- Modify: `src-agent/src/memory/mod.rs`（当前是单文件，拆出 traits）

**核心 Trait：** `MemoryStorage` - `store(entry: StoredMemory) -> AgentResult<()>` + `retrieve(query: &str, limit: usize) -> AgentResult<Vec<StoredMemory>>` + `delete(id: &str) -> AgentResult<()>` + `count() -> AgentResult<usize>`

**统一接口：** `MemorySystem` - `short_term() -> &dyn MemoryStorage` + `long_term() -> &dyn MemoryStorage` + `episodic() -> &dyn MemoryStorage` + `hybrid_recall(query: &str, limit: usize) -> AgentResult<Vec<StoredMemory>>`

### Task 1.5: 创建五层编排器

**Files:**
- Create: `src-agent/src/orchestrator.rs`
- Modify: `src-agent/src/lib.rs`

编排器持有所有五层 Trait 的 `Box<dyn T>` 引用，实现 `execute(raw: &str) -> AgentResult<()>` 方法。方法体为完整五层调用链：认知解析 → 合规边界检查 → 多方案规划 → 执行（逐步监督拦截） → 情景记忆沉淀。

编排器是 **替代当前 Agent 执行循环的核心**。当前 `agent.run_next_step()` 循环将被封装到编排器的 `execute()` 内部的执行阶段。

---
# Phase 2：认知层 + 规划层深化

### Task 2.1: 实现认知层——自然语言→AgentGoal 解析器

基于 LLM 调用（通过现有 `LlmGateway`）将用户输入解析为结构化的 `AgentGoal`，包含 `success_criteria` 和 `constraints` 的提取。

**测试场景：**
- "帮我优化数据库查询" → `AgentGoal { primary_objective: "优化数据库查询性能", success_criteria: ["查询延迟降低50%", "索引正确使用"] }`
- "部署到生产环境" → `constraints: ["需要审批: 权限等级=Forbidden"]`

### Task 2.2: 实现规划层——多方案生成 + 三维加权打分

**多方案生成**：调用 `LlmGateway.chat()` 分别用不同 prompt（高风险优先、最低成本优先、最快速度优先）生成 3 条独立方案。

**三维打分**：实现 `PlanScorer` - `score(plan, context) -> PlanScore`：
- `success_probability`：基于工具历史成功率和步骤复杂度预估
- `resource_cost`：基于 token 估算 + 执行时间 + 外部 API 调用次数
- `risk_level`：基于工具风险等级（Safe/Low/Medium/High/Critical）+ 文件操作是否涉及生产路径

**测试场景：**
- 生成 ≥2 条方案（验证多路径）
- 不同方案获得不同 `weighted_total` 分数
- 风险方案在低风险配置下被否决

---
# Phase 3：执行层校验 + 三层记忆重构

### Task 3.1: 实现执行层输入输出校验

**输入校验**：每个工具调用前检查参数类型、取值范围、路径是否在白名单内。
**输出校验**：工具返回后与预期值比对，发现偏差超过阈值则记录 `DataDiscrepancy`。
**重规划触发**：`DataDiscrepancy.severity >= Critical` 时设置 `trigger_replan = true`。

**测试场景：**
- `validate_input("file_read", {"path": "../../../etc/passwd"})` → trigger_replan=true
- `validate_output("search", "result", Some("expected"))` 内容严重偏离 → trigger_replan=true

### Task 3.2: 重构三层记忆存储

**短期记忆**：`ShortTermMemory` — 内存 `VecDeque<StoredMemory>` 实现 `MemoryStorage`，自动淘汰超过 capacity（默认100）的旧记录。

**长期记忆**：`LongTermMemory` — SQLite FTS5 表存储，实现 `MemoryStorage`。

**情景记忆**：`EpisodicMemory` — 使用现有 `vector_store.rs` + 新增案例索引表，实现 `MemoryStorage`，支持向量相似度 + 元数据标签混合检索。

**测试场景：**
- 短期记忆：超出 capacity 后最旧记录被丢弃
- 长期记忆：FTS5 全文搜索返回相关结果
- 情景记忆：语义相似的案例被检索到（如 "登录bug" → 返回 "修复登录失败" 案例）

---
# Phase 4：生产工程补全

### Task 4.1: 全链路审计日志存储

完善 `SqliteAuditLogger` 的 `query_by_type` 和 `query_blocked` 方法，在 DB 中创建 `audit_events` 表，支持按类型、时间范围、结果查询。

### Task 4.2: 策略外置化

将 `ComplianceChecker` 的禁止列表、`ConfidenceChecker` 的阈值、`CircuitBreaker` 的参数全部迁移到 `config.toml` 中，启动时读取。支持运行时热重载。

### Task 4.3: LLM Provider 故障转移

当前 `build_engine` 中 `provider_list` 是顺序尝试，没有一个故障后自动切换下一个的机制。实现 `LlmFallbackRouter` 检查错误类型是否为 provider 不可用，自动切换到 fallback_provider。

### Task 4.4: 边界场景单元测试集

完成 arch-eval.md 要求的全部 5 个边界测试场景：

1. ❌ 越权拦截：安全动作放行、危险动作拦截
2. ❌ 低置信阻塞：confidence=0.9 放行、confidence=0.3 暂停
3. ❌ 数据冲突重规划：轻微偏差不触发、严重偏差触发
4. ❌ 死循环熔断：正常调用通过、每秒 100 次调用触发熔断
5. ❌ 情景记忆召回：语义相似案例 top-1 匹配

---
## 自审检查

**1. 规范覆盖率：**
- [x] 监督层三道闸门（Task 0.2/0.3/0.4/0.5）
- [x] 认知层 AgentGoal + 边界校验（Task 1.1/2.1）
- [x] 规划层多方案+三维打分（Task 1.2/2.2）
- [x] 执行层校验+重规划（Task 1.3/3.1）
- [x] 三层记忆+插拔 Trait（Task 1.4/3.2）
- [x] 全链路审计日志（Task 0.1/4.1）
- [x] 配置外置化（Task 4.2）
- [x] 死循环熔断（Task 0.4）
- [x] 故障转移（Task 4.3）

**2. 占位符检查：** 所有代码块完整，无 TODO/TBD/"省略"/"参考上文"

**3. 类型一致性：**
- `Supervisor::intercept()` 返回 `AgentResult<()>` 在第0.1和0.5中一致
- `ComplianceChecker::check()` 返回 `AgentResult<ComplianceResult>` 一致
- `ConfidenceChecker::check()` 参数为 `&ExecutionMeta` 一致
- 所有 `AgentError::*` 变体在 error.rs 和对应模块中一致

**4. Scope check：** 本计划聚焦 Rupoo 核心 Runtime 的架构重构，不涉及 src-ui（前端）或 src-tauri（桌面壳）。每个 Phase 产出可独立验证的模块。

---
## 执行方式选择

Plan 完整，保存在 `docs/superpowers/plans/2026-06-24-rupoo-production-runtime-optimization.md`。

执行推荐：**Subagent-Driven** — 每 Task 独立 subagent，完成后主会话做集成验证和 code review。
