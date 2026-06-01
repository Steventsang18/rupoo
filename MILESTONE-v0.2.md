# Rupoo 里程碑 v0.2 — 开发总结

> 日期：2026-05-12
>
> 从零构建的 AI 终端助手，历经 Phase 0 ~ Phase 2 核心开发，完成 50 项测试全覆盖。

---

## 项目概况

| 项目 | 值 |
|------|-----|
| 名称 | Rupoo |
| 版本 | v0.2.0 |
| 语言 | Rust 2021 edition |
| 架构 | tokio 异步 + SQLite + rig-core LLM |
| 二进制大小 | ~10 MB (ARM64) |
| 测试 | 50 (48 单元 + 2 集成) |
| 编译警告 | 4 (minor) |
| 生产代码 unwrap/expect | 0 |

---

## 架构全景

```
CLI (clap)
  │
  ├── Agent 状态机
  │   ├── Think (LLM 推理 / rig-core)
  │   ├── ToolCall (MCP 工具)
  │   ├── Exec (终端命令 / tokio::process)
  │   ├── HttpRequest (HTTP 请求 / reqwest)
  │   ├── BrowserAction (浏览器 / headless Chrome)
  │   ├── WaitForInput (等待用户)
  │   └── Finish (完成)
  │
  ├── LLM 网关
  │   └── rig-core (Anthropic / OpenAI / DeepSeek / Ollama)
  │
  ├── 工具系统
  │   ├── rig_tools — 4 个类型安全 Tool 实现
  │   ├── mcp — 内部工具调度器
  │   └── mcp_server — MCP Server (JSON-RPC over stdio)
  │
  ├── 持久层
  │   ├── SQLite (rusqlite + std::sync::Mutex)
  │   ├── FTS5 全文记忆
  │   └── Settings 键值存储
  │
  ├── 安全沙箱
  │   ├── path_jail — 文件路径监狱
  │   ├── 命令黑名单 (sudo, rm, mkfs…)
  │   ├── SSRF 防护 (localhost 拦截)
  │   └── 30s 默认超时
  │
  ├── 技能系统
  │   ├── JSON 技能文件管理
  │   ├── 内置技能 (code-review, generate-readme)
  │   └── 自动技能学习 (计划完成自动提取)
  │
  ├── Git 集成 (git2)
  │   ├── status / commit / PR
  │   └── task ID 引用
  │
  ├── 桌面 GUI (feature-gated)
  │   └── egui + eframe + tray-icon
  │
  └── 终端体验
      ├── rustyline (Readline 编辑 + 历史)
      ├── indicatif (Spinner)
      └── console (样式)
```

---

## 开发里程碑

### Phase 0 — 核心引擎 (v0.1)

| 功能 | 说明 |
|------|------|
| Plan 执行引擎 | Think/ToolCall/WaitForInput/Finish 四步状态机 |
| 崩溃恢复 | Checkpoint 事务 + resume |
| 心跳检查点 | 步骤级 Running checkpoint |
| CLI 框架 | clap 子命令 + `--input` 参数注入 |
| MCP 基础 | 内置工具注册表 + StdioClient |

### Phase 1 — 产品化 (v0.1)

| 功能 | 说明 |
|------|------|
| 记忆系统 | SQLite FTS5 全文搜索，BM25 排序 |
| 技能系统 | JSON 技能文件，CLI 管理 |
| 桌面 GUI | egui/eframe 原生窗口 (feature-gated) |
| 系统托盘 | tray-icon (feature-gated) |
| 安装包 | cargo-bundle macOS .app / Linux AppImage |

### Phase 2 — 自进化与集成 (v0.2)

| 功能 | 说明 |
|------|------|
| LLM 网关 | rig-core 0.30，3 provider (Anthropic/OpenAI/Ollama) |
| Tool calling | rig Agent 原生工具绑定 |
| 自动技能学习 | 计划完成自动提取技能 |
| 记忆注入 | Think 步骤自动检索 + 注入 LLM |
| 终端执行 | tokio::process + 环境变量清洗 |
| HTTP 请求 | reqwest + SSRF 保护 |
| 浏览器自动化 | headless Chrome 截图/导航 |
| 安全沙箱 | path_jail + 命令黑名单 + 超时 |
| Git 集成 | status/commit/PR + task ID 引用 |
| MCP Server | JSON-RPC over stdio，工具对外暴露 |
| 终端体验 | rustyline + indicatif + console |

---

## 技术选型

### 核心依赖

