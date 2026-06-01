# Rupoo 向 Pi 学习 — 优化方案设计文档

## 概述

基于 pi 与 rupoo 的对比分析，确认 5 个优化方向。本文档定义每个方向的设计方案、实现策略和验证标准。

---

## 1. LLM 网关扩展（混合架构）

### 设计目标

从当前 3 个 provider（Anthropic / OpenAI 兼容 / Ollama）扩展到 6+ provider，同时获得 thinking/prompt caching 等高级参数控制能力。

### 架构

```
┌─ LlmConfig ──────────────────────────────────────────────┐
│  thinking: ThinkingConfig (off|low|medium|high|xhigh)    │
│  cache: CacheControl (none|short|long)                   │
│  provider: String (anthropic|openai|google|...|rig-*)    │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  LlmRouter                                               │
│  ┌────────────────────────────────────────────────────┐  │
│  │  NativeProviderRegistry                             │  │
│  │  ├── AnthropicProvider (原生 HTTP, 支持 thinking)   │  │
│  │  ├── OpenAIProvider (原生 HTTP, 支持 reasoning)     │  │
│  │  ├── GoogleProvider (原生 HTTP/SSE)                 │  │
│  │  └── ...                                           │  │
│  └────────────────────────────────────────────────────┘  │
│  ┌────────────────────────────────────────────────────┐  │
│  │  RigFallbackAdapter (rig-core, 老旧/未知 provider)  │  │
│  └────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

### 实现阶段

| 阶段 | 内容 | 依赖 |
|------|------|------|
| 1 | `NativeProvider` trait 定义，`LlmRouter` 分发逻辑 | 无 |
| 2 | Anthropic 原生 provider（含 thinking 参数） | 阶段 1 |
| 3 | OpenAI 原生 provider（含 reasoning_effort） | 阶段 1 |
| 4 | Google Gemini 原生 provider | 阶段 1 |
| 5 | Config 层支持 `thinking_level` / 缓存的声明和管理 | 阶段 2 |
| 6 | 运行时 provider 热切换 | 阶段 1-4 |

### 验证标准

- 每个 provider 有独立的 integration test（`#[ignore]` + 需要 API key 环境变量）
- 切换 provider 后同一 prompt 获得合理回复
- rig-core fallback: 配置错误 key → 报错信息明确
- thinking 开关: 同一复杂问题, on→有思考过程, off→无思考过程

---

## 2. 思考链 / Reasoning 支持

### 设计目标

用户可通过配置控制 LLM 的推理深度，支持 Anthropic thinking 和 OpenAI reasoning_effort 两套机制。

### 数据模型

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
    XHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub level: ThinkingLevel,
    /// Anthropic: budget_tokens (默认从 level 映射)
    /// OpenAI: 无关 (用 max_tokens 控制)
    pub budget_tokens: Option<u32>,
}
```

### Thinking Level → Provider 参数映射

| Level | Anthropic `thinking.type` | Anthropic `thinking.budget_tokens` | OpenAI `reasoning_effort` |
|-------|--------------------------|-----------------------------------|--------------------------|
| off | (不发送) | — | (不发送) |
| low | `enabled` | 2048 | `low` |
| medium | `enabled` | 8192 | `medium` |
| high | `enabled` | 16384 | `high` |
| xhigh | `enabled` | 32768 | （无映射，用 high） |

### 静默降级规则

- rig-core 路径: 忽略 thinking 设置, 正常 chat
- 未知 provider: 忽略 thinking 设置, 正常 chat
- 用户切换 provider 后: 保留 config, provider 自行决定是否支持
- 切换时无 warning（避免噪音）, 除非 `--verbose`

### 验证标准

- 复杂数学/逻辑题在 thinking=high 时返回包含推理过程
- thinking=off 时同一题目返回简洁答案
- 切换到 Ollama（rig-core 路径）: 正常回复, 无报错
- `budget_tokens` 超限时 provider 截断处理, 不 panic

---

## 3. TUI 虚拟列表 + 脏区域差分渲染

### 设计目标

消除 50+ 消息历史时的帧率下降, 将单帧渲染时间降低 60%+。

### 核心组件

```
DirtyRegionTracker
  位掩码标记: CHAT | SIDEBAR | STATUS | INPUT | PALETTE
  mark_dirty(region)
  is_dirty(region) → bool
  clear_all()

VirtualMessageList
  messages: Vec<Message>
  visible_range: Range<usize>  // 从 scroll_offset 计算
  cached_lines: BTreeMap<usize, Vec<Line>>  // 换行结果缓存
  scroll_offset: usize
  auto_scroll: bool

  on_new_message(): 追加到 messages, scroll_offset = 最后, auto_scroll = true
  on_scroll_up(): 减少 scroll_offset, auto_scroll = false
  on_scroll_down(): 增加 scroll_offset, 到底部则 auto_scroll = true
  render(area, dirty): 只在 dirty 或 offset 变化时重新计算
```

### 滚动行为

- 新消息到达 → 自动滚到底部（保留自动滚动标志）
- 用户手动 ↑/PgUp/PgDn/Shift+↑ → 暂停自动滚动，显示手动滚动位置
- 用户手动滚到底部 → 恢复自动滚动
- 窗口 resize → 所有缓存失效，重新布局（全量渲染一次）

### 渲染优化

- 换行结果缓存: 每个消息在固定宽度下的换行结果缓存到 `cached_lines`
- 窗口 resize 时清除缓存（宽度变化导致换行结果变化）
- 状态栏 / 侧栏只在 dirty 时渲染

### 验证标准

- 200 条历史消息加载后, draw() 耗时从 ~8ms → ~2ms
- 快速连续 ↓ 操作, 不出现空白行或闪烁
- 窗口 resize → 正确重排, 不出现截断或错位
- auto_scroll / manual_scroll 状态转换正确

---

## 4. CI/CD + 发布流程

### 设计目标

自动化代码质量检查和发布流程, 确保每次提交可验证。

### 工作流设计

#### CI (`.github/workflows/ci.yml`)

```yaml
触发: push (所有分支) + pull_request

