# Rupoo Loop Engineering — 完整设计文档

## Context

**背景**: rupoo v0.4.1 已具备成熟的 Plan/Step 顺序执行模型、三层重试机制、检查点崩溃恢复、技能系统、并行计划池和 MCP 协议集成。但缺少自适应迭代、递归任务分解和持续守护运行的能力。

**目标**: 在 rupoo 中集成 Loop Engineering，实现三种核心循环模式：
- **A. 自适应 Agent 循环** — 执行 → 评估 → 修正 → 重执行，直到达标
- **B. 递归任务分解** — 大目标自动拆解为子任务，独立执行后汇聚
- **C. 持续守护循环** — Agent 作为长期服务，监控条件、触发行动

**设计原则**: 新建 Loop 抽象层包装 Plan，Plan/Step 模型保持不变。Loop 是"元认知层"（评估、决策、调度），Plan 是"执行单元"（顺序执行步骤）。

**架构决策记录**:
| 决策点 | 选择 |
|--------|------|
| Q1: Loop Engineering 范围 | A+B+C 全部，按 A→B→C 顺序交付 |
| Q2: 交付次序 | 架构一次性设计，A→B→C 迭代实现 |
| Q3: 人机协作边界 | 分层审批 + 可配置自治等级 L1-L5 |
| Q4: Loop 与 Plan 关系 | 新建 Loop 抽象层，包装 Plan |
| Q5: 评估机制 | A+C 融合：结构化 JSON + diff 清单 (met/unmet/new_issues)，D 多镜头可选 |
| Q6: Loop 状态机 | 9 状态 + 预算/超时守卫 |
| Q7: 数据模型 | 三元模型: Loop → LoopRun → Plan |
| Q8: 持久化 | 复用 TaskRepo (SQLite)，新增 loops + loop_runs 表 |
| Q9: 调用入口 | LoopEngine 作为 Agent 内部组件 + Loop Mode 路由 |
| Q10: LLM 评估形态 | A+C 融合 + D 可选，保守兜底 |
| Q11: 递归分解 | D+A 组合：递归自相似分解 + 顺序汇聚 |

---

## 第 1 章：总体架构

### 1.1 分层架构

```
┌─────────────────────────────────────────────────────────────┐
│                    用户入口（不变）                           │
│          TUI (bridge.rs)  │  Tauri IPC  │  CLI               │
└──────────────────────────┬──────────────────────────────────┘
                           │
              ┌────────────▼────────────┐
              │      Agent (不变)        │
              │  ┌──────────────────┐   │
              │  │  LoopEngine (新增) │   │
              │  │  - 状态机驱动       │   │
              │  │  - LLM 评估        │   │
              │  │  - 递归分解        │   │
              │  │  - 预算守卫        │   │
              │  └──────┬───────────┘   │
              │         │ 调用          │
              │  ┌──────▼───────────┐   │
              │  │ Plan/Step (不变)  │   │
              │  └──────────────────┘   │
              │  ┌──────────────────┐   │
              │  │ LLM Gateway (不变) │   │
              │  └──────────────────┘   │
              └─────────────────────────┘
                           │
              ┌────────────▼────────────┐
              │    持久化层              │
              │    loops 表 (新增)       │
              │    loop_runs 表 (新增)   │
              │    plans / checkpoints (不变) │
              └─────────────────────────┘
```

### 1.2 路由层次

bridge.rs 新增 Loop Mode：

```
/loop "目标"  →  Loop Mode（新）    → LoopEngine.start_loop()
/plan "目标"  →  Plan Mode（不变）   → Agent.generate_plan()
其他           →  Chat Mode（不变）   → Agent.agent_chat()
```

### 1.3 三种运行模式共用 LoopEngine

- **自适应模式**: Loop → Plan → 评估 → 不达标 → 修正 Plan → 再执行
- **递归分解模式**: Loop → 评估 → decompose → 子 Loop → 汇聚 → 再评估
- **守护模式**: Loop + daemon:true → 定时检查触发条件 → 执行 Plan → 等待下一轮

---

## 第 2 章：核心数据模型

### 2.1 Loop 状态机（9 状态）

