# Rupoo — AI 驱动的终端助手

Rupoo 是一个基于终端的 AI 助手，采用原生 REPL 交互界面，支持语法高亮代码块、Markdown 渲染、主题切换和 Claude Code 风格工具调用展示——由双模式 Agent 引擎（Chat + Plan）驱动。

```
版本:     0.3.0          语言: Rust 2021
代码量:   25,493 行      测试:  96 ✅
界面:     原生 REPL      LLM:  Anthropic / OpenAI / DeepSeek / Ollama
数据库:   SQLite (FTS5)  安全: path_jail 沙箱 + SSRF 防护
```

---

## 功能特性

| 功能 | 描述 |
|------|------|
| **原生 REPL** | 流畅滚动、resize 安全、无帧缓冲 |
| **语法高亮** | syntect 驱动，支持 3 套主题（ocean / GitHub / mocha） |
| **Markdown 渲染** | 表格、引用块、任务列表、代码块、链接 |
| **主题系统** | `/theme dark\|light\|monokai`，偏好持久化存储 |
| **聊天气泡** | 用户消息右对齐（▸），AI 左对齐（◂） |
| **工具卡片** | Claude Code 风格折叠卡片展示工具调用 |
| **思考链** | 流式 spinner + 气泡展示 AI 推理过程 |
| **历史搜索** | `Ctrl+R` 增量搜索（1000 条记录） |
| **双模式 Agent** | 聊天模式 + 计划模式（带检查点） |
| **7 种步骤类型** | Think、ToolCall、WaitForInput、Exec、HttpRequest、BrowserAction、Finish |
| **长期记忆** | FTS5 全文搜索，自动上下文注入 |
| **技能系统** | JSON 格式技能，支持自动学习 |
| **崩溃恢复** | 心跳检查点 + 事务原子性 |

---

## 快速开始

### 安装

```bash
# 从源码安装
cargo install --path .

# 或直接运行
cargo run --release
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

### 启动

```bash
# 交互式 REPL（默认）
rupoo
```

#### 键盘快捷键

| 按键 | 功能 |
|------|------|
| `Enter` | 发送消息 |
| `↑` / `↓` | 浏览历史 |
| `Ctrl+R` | 增量搜索 |
| `Ctrl+C` | 取消操作 |
| `Ctrl+D` | 退出 |

#### REPL 命令

| 命令 | 描述 |
|------|------|
| `/new` | 新建对话 |
| `/model` | 切换 LLM 模型 |
| `/plan` | 进入计划模式 |
| `/theme dark\|light\|monokai` | 切换主题 |
| `?` | 显示帮助 |

---

## CLI 命令

```
rupoo [OPTIONS] [COMMAND]
```

| 命令 | 描述 |
|------|------|
| _(none)_ | 启动交互式 REPL |
| `demo` | 运行内置演示 |
| `status` | 系统状态概览 |
| `model [show\|list\|set]` | 管理 LLM 提供商 |
| `session [list\|show\|resume\|delete]` | 管理计划 |
| `skills [list\|show\|install-builtin]` | 技能管理 |
| `config [set\|get\|list]` | 配置管理 |
| `git [status\|commit\|pr]` | Git 集成 |
| `doctor [--fix]` | 诊断问题 |
| `logs [--follow]` | 查看运行日志 |
| `mcp-server` | 启动 MCP 协议服务器 |

---

## 安全特性

| 保护层 | 实现方式 |
|--------|----------|
| 命令黑名单 | 阻止 20+ 危险命令 |
| 路径沙箱 | `path_jail` 防止路径遍历 |
| SSRF 防护 | 阻止本地和内部 IP |
| 超时保护 | 命令/HTTP/浏览器 30s 限制 |
| 环境变量清理 | 只保留安全的环境变量 |
| 输出截断 | 限制命令输出和文件读取大小 |

---

## 构建

```bash
# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release

# 带 GUI 支持
cargo build --release --features gui
```

---

## 测试

```bash
# 运行所有测试
cargo test

# 运行演示
cargo run --release demo
```

---

## 许可证

MIT