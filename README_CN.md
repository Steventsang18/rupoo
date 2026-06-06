# Rupoo — AI 驱动的终端助手

Rupoo 是一个基于终端的 AI 助手，采用原生 REPL 界面，支持语法高亮代码块、Markdown 渲染、主题切换和 Claude Code 风格的工具调用显示 —— 所有功能都由双模式代理引擎（聊天 + 计划）驱动。

```
版本:    0.4.0          语言: Rust 2021
代码行数: ~26,000       测试:    110 ✅
界面:    原生 REPL      LLM:     Anthropic / OpenAI / DeepSeek / Ollama
数据库:  SQLite (FTS5)  记忆:    混合搜索 (FTS5 + 向量)
安全:    path_jail 沙箱 + SSRF 防护
```

---

## ✨ 功能特性

| 特性 | 描述 |
|------|------|
| **原生 REPL** | 流畅滚动，支持窗口调整，无需帧缓冲 |
| **语法高亮** | 基于 syntect，支持 3 种主题（ocean / GitHub / mocha） |
| **Markdown 渲染** | 表格、引用、任务列表、代码块、链接 |
| **主题系统** | `/theme dark\|light\|monokai` 命令切换，持久化存储 |
| **聊天气泡** | 用户右对齐 (▸)，AI 左对齐 (◂) |
| **工具卡片** | Claude Code 风格的折叠卡片显示工具调用 |
| **思考链** | 流式加载动画 + 气泡显示 AI 推理过程 |
| **历史搜索** | `Ctrl+R` 增量搜索（1000 条记录） |
| **双模式代理** | 聊天模式 + 计划模式，支持检查点 |
| **7 种步骤类型** | Think、ToolCall、WaitForInput、Exec、HttpRequest、BrowserAction、Finish |
| **长期记忆** | SQLite FTS5 全文搜索 + 向量语义搜索 |
| **混合搜索** | 结合 FTS5 关键词匹配 + 向量语义理解 |
| **记忆开关** | 通过 `/memory on/off` 启用/禁用记忆 |
| **深度搜索开关** | 通过 `/deep on/off` 启用/禁用混合搜索 |
| **技能系统** | 基于 JSON 的技能，支持自动学习 |
| **崩溃恢复** | 心跳检查点，事务性原子操作 |

---

## 🚀 快速开始

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

---

## 🎯 v0.4.0 新功能

### 记忆系统

```bash
# 查看记忆状态
/memory

# 启用/禁用记忆
/memory on
/memory off

# 列出最近记忆
/memory list

# 搜索记忆
/memory search <关键词>
```

### 深度搜索（混合搜索）

深度搜索结合 FTS5 全文搜索和向量语义搜索，提供更好的搜索相关性。

```bash
# 查看深度搜索状态
/deep

# 启用深度搜索（FTS5 + 向量）
/deep on

# 禁用深度搜索（仅 FTS5）
/deep off
```

#### 混合搜索原理

```
用户查询
    │
    ├──► FTS5 搜索（关键词匹配）
    │         │ 快速、精确的关键词匹配
    │
    └──► 向量搜索（语义理解）
              │ 理解意图和含义

组合结果（RRF 排序）
```

---

## ⌨️ 键盘快捷键

| 按键 | 操作 |
|------|------|
| `Enter` | 发送消息 |
| `↑` / `↓` | 导航历史记录 |
| `Ctrl+R` | 增量搜索 |
| `Ctrl+C` | 取消操作 |
| `Ctrl+D` | 退出 |
| `Ctrl+L` | 清空屏幕 |
| `Tab` | 自动补全命令 |
| `Ctrl+N` | 新建会话 |

---

## 📝 REPL 命令

| 命令 | 描述 |
|------|------|
| `/new` | 新建对话 |
| `/model` | 切换 LLM 模型 |
| `/plan` | 进入计划模式 |
| `/memory` | 记忆管理 |
| `/memory on/off` | 启用/禁用记忆 |
| `/memory list` | 列出最近记忆 |
| `/memory search <query>` | 搜索记忆 |
| `/deep` | 深度搜索状态 |
| `/deep on/off` | 启用/禁用混合搜索 |
| `/theme dark\|light\|monokai` | 切换主题 |
| `?` 或 `/help` | 显示帮助 |

---

## 🔧 CLI 命令

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

## 🔒 安全特性

| 保护措施 | 实现方式 |
|----------|----------|
| 命令黑名单 | 阻止 20+ 危险命令 |
| 路径沙箱 | `path_jail` 防止路径遍历 |
| SSRF 防护 | 阻止本地和内网 IP |
| 超时保护 | 命令/HTTP/浏览器操作 30s 限制 |
| 环境清理 | 仅保留安全的环境变量 |
| 输出截断 | 限制命令输出和文件读取大小 |

---

## 🏗️ 构建

```bash
# 开发构建
cargo build

# 发布构建（推荐）
cargo build --release

# 带 GUI 支持
cargo build --release --features gui
```

---

## 🧪 测试

```bash
# 运行所有测试
cargo test

# 详细输出
cargo test -- --nocapture

# 运行特定测试
cargo test test_name

# 运行基准测试
cargo bench
```

---

## 📊 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                       用户层 (CLI/TUI)                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐   │
│  │  聊天    │  │  计划    │  │  命令    │  │   记忆      │   │
│  │  模式    │  │  模式    │  │  系统    │  │   系统      │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └──────┬───────┘   │
└───────┼─────────────┼─────────────┼────────────────┼───────────┘
        │             │             │                │
┌───────▼─────────────▼─────────────▼────────────────▼───────────┐
│                      代理核心层                                │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Agent + Memory System + LLM Gateway                    │   │
│  │  ┌──────────┐ ┌─────────────┐ ┌─────────────────────┐  │   │
│  │  │ TaskRepo │ │ MemoryStore │ │ MemoryCache (LRU)   │  │   │
│  │  └──────────┘ └─────────────┘ └──────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 📈 性能指标

| 指标 | 目标 | 状态 |
|------|------|------|
| 冷启动 | < 2s | ✅ |
| LLM 调用延迟 | < 10s | ✅ |
| 工具执行超时 | < 5s | ✅ |
| 记忆搜索响应 | < 100ms | ✅ |
| 信号压缩 | < 50ms | ✅ |

---

## 📚 文档

- [用户指南](docs/USER_GUIDE.md) - 完整的用户文档
- [性能优化](docs/PERFORMANCE.md) - 性能优化详情
- [贡献指南](CONTRIBUTING.md) - 贡献指南

---

## 📝 更新日志

详细版本历史请见 [CHANGELOG.md](CHANGELOG.md)。

---

## 🤝 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md) 获取指南。

---

## 📄 许可证

MIT License - 详见 [LICENSE](LICENSE)。

---

## 🙏 致谢

- [rig-core](https://github.com/gregpr07/rig) - LLM 代理框架
- [syntect](https://github.com/trishume/syntect) - 语法高亮
- [rustyline](https://github.com/kknghk/rustyline) - Readline 实现