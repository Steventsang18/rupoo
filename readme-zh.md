# Rupoo — AI-powered Terminal Assistant

Rupoo 是一个运行在终端中的 AI 助手，支持计划执行、技能管理、长期记忆、安全沙箱、Git 集成和 MCP 协议——全部通过自然语言或 TUI 交互。

```
Version:  0.2.0        Language: Rust 2021
Tests:    106 ✅       Binary:   ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Safety:  path_jail 沙箱 + SSRF 防护
```

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

# OpenAI / DeepSeek 等兼容接口
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama 本地模型
# 无需 API Key，Ollama 默认 http://localhost:11434
```

### 启动

```bash
# 交互式 TUI（默认）
rupoo

# TUI 快捷键
# Ctrl+P   命令面板
# Ctrl+C   退出
# Tab      切换焦点（输入区 ↔ 侧栏）
# ↑/↓      输入历史
# Shift+↑/↓  聊天区滚动（或鼠标滚轮）
# PgUp/PgDn  大幅滚动
```

---

## 命令行接口

```
rupoo [OPTIONS] [COMMAND]
```

### 全局选项

| 选项 | 说明 |
|------|------|
| `--verbose` | 在 stderr 输出调试日志 |

### 子命令

| 命令 | 说明 |
|------|------|
| _(无)_ | 进入交互式 TUI（三栏布局） |
| `run --task <id>` | 执行一个已保存的计划 |
| `demo` | 运行内置演示计划 |
| `status [--short]` | 显示系统状态概览 |
| `model [show|list|set]` | 查看/切换 LLM 提供商和模型 |
| `session [list|show|resume|delete|prune]` | 管理执行计划 |
| `skills [list|show|run|install-builtin|learn]` | 技能系统管理 |
| `config [set|get|list]` | 配置管理与 API Keys |
| `git [status|commit|pr]` | Git 集成 |
| `doctor [--fix]` | 诊断环境和配置问题 |
| `logs [--follow] [--lines N] [--level LEVEL]` | 查看运行日志 |
| `mcp-server` | 启动 MCP 协议服务器（JSON-RPC over stdio） |
| `serve --port <port>` | 服务器模式 |

---

## 架构

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  子命令 (status/model/session/doctor/logs...)   │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent 状态机                                              │
│  Think → ToolCall → WaitForInput → Finish               │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                  │
│  Anthropic / OpenAI / Ollama 统一接口                     │
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                     │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls) │
│  + MCP Server (JSON-RPC stdio)                          │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                           │
│  path_jail 沙箱 · 命令黑名单 · SSRF 防护 · 超时保护        │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  计划持久化 · 检查点崩溃恢复 · 会话历史 · 长期记忆    │
└──────────────────────────────────────────────────────────┘
```

### 模块说明

| 模块 | 行数 | 职责 |
|------|------|------|
| `main.rs` | 700+ | CLI 入口，命令分发，`build_engine` |
| `agent.rs` | 840+ | Agent 状态机，7 种步骤类型，崩溃恢复 |
| `db.rs` | 890 | SQLite 层，计划 CRUD + 检查点 + FTS5 记忆 |
| `llm.rs` | 350 | LLM 网关，统一 Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | TUI 事件循环，Agent 桥接线程 |
| `cli/app.rs` | 370 | TUI 应用状态，会话管理，消息路由 |
| `cli/ui.rs` | 420 | TUI 渲染：三栏布局、气泡、代码块、状态栏 |
| `cli/handlers.rs` | 380 | 输入模式策略（Chat/Thinking/Approval/Palette） |
| `safety.rs` | 250 | 安全沙箱、path_jail、SSRF、命令黑名单 |
| `mcp.rs` | 250+ | MCP 工具调度器 + JSON-RPC 客户端 |
| `mcp_server.rs` | 380 | MCP 服务器（复用 McpToolExecutor） |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir 工具 |
| `task.rs` | 340 | Step/Plan/Checkpoint 类型定义 |
| `memory.rs` | 140 | 长期记忆（FTS5 全文搜索） |
| `skill.rs` | 390 | 技能系统（JSON 文件 + 自动学习） |
| `git.rs` | 240 | Git 集成（git2 + gh CLI） |
| `error.rs` | 34 | 统一错误类型 |

