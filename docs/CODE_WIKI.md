# Rupoo Code Wiki

**版本**: 0.4.0  
**最后更新**: 2026-06-08

---

## 目录

1. [项目概述](#项目概述)
2. [系统架构](#系统架构)
3. [核心模块](#核心模块)
4. [关键数据结构](#关键数据结构)
5. [主要功能流程](#主要功能流程)
6. [运行方式](#运行方式)
7. [开发指南](#开发指南)

---

## 项目概述

### 项目简介

Rupoo 是一个基于 Rust 语言开发的 AI 驱动终端助手，具有以下特点：

- **原生 REPL 界面**: 流畅的交互式体验
- **双模式 Agent**: Chat Mode（聊天模式）+ Plan Mode（计划模式）
- **长时记忆系统**: 结合 FTS5 全文搜索和向量语义搜索的混合搜索
- **工具执行**: 支持命令执行、HTTP 请求、浏览器控制等工具
- **安全机制**: 沙箱隔离、命令白名单、超时保护

### 技术栈

| 类别 | 技术 |
|------|------|
| 语言 | Rust 2021 Edition |
| 异步运行时 | Tokio |
| 数据库 | SQLite (FTS5 全文检索) |
| LLM 集成 | Anthropic Claude / OpenAI / DeepSeek / Ollama |
| Web UI | Svelte |

### 项目结构

```
rupoo/
├── src/
│   ├── cli/              # CLI/REPL 界面
│   │   ├── cmds/         # 命令实现
│   │   ├── app.rs        # 主应用
│   │   └── ...
│   ├── db/               # 数据库层
│   │   ├── mod.rs        # TaskRepo 核心
│   │   ├── plans.rs      # 计划/检查点 CRUD
│   │   └── settings.rs   # 设置/记忆 CRUD
│   ├── llm/              # LLM 网关
│   │   ├── gateway.rs    # 核心网关
│   │   ├── history.rs    # 对话历史
│   │   ├── providers.rs  # 提供商集成
│   │   └── traits.rs     # 特征定义
│   ├── tools/            # 工具实现
│   │   ├── terminal.rs   # 命令执行
│   │   ├── network.rs    # HTTP 请求
│   │   ├── browser.rs    # 浏览器控制
│   │   └── ...
│   ├── agent.rs          # Agent 核心逻辑
│   ├── task.rs           # 任务/步骤数据结构
│   ├── memory.rs         # 记忆系统
│   ├── mcp.rs            # MCP 协议
│   ├── safety.rs         # 安全机制
│   ├── build_engine.rs   # 引擎构建
│   └── main.rs           # 入口
├── web-ui/               # Web 界面
├── docs/                 # 文档
└── tests/                # 测试
```

---

## 系统架构

### 整体架构图

```
┌─────────────────────────────────────────────────────────────┐
│                      用户层 (UI/CLI)                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
│  │   CLI    │  │  REPL    │  │ Web UI   │  │  Tray    │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  │
└───────┼─────────────┼─────────────┼─────────────┼─────────┘
        │             │             │             │
┌───────▼─────────────▼─────────────▼─────────────▼─────────┐
│                      Agent 核心层                          │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                Agent 结构体                          │  │
│  │  ┌───────────────────────────────────────────────┐  │  │
│  │  │  - TaskRepo (数据访问)                         │  │  │
│  │  │  - MemoryStore (长时记忆)                       │  │  │
│  │  │  - LlmGateway (LLM 网关)                       │  │  │
│  │  │  - ToolExecutor (工具执行)                     │  │  │
│  │  │  - SafetyContext (安全上下文)                  │  │  │
│  │  └───────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
┌───────▼───────┐   ┌───────▼───────┐   ┌───────▼───────┐
│  数据库层     │   │   LLM 层      │   │   工具层      │
│  (SQLite)     │   │  (多提供商)   │   │  (MCP 工具)   │
└───────────────┘   └───────────────┘   └───────────────┘
```

### 数据流

1. **用户输入** → CLI/REPL 解析
2. **Agent 处理** → 根据模式选择 Chat/Plan
3. **记忆检索** → 混合搜索（FTS5 + Vector）
4. **LLM 调用** → 通过网关调用提供商
5. **工具执行** → 安全执行工具调用
6. **结果反馈** → 存储记忆并返回用户

---

## 核心模块

### 1. Agent 模块 ([`src/agent.rs`](file:///Users/pengxiangzeng/rust-project/src/agent.rs))

**职责**: 核心决策和执行引擎

#### 主要结构体

```rust
pub struct Agent {
    repo: Arc<TaskRepo>,
    memory_cache: Arc<MemoryCache>,
    memory_store: Arc<MemoryStore>,
    embedding_service: Option<Arc<EmbeddingService>>,
    memory_enabled: AtomicBool,
    hybrid_search_enabled: AtomicBool,
    tool_executor: Box<dyn ToolExecutor>,
    llm_gateway: Option<LlmGateway>,
    safety_ctx: SafetyContext,
    plan_cache: Arc<PlanCache>,
    // ...
}
```

#### 核心方法

| 方法 | 描述 |
|------|------|
| `new()` | 创建新 Agent 实例 |
| `with_llm()` | 附加 LLM 网关 |
| `agent_chat()` | 执行聊天模式对话 |
| `resume()` | 恢复计划执行 |
| `run_next_step()` | 执行计划的下一步 |
| `remember()` | 存储记忆 |
| `recall()` | 检索记忆 |

#### 步骤执行器

Agent 支持 7 种步骤类型：

| 步骤类型 | 描述 |
|----------|------|
| `Think` | LLM 推理思考 |
| `ToolCall` | 调用 MCP 工具 |
| `Exec` | 执行外部命令 |
| `HttpRequest` | 发送 HTTP 请求 |
| `BrowserAction` | 浏览器操作 |
| `WaitForInput` | 等待用户输入 |
| `Finish` | 计划完成 |

### 2. 数据库模块 ([`src/db/mod.rs`](file:///Users/pengxiangzeng/rust-project/src/db/mod.rs))

**职责**: 数据持久化管理

#### TaskRepo 核心

```rust
pub struct TaskRepo {
    conn: Arc<Mutex<rusqlite::Connection>>,
    db_path: String,
}
```

#### 数据库表

| 表名 | 用途 |
|------|------|
| `plans` | 存储执行计划 |
| `checkpoints` | 检查点（用于崩溃恢复） |
| `settings` | 键值配置存储（API 密钥等） |
| `ui_sessions` | UI 会话历史 |
| `conversation_histories` | 对话历史 |
| `memories` | FTS5 记忆表 |

#### 关键优化

- **WAL 模式**: 提高并发读取性能
- **读写分离**: 写操作使用互斥锁保护，读操作使用独立连接
- **权限控制**: 数据库文件权限设为 0o600

### 3. LLM 网关模块 ([`src/llm/mod.rs`](file:///Users/pengxiangzeng/rust-project/src/llm/mod.rs))

**职责**: 统一多提供商 LLM 接口

#### 支持的提供商

| 提供商 | 默认模型 |
|--------|----------|
| Anthropic | claude-sonnet-4-20250514 |
| OpenAI | gpt-4o |
| DeepSeek | deepseek-chat |
| Ollama | llama3.2 |

#### 核心配置

```rust
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f64,
    pub embedding_model: Option<String>,
}
```

### 4. 记忆系统 ([`src/memory.rs`](file:///Users/pengxiangzeng/rust-project/src/memory.rs))

**职责**: 长时记忆存储和检索

#### 混合搜索架构

```
用户查询
    │
    ├───> FTS5 搜索（关键词匹配）
    │         │  精确匹配、关键词命中
    │
    └───> 向量搜索（语义理解）
              │  理解意图和含义

组合结果（RRF 排序）
```

#### HybridSearchConfig

```rust
pub struct HybridSearchConfig {
    pub enable_vector_search: bool,
    pub fts_weight: f32,        // FTS 结果权重
    pub vector_weight: f32,     // 向量结果权重
    pub min_similarity: f32,    // 最小相似度阈值
    pub use_rrf: bool,          // 使用 RRF 排序
    pub rrf_k: u32,             // RRF 常数
}
```

#### RRF（Reciprocal Rank Fusion）公式

```
RRF_score(d) = Σ (1 / (k + rank(d)))
```

### 5. 安全模块 ([`src/safety.rs`](file:///Users/pengxiangzeng/rust-project/src/safety.rs))

**职责**: 确保工具执行安全

#### 安全机制

| 机制 | 描述 |
|------|------|
| 命令黑名单 | 阻止危险命令（rm, dd 等） |
| 路径沙箱 | path_jail 防止路径遍历 |
| SSRF 保护 | 阻止本地和内部网络访问 |
| 超时保护 | 命令/HTTP/浏览器超时限制 |
| 环境清理 | 仅保留安全环境变量 |
| 输出截断 | 限制输出大小 |

---

## 关键数据结构

### 1. Plan 计划 ([`src/task.rs`](file:///Users/pengxiangzeng/rust-project/src/task.rs#L162-L182))

```rust
pub struct Plan {
    pub id: String,
    pub name: String,
    pub steps: Vec<Step>,
    pub current_step_index: usize,
    pub status: PlanStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 2. Step 步骤 ([`src/task.rs`](file:///Users/pengxiangzeng/rust-project/src/task.rs#L48-L107))

```rust
pub enum Step {
    Think {
        id: String,
        instruction: String,
        status: StepStatus,
        output: Option<String>,
    },
    ToolCall {
        id: String,
        tool_name: String,
        params: serde_json::Value,
        status: StepStatus,
        result: Option<serde_json::Value>,
    },
    WaitForInput {
        id: String,
        prompt: String,
        status: StepStatus,
        response: Option<String>,
    },
    // ... 其他步骤类型
}
```

### 3. MemoryEntry 记忆项 ([`src/task.rs`](file:///Users/pengxiangzeng/rust-project/src/task.rs#L354-L362))

```rust
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}
```

### 4. StepOutcome 步骤结果 ([`src/agent.rs`](file:///Users/pengxiangzeng/rust-project/src/agent.rs#L23-L39))

```rust
pub enum StepOutcome {
    Advanced,           // 成功执行，继续下一步
    Finished,           // 计划完成
    WaitingForInput(String),  // 等待用户输入
    RequiresApproval {  // 需要用户批准
        tool_name: String,
        params: serde_json::Value,
        step_index: usize,
    },
    Failed(String),     // 执行失败
}
```

---

## 主要功能流程

### 1. 启动流程 ([`src/main.rs`](file:///Users/pengxiangzeng/rust-project/src/main.rs))

```
main()
  │
  ├─> 解析 CLI 参数
  │
  ├─> build_engine() 构建引擎
  │     ├─> 初始化 TaskRepo
  │     ├─> 加载安全配置
  │     ├─> 创建 McpToolExecutor
  │     ├─> 初始化 Agent
  │     └─> 配置 LLM 网关
  │
  ├─> 若无子命令 → 启动 REPL
  │
  └─> 否则 → 执行对应子命令
```

### 2. Chat Mode 流程

```
用户输入
   │
   ├─> 检索相关记忆
   │
   ├─> LLM 网关对话
   │     ├─> 构建系统提示词
   │     ├─> 注入记忆上下文
   │     ├─> 调用 LLM
   │     └─> 流式回调（AgentEvent）
   │
   ├─> 存储对话记忆
   │
   └─> 返回响应
```

### 3. Plan Mode 流程

```
用户任务描述
   │
   ├─> LLM 生成执行计划
   │
   ├─> 存储计划到数据库
   │
   ├─> 循环执行步骤:
   │     │
   │     ├─> 获取当前步骤
   │     ├─> heartbeat() 写入检查点
   │     ├─> 执行步骤
   │     ├─> 记录完成检查点
   │     ├─> 更新计划状态
   │     └─> 前进到下一步
   │
   ├─> 自动学习技能（可选）
   │
   └─> 完成
```

### 4. 崩溃恢复流程 ([`src/agent.rs`](file:///Users/pengxiangzeng/rust-project/src/agent.rs#L654-L717))

```
resume(plan_id)
   │
   ├─> 重置运行状态的计划
   │
   ├─> 加载计划
   │
   ├─> 获取最后检查点
   │     │
   │     ├─> Completed → 从下一步继续
   │     ├─> Running → 重试当前步骤
   │     └─> Failed → 重试失败步骤
   │
   └─> 返回可执行的计划
```

---

## 运行方式

### 从源码构建

```bash
# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release

# 带 GUI 支持
cargo build --release --features gui
```

### 运行

```bash
# 交互式 REPL（默认）
cargo run

# 或使用已编译的二进制
./target/release/rupoo
```

### 配置 LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama（无需 API 密钥）
rupoo config set active_provider ollama
rupoo config set model.ollama llama3
```

### 主要 CLI 命令

| 命令 | 描述 |
|------|------|
| `rupoo` | 启动交互式 REPL |
| `rupoo status` | 系统状态概览 |
| `rupoo model [show|list|set]` | 管理 LLM 提供商 |
| `rupoo session [list|show|resume|delete]` | 管理计划会话 |
| `rupoo config [set|get|list]` | 配置管理 |
| `rupoo doctor [--fix]` | 诊断问题 |
| `rupoo logs [--follow]` | 查看运行日志 |

### 测试

```bash
# 运行所有测试
cargo test

# 详细输出
cargo test -- --nocapture

# 运行特定测试
cargo test test_name

# 基准测试
cargo bench
```

---

## 开发指南

### 代码约定

- **模块组织**: 按功能领域划分模块
- **错误处理**: 使用 `anyhow::Result` 和自定义 `AgentError`
- **异步**: 使用 `tokio` 异步运行时
- **日志**: 使用 `tracing` crate 记录日志

### 添加新工具

1. 在 [`src/tools/`](file:///Users/pengxiangzeng/rust-project/src/tools/) 中实现工具
2. 注册到 MCP 工具注册表
3. 更新安全上下文（如需要）

### 添加新 LLM 提供商

1. 在 [`src/llm/providers.rs`](file:///Users/pengxiangzeng/rust-project/src/llm/providers.rs) 中添加提供商枚举
2. 实现 `LlmGatewayBackend` 特征
3. 更新配置和构建逻辑

### 调试技巧

```bash
# 启用详细日志
RUST_LOG=debug cargo run

# 或使用 --verbose
cargo run --verbose
```

### 性能优化参考

见 [`docs/PERFORMANCE.md`](file:///Users/pengxiangzeng/rust-project/docs/PERFORMANCE.md)

---

## 附录

### 相关文档

- [`README.md`](file:///Users/pengxiangzeng/rust-project/README.md) - 项目说明
- [`docs/USER_GUIDE.md`](file:///Users/pengxiangzeng/rust-project/docs/USER_GUIDE.md) - 用户指南
- [`docs/PERFORMANCE.md`](file:///Users/pengxiangzeng/rust-project/docs/PERFORMANCE.md) - 性能优化
- [`CONTRIBUTING.md`](file:///Users/pengxiangzeng/rust-project/CONTRIBUTING.md) - 贡献指南

### 关键依赖版本

| Crate | 版本 | 用途 |
|-------|------|------|
| tokio | 1.x | 异步运行时 |
| rusqlite | 0.31 | SQLite 绑定 |
| serde | 1.x | 序列化 |
| clap | 4.x | CLI 解析 |
| tracing | 0.1 | 日志 |

---

*本 Code Wiki 会随项目演进持续更新。*