```
Pending ──► Running ──► StepComplete ──► Evaluating
  │            │              │               │
  │            ├─► Paused     │               ├─► Completed
  │            │   (可恢复)    │               ├─► Running (继续)
  │            │              │               ├─► Decomposing
  │            │              │               ├─► Failed
  │            │              │               ├─► BudgetExceeded
  │            │              │               └─► TimedOut
  │            │              │
  │            └─► WaitingForApproval
  │                 (通过→Running, 拒绝→Cancelled)
  │
  └─► (取消) Cancelled
```

### 2.2 Loop struct

```rust
// 新增文件: src-agent/src/loop_engine.rs

pub struct Loop {
    pub id: String,
    pub goal: String,
    pub status: LoopStatus,
    pub config: LoopConfig,
    pub current_run_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct LoopConfig {
    pub max_iterations: u32,           // 默认 10，硬上限防死循环
    pub token_budget: Option<u64>,     // None = 无限
    pub time_budget_secs: Option<u64>, // None = 无限
    pub autonomy_level: AutonomyLevel,
    pub daemon: bool,                  // 守护模式
    pub daemon_trigger: Option<String>,// 触发条件
    pub parallel_decomposition: bool,  // 默认 false
}

pub enum AutonomyLevel {
    L1Manual,       // 每步审批
    L2StepCheck,    // 仅高风险步骤审批
    L3RoundCheck,   // 每轮结束后审批（默认）
    L4AutoCorrect,  // 自主修正，仅不可恢复时暂停
    L5FullAuto,     // 完全自主
}

pub enum LoopStatus {
    Pending,
    Running,
    StepComplete,
    Evaluating,
    WaitingForApproval,
    WaitingForInput,
    Decomposing,
    Paused,
    Completed,
    Failed,
    BudgetExceeded,
    TimedOut,
    Cancelled,
}
```

### 2.3 LoopRun struct

```rust
pub struct LoopRun {
    pub id: String,
    pub loop_id: String,
    pub iteration: u32,
    pub plan_id: String,
    pub status: LoopRunStatus,
    pub evaluation: Option<EvaluationResult>,
    pub decision: Option<LoopDecision>,
    pub token_usage: Option<TokenUsage>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

pub struct EvaluationResult {
    pub verdict: LoopDecision,
    pub confidence: f64,
    pub met: Vec<String>,           // 已满足，每条附证据
    pub unmet: Vec<String>,         // 未满足，每条关联目标中的具体需求
    pub new_issues: Vec<String>,    // 新发现的问题
    pub next_action: String,        // 下一轮的具体行动项
}

pub enum LoopDecision {
    Done,
    Continue,
    Decompose,
    Impossible,
}
```

### 2.4 数据库 Schema

```sql
CREATE TABLE loops (
    id TEXT PRIMARY KEY,
    goal TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Pending',
    config_json TEXT NOT NULL,
    current_run_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE loop_runs (
    id TEXT PRIMARY KEY,
    loop_id TEXT NOT NULL REFERENCES loops(id),
    iteration INTEGER NOT NULL,
    plan_id TEXT REFERENCES plans(id),
    status TEXT NOT NULL,
    evaluation_json TEXT,
    decision TEXT,
    token_usage_json TEXT,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    UNIQUE(loop_id, iteration)
);

CREATE INDEX idx_loop_runs_loop_id ON loop_runs(loop_id);
CREATE INDEX idx_loop_runs_status ON loop_runs(status);
CREATE INDEX idx_loops_status ON loops(status);
```

### 2.5 与 Plan 的关系（三元模型）

```
Loop "L1", goal="优化项目性能"
  ├── LoopRun (iteration=1, plan_id="P1")
  │     Plan P1: 性能分析 → 找到瓶颈
  │     评估: Continue, unmet=["优化数据库查询", "减少 bundle size"]
  │
  ├── LoopRun (iteration=2, plan_id="P2")
  │     Plan P2: 优化数据库 + code splitting
  │     评估: Decompose (太复杂)
  │
  ├── LoopRun (iteration=3) → 子 Loop "L1.1" (优化数据库查询)
  ├── LoopRun (iteration=4) → 子 Loop "L1.2" (减少 bundle size)
  │
  └── LoopRun (iteration=5) 汇聚 → 评估: Done
```

---

## 第 3 章：LoopEngine 核心流程

### 3.1 LoopEngine 结构

