# Rupoo — AI 驱动的终端助手

Rupoo 是一个基于终端的 AI 助手，采用原生 REPL 交互界面，支持语法高亮代码块、Markdown 渲染、主题切换和 Claude Code 风格工具调用展示——由双模式 Agent 引擎（Chat + Plan）驱动。

```
版本:     0.3.0          语言: Rust 2021
代码量:   25,493 行      测试:  96 ✅
界面:     原生 REPL      LLM:  Anthropic / OpenAI / DeepSeek / Ollama
数据库:   SQLite (FTS5)  安全: path_jail 沙箱 + SSRF 防护
```

---

## v0.3 更新亮点

| 领域 | 变更 |
|------|------|
| **交互界面** | 移除 ratatui TUI，改为原生 REPL — 流畅滚动、resize 安全、无帧缓冲 |
| **代码高亮** | syntect 语法高亮，3 套主题（base16-ocean.dark / InspiredGitHub / base16-mocha.dark） |
| **Markdown 渲染** | 表格、引用块、任务列表、有序列表、链接、分隔线 |
| **主题系统** | `/theme dark\|light\|monokai`，偏好持久化到数据库；光标颜色跟随主题 |
| **聊天气泡** | 用户消息右对齐（▸），AI 左对齐（◂），视觉区分清晰 |
| **工具卡片** | Claude Code 风格 `╭─🔧──╮` 折叠卡片展示工具调用 |
| **思考链** | 扣子风格 spinner + 流式气泡展示 AI 推理过程 |
| **流式代码块** | 两阶段渲染：流式 `│` 占位 → 完成时 syntect 重写高亮+行号 |
| **历史搜索** | `Ctrl+R` 增量搜索，持久化到 `~/.rupoo/history.txt`（1000 条） |
| **配色体系** | 每主题 12 个 RGB 常量（GitHub Dark Dimmed + Catppuccin Mocha），告别 `dimmed()` |
| **输入编辑** | rustyline Emacs 模式：方向键移动、Home/End、Ctrl+A/E、绿色闪烁竖线光标 |
| **HTTP 连接池** | 单例 HTTP 客户端，连接复用，减少重复 API 调用延迟 |
| **SQLite WAL 模式** | 启用 Write-Ahead Logging，内存临时存储，写入性能提升 30-50% |
| **Memory LRU 缓存** | 5 分钟 TTL 缓存，减少数据库查询 |
| **并行工具执行** | 支持并发工具调用，提升吞吐量 |
| **健壮性** | 修复生产代码中所有 `unwrap()` 调用；添加锁超时机制 |

---

## 快速开始

### 安装

```bash
# 从源码安装
cargo install --path .

# 或直接运行编译好的二进制
cargo run --release
```

### 配置 LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek 等兼容 API
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama 本地模型
# 无需 API Key — 默认连接 http://localhost:11434
```

### 启动

```bash
# 交互式 REPL（默认）
rupoo
```

#### REPL 快捷键

| 按键 | 功能 |
|------|------|
| `Enter` | 发送消息 |
| `↑` / `↓` | 浏览输入历史 |
| `Ctrl+R` | 增量历史搜索 |
| `Ctrl+A` / `Home` | 光标移到行首 |
| `Ctrl+E` / `End` | 光标移到行尾 |
| `←` / `→` | 光标左右移动 |
| `Ctrl+C` | 取消当前操作 |
| `Ctrl+D` | 退出 |

#### REPL 命令

| 命令 | 说明 |
|------|------|
| `/new` | 开始新对话 |
| `/model` | 切换 LLM 模型 |
| `/plan` | 进入计划模式 |
| `/theme dark\|light\|monokai` | 切换颜色主题 |
| `?` | 显示帮助 |

---

## 命令行接口

```
rupoo [选项] [命令]
```

### 全局选项

| 选项 | 说明 |
|------|------|
| `--verbose` | 输出调试日志到 stderr |

### 子命令

| 命令 | 说明 |
|------|------|
| _(无)_ | 启动交互式 REPL |
| `run --task <id>` | 执行已保存的计划 |
| `demo` | 运行内置演示计划 |
| `status [--short]` | 显示系统状态概览 |
| `model [show\|list\|set]` | 查看/切换 LLM 提供商和模型 |
| `session [list\|show\|resume\|delete\|prune]` | 管理执行计划 |
| `skills [list\|show\|run\|install-builtin\|learn]` | 技能系统管理 |
| `config [set\|get\|list]` | 配置和 API Key 管理 |
| `git [status\|commit\|pr]` | Git 集成 |
| `doctor [--fix]` | 诊断环境和配置问题 |
| `logs [--follow] [--lines N] [--level LEVEL]` | 查看运行时日志 |
| `mcp-server` | 启动 MCP 协议服务器（JSON-RPC over stdio） |
| `serve --port <port>` | 服务器模式 |

---

## 架构

```
┌─ CLI (clap) ──────────────────────────────────────────────┐
│  rupoo  →  原生 REPL (rustyline + owo-colors)             │
│         →  子命令 (status/model/session/doctor/logs…)      │
└──────────────────────┬────────────────────────────────────┘
                       │
