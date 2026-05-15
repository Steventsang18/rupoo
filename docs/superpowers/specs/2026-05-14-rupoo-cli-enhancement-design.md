# Rupoo CLI 增强设计文档

> 日期: 2026-05-14
> 状态: 设计定稿，待实施
> 影响范围: 新增 5 个子命令，1 个配置项，4 个 DB 查询方法

---

## 概述

为 Rupoo CLI 新增 5 个子命令，参考 Hermes CLI 的设计模式，基于现有代码基础设施实现，零新外部依赖。

**设计原则:**
- 不引入新的外部 crate（依赖），全部使用现有依赖
- 不修改核心引擎（agent.rs, task.rs, llm.rs）的现有行为
- 所有新增代码在 `src/cli/cmds/` 目录下，与核心逻辑分离
- 每个命令一个文件，独立测试

---

## 1. `rupoo status` — 系统状态总览

### 子命令结构

```
rupoo status          # 完整状态
rupoo status --short  # 一行摘要，供脚本 grep：Rupoo v0.2.0 | 12 plans | anthropic/claude-sonnet-4 | 3 skills | OK
```

### 输出内容

```
Rupoo v0.2.0
├── Data       ~/.rupoo/agent.db     (3.2 MB · WAL mode)
├── Plans      12 total  ● 8 completed  ● 2 running  ● 2 failed
├── LLM        ● anthropic / claude-sonnet-4-20250514
├── Skills     3 installed (code-review, generate-readme, plan-executor)
├── Memory     47 entries (FTS5 indexed)
├── Git        ./  (master) · 4 uncommitted files
└── Log        ~/.rupoo/rupoo.log   (2.1 KB, this session)
```

### 数据来源（全部现有）

| 数据项 | 代码来源 |
|--------|----------|
| 版本号 | `cargo_version!()` 或 clap 自动生成 |
| DB 路径 | `tracing_setup::data_dir().join("agent.db")` |
| DB 大小 | `std::fs::metadata()` |
| WAL 模式 | `PRAGMA journal_mode` → db.rs 中已启用 |
| Plan 计数 | `SELECT status, COUNT(*) FROM plans GROUP BY status` |
| LLM Provider | `get_setting("active_provider")` — 见 6.1 |
| LLM Model | `get_setting("model.{provider}")` |
| API Key | 只显示 `sk-ant-xxx` 前缀（redact 实现） |
| 技能数 | `SkillManager::list_skills()` |
| 记忆数 | `SELECT COUNT(*) FROM memories` |
| Git | `GitRepo::open(".")` + `current_branch()` + `status()` |

### 实现文件

- 新增 `src/cli/cmds/status.rs` (~100 行)
- 不修改任何核心模块

---

## 2. `rupoo model` — LLM 模型和提供商管理

### 子命令结构

```
rupoo model                         # 无参数 = rupoo model show
rupoo model set                     # 无 provider 参数 → 进入交互选择器
rupoo model set anthropic           # 只切 provider，保留默认 model
rupoo model set anthropic/claude-sonnet-4  # 切 provider + 指定 model
rupoo model show                    # 显示当前配置
rupoo model list                    # 列出所有可用 provider 及其默认模型
rupoo model set <provider>[/<model>]  # 切换 provider，可选指定模型
```

### 数据流

```
model set anthropic/claude-sonnet-4
  └→ set_setting("active_provider", "anthropic")
  └→ set_setting("model.anthropic", "claude-sonnet-4")
```

### 核心复用

- `LlmProvider` 枚举 (anthropic/openai/ollama) — `src/llm.rs:22-27`
- `LlmConfig::new(provider)` 默认 model 名 — `src/llm.rs:41-56`
- `TaskRepo::set_setting / get_setting` — `src/db.rs:422-463`

### 特殊考虑

- 当前系统没有 `active_provider` 配置项。`model set` 需要新增该配置。
- `model set` 只切换配置，不验证 API key 是否有效。验证在运行时由 `build_engine` 执行。

### 实现文件

- 新增 `src/cli/cmds/model.rs` (~150 行)

---

## 3. `rupoo session` — 计划/会话管理

### 子命令结构

