# Rupoo — AI-powered Terminal Assistant

Rupoo is a terminal-based AI assistant with a native REPL interface, featuring syntax-highlighted code blocks, Markdown rendering, theme switching, and Claude Code–style tool call display — all driven by a dual-mode agent engine (Chat + Plan).

```
Version:   0.3.0          Language: Rust 2021
Lines:     12,578         Tests:    67 ✅
Interface: Native REPL    LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:        SQLite (FTS5)  Safety:   path_jail sandbox + SSRF protection
```

---

## What's New in v0.3

| Area | Change |
|------|--------|
| **Interface** | Replaced ratatui TUI with native REPL — smooth scrolling, resize-safe, no frame buffer |
| **Code Highlighting** | syntect-powered syntax highlighting with 3 themes (base16-ocean.dark / InspiredGitHub / base16-mocha.dark) |
| **Markdown Rendering** | Tables, blockquotes, task lists, ordered lists, links, horizontal rules |
| **Theme System** | `/theme dark\|light\|monokai` with persistent DB storage; cursor color follows theme |
| **Chat Bubbles** | User messages right-aligned (▸), AI left-aligned (◂), clear visual separation |
| **Tool Cards** | Claude Code–style `╭─🔧──╮` folding cards for tool calls |
| **Thinking Chain** | Coze-style spinner + streaming bubble for AI reasoning |
| **Streaming Code** | Two-phase render: `│` placeholders during stream → syntect rewrite on completion |
| **History Search** | `Ctrl+R` incremental search with `~/.rupoo/history.txt` persistence (1000 entries) |
| **Color Palette** | 12 RGB constants per theme (GitHub Dark Dimmed + Catppuccin Mocha), no more `dimmed()` |
| **Input Editing** | rustyline Emacs mode: arrow keys, Home/End, Ctrl+A/E, green blinking bar cursor |

---

## Quick Start

### Installation

```bash
# Install from source
cargo install --path .

# Or run the compiled binary directly
cargo run --release
```

### Configure LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek and other compatible APIs
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama local models
# No API key needed — Ollama defaults to http://localhost:11434
```

### Launch

```bash
# Interactive REPL (default)
rupoo
```

#### REPL Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `↑` / `↓` | Navigate input history |
| `Ctrl+R` | Incremental history search |
| `Ctrl+A` / `Home` | Move cursor to start |
| `Ctrl+E` / `End` | Move cursor to end |
| `←` / `→` | Move cursor left/right |
| `Ctrl+C` | Cancel current operation |
| `Ctrl+D` | Exit |

#### REPL Commands

| Command | Description |
|---------|-------------|
| `/new` | Start a new conversation |
| `/model` | Switch LLM model |
| `/plan` | Enter Plan Mode |
| `/theme dark\|light\|monokai` | Switch color theme |
| `?` | Show help |

---

## Command Line Interface

```
rupoo [OPTIONS] [COMMAND]
```

### Global Options

| Option | Description |
|--------|-------------|
| `--verbose` | Output debug logs to stderr |

### Subcommands

| Command | Description |
|---------|-------------|
| _(none)_ | Launch the interactive REPL |
| `run --task <id>` | Execute a saved Plan |
| `demo` | Run the built-in demo Plan |
| `status [--short]` | Display system status overview |
| `model [show\|list\|set]` | View/switch LLM providers and models |
| `session [list\|show\|resume\|delete\|prune]` | Manage execution plans |
| `skills [list\|show\|run\|install-builtin\|learn]` | Skill system management |
| `config [set\|get\|list]` | Configuration and API key management |
| `git [status\|commit\|pr]` | Git integration |
| `doctor [--fix]` | Diagnose environment and configuration issues |
| `logs [--follow] [--lines N] [--level LEVEL]` | View runtime logs |
| `mcp-server` | Start an MCP protocol server (JSON-RPC over stdio) |
| `serve --port <port>` | Server mode |

---

## Architecture

```
┌─ CLI (clap) ──────────────────────────────────────────────┐
│  rupoo  →  Native REPL (rustyline + owo-colors)           │
│         →  Subcommands (status/model/session/doctor/logs…) │
└──────────────────────┬────────────────────────────────────┘
                       │
