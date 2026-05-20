# Rupoo — AI-powered Terminal Assistant

Rupoo is a terminal-based AI assistant that supports plan execution, skill management, long-term memory, a secure sandbox, Git integration, and the MCP protocol — all through natural language or TUI interaction.

```
Version:  0.2.0        Language: Rust 2021
Tests:    106 ✅       Binary:   ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Safety:  path_jail sandbox + SSRF protection
```

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
# Interactive TUI (default)
rupoo

# TUI keyboard shortcuts
# Ctrl+P   Command palette
# Ctrl+C   Exit
# Tab      Switch focus (input area ↔ sidebar)
# ↑/↓      Input history
# Shift+↑/↓   Scroll chat area (or mouse wheel)
# PgUp/PgDn   Scroll by larger increments
```

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
| _(none)_ | Launch the interactive TUI (three-column layout) |
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
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  Subcommands (status/model/session/doctor/logs…)│
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent State Machine                                      │
│  Think → ToolCall → WaitForInput → Finish               │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                  │
│  Anthropic / OpenAI / Ollama unified interface           │
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                     │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls)  │
│  + MCP Server (JSON-RPC stdio)                          │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                           │
│  path_jail sandbox · Command blocklist · SSRF protection │
│  · Timeout protection                                    │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  Plan persistence · Checkpoint crash recovery · Session  │
│  history · Long-term memory                              │
└──────────────────────────────────────────────────────────┘
```

### Module Overview

| Module | Lines | Responsibility |
|--------|-------|----------------|
| `main.rs` | 700+ | CLI entry point, command dispatch, `build_engine` |
| `agent.rs` | 840+ | Agent state machine, 7 Step types, crash recovery |
| `db.rs` | 890 | SQLite layer, Plan CRUD + Checkpoints + FTS5 memory |
| `llm.rs` | 350 | LLM gateway, unified Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | TUI event loop, Agent bridge thread |
| `cli/app.rs` | 370 | TUI application state, session management, message routing |
| `cli/ui.rs` | 420 | TUI rendering: three-column layout, bubbles, code blocks, status bar |
| `cli/handlers.rs` | 380 | Input mode strategies (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | Security sandbox, path_jail, SSRF, command blocklist |
| `mcp.rs` | 250+ | MCP Tool dispatcher + JSON-RPC client |
| `mcp_server.rs` | 380 | MCP server (reuses McpToolExecutor) |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir tools |
| `task.rs` | 340 | Step/Plan/Checkpoint type definitions |
| `memory.rs` | 140 | Long-term memory (FTS5 full-text search) |
| `skill.rs` | 390 | Skill system (JSON files + auto-learning) |
| `git.rs` | 240 | Git integration (git2 + gh CLI) |
| `error.rs` | 34 | Unified error types |

### Security Architecture

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

## Core Features

### Plan Execution Engine

Supports 7 step types:

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

### TUI

- **Three-column layout**: Session list on the left, chat area in the center, status panel on the right
- **Message bubbles**: Three colors distinguish user / assistant / system messages
- **Code block highlighting**: Code rendered with borders and pre-wrapping
- **Input history**: ↑/↓ navigates through the last 100 inputs
- **Auto-scroll**: New messages auto-scroll to the bottom; manual scroll resets after sending a new message
- **Adaptive layout**: Automatically re-layouts and re-wraps when terminal size changes

### Skill System

- **JSON file management**: `~/.skills/*.json`
- **Built-in skills**: code-review, generate-readme
- **Auto-learning**: Automatically extracted as a reusable skill after Plan execution completes
- **Manual learning**: `rupoo skills learn <plan_id> <skill_name>`

### Long-term Memory

- **FTS5 full-text search**: Supports BM25 relevance ranking
- **Session persistence**: SQLite stores UI session history
- **Context injection**: Think steps automatically retrieve relevant memories

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| tokio | Async runtime |
| clap | CLI argument parsing |
| ratatui + crossterm | TUI framework |
| rig-core 0.30 | Multi-provider LLM gateway |
| rusqlite (WAL + FTS5) | SQLite database |
| git2 | Git operations |
| reqwest | HTTP client |
| path_jail | File path security |
| tui-textarea | TUI input component |
| serde + serde_json | Serialization |
| tracing + tracing-subscriber | Logging |
| uuid | Plan / Step IDs |
| chrono | Timestamps |
| crossbeam-channel | Cross-thread communication |

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

106 tests covering:
- 54 unit tests (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 main crate tests (CLI commands + TUI handler)
- 4 CLI-DB integration tests
- 2 crash recovery integration tests
- 13 DB integration tests

---

## Building

```bash
# Development build
cargo build

# Release build (recommended)
cargo build --release

# With GUI support
cargo build --release --features gui

# Binary size
# ~14 MB (release, ARM64)
```

---

## License

MIT