```rust
// src-agent/src/loop_engine.rs

pub struct LoopEngine {
    repo: Arc<TaskRepo>,
    llm: Option<LlmGateway>,
    tool_executor: Box<dyn ToolExecutor>,
    safety: SafetyContext,
    plan_cache: PlanCache,
    memory: MemoryCache,
    active_loops: HashMap<String, LoopState>,
    cancel_flag: Arc<AtomicBool>,
}

struct LoopState {
    loop_data: Loop,
    current_run: LoopRun,
    agent: Arc<Agent>,
    child_loops: Vec<String>,
}
```

### 3.2 主循环流程

```
start_loop(goal)
  ├─ 1. 创建 Loop (status=Pending)
  ├─ 2. 生成初始 Plan (LLM: goal → Plan)
  ├─ 3. 创建 LoopRun (iteration=1, plan_id)
  ├─ 4. status → Running
  ▼
┌─────────────────────────────────────────┐
│         主循环: run_loop()               │
│  loop {                                  │
│    ├─ 检查: cancel? → Cancelled          │
│    ├─ 检查: budget? → BudgetExceeded      │
│    ├─ 检查: timeout? → TimedOut           │
│    ├─ 检查: max_iterations? → Failed      │
│    ├─ 检查: 震荡? → Paused + 人工介入     │
│    ├─ 检查: 无进展衰减? → Paused          │
│    │                                      │
│    ├─ [Running]                          │
│    │   执行 Plan (复用 agent.run_next_step)│
│    │   → StepComplete                    │
│    │                                      │
│    ├─ [StepComplete → Evaluating]        │
│    │   写 LoopRun checkpoint              │
│    │   压缩上下文                          │
│    │   调用 LLM 评估 (带一致性检查)        │
│    │                                      │
│    ├─ [Evaluating → 分支]                │
│    │   ├─ Done → Completed               │
│    │   ├─ Continue → 修正 Plan → 新 LoopRun│
│    │   ├─ Decompose → Decomposing        │
│    │   └─ Impossible → Failed            │
│    │                                      │
│    ├─ [Decomposing]                      │
│    │   LLM: goal → 子目标列表 (≤5)        │
│    │   每个子目标 → start_loop(子目标)    │
│    │   等待所有子 Loop 完成               │
│    │   部分失败 → 标记，父 Loop 决定      │
│    │   汇聚结果 → Evaluating             │
│    │                                      │
│    ├─ [WaitingForApproval]               │
│    │   挂起，等待用户输入                 │
│    │                                      │
│    └─ [Paused]                           │
│       挂起，等待 resume_loop()            │
│  }                                        │
└─────────────────────────────────────────┘
```

### 3.3 收敛性保证（三层）

**第一层：评估一致性检查**
- 上轮 unmet 清单中的条目，本轮评估不能凭空消失
- 如果 vanished_unmet 非空但 verdict=Done → 强制改为 Continue
- 置信度 < 0.7 的 Done → 强制改为 Continue

**第二层：震荡检测**
- 最近 3 轮 verdict 序列 [Done,Continue,Done] 或 [Continue,Done,Continue]
- 检测到震荡 → 强制 Paused，要求人工介入

**第三层：硬收敛期限**
- max_iterations 绝对上限 → 标记 Failed
- 最近 3 轮 unmet 数量没有缩小 → 标记 Stalled，暂停

### 3.4 上下文管理

评估 prompt 不送完整步骤日志，只送：
1. goal（始终在开头）
2. 上轮 unmet + new_issues（延续）
3. 本轮 Plan 的 summary（一段话）
4. 执行结果关键输出（压缩：头 200 + 尾 200 字符 + line count）
5. 累计统计：总轮次、总 token 消耗

修正 Plan 生成：只送 `goal + 最近一轮 unmet + new_issues`。

复用 `signal.rs` 的 `compress_output()`。

### 3.5 评估 Prompt 模板

```
[SYSTEM] You are a rigorous technical evaluator. Compare actual
execution results against the stated goal.

[RULES]
1. "met" must cite specific evidence from the actual output
2. "unmet" must reference a specific requirement from the goal
3. "new_issues" are problems not in the goal but clearly wrong
4. Prefer "decompose" over "continue" for complex goals
5. Only verdict="done" if ALL aspects satisfied with evidence
6. confidence=0.95 means you are 95% certain

[INPUT]
Goal: {goal}
Previous unmet: {prev_unmet}
Plan summary: {plan_summary}
Actual results: {actual_output}

[OUTPUT]
{
  "verdict": "done" | "continue" | "decompose" | "impossible",
  "confidence": 0.0-1.0,
  "met": ["requirement → evidence"],
  "unmet": ["requirement → what's missing"],
  "new_issues": ["problem → why it matters"],
  "next_action": "concrete next step"
}
```

