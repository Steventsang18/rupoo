# Rupoo REPL 增强设计文档 — 快捷指令 + 状态栏 + Token 显示

> 日期: 2026-05-15
> 状态: 设计定稿

---

## 概述

在 REPL 交互界面中新增三个 UX 功能：

1. **快捷指令轮播栏** — 输入栏下方左侧，8 条快捷指令（命令 + 自然语言说明），每 3 秒切换一条
2. **状态信息栏** — 输入栏下方右侧，显示当前模型名、累计 token、会话时长、计划数
3. **Token 消耗** — 每条 AI 回复右侧显示 `↑ input · ↓ output tok`，流式输出时实时跳动

---

## 实现方案

### 1. LLM Gateway 改造（`src/llm.rs`）

**现状：**
```rust
pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<String>
```

**改造后：**
```rust
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)>
```

rig-core 的每个 provider 的 response 结构中都包含 token 信息。对于 OpenAI 兼容 API（包括 DeepSeek），响应通过 `rig::providers::openai::CompletionModel` 返回，其 response 包含 `usage` 字段。需要通过 rig-core 的底层 API 获取。

由于 rig-core 的 `Prompt` trait 只返回 `String`，需要改用 `CompletionModel::completion()` 或直接调用 provider 的 HTTP API 来获取完整的 response。

实际上，更轻量的做法：在 `LlmGateway` 中直接使用 `reqwest` 调用 OpenAI 兼容的 `/v1/chat/completions` API，这样可以完全控制 token usage 的获取。但目前代码通过 rig-core 来调用，所以兼容性需要保证。

最务实的路径：**在 `chat()` 返回前，对 response body 进行 JSON 解析提取 `usage` 字段，从 rig-core 结果中提取 token 信息。**

具体实现：

```rust
// 在 LlmGateway 中添加字段或通过 response JSON 提取
pub async fn chat(&self, messages: &[ChatMessage]) -> AgentResult<(String, TokenUsage)> {
    // ... 现有逻辑 ...

    // rig-core 的 Prompt trait 不返回 usage，需要改用 completion API
    match &self.config.provider {
        LlmProvider::OpenAI => {
            let agent = build_openai_agent(&self.config, preamble)?;
            // 使用 agent.completion() 替代 agent.prompt() 
            // completion 返回包含 usage 的完整 response
            let response = agent.completion(&prompt).await?;  // 需要查看 rig-core API
            let text = response.text();
            let usage = TokenUsage {
                prompt_tokens: response.usage.prompt_tokens,
                completion_tokens: response.usage.completion_tokens,
            };
            Ok((text, usage))
        }
        // ... 其他 provider ...
    }
}
```

**备选方案（更可靠）：** 如果 rig-core 不支持提取 token，则通过 HTTP 直接调用 provider API，在获得完整 response JSON 后解析 `usage` 字段。这需要处理 API key、base URL 等配置，但代码更可控。

### 2. Agent 层改造（`src/agent.rs`）

将 `StepOutcome` 中的 `Advanced` 和 `Finished` 变体中加入 token usage 可选值。

**现状：**
```rust
pub enum StepOutcome {
    Advanced,
    Finished,
    WaitingForInput(String),
    Failed(String),
}
```

**改造后：**
```rust
pub enum StepOutcome {
    Advanced,
    Finished { usage: Option<TokenUsage> },
    WaitingForInput(String),
    Failed(String),
}
```

### 3. 进度显示增强（`src/cli/progress.rs`）

新增功能：
- 支持在步骤结束后显示 token 消耗标签
- 支持流式 token 更新（通过 `print!` + `\r` 实现单行动态更新）

```rust
impl StepProgress {
    /// Display a token badge for the last completed step.
    pub fn token_badge(usage: &TokenUsage) {
        println!("  {}  ↑ {} · ↓ {} tok",
            style("■").dim(),
            usage.prompt_tokens,
            usage.completion_tokens,
        );
    }
}
```

### 4. REPL 改造（`src/main.rs`）

#### 4.1 REPL 状态追踪

新增 `ReplSession` 结构体，在 REPL 循环中维护：

```rust
struct ReplSession {
    /// Total tokens consumed across all exchanges in this session.
    total_tokens: u32,
    /// When the session started.
    started_at: Instant,
    /// Current model display string.
    model_label: String,
    /// Live plan count.
    plan_count: usize,
}
```