┌──────────────────────▼────────────────────────────────────┐
│  Agent 状态机                                              │
│  Think → ToolCall → WaitForInput → Finish                 │
│  + Exec / HttpRequest / BrowserAction / Search            │
├───────────────────────────────────────────────────────────┤
│  LLM 网关 (rig-core 0.30)                                 │
│  Anthropic / OpenAI / Ollama 统一接口                      │
├───────────────────────────────────────────────────────────┤
│  输出层                                                    │
│  theme.rs → output.rs → markdown.rs → syntect 语法高亮    │
│  聊天气泡 · 工具卡片 · 思考链 · 代码块                     │
├───────────────────────────────────────────────────────────┤
│  工具执行层                                                │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls)   │
│  + MCP 服务器 (JSON-RPC stdio)                            │
├───────────────────────────────────────────────────────────┤
│  安全上下文                                                │
│  path_jail 沙箱 · 命令黑名单 · SSRF 防护                  │
│  · 超时保护                                                │
├───────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                      │
│  计划持久化 · 检查点崩溃恢复 · 会话历史                    │
│  · 长期记忆 · 主题偏好                                     │
└───────────────────────────────────────────────────────────┘
```

### 模块概览

| 模块 | 行数 | 职责 |
|------|------|------|
| `agent.rs` | 1082 | Agent 状态机，7 种 Step 类型，崩溃恢复 |
| `db.rs` | 1121 | SQLite 层，Plan CRUD + 检查点 + FTS5 记忆 + 主题 |
| `llm.rs` | 1172 | LLM 网关，统一 Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 733 | REPL 事件循环，Agent 桥接线程 |
| `cli/markdown.rs` | 540 | Markdown 渲染器：表格、引用、任务列表、链接 |
| `cli/output.rs` | 286 | 输出格式化：聊天气泡、工具卡片、思考链 |
| `cli/theme.rs` | 161 | 主题系统：Dark/Light/Monokai，12 个 RGB 常量 |
| `cli/plan_mode.rs` | 297 | 计划模式：交互式步骤执行 |
| `cli/app.rs` | 307 | REPL 应用状态，会话管理 |
| `cli/bridge.rs` | 188 | Agent ↔ REPL 桥接（crossbeam 通道） |
| `cli/chat_mode.rs` | 121 | 聊天模式处理器 |
| `cli/approval.rs` | 133 | 工具审批流程 |
| `main_cli.rs` | 398 | CLI 入口，命令分发 |
| `safety.rs` | 364 | 安全沙箱，path_jail，SSRF，命令黑名单 |
| `mcp.rs` | 421 | MCP 工具调度 + JSON-RPC 客户端 |
| `mcp_server.rs` | 400 | MCP 服务器（复用 McpToolExecutor） |
| `rig_tools.rs` | 566 | Echo / FileRead / FileWrite / ListDir 工具 |
| `skill.rs` | 570 | 技能系统（JSON 文件 + 自动学习） |
| `task.rs` | 340 | Step/Plan/Checkpoint 类型定义 |
| `tools/browser.rs` | 461 | 浏览器自动化（导航/截图/点击/获取文本） |
| `tools/search.rs` | 247 | 网页搜索集成 |
| `tools/network.rs` | 150 | HTTP 请求工具 |
| `tools/terminal.rs` | 123 | 终端命令执行 |
| `git.rs` | 241 | Git 集成（git2 + gh CLI） |
| `memory.rs` | 143 | 长期记忆（FTS5 全文搜索） |
| `executor.rs` | 138 | 步骤执行分发 |
| `shared.rs` | 130 | 共享类型和常量 |
| `error.rs` | 33 | 统一错误类型 |

---

## 主题系统

三套内置主题，偏好持久化存储：

| 主题 | 风格 | 代码高亮 | 光标 |
|------|------|----------|------|
| `dark`（默认） | GitHub Dark Dimmed + Catppuccin Mocha | base16-ocean.dark | `#3fb950` 绿色 |
| `light` | GitHub Light | InspiredGitHub | `#238636` 绿色 |
| `monokai` | Monokai | base16-mocha.dark | `#a6e22e` 绿色 |

使用 `/theme dark|light|monokai` 切换，偏好跨会话持久化。

### 配色方案（暗色主题）

