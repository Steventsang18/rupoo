# 🔍 Rupoo 项目审查报告 (v2 — 最新)

> **审查日期**：2026-05-14  
> **审查工具**：DeepSeek TUI v0.8.35 (DeepSeek V4)  
> **项目路径**：`/Users/pengxiangzeng/rust-project`  
> **Git 状态**：5 个新提交在 HEAD 之后（未跟踪文件较多）

---

## 一、项目概要

| 维度 | 详情 |
|------|------|
| **项目名称** | Rupoo (内部 Cargo 包名 `rupoo`) |
| **版本** | v0.2.0 |
| **定位** | AI 驱动的终端助手 — 计划执行、技能管理、记忆存储、系统操作、TUI 交互 |
| **语言** | Rust 2021 edition |
| **异步运行时** | tokio (full features) |
| **数据库** | SQLite (rusqlite, WAL 模式, FTS5 全文搜索) |
| **LLM 框架** | rig-core 0.30 (Anthropic / OpenAI 兼容 / Ollama) |
| **TUI 框架** | ratatui 0.29 + crossterm 0.28 + tui-textarea 0.7 |
| **二进制大小** | ~10 MB (ARM64, release) |
| **测试覆盖** | **74 项全通过** (49 单元-lib + 19 单元-main + 4 CLI-DB + 2 集成) |
| **编译状态** | ✅ `cargo check` 零错误零警告 |
| **源码总行数** | ~5,700 行 (含新增 CLI 命令模块) |

---

## 二、自上次审查以来的变化 (v0.2.0 → v0.2.0+)

### 2.1 新增的 5 个 Git 提交

| 提交 | 内容 |
|------|------|
| `bdb4dee` | chore: 移除未使用的 PlanSummary 导入 (最终清理) |
| `1e79085` | feat(cli): 添加 `rupoo logs` 命令 |
| `9dabe43` | feat(cli): 添加 `rupoo doctor` 命令 |
| `e33ad4e` | feat(cli): 添加 `rupoo session` 命令 |
| `6c48aa5` | feat(cli): 添加 `rupoo model` 命令 |

### 2.2 主要演进

| 维度 | 之前 | 现在 | 变化 |
|------|------|------|------|
| REPL 实现 | rustyline 行编辑 | **ratatui TUI** (全屏终端界面) | 🔺 重大升级 |
| CLI 子命令 | run/demo/skills/git/config/mcp-server/serve/gui | + **status/model/session/doctor/logs** | 🔺 5 个新命令 |
| 斜杠命令 | 无 | `/help /status /model /skills /memory /config /git /run /plan /demo` | 🆕 完整系统 |
| 测试数量 | 55 | **74** | 🔺 +19 |
| main.rs 行数 | 847 | **958** | 🔺 +111 |
| 新增模块 | 无 | `cli/cmds/{status,model,session,doctor,logs}.rs` | 🆕 5 个文件 |

---

## 三、架构评估 (更新)

### 3.1 整体设计 — ⭐⭐⭐⭐⭐ 优秀 (维持)

```
CLI (clap) ──→ Agent 状态机 ──→ Tool Executor
     │                │                    ├── rig_tools (类型安全)
     ├── TUI (ratatui) ├── LLM Gateway      ├── mcp (内部调度)
     ├── Slash Cmds    ├── SafetyContext    └── mcp_server (JSON-RPC stdio)
     └── CLI Cmds      ├── TaskRepo (SQLite)
                       ├── MemoryStore (FTS5)
                       ├── SkillManager
                       └── GitRepo (git2)
```

**新增亮点**：
- **TUI 事件循环**：`main.rs` 中 `run_tui()` 函数实现了完整的 ratatui 终端界面，包括标题栏、聊天区、输入区、状态栏
- **斜杠命令系统**：`/help`、`/status`、`/model` 等 10 个内置命令，统一通过 `handle_cmd()` 分发
- **自然语言执行**：非斜杠输入通过 `execute_nl()` 交给 Agent 处理（LLM Think → ToolCall）
- **Ctrl+C/D 优雅退出**：TUI 事件正确处理

### 3.2 TUI 实现质量 — ⭐⭐⭐⭐ 良好 (新评)

`src/cli/ui.rs` 实现了完整的 ratatui 渲染管线：

- **布局**：标题栏 (1行) | 聊天区 (弹性) | 输入区 (3行) | 状态栏 (1行)
- **消息气泡**：用户消息绿色、助手消息青色，清晰区分
- **欢迎界面**：空消息时显示使用提示
- **tui-textarea 集成**：支持多行输入、历史记录、占位符文本
- **加载状态**：loading 时状态栏变黄，显示 "Ctrl+C to cancel"

**可改进**：
- 消息列表没有虚拟滚动，大量消息时可能有性能问题
- 无语法高亮（syntect 已引入但未使用）
- 无消息时间戳

### 3.3 新增 CLI 命令质量 — ⭐⭐⭐⭐⭐ 优秀 (新评)

