# Rupoo REPL Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add shortcut carousel, status bar, and token consumption display to the REPL.

**Architecture:** Use `rig-core 0.30`'s built-in `Usage` struct (already in `CompletionResponse`). Add `TokenUsage` type, modify `LlmGateway::chat()` to return `(String, TokenUsage)`, wire into REPL state.

**Tech Stack:** Rust, rig-core 0.30, rustyline, console

---

### Task 1: Add TokenUsage type + modify LlmGateway

**Files:**
- Modify: `src/llm.rs`
- Modify: `src/agent.rs`

**Changes:**

Add `TokenUsage` type to `src/llm.rs`:
```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}
```

Change `LlmGateway::chat()` return type from `AgentResult<String>` to `AgentResult<(String, TokenUsage)>`.

In each provider match arm, use `agent.prompt_request(prompt).extended_details().await` instead of `agent.prompt(prompt).await`. This returns `PromptResponse` with `total_usage: Usage`.

```rust
// Before:
let response = agent.prompt(&prompt).await;
let text = response.map_err(|e| ...)?;

// After:
let response = agent.prompt_request(prompt)
    .extended_details()
    .await
    .map_err(|e| AgentError::Other(format!("LLM request failed: {e}")))?;
let text = response.output;
let usage = TokenUsage {
    prompt_tokens: response.total_usage.input_tokens as u32,
    completion_tokens: response.total_usage.output_tokens as u32,
};
Ok((text, usage))
```

Change the per-provider builder functions to return `Agent<CompletionModel>` instead of `impl Prompt`:

- `build_anthropic_agent` → `Agent<AnthropicCompletionModel>`
- `build_openai_agent` → `Agent<OpenAICompletionModel>`
- `build_ollama_agent` → `Agent<OllamaCompletionModel>`

Update all callers in `chat()` to match the new return type.

Update `StepOutcome`:
```rust
pub enum StepOutcome {
    Advanced,
    Finished { usage: Option<TokenUsage> },
    WaitingForInput(String),
    Failed(String),
}
```

Update `Agent::run_next_step` to propagate usage info.

Update `crate::task` module to expose `Usage` if needed, or keep it in `llm.rs`.

Update all tests that call `chat()` or match on `StepOutcome`.

Commit after tests pass.

---

### Task 2: Update progress.rs for token display

**Files:**
- Create/Move: progress.rs already exists, modify it

**Changes:**

Add a static method to display token badge:
```rust
impl StepProgress {
    pub fn token_badge(usage: &TokenUsage, total: u32) {
        println!("  {}  ↑ {} · ↓ {} tok  {} {} tok",
            style("■").dim(),
            style(usage.prompt_tokens.to_string()).cyan(),
            style(usage.completion_tokens.to_string()).yellow(),
            style("(total").dim(),
            style(total).dim(),
            style(")").dim(),
        );
    }
}
```

---

### Task 3: Implement REPL session state + status bar + shortcut carousel

**Files:**
- Modify: `src/main.rs`

**Changes:**

Add `ReplSession` struct for tracking session state:
```rust
struct ReplSession {
    total_tokens: u32,
    started_at: std::time::Instant,
    model_label: String,
    plan_count: usize,
}
```

Add shortcut rotation array:
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

fn current_shortcut() -> &'static (&'static str, &'static str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let idx = (secs / 3) as usize % SHORTCUTS.len();
    &SHORTCUTS[idx]
}
```

In `run_repl_sync`, before `rl.readline()`, print the status bar line:

```rust
// Before readline
let shortcut = current_shortcut();
let elapsed = session.started_at.elapsed();
let elapsed_str = format!("{:02}:{:02}", elapsed.as_secs() / 60, elapsed.as_secs() % 60);
let tok_str = if session.total_tokens >= 1000 {
    format!("{:.1}k", session.total_tokens as f64 / 1000.0)
} else {
    format!("{}", session.total_tokens)
};
println!("  ⌘ /{} — {}  {}{}  ● {} | {} tok | ⏱ {} | 📦 {} plans",
    shortcut.0, shortcut.1,
    style("│").dim(), style("│").dim(),  // spacer that matches the nav dots
    session.model_label, tok_str, elapsed_str, session.plan_count,
);
// rustyline reads on the NEXT line
```

After NL execution, update `session.total_tokens`:
```rust
// After execute_nl returns, update tokens
// (this requires execute_nl to return usage info)
```

Run `cargo build`, `cargo test`, install.

---

### Task 4: Wire everything together — token flow end-to-end

**Files:**
- Modify: `src/main.rs`

**Changes:**

1. `execute_nl` returns `Result<String>`. Change to return `Result<(String, Option<TokenUsage>)>` so the REPL can track tokens.

2. REPL loop:
```rust
// Before:
let result = rt_handle.block_on(execute_nl(&state, &text))?;

// After:
let (result, usage) = rt_handle.block_on(execute_nl_with_usage(&state, &text))?;
if let Some(u) = usage {
    session.total_tokens += u.prompt_tokens + u.completion_tokens;
    StepProgress::token_badge(&u, session.total_tokens);
}
```

3. Update plan count after NL queries:
```rust
session.plan_count = rt_handle.block_on(state.repo.list_plans(1, 0))
    .map(|plans| plans.len())
    .unwrap_or(0);
```

---

### Task 5: Integration check

- `cargo build` — 0 warnings
- `cargo test` — all tests pass
- `cargo install --path .`
- Verify REPL: `rupoo` → banner → `/status` works → NL query shows progress + token badge