| Crate | 用途 | 选型理由 |
|-------|------|----------|
| `rig-core` | LLM 框架 | 统一 20+ provider，tool calling 原生 |
| `rusqlite` | 数据库 | 直接操作 SQLite，FTS5 支持 |
| `clap` | CLI 解析 | 行业标准，derive macro |
| `serde` | 序列化 | Rust 生态事实标准 |
| `git2` | Git 集成 | 成熟 libgit2 绑定 |
| `reqwest` | HTTP 客户端 | 异步 + TLS，生态最成熟 |
| `tokio` | 异步运行时 | Rust 异步事实标准 |

### 开发铁律遵守

| 铁律 | 状态 |
|------|------|
| spawn_blocking 封装 DB | ✅ |
| std::sync::Mutex 保护 Connection | ✅ |
| checkpoint + plan 同一事务 | ✅ |
| 生产代码零 unwrap/expect | ✅ |
| serde(tag) 类型安全序列化 | ✅ |
| tracing 全步骤日志 | ✅ |

---

## 安全设计

| 防护层 | 实现 |
|--------|------|
| 命令黑名单 | sudo, rm, mkfs, dd 等 20+ 禁止 |
| 文件路径监狱 | path_jail 库，防 `../../` 穿越 |
| SSRF 保护 | 拦截 localhost/127.0.0.1 请求 |
| 超时保护 | 命令 30s / HTTP 30s / 浏览器 30s |
| 环境变量清洗 | 仅保留 PATH/HOME/USER/SHELL/LANG/TERM |
| 响应体限制 | 5MB HTTP / 10K 命令输出 / 5K 文本 |

---

## 测试覆盖

| 模块 | 测试数 | 覆盖场景 |
|------|--------|----------|
| agent | 8 | 完整执行、崩溃恢复、WaitForInput、inject_input、heartbeat |
| db | 3 | 事务、checkpoint、清理 |
| mcp | 2 | echo 工具、未知工具 |
| memory | 4 | 存储、搜索、格式、关联记忆 |
| skill | 5 | 保存/加载/列表/转换/删除 |
| rig_tools | 4 | echo、file_read、tool set、list_directory |
| llm | 4 | 配置、序列化 |
| safety | 5 | 命令黑名单、SSRF、超时、路径穿越、空字节注入 |
| terminal | 3 | echo、禁止命令、超时 |
| network | 2 | localhost 拦截、127.0.0.1 拦截 |
| browser | 2 | 浏览器查找、不支持操作 |
| mcp_server | 4 | 初始化、工具列表、工具调用、未知方法 |
| git | 2 | 状态描述、仓库打开 |
| **集成测试** | **2** | 崩溃恢复、中断步骤重试 |

---

## 项目文件结构

```
rupoo/
├── Cargo.toml
├── rupoo-config.example.toml
├── README.md
├── MILESTONE-v0.2.md
├── scripts/
│   ├── rupoo.command      # macOS 双击启动
│   └── install.sh          # Linux 安装
├── src/
│   ├── main.rs             # CLI 入口 (clap)
│   ├── lib.rs              # 库入口
│   ├── agent.rs            # Agent 状态机
│   ├── task.rs             # Plan/Step/Checkpoint 类型
│   ├── db.rs               # SQLite (rusqlite + FTS5)
│   ├── llm.rs              # LLM 网关 (rig-core)
│   ├── rig_tools.rs        # 类型安全工具定义
│   ├── mcp.rs              # MCP 工具调度器
│   ├── mcp_server.rs       # MCP Server (JSON-RPC)
│   ├── safety.rs           # 安全沙箱 (path_jail)
│   ├── memory.rs           # FTS5 记忆系统
│   ├── skill.rs            # 技能系统
│   ├── git.rs              # Git 集成 (git2)
│   ├── error.rs            # AgentError
│   ├── gui.rs              # egui 桌面 (feature-gated)
│   └── tray.rs             # 系统托盘 (feature-gated)
├── src/tools/
│   ├── mod.rs
│   ├── network.rs           # HTTP 请求
│   ├── terminal.rs          # 命令执行
│   └── browser.rs           # 浏览器自动化
└── tests/
    └── crash_recovery_test.rs
```

---

## 后续方向

| 方向 | 说明 | 优先级 |
|------|------|--------|
| 浏览器 DOM 交互 | chromiumoxide 实现 Click/GetText | 中 |
| MCP Server 增强 | 添加 SSE 传输、动态工具注册 | 中 |
| 自进化循环 | 任务完成 → 自动学习技能 → Git commit | 低 |
| CI/CD | GitHub Actions 自动化测试 | 低 |
| VSCode 插件 | 编辑器内直接调用 Rupoo | 低 |