```
rupoo session list                  # 列出计划（按 updated_at 降序）
rupoo session list --limit 20      # 指定数量
rupoo session show <id>            # 显示计划详情和步骤
rupoo session resume <id>          # 恢复执行（复用现有 Commands::Run）
rupoo session delete <id>          # 删除计划及关联 checkpoint
rupoo session prune --days 30      # 清理 30 天前的已完成计划
```

### 需要新增的 DB 方法

```rust
// 在 TaskRepo 中新增（src/db.rs）

/// 列出所有 plan，按 updated_at 降序
pub async fn list_plans(&self, limit: usize, offset: usize)
    -> AgentResult<Vec<PlanSummary>>;

/// 统计各状态的 plan 数量
pub async fn count_plans_by_status(&self)
    -> AgentResult<Vec<(String, usize)>>;

/// 删除 plan 及关联的 checkpoints
pub async fn delete_plan(&self, plan_id: &str) -> AgentResult<()>;

/// 删除指定时间前的已完成 plan
pub async fn prune_plans(&self, before: &str) -> AgentResult<usize>;
```

对应的 SQL 语句均为基本查询，不走 ORM，直接 `rusqlite` 执行。

### `PlanSummary` 结构体

```rust
#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub current_step: usize,
    pub total_steps: usize,
    pub status: PlanStatus,
    pub created_at: String,
    pub updated_at: String,
}
```

`total_steps` 从 `steps_json` 反序列化后取 `len()`，或作为 `JSON_ARRAY_LENGTH(steps_json)` SQL 查询。

### 实现文件

- 新增 `src/cli/cmds/session.rs` (~120 行)
- 修改 `src/db.rs` 新增 4 个方法 (~60 行)

---

## 4. `rupoo doctor` — 环境诊断

### 子命令结构

```
rupoo doctor            # 运行所有检查
rupoo doctor --fix      # 尝试自动修复可修复项
```

### 检查项

| 检查 | 实现 | 可修复 | 判定 |
|------|------|--------|------|
| DB 连通性 | `TaskRepo::new` | 否 | PASS/FAIL |
| DB 表完整性 | `SELECT name FROM sqlite_master` | 否 | PASS/WARN |
| LLM API Key | `get_setting("api_key.*")` | 否，提示运行 `config set` | PASS/WARN |
| Ollama 可达性 | TCP connect `localhost:11434` (30s 超时) | 否，提示启动 Ollama | PASS/WARN |
| 技能目录 | `SkillManager::default_dir().exists()` | 是，创建空目录 | PASS/WARN |
| 技能文件 | 尝试 `load_skill()` 每个 | 否 | PASS/FAIL |
| Git 仓库 | `git2::Repository::open` | 否 | PASS/WARN |
| 数据目录 | `data_dir().exists()` | 是，创建 | PASS/WARN |
| 日志可写 | `OpenOptions::new().append()` | 是，创建 | PASS/WARN |

### 输出格式

```
Rupoo Diagnostics
├── ● Database
│     ~/.rupoo/agent.db — 3.2 MB, WAL mode active
│     All 5 tables present, no corruption detected
├── ● LLM Configuration
│     anthropic: configured (sk-ant-xxx08ab)
│     openai:    ⚠ WARN — no api_key.openai set
│     ollama:    ⚠ WARN — connection refused at localhost:11434
├── ● Skills
│     3 installed at ~/.rupoo/skills/ — all valid JSON
├── ● Git
│     libgit2 available, repository at ./ (branch: master)
├── ● Data Directory
│     ~/.rupoo/ exists and is writable
└── ● Log File
      ~/.rupoo/rupoo.log — exists, writable, 2.1 KB this session

● 4 passed  ● 2 warnings  ● 0 errors
  rupoo doctor --fix  will attempt to resolve warnings
```

### 实现文件

- 新增 `src/cli/cmds/doctor.rs` (~120 行)

---

## 5. `rupoo logs` — 日志查看

### 子命令结构

```
rupoo logs                  # 显示最后 50 行
rupoo logs --follow         # tail -f 模式
rupoo logs --lines 200      # 指定行数
rupoo logs --level WARN     # 通过 grep 过滤级别
rupoo logs --prev           # 查看 ~/.rupoo/rupoo.prev.log
```

### 实现