### 3.6 评估失败处理

- LLM 调用失败 → 最多重试 2 次（复用 `retry.rs`）
- 2 次后仍失败 → 保守返回 Continue（不误判 Done）
- 评估结果合理性校验：met+unmet 都为空但 verdict=Done → 重试

### 3.7 修正 Plan 生成

不对着整个目标重做 Plan，只对着 unmet 清单生成补丁式 Plan：

```
[SYSTEM] Generate a focused plan that ONLY addresses the missing items.
Original goal: {goal}
ALREADY DONE (do NOT redo): {met}
MISSING (focus here): {unmet}
NEW PROBLEMS: {new_issues}
Suggested approach: {next_action}
```

### 3.8 递归分解细节

**拆解约束** (LLM prompt 要求):
1. 子目标之间尽量独立
2. 子目标数量 ≤ 5
3. 每个子目标可独立验证
4. 子目标汇总后覆盖原目标所有 unmet

**部分失败处理**:
- 子 Loop 部分失败 → 标记，不阻塞其他子 Loop
- 汇聚时包含失败信息
- 父 Loop 决定：调整目标重新拆解 / 接受部分成果

**预算继承**:
- 父 Loop 剩余预算按子目标数量均分，或按 LLM 评估的难度加权分配

### 3.9 守护模式特殊流程

```
daemon loop:
  1. 读取触发条件
  2. LLM 判断当前状态是否满足触发
  3. 满足 → 生成 Plan → 执行一次迭代 → 评估
  4. 等待 poll_interval → 回到 2
  5. 守护 Loop 无"完成"概念，只能手动取消
  6. 评估 Done → 本轮已解决，等待下次触发（不停止 Loop）
```

### 3.10 崩溃恢复流程

```
resume_loop("L1")
  ├─ 查询 loops 表 → Running/BudgetExceeded/TimedOut
  ├─ 查询 loop_runs 表，max(iteration) → 最新 LoopRun
  ├─ 查询关联 Plan → 用 Plan.checkpoint 确定步骤恢复点
  │
  ├─ 情况 A: LoopRun=Running, Plan 未完成 → 从断点继续执行
  ├─ 情况 B: LoopRun=Running, Plan 已完成 → 直接 Evaluating
  └─ 情况 C: Loop=BudgetExceeded/TimedOut
      ├─ 预算恢复 → 继续 → Evaluating
      └─ 预算未恢复 → 返回错误
```

### 3.11 审批织入矩阵

| 审批点 | L1 | L2 | L3 | L4 | L5 |
|--------|----|----|----|----|----|
| 每步执行前 | ✅ | ✅高风险 | ❌ | ❌ | ❌ |
| 每轮结束后 | ✅ | ✅ | ✅ | ❌ | ❌ |
| 拆解子目标前 | ✅ | ✅ | ✅ | ✅ | ❌ |
| 不可恢复错误 | — | — | — | ✅暂停 | ✅暂停 |

### 3.12 已有代码复用清单

| 复用对象 | 源文件 | 在 Loop 中的用途 |
|---------|--------|-----------------|
| `SafetyContext::check_command()` | `safety.rs` | 修正 Plan 的 Exec 步骤安全检查 |
| `PlanCache` | `agent.rs` | 缓存有效 Plan |
| `RetryConfig` / `retry_async()` | `retry.rs` | 评估调用失败重试 |
| `compress_output()` | `signal.rs` | 压缩执行结果后喂给评估 prompt |
| `SkillManager::plan_to_skill()` | `skill.rs` | Loop Done 后自动学习为技能 |
| `CheckpointRepo` upsert 模式 | `db/plans.rs` | LoopRun 持久化 |
| `ToolRegistry::categorize_by_risk()` | `tool_selector.rs` | 审批织入时判断风险等级 |
| `TokenUsage` | `llm/mod.rs` | 预算追踪 |
| `generate_plan()` | `llm/gateway.rs` | 初始 Plan 和修正 Plan 生成 |

---

## 第 4 章：三入口适配

### 4.1 TUI 入口 — Loop Mode