### 安全架构

| 防护层 | 实现 |
|--------|------|
| 命令黑名单 | 20+ 危险命令（sudo, rm, mkfs, dd 等） |
| 文件路径沙箱 | `path_jail` crate，防 `../../etc/passwd`、符号链接逃逸 |
| SSRF 防护 | 封锁 localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| 超时保护 | 命令 30s / HTTP 30s / 浏览器 30s |
| 环境变量清洗 | 仅保留 PATH/HOME/USER/SHELL/LANG/TERM |
| 输出截断 | 命令 10K / 文件读取 4K |
| 多路径安全 | McpToolExecutor + LLM Agent + MCP Server 三重防护 |

---

## 核心特性

### 计划执行引擎

支持 7 种步骤类型：

| 步骤 | 说明 |
|------|------|
| Think | LLM 推理，附带 FTS5 记忆检索上下文 |
| ToolCall | 调用内置工具（文件读写、目录列表、Echo） |
| WaitForInput | 等待用户输入后继续 |
| Exec | 执行外部命令（受安全沙箱限制） |
| HttpRequest | HTTP GET/POST 请求（带 SSRF 防护） |
| BrowserAction | 浏览器自动化（Navigate/Screenshot/Click/GetText） |
| Finish | 完成计划，自动触发技能学习 |

### 崩溃恢复

- **心跳检查点**：长时间操作前写入 Running 状态检查点
- **事务原子性**：`record_step_completion` 在单 SQLite 事务中更新计划 + 检查点
- **三层恢复**：`reset_running_plans→get_last_checkpoint→按状态决定恢复点`

### TUI

- **三栏布局**：左侧会话列表、中心聊天区、右侧状态面板
- **消息气泡**：用户/助手/系统三色区分
- **代码块高亮**：代码边框渲染 + 预折行
- **输入历史**：↑/↓ 导航前 100 条输入
- **自动滚动**：新消息自动滚到底部，手动翻看后发消息恢复
- **窗口自适应**：终端大小变化自动重新布局、重新折行

### 技能系统

- **JSON 文件管理**：`~/.skills/*.json`
- **内置技能**：code-review, generate-readme
- **自动学习**：计划执行完成后自动抽取为可复用技能
- **手动学习**：`rupoo skills learn <plan_id> <skill_name>`

### 长期记忆

- **FTS5 全文搜索**：支持 BM25 相关性排序
- **会话持久化**：SQLite 存储 UI 会话历史
- **上下文注入**：Think 步骤自动检索相关记忆

---

## 依赖

| Crate | 用途 |
|-------|------|
| tokio | 异步运行时 |
| clap | CLI 解析 |
| ratatui + crossterm | TUI 框架 |
| rig-core 0.30 | LLM 多提供商网关 |
| rusqlite (WAL + FTS5) | SQLite 数据库 |
| git2 | Git 操作 |
| reqwest | HTTP 客户端 |
| path_jail | 文件路径安全 |
| tui-textarea | TUI 输入组件 |
| serde + serde_json | 序列化 |
| tracing + tracing-subscriber | 日志 |
| uuid | Plan / Step ID |
| chrono | 时间戳 |
| crossbeam-channel | 跨线程通信 |

---

## 测试

```bash
# 全部测试
cargo test

# 仅库测试
cargo test --lib

# 仅集成测试
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# 执行计划
cargo run --release demo
```

106 项测试覆盖：
- 54 单元测试（Agent、DB、LLM、MCP、Safety、Memories、Skills、Git）
- 33 main crate 测试（CLI 命令 + TUI handler）
- 4 CLI-DB 集成测试
- 2 崩溃恢复集成测试
- 13 DB 集成测试

---

## 构建

```bash
# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release

# 带 GUI 支持
cargo build --release --features gui

# 二进制大小
# ~14 MB (release, ARM64)
```

---

## 许可证

MIT