#### `rupoo status` — 系统状态概览
- 显示版本、数据库路径、计划统计（按状态分组）、LLM 配置、技能、Git 状态、日志
- 支持 `--short` 单行模式
- 漂亮的树形 ASCII 布局，`console::style` 彩色输出
- 测试覆盖：3 项（状态计数构建、空状态、短格式）

#### `rupoo model` — LLM 提供商管理
- `show`：显示当前提供商、模型、API 密钥状态（部分脱敏）
- `list`：列出所有支持的提供商及配置状态
- `set <provider>/<model>`：交互式切换提供商和模型
- 自动提示未配置 API 密钥的修复建议
- 测试覆盖：4 项（模型查找、密钥渲染、目标解析）

#### `rupoo session` — 计划会话管理
- `list`：分页列出计划，彩色状态标记
- `show <id>`：显示单个计划的详细步骤和执行状态
- `resume <id>`：提示如何恢复计划
- `delete <id>`：删除计划
- `prune --days N`：清理过期计划
- `step_icon()` 和 `step_label()` 辅助函数，输出美观
- 测试覆盖：4 项（图标、标签渲染）

#### `rupoo doctor` — 系统诊断
- 6 项诊断检查：数据库、LLM 配置、技能、Git、数据目录、日志文件
- 彩色通过/警告/错误图标 (●/○/✗)
- 支持 `--fix` 自动修复目录缺失等问题
- 连 Ollama 本地服务的可达性也做了探测
- 测试覆盖：4 项（状态汇总、缩进格式化）

#### `rupoo logs` — 日志查看
- 显示最后 N 行日志（默认 50）
- `--follow` 实时跟踪（每秒轮询）
- `--level` 按日志级别过滤（ERROR/WARN 等）
- `--prev` 查看上次会话日志
- 测试覆盖：4 项（过滤器逻辑）

### 3.4 斜杠命令系统 — ⭐⭐⭐⭐ 良好 (新评)

TUI 中键入 `/` 开头的消息触发 `handle_cmd()`：

| 命令 | 功能 |
|------|------|
| `/help` | 显示可用命令列表 |
| `/status` | 系统状态概览 |
| `/model` | 查看/切换 LLM |
| `/skills` | 列出已安装技能 |
| `/memory <query>` | 搜索记忆 |
| `/config` | 配置管理提示 |
| `/git` | Git 状态 |
| `/run <id>` | 执行计划 |
| `/plan` | 创建新计划并展示 |
| `/demo` | 运行演示计划 |

`execute_nl()` 将非斜杠的自然语言交给 Agent 处理，形成 Think → ToolCall 循环。

---

## 四、代码质量评估 (更新)

### 4.1 类型安全 — ⭐⭐⭐⭐⭐ 优秀 (维持)

无变化，生产代码依然零 `unwrap`/`expect`。

### 4.2 错误处理 — ⭐⭐⭐⭐⭐ 优秀 (提升)

新增模块全面使用 `anyhow::Result`，错误传播规范。`doctor.rs` 中的 `CheckResult` 结构体设计清晰。

### 4.3 代码重复 — ⭐⭐⭐ 依然存在 (未改进)

`mcp.rs`、`mcp_server.rs`、`rig_tools.rs` 三套独立的工具实现问题**仍未解决**。这是项目当前最大的技术债务。

### 4.4 测试覆盖 — ⭐⭐⭐⭐⭐ 优秀 (提升)

新增 19 项测试全部覆盖新模块：

| 新模块 | 测试数 |
|--------|--------|
| cli/cmds/status | 3 |
| cli/cmds/model | 4 |
| cli/cmds/session | 4 |
| cli/cmds/doctor | 4 |
| cli/cmds/logs | 4 |

---

## 五、安全评审 (更新)

### 5.1 之前标记的风险项复查

| 风险 | 等级 | 最新状态 |
|------|------|----------|
| MCP Server 无认证 | 🔴 高 | **未修复** |
| 命令黑名单可绕过 | 🟡 中 | **未修复** |
| SSRF 不完整 | 🟡 中 | **未修复** |
| 代码重复（三套工具） | 🟡 中 | **未修复** |
| 文件写入无路径隔离 | 🟡 中 | **未修复** |

### 5.2 新增关注

| 风险 | 等级 | 说明 |
|------|------|------|
| TUI 中输入无长度限制 | 🟢 低 | `tui-textarea` 无内置截断，但 ratatui 渲染会限制 |
| `execute_nl` 无超时 | 🟡 中 | 自然语言提交给 Agent 后没有整体超时，可能长时间挂起 |

---

## 六、Git 仓库状态

```
5 个提交在 master 上 (未推送)
大量未跟踪文件：
  - 文档：MILESTONE-v0.2.md, README.md, PROJECT-AUDIT-REPORT.md
  - 配置：rupoo-config.example.toml
  - 演示：repl-option1-demo.html, repl-option2-demo.html 等
  - 脚本：scripts/
  - 数据：agent.db
  - CI/CD：.github/
  - 开发文档：多个 Rust 开发指令 .md
```

建议：决定哪些文件应纳入版本控制，将 `agent.db` 加入 `.gitignore`。

---

## 七、总体评分 (更新)