**bridge.rs 新增路由**:
```rust
_ if input.starts_with("/loop") => handle_loop_mode(agent, input),
```

**命令体系**:
```
/loop "目标"                      → 启动 (默认 L3, 10 轮)
/loop -L4 "目标"                  → 指定自治等级
/loop -daemon "监控目标"          → 守护模式
/loop resume <id>                 → 恢复
/loop pause <id>                  → 暂停
/loop cancel <id>                 → 取消
/loop status <id>                 → 查看状态
/loop list                        → 列出所有
/loop approve <id>                → 审批通过
/loop deny <id>                   → 审批拒绝
```

**TUI Panel 设计**:
```
┌─ Loop: 优化项目性能 ──────────── L3 │ 第 3/10 轮 ─┐
│                                                    │
│  ● met (2)                      token: 12,340/50,000 │
│  ├─ 数据库慢查询优化完成                              │
│  └─ 图片懒加载已实现                                 │
│  ○ unmet (1)                                        │
│  ├─ bundle 体积仍超过 500KB                          │
│  ○ new_issues (1)                                   │
│  └─ 图片格式应改为 WebP                              │
│                                                    │
│  [当前步骤] Running plan "减少 bundle"              │
│                                                    │
│  [快捷键] p:暂停 c:取消 a:审批                       │
└────────────────────────────────────────────────────┘
```

### 4.2 Tauri IPC 入口

**新增 6 个 IPC 命令**:
| 命令 | 输入 | 输出 |
|------|------|------|
| `loop_start` | `{ goal, config? }` | `Loop` |
| `loop_resume` | `{ loop_id }` | `Loop` |
| `loop_pause` | `{ loop_id }` | `()` |
| `loop_cancel` | `{ loop_id }` | `()` |
| `loop_status` | `{ loop_id }` | `Loop + LoopRun + 评估` |
| `loop_list` | `{ status_filter? }` | `Vec<Loop>` |

前端适配：新增 `LoopPanel.vue` 或扩展 `AgentPanel.vue`，展示 met/unmet 进度条和评估结果。

### 4.3 CLI 入口

```bash
rupoo loop start "优化项目性能" --autonomy L4 --max-iterations 20
rupoo loop status abc123
rupoo loop resume abc123
rupoo loop list
```

CLI 输出纯文本，每轮结束后打印评估摘要 JSON。

### 4.4 三入口一致性

| 维度 | TUI | Tauri | CLI |
|------|-----|-------|-----|
| 审批交互 | 内嵌面板 | 前端 dialog | stdin y/n |
| 进度展示 | met/unmet 面板 | Pinia → 组件 | printf 评估 JSON |
| 守护模式 | 后台线程 + 状态栏 | 后台 task + 通知 | 阻塞进程 |
| 历史追溯 | Loop 列表面板 | LoopPanel 列表 | `loop list` |

---

## 第 5 章：测试策略与验证方案

### 5.1 Unit Tests (~40 个)

| 测试对象 | 关键用例 |
|---------|---------|
| `LoopStatus` 状态转换合法性 | 9 状态 × 允许/禁止转换矩阵 |
| `EvaluationResult` JSON 解析 | 正常、缺字段、非法值 |
| 一致性检查 `vanished_unmet` | unmet 本轮消失 → 强制 Continue |
| 震荡检测 | [C,C,C]=无, [C,D,C]=震荡, [D,C,D]=震荡 |
| 预算检查 `check_budget` | 超限、边界、正常 |
| 无进展衰减检测 | unmet 数递减/持平/增加 × 3 轮 |
| 拆解输出校验 | 子目标 ≤5、无重复、覆盖 unmet |
| `LoopConfig` 默认值 | max_iterations=10, autonomy=L3 |
| 上下文压缩 | prompt 长度不随轮次线性增长 |
| LoopRun 持久化 CRUD | 含 UNIQUE(loop_id, iteration) 约束 |

### 5.2 Integration Tests (~15 个，使用 Mock LLM)