┌──────────────────────▼────────────────────────────────────┐
│  Agent State Machine                                       │
│  Think → ToolCall → WaitForInput → Finish                 │
│  + Exec / HttpRequest / BrowserAction / Search            │
├───────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core 0.30)                              │
│  Anthropic / OpenAI / Ollama unified interface            │
├───────────────────────────────────────────────────────────┤
│  Output Layer                                             │
│  theme.rs → output.rs → markdown.rs → syntect highlighting│
│  Chat bubbles · Tool cards · Thinking chain · Code blocks │
├───────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                      │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls)   │
│  + MCP Server (JSON-RPC stdio)                           │
├───────────────────────────────────────────────────────────┤
│  SafetyContext                                            │
│  path_jail sandbox · Command blocklist · SSRF protection  │
│  · Timeout protection                                     │
├───────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                      │
│  Plan persistence · Checkpoint crash recovery · Session   │
│  history · Long-term memory · Theme preferences           │
└───────────────────────────────────────────────────────────┘
```

### Module Overview

| Module | Lines | Responsibility |
|--------|-------|----------------|
| `agent.rs` | 1082 | Agent state machine, 7 Step types, crash recovery |
| `db.rs` | 1121 | SQLite layer, Plan CRUD + Checkpoints + FTS5 memory + theme |
| `llm.rs` | 1172 | LLM gateway, unified Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 733 | REPL event loop, Agent bridge thread |
| `cli/markdown.rs` | 540 | Markdown renderer: tables, blockquotes, task lists, links |
| `cli/output.rs` | 286 | Output formatting: chat bubbles, tool cards, thinking chain |
| `cli/theme.rs` | 161 | Theme system: Dark/Light/Monokai with 12 RGB constants |
| `cli/plan_mode.rs` | 297 | Plan Mode: interactive step execution |
| `cli/app.rs` | 307 | REPL application state, session management |
| `cli/bridge.rs` | 188 | Agent ↔ REPL bridge (crossbeam channels) |
| `cli/chat_mode.rs` | 121 | Chat Mode handler |
| `cli/approval.rs` | 133 | Tool approval workflow |
| `main_cli.rs` | 398 | CLI entry point, command dispatch |
| `safety.rs` | 364 | Security sandbox, path_jail, SSRF, command blocklist |
| `mcp.rs` | 421 | MCP Tool dispatcher + JSON-RPC client |
| `mcp_server.rs` | 400 | MCP server (reuses McpToolExecutor) |
| `rig_tools.rs` | 566 | Echo / FileRead / FileWrite / ListDir tools |
| `skill.rs` | 570 | Skill system (JSON files + auto-learning) |
| `task.rs` | 340 | Step/Plan/Checkpoint type definitions |
| `tools/browser.rs` | 461 | Browser automation (Navigate/Screenshot/Click/GetText) |
| `tools/search.rs` | 247 | Web search integration |
| `tools/network.rs` | 150 | HTTP request tool |
| `tools/terminal.rs` | 123 | Terminal command execution |
| `git.rs` | 241 | Git integration (git2 + gh CLI) |
| `memory.rs` | 143 | Long-term memory (FTS5 full-text search) |
| `executor.rs` | 138 | Step executor dispatch |
| `shared.rs` | 130 | Shared types and constants |
| `error.rs` | 33 | Unified error types |

---

## Theme System

Three built-in themes with persistent preference storage:

| Theme | Style | Code Highlighting | Cursor |
|-------|-------|-------------------|--------|
| `dark` (default) | GitHub Dark Dimmed + Catppuccin Mocha | base16-ocean.dark | `#3fb950` green |
| `light` | GitHub Light | InspiredGitHub | `#238636` green |
| `monokai` | Monokai | base16-mocha.dark | `#a6e22e` green |

Switch with `/theme dark|light|monokai` — preference persists across sessions.

### Color Palette (Dark Theme)

| Role | Color | Hex |
|------|-------|-----|
| User message | Green | `#7ee787` |
| User accent | Green | `#3fb950` |
| AI message | Blue | `#58a6ff` |
| AI accent | Blue | `#79c0ff` |
| Tool call | Purple | `#d2a8ff` |
| Thinking | Yellow | `#e3b341` |
| Error | Red | `#f85149` |
| Dim text | Gray | `#484f58` |
| Border | Gray | `#30363d` |