job: check
  - cargo check (stable)
  - cargo fmt --check
  - cargo clippy -- -D warnings

job: test  
  - cargo test (unit + integration, 跳过 #[ignore])

job: audit
  - cargo audit (依赖漏洞扫描)
  - cargo deny (license 合规检查)

job: build
  - cargo build --release
  - 上传 artifact (可选)
```

#### Release (`.github/workflows/release.yml`)

```yaml
触发: tag push (v*)

job: build-mac
  - cargo build --release (aarch64-apple-darwin)
  - 压缩 binary → rupoo-{version}-mac-aarch64.tar.gz

job: build-linux
  - cargo build --release (x86_64-unknown-linux-gnu)
  - 压缩 binary → rupoo-{version}-linux-x86_64.tar.gz

job: release
  - 创建 GitHub Release
  - 上传所有 artifacts
  - 自动生成 changelog (git log --oneline)
```

### 测试治理

- `#[ignore]` on tests that need LLM API keys (标记 `requires-llm-key`)
- CI 运行 `cargo test -- --skip requires-llm-key`
- 可选的 nightly CI 用 GitHub Secrets 注入 API key 运行完整测试

### 验证标准

- PR 提交 → CI 自动触发, 全部通过
- 创建 v0.3.0 tag → release workflow 自动完成
- Release artifacts 可下载、解压、运行
- `cargo audit` 发现漏洞 → CI 失败

---

## 5. Extension 系统（基于 MCP）

### 设计目标

允许第三方通过 MCP 协议扩展 rupoo 功能, 实现 `rupoo ext install/list/uninstall` 命令体系。

### 架构

```
~/.rupoo/extensions/
└── my-ext/
    ├── extension.toml    ← 扩展清单
    ├── server.py         ← MCP server 脚本
    └── README.md

rupoo ext install <path|url>
  → 复制/下载到 ~/.rupoo/extensions/<name>/
  → 解析 extension.toml
  → 启动 MCP 子进程 (spawn)
  → 注册 tools + skills + slash-commands

rupoo ext list
  → 读取 extensions/ 目录
  → 显示名称、版本、状态 (running/stopped/errored)
  → 如果 MCP 子进程存活: running; 否则: stopped

rupoo ext uninstall <name>
  → 终止 MCP 子进程 (SIGTERM + 2s 超时 → SIGKILL)
  → 删除 ~/.rupoo/extensions/<name>/
  → 注销 tools/skills

Agent 调用:
  agent 收到包含扩展 tool 的 step
  → 检查 MCP 子进程存活
  → 通过现有 mcp.rs JSON-RPC 通道发送请求
  → 超时/异常 → agent 继续, 标记 tool 不可用
```

### extension.toml 格式

```toml
[extension]
name = "my-ext"
version = "1.0.0"
description = "自定义工具集"
author = "user"

# 支持的钩子
hooks = ["before_tool_call", "after_tool_call"]

[mcp]
# 启动命令
command = "python"
args = ["server.py"]
transport = "stdio"

[tools]
# 声明提供的工具名称
provides = ["my-ext-tool1", "my-ext-tool2"]
```

### MCP 子进程生命周期管理

| 事件 | 行为 |
|------|------|
| `ext install` 完成 | spawn 进程, 等待就绪信号 |
| agent 启动 | 拉起所有 running 状态的扩展子进程 |
| 子进程崩溃 | 标记 errored, agent 继续, 日志记录 |
| `ext list` | 检测子进程存活, 更新状态显示 |
| `ext uninstall` | SIGTERM → 等待 2s → SIGKILL |
| agent 退出 | SIGTERM 所有子进程 |

### 安全约束

- 子进程在 path_jail 沙箱外运行（它自己就是被限制的工具提供者）
- 扩展声明提供的工具名称需进入 allowlist（防止覆盖内置工具）
- 扩展的 MCP 请求受现有超时/SSRF 防护约束

### 验证标准

- `rupoo ext install ./test-ext` → 扩展出现在 list 中
- Agent 调用扩展提供的 tool → 正确响应
- 子进程崩溃 → agent 不 panic, 显示 tool 不可用
- `rupoo ext uninstall test-ext` → tool 从 agent 中移除
- 安装同名扩展 → 报错（已存在）, 不覆盖

---

## 优先级排序

| 优先级 | 领域 | 理由 |
|--------|------|------|
| P0 | CI/CD + 发布流程 | 基础设施, 其他所有变更的验证前提 |
| P1 | LLM 网关混合架构 | 核心能力扩展, 影响所有用户 |
| P2 | 思考链支持 | 依赖 P1 的原生 provider 层 |
| P3 | TUI 差分渲染 | 用户体验优化, 独立进行 |
| P4 | Extension 系统 | 生态扩展, 依赖 MCP 基础设施已就位 |

---

## 总体验证策略

每个优化领域必须满足闭环验证：

```
代码变更 → 单元测试 → 集成测试 → 手动功能验证 → 性能基准对比
```

所有测试最终通过 CI 自动化执行（P0 先落地）。