- 简单目标 1 轮 Done: Pending→Running→Evaluating→Completed
- 3 轮迭代: 3 个 LoopRun，unmet 逐轮减少
- 触发拆解: Decompose → 2 个子 Loop → 汇聚 → Done
- 子 Loop 部分失败: 1/3 Failed，父 Loop 继续
- 预算耗尽: 第 3 轮 → BudgetExceeded → 保存 checkpoint
- 崩溃恢复: Running 状态崩溃 → resume 从断点继续
- 震荡检测触发: [Done,Continue,Done] → Paused
- 审批织入 L1/L4: 每步暂停 / 自动执行
- 守护模式: 触发 → 执行 → 等待 → 再检查
- 评估重试/连续失败 → 保守 Continue

### 5.3 Mock LLM 策略

复用 `llm/traits.rs` 的 `LlmGatewayBackend` trait：

```rust
struct MockLoopLlm {
    eval_responses: Vec<EvaluationResult>,
    plan_responses: Vec<Plan>,
    decompose_responses: Vec<Vec<String>>,
}
// 按调用顺序返回预设值，保证测试可重复
```

### 5.4 E2E Tests（使用真实 LLM）

| 场景 | 验收标准 |
|------|---------|
| "添加一个 hello world 测试" | 写文件 → 运行 → 失败 → 修正 → 通过 |
| "分析 Cargo.toml 依赖健康状况" | 搜索 → 评估 → Done（1 轮） |
| "修复已知编译错误" | 定位 → 修改 → 编译 → 报错 → 再修改 → 通过 |

### 5.5 验证闭环（5 步门）

```
1. Unit Tests        → cargo test --lib loop_engine
2. Integration Tests → cargo test --test loop_integration
3. Code Review       → code-review skill
4. Functional Verify → rupoo loop start "..." 手动验证 3 个场景
5. Scale Verify      → 模拟 100 轮 Loop，验证内存/DB 不膨胀
```

---

## 实现计划

### Phase A: 基础架构 + 自适应循环（目标：2 周）

1. **DB migration** — 新增 `loops` + `loop_runs` 表（`db/mod.rs` + migration）
2. **LoopEngine 骨架** — `loop_engine.rs` 基础 struct + 状态机
3. **Loop Mode 路由** — `bridge.rs` 新增 `/loop` 命令处理
4. **评估流程** — LLM 结构化输出 + 一致性检查 + 震荡检测
5. **修正 Plan 生成** — 基于 unmet 清单生成补丁 Plan
6. **预算守卫** — token/time 追踪 + BudgetExceeded/TimedOut
7. **崩溃恢复** — resume_loop 实现
8. **TUI Panel** — Loop Mode 的 met/unmet 展示
9. **Unit + Integration Tests**

### Phase B: 递归分解（目标：1 周）

1. **拆解 prompt** — decompose_goal + 约束（≤5 子目标）
2. **子 Loop 管理** — 创建、执行、追踪子 Loop
3. **汇聚逻辑** — aggregate_children + 部分失败处理
4. **预算继承** — 父→子预算分配
5. **Tests**

### Phase C: 守护模式 + 审批 + CLI/Tauri（目标：1 周）

1. **守护循环** — daemon trigger 判断 + poll loop
2. **审批织入** — 5 级自治 × 3 个审批点
3. **Tauri IPC** — 6 个 loop_* 命令
4. **CLI** — `rupoo loop *` 子命令
5. **前端 LoopPanel** — Vue 组件
6. **E2E Tests**

---

## 新增文件清单

| 文件 | 说明 |
|------|------|
| `src-agent/src/loop_engine.rs` | LoopEngine 核心实现 |
| `src-agent/src/db/loops.rs` | Loop + LoopRun 持久化 |
| `src-agent/src/cli/loop_mode.rs` | TUI Loop Mode 处理 |
| `src-agent/tests/loop_test.rs` | Unit tests |
| `src-agent/tests/loop_integration_test.rs` | Integration tests |
| `src-ui/src/components/LoopPanel.vue` | 前端 Loop 展示 |

## 修改文件清单

| 文件 | 变更 |
|------|------|
| `src-agent/src/agent.rs` | 添加 `loop_engine` 字段 |
| `src-agent/src/db/mod.rs` | 添加 `LoopRepo` |
| `src-agent/src/cli/bridge.rs` | 新增 `/loop` 路由 |
| `src-agent/src/main_cli.rs` | 新增 `Loops` 子命令 |
| `src-agent/src/shared.rs` | 新增 Loop 相关共享类型 |
| `src-tauri/src/commands.rs` | 新增 6 个 IPC 命令 |
| `Cargo.toml` | 无新依赖（全部复用现有） |