| 维度 | 评分 | 变化 | 说明 |
|------|------|------|------|
| 架构设计 | ⭐⭐⭐⭐⭐ | — | 分层清晰，新增模块位置合理 |
| 代码质量 | ⭐⭐⭐⭐ | — | 新代码质量好，但旧有重复未清理 |
| TUI/UX 体验 | ⭐⭐⭐⭐ | 🆕 N/A→4 | 完整 TUI 实现，交互流畅 |
| CLI 工具链 | ⭐⭐⭐⭐⭐ | 🔺 +2 | 5 个新子命令，每个都有测试 |
| 崩溃恢复 | ⭐⭐⭐⭐⭐ | — | 设计周密 |
| 安全设计 | ⭐⭐⭐⭐ | — | 风险点未修复 |
| 测试覆盖 | ⭐⭐⭐⭐⭐ | 🔺 55→74 | 新模块全覆盖 |
| 可扩展性 | ⭐⭐⭐⭐ | — | trait 抽象好 |

**综合评级：A- → A (项目成熟度显著提升)**

---

## 八、改进建议 (更新)

### P0 — 必须修复

1. **消除工具代码重复**：`rig_tools`/`mcp`/`mcp_server` 三处独立实现统一
2. **MCP Server 认证**：添加 token 验证

### P1 — 建议修复

3. **命令黑名单加固**：用 `which` 解析真实路径后比对
4. **SSRF 加固**：添加 `0.0.0.0`、`169.254.169.254`、IPv6 链路本地地址拦截
5. **`.gitignore` 完善**：添加 `agent.db`、`*.log`
6. **Git 仓库整理**：决定未跟踪文件的去留（docs/、.github/、scripts/、demo HTML）

### P2 — 可选改进

7. **TUI 消息虚拟滚动**：大量消息时的性能优化
8. **语法高亮**：利用已引入的 `syntect`
9. **TUI 消息时间戳**：每条消息显示时间
10. **execute_nl 超时**：防止 Agent 无限挂起
11. **GUI 版本号**：`gui.rs` 中 v0.1.0 → v0.2.0
12. **清理未使用依赖**：`syntect`、`miette` 如确定不用则移除

---

## 九、文件结构一览 (更新)

```
rust-project/
├── Cargo.toml                          # 30 项依赖
├── Cargo.lock
├── README.md
├── MILESTONE-v0.2.md
├── PROJECT-AUDIT-REPORT.md             # 上次审查报告
├── PROJECT-AUDIT-REPORT-v2.md          # 本报告
├── rupoo-config.example.toml
├── agent.db
├── .gitignore                          # 仅含 /target
├── scripts/                            # install.sh, rupoo.command
├── docs/superpowers/
├── tests/
│   └── crash_recovery_test.rs          # 2 项集成测试
├── src/
│   ├── main.rs                         # CLI + TUI 入口 (958行) 🆕
│   ├── lib.rs
│   ├── agent.rs                        # Agent 状态机 (794行)
│   ├── task.rs                         # 类型定义 (338行)
│   ├── db.rs                           # SQLite (656行)
│   ├── llm.rs                          # LLM 网关 (320行)
│   ├── rig_tools.rs                    # rig 工具 (400行)
│   ├── mcp.rs                          # MCP 调度 (346行)
│   ├── mcp_server.rs                   # MCP Server (485行)
│   ├── safety.rs                       # 安全沙箱 (225行)
│   ├── memory.rs                       # 记忆系统 (136行)
│   ├── skill.rs                        # 技能管理 (391行)
│   ├── git.rs                          # Git 集成 (241行)
│   ├── gui.rs                          # egui (241行)
│   ├── tray.rs                         # 托盘 (84行)
│   ├── error.rs
│   ├── tracing_setup.rs                # 日志系统
│   ├── cli/
│   │   ├── mod.rs                      # 声明 app/ui/cmds
│   │   ├── app.rs                      # TUI 应用状态
│   │   ├── ui.rs                       # TUI 渲染引擎 🆕
│   │   └── cmds/                       # CLI 子命令 🆕
│   │       ├── mod.rs
│   │       ├── status.rs               # rupoo status
│   │       ├── model.rs                # rupoo model
│   │       ├── session.rs              # rupoo session
│   │       ├── doctor.rs               # rupoo doctor
│   │       └── logs.rs                 # rupoo logs
│   └── tools/
│       ├── mod.rs
│       ├── terminal.rs                 # 终端命令
│       ├── network.rs                  # HTTP 请求
│       └── browser.rs                  # 浏览器自动化
└── target/
```

---

> **审查结论 (v2)**：Rupoo 在短时间内完成了从"可用 CLI 工具"到"成熟终端 AI 助手"的跨越。TUI 交互层、5 个诊断/管理子命令、斜杠命令系统共同构成了一个完整的产品体验。测试从 55 项增长到 74 项，新增代码质量良好。主要短板仍是**工具系统的代码重复**和**安全边界的几个可绕过点**——这两项在功能快速迭代期间被搁置，建议在下一阶段优先处理。

*报告生成时间：2026-05-14 | 审查耗时：~6 分钟*