---

## Core Features

### Dual-Mode Agent Engine

| Mode | Trigger | Description |
|------|---------|-------------|
| **Chat Mode** | Default | Free-form conversation with streaming output |
| **Plan Mode** | `/plan` | Structured multi-step execution with checkpoints |

### 7 Step Types

| Step | Description |
|------|-------------|
| Think | LLM reasoning with FTS5 memory retrieval for context |
| ToolCall | Invoke built-in tools (file read/write, directory listing, Echo) |
| WaitForInput | Pause and wait for user input before continuing |
| Exec | Run external commands (restricted by the security sandbox) |
| HttpRequest | HTTP GET/POST requests (with SSRF protection) |
| BrowserAction | Browser automation (Navigate/Screenshot/Click/GetText) |
| Finish | Complete the plan, triggers automatic skill learning |

### Crash Recovery

- **Heartbeat Checkpoint**: Writes a Running-state checkpoint before long-running operations
- **Transactional atomicity**: `record_step_completion` updates Plan + Checkpoint in a single SQLite transaction
- **Three-tier recovery**: `reset_running_plans → get_last_checkpoint → resume point determined by state`

### Markdown Rendering

Full inline + block rendering:

- **Tables**: Aligned columns with `│` borders
- **Blockquotes**: `▎` left border + dim text
- **Task lists**: `☐` unchecked / `☑` checked
- **Ordered / unordered lists**: Indented with proper markers
- **Code blocks**: syntect syntax highlighting + line numbers
- **Inline code**: Background-highlighted spans
- **Links**: `[text](url)` parsed and colored
- **Horizontal rules**: `─` separator

### Streaming Code Blocks

Two-phase rendering eliminates flicker:

1. **Stream phase**: Fast `│` placeholders with plain text
2. **Completion phase**: Erase and rewrite with syntect highlighting + line numbers

### Skill System

- **JSON file management**: `~/.skills/*.json`
- **Built-in skills**: code-review, generate-readme
- **Auto-learning**: Automatically extracted as a reusable skill after Plan execution completes
- **Manual learning**: `rupoo skills learn <plan_id> <skill_name>`

### Long-term Memory

- **FTS5 full-text search**: Supports BM25 relevance ranking
- **Session persistence**: SQLite stores conversation history
- **Context injection**: Think steps automatically retrieve relevant memories

---

## Security Architecture

| Protection Layer | Implementation |
|------------------|----------------|
| Command blocklist | 20+ dangerous commands blocked (sudo, rm, mkfs, dd, etc.) |
| File path sandbox | `path_jail` crate — prevents `../../etc/passwd`, symlink escapes |
| SSRF protection | Blocks localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| Timeout protection | Command 30s / HTTP 30s / Browser 30s |
| Environment sanitization | Only PATH/HOME/USER/SHELL/LANG/TERM preserved |
| Output truncation | Command output 10K / file reads 4K |
| Multi-path security | Triple protection: McpToolExecutor + LLM Agent + MCP Server |

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| tokio | Async runtime |
| clap | CLI argument parsing |
| rustyline | REPL input with history + Ctrl+R search |
| owo-colors | Zero-cost RGB color output |
| syntect | Syntax highlighting (offline, 100+ languages) |
| rig-core 0.30 | Multi-provider LLM gateway |
| rusqlite (WAL + FTS5) | SQLite database |
| git2 | Git operations |
| reqwest | HTTP client |
| path_jail | File path security |
| serde + serde_json | Serialization |
| tracing + tracing-subscriber | Logging |
| uuid | Plan / Step IDs |
| chrono | Timestamps |
| crossbeam-channel | Cross-thread communication |
| indicatif | Progress bars and spinners |

---

## Testing

```bash
# Run all tests
cargo test

# Library tests only
cargo test --lib

# Integration tests only
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# Execute the demo plan
cargo run --release demo
```

67 tests covering:
- Agent state machine, DB CRUD, LLM gateway, MCP, Safety, Memory, Skills, Git, Tools

---

## Building

```bash
# Development build
cargo build

# Release build (recommended)
cargo build --release

# With GUI support
cargo build --release --features gui

# If project path contains non-ASCII characters:
CARGO_TARGET_DIR=/tmp/rupoo-target cargo build --release
```

---

## License

MIT