在 `run_repl_sync` 中维护这个状态，每次 LLM 调用后更新 token 计数。

#### 4.2 输入栏底部布局

在每次 prompt 输出时，打印三行：

```
❯ _
⌘ /status — view system health overview               ● deepseek-v4-flash | 1.6k tok | ⏱ 12:34 | 📦 14 plans
```

实现方式：rustyline 的 `readline` 之前，先打印提示行（第 2 行）。由于 rustyline 会覆盖当前行，需要在调用 `readline` 之前打印好状态栏。

```rust
// 在 readline 调用之前
let status_bar = format!(
    "⌘ {} — {}  {spacer}  ● {} | {} tok | ⏱ {} | 📦 {} plans",
    shortcut_cmd, shortcut_hint,
    model, total_tokens, elapsed_str, plan_count,
);
// 用 println 提前打印，然后 rustyline 在其上方渲染 prompt
println!("{}", status_bar);
```

但 rustyline 会使用当前行作为输入，所以状态栏应该打印在 `rl.readline()` 之前，rustyline 会在下一行渲染 `> ` prompt。

实际上，由于 user 输入之前有一行状态栏，在 `rl.readline()` 调用之前直接 `println!` 即可。每次迭代循环都会打印新的状态栏。

#### 4.3 快捷指令轮播

8 条指令轮流显示，每条 3 秒。在 status bar 中用计数器控制：

```rust
const SHORTCUTS: &[(&str, &str)] = &[
    ("/status", "view system health overview"),
    ("/model show", "check your current AI model"),
    ("/session list", "browse your past plans"),
    ("/doctor", "diagnose your environment"),
    ("/logs 10", "peek at recent agent logs"),
    ("/config set", "configure API keys & settings"),
    ("/help", "list all available commands"),
    ("/quit", "exit Rupoo"),
];

fn shortcut_at(epoch_secs: u64) -> &'static (&'static str, &'static str) {
    let idx = (epoch_secs / 3) as usize % SHORTCUTS.len();
    &SHORTCUTS[idx]
}
```

每次循环的 `readline()` 前根据当前时间戳决定显示哪条。

#### 4.4 Token 显示格式

```rust
// 每次 LLM 返回后
if let Some(usage) = &step_outcome.usage {
    session.total_tokens += usage.prompt_tokens + usage.completion_tokens;
    // 输出到进度显示
    let prompt = style(usage.prompt_tokens.to_string()).cyan();
    let completion = style(usage.completion_tokens.to_string()).yellow();
    println!("  {}  ↑ {} · ↓ {} tok  {}", 
        style("■").dim(), prompt, completion, style("(this turn)").dim(),
    );
}
```

累计 token 显示在底部状态栏：
```rust
// 格式化累计 token
let tok_str = if session.total_tokens >= 1000 {
    format!("{:.1}k", session.total_tokens as f64 / 1000.0)
} else {
    format!("{}", session.total_tokens)
};
```

### 5. 标准库（纯 Rust，零新依赖）

| 功能 | 实现方式 |
|------|----------|
| 轮播计时 | `std::time::SystemTime::now()` 计算 epoch secs |
| 会话计时 | `std::time::Instant` 记录开始时间 |
| Token 累计 | 普通 `u32` 累加 |
| 流式 token | `print!(\r...)` + `flush()` 原地更新 |

---

## 实施顺序

```
Step 1: LlmGateway 返回 token usage（修改 llm.rs + agent.rs）
Step 2: ReplSession 状态追踪（修改 main.rs）
Step 3: 状态栏 + 快捷指令轮播（修改 main.rs）
Step 4: Token 显示集成（修改 progress.rs + main.rs）
Step 5: 测试 + 修复 + 安装
```

每个 Step 可独立验证。

---

## 不做的事

| 功能 | 原因 |
|------|------|
| 键盘快捷键执行快捷指令 | 超出当前需求，用户点击轮播提示后手动输入 |
| 持久化 token 统计 | 跨 session 的 token 统计需要独立存储，scope 过大 |
| 实时 token 价格换算 | 需要维护 provider 价格表，超出当前需求 |
| 多行状态栏动画 | 当前用简单文本切换，足够清晰 |