```rust
fn log_path() -> PathBuf {
    tracing_setup::data_dir().join("rupoo.log")
}
fn prev_log_path() -> PathBuf {
    tracing_setup::data_dir().join("rupoo.prev.log")
}
```

- 默认模式：`BufReader` + 读取最后 N 行（通过 `Seek` 从末尾倒读优化）
- `--follow` 模式：`tokio::fs` 读取 + `tokio::signal::ctrl_c` 停止，采用简单轮询（每秒检查文件大小变化，避免引入 inotify/kqueue 依赖）
- `--level` 过滤：读取后 `lines.filter(|l| l.contains(level))`
- `--prev`：读取 `rupoo.prev.log` 而非 `rupoo.log`

### 输出格式

日志行格式由 `tracing_subscriber::fmt()` 默认格式决定：
```
YYYY-MM-DDTHH:MM:SS.mmmZ  LEVEL  rupoo::target: message key=value
```

例：
```
2026-05-14T10:20:01.123Z  INFO  rupoo::db: database initialized db_path=agent.db
2026-05-14T10:20:01.456Z  INFO  rupoo::agent: loading plan plan_id=a1b2c3d4
2026-05-14T10:20:06.200Z  WARN  rupoo::safety: path jail check passed plan_id=a1b2c3d4
```

### 实现文件

- 新增 `src/cli/cmds/logs.rs` (~80 行)

---

## 6. 共享变更

### 6.1 配置项

在所有 5 个命令之上，需要新增一个配置项：

| Key | 类型 | 默认值 | 用途 |
|-----|------|--------|------|
| `active_provider` | `string` | 无 | 记录当前选中的 LLM 提供商 |

读取路径：
```
model show     → get_setting("active_provider") + get_setting("model.{provider}")
model set      → set_setting("active_provider", provider) + set_setting("model.{provider}", model)
status         → get_setting("active_provider")
doctor         → get_setting("active_provider")
```

### 6.2 文件布局

```
src/
├── main.rs                     # clap Commands 枚举新增 5 个变体
├── cli/
│   ├── cmds/                   # 新增目录
│   │   ├── mod.rs              # pub mod 声明
│   │   ├── status.rs           # rupoo status
│   │   ├── model.rs            # rupoo model
│   │   ├── session.rs          # rupoo session
│   │   ├── doctor.rs           # rupoo doctor
│   │   └── logs.rs             # rupoo logs
├── db.rs                       # 新增 4 个 TaskRepo 方法
```

### 6.3 测试

| 命令 | 测试数 | 说明 |
|------|--------|------|
| model | 10 | 显示、列表示、设置、provider 切换、边界情况 |
| session | 12 | list、show、delete、prune、空表、无效 ID |
| doctor | 8 | 各检查项独立测试、--fix、全 PASS/混合状态 |
| logs | 4 | 读取、tail、级别过滤、prev 文件 |
| status | 6 | 完整输出、short 模式、空状态、各字段单独测试 |

---

## 7. 实施顺序

建议按依赖关系实施：

```
Week 1:
  Step 1: db.rs 新增 4 个方法 + PlanSummary 结构体   (已含 session 基础)
  Step 2: cli/cmds/mod.rs 目录结构                     (~20 行)
  Step 3: rupoo status                                 (~100 行)

Week 2:
  Step 4: rupoo model (+ active_provider 配置项)       (~150 行)
  Step 5: rupoo session                                 (~180 行)

Week 3:
  Step 6: rupoo doctor                                  (~120 行)
  Step 7: rupoo logs                                    (~80 行)
```

每个 Step 独立可测试、可合并。

---

## 8. 不做的事

以下来自 Hermes 的特性明确不做（或推迟到 v0.3+）：

| 特性 | 原因 |
|------|------|
| Gateway/Messaging | 定位不同，Rupoo 是本地执行引擎 |
| 中央命令注册表 COMMAND_REGISTRY | 当前 8→13 命令规模不需要抽象层 |
| 插件系统 | Rust 插件机制代价大，MCP 已有扩展性 |
| 自动更新 | Rust 二进制更新需要额外基础设施 |
| 会话级别配置 | 当前配置是全局的，不涉及 session override |
| `setup` 向导 | 暂缓，当前 `config set` 可完成 |
| TUI 斜杠命令增强 | 现有 `/skills /config /git` 已够用，暂不扩展 |