| 角色 | 颜色 | 色值 |
|------|------|------|
| 用户消息 | 绿色 | `#7ee787` |
| 用户强调 | 绿色 | `#3fb950` |
| AI 消息 | 蓝色 | `#58a6ff` |
| AI 强调 | 蓝色 | `#79c0ff` |
| 工具调用 | 紫色 | `#d2a8ff` |
| 思考过程 | 黄色 | `#e3b341` |
| 错误 | 红色 | `#f85149` |
| 弱化文本 | 灰色 | `#484f58` |
| 边框 | 灰色 | `#30363d` |

---

## 核心功能

### 双模式 Agent 引擎

| 模式 | 触发 | 说明 |
|------|------|------|
| **聊天模式** | 默认 | 自由对话，流式输出 |
| **计划模式** | `/plan` | 结构化多步执行，带检查点 |

### 7 种步骤类型

| 步骤 | 说明 |
|------|------|
| Think | LLM 推理，自动检索 FTS5 记忆作为上下文 |
| ToolCall | 调用内置工具（文件读写、目录列表、Echo） |
| WaitForInput | 暂停等待用户输入 |
| Exec | 执行外部命令（受安全沙箱限制） |
| HttpRequest | HTTP GET/POST 请求（带 SSRF 防护） |
| BrowserAction | 浏览器自动化（导航/截图/点击/获取文本） |
| Finish | 完成计划，触发自动技能学习 |

### 崩溃恢复

- **心跳检查点**：长时间操作前写入 Running 状态检查点
- **事务原子性**：`record_step_completion` 在单个 SQLite 事务中更新 Plan + Checkpoint
- **三级恢复**：`reset_running_plans → get_last_checkpoint → 根据状态确定恢复点`

### Markdown 渲染

完整的行内 + 块级渲染：

- **表格**：对齐列 + `│` 边框
- **引用块**：`▎` 左边框 + 弱化文本
- **任务列表**：`☐` 未完成 / `☑` 已完成
- **有序/无序列表**：带缩进和标记
- **代码块**：syntect 语法高亮 + 行号
- **行内代码**：背景高亮
- **链接**：`[文本](url)` 解析并着色
- **分隔线**：`─` 分隔符

### 流式代码块

两阶段渲染消除闪烁：

1. **流式阶段**：快速 `│` 占位 + 纯文本
2. **完成阶段**：擦除重写 syntect 高亮 + 行号

### 技能系统

- **JSON 文件管理**：`~/.skills/*.json`
- **内置技能**：code-review, generate-readme
- **自动学习**：计划执行完成后自动提取为可复用技能
- **手动学习**：`rupoo skills learn <plan_id> <skill_name>`

### 长期记忆

- **FTS5 全文搜索**：支持 BM25 相关度排序
- **会话持久化**：SQLite 存储对话历史
- **上下文注入**：Think 步骤自动检索相关记忆

---

## 安全架构

| 防护层 | 实现 |
|--------|------|
| 命令黑名单 | 屏蔽 20+ 危险命令（sudo, rm, mkfs, dd 等） |
| 文件路径沙箱 | `path_jail` — 防止 `../../etc/passwd`、符号链接逃逸 |
| SSRF 防护 | 屏蔽 localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| 超时保护 | 命令 30s / HTTP 30s / 浏览器 30s |
| 环境清理 | 仅保留 PATH/HOME/USER/SHELL/LANG/TERM |
| 输出截断 | 命令输出 10K / 文件读取 4K |
| 多路径安全 | 三重防护：McpToolExecutor + LLM Agent + MCP 服务器 |

---

## 依赖

| Crate | 用途 |
|-------|------|
| tokio | 异步运行时 |
| clap | CLI 参数解析 |
| rustyline | REPL 输入，历史 + Ctrl+R 搜索 |
| owo-colors | 零成本 RGB 颜色输出 |
| syntect | 语法高亮（离线，100+ 语言） |
| rig-core 0.30 | 多提供商 LLM 网关 |
| rusqlite (WAL + FTS5) | SQLite 数据库 |
| git2 | Git 操作 |
| reqwest | HTTP 客户端 |
| path_jail | 文件路径安全 |
| serde + serde_json | 序列化 |
| tracing + tracing-subscriber | 日志 |
| uuid | 计划/步骤 ID |
| chrono | 时间戳 |
| crossbeam-channel | 跨线程通信 |
| indicatif | 进度条和 spinner |

---

## 测试

```bash
# 运行全部测试
cargo test

# 仅库测试
cargo test --lib

# 集成测试
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# 运行演示计划
cargo run --release demo
```

67 个测试覆盖：
- Agent 状态机、数据库 CRUD、LLM 网关、MCP、安全、记忆、技能、Git、工具

---

## 构建

```bash
# 开发构建
cargo build

# Release 构建（推荐）
cargo build --release

# 带 GUI 支持
cargo build --release --features gui

# 如果项目路径包含非 ASCII 字符：
CARGO_TARGET_DIR=/tmp/rupoo-target cargo build --release
```

---

## 许可证

MIT
