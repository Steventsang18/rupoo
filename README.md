# Rupoo — AI-powered Terminal Assistant

Rupoo is a terminal-based AI assistant with a native REPL interface, featuring syntax-highlighted code blocks, Markdown rendering, theme switching, and Claude Code–style tool call display — all driven by a dual-mode agent engine (Chat + Plan).

```
Version:   0.3.0          Language: Rust 2021
Lines:     25,493         Tests:    96 ✅
Interface: Native REPL    LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:        SQLite (FTS5)  Safety:   path_jail sandbox + SSRF protection
```

---

## Features

| Feature | Description |
|---------|-------------|
| **Native REPL** | Smooth scrolling, resize-safe, no frame buffer |
| **Syntax Highlighting** | syntect-powered with 3 themes (ocean / GitHub / mocha) |
| **Markdown Rendering** | Tables, blockquotes, task lists, code blocks, links |
| **Theme System** | `/theme dark\|light\|monokai` with persistent storage |
| **Chat Bubbles** | User right-aligned (▸), AI left-aligned (◂) |
| **Tool Cards** | Claude Code–style folding cards for tool calls |
| **Thinking Chain** | Streaming spinner + bubble for AI reasoning |
| **History Search** | `Ctrl+R` incremental search (1000 entries) |
| **Dual-Mode Agent** | Chat Mode + Plan Mode with checkpoints |
| **7 Step Types** | Think, ToolCall, WaitForInput, Exec, HttpRequest, BrowserAction, Finish |
| **Long-term Memory** | FTS5 full-text search with automatic context injection |
| **Skill System** | JSON-based skills with auto-learning capability |
| **Crash Recovery** | Heartbeat checkpoints with transactional atomicity |

---

## Quick Start

### Installation

```bash
# Install from source
cargo install --path .

# Or run directly
cargo run --release
```

### Configure LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama (no API key needed)
rupoo config set active_provider ollama
rupoo config set model.ollama llama3
```

### Launch

```bash
# Interactive REPL (default)
rupoo
```

#### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `↑` / `↓` | Navigate history |
| `Ctrl+R` | Incremental search |
| `Ctrl+C` | Cancel operation |
| `Ctrl+D` | Exit |

#### REPL Commands

| Command | Description |
|---------|-------------|
| `/new` | New conversation |
| `/model` | Switch LLM model |
| `/plan` | Enter Plan Mode |
| `/theme dark\|light\|monokai` | Switch theme |
| `?` | Show help |

---

## CLI Commands

```
rupoo [OPTIONS] [COMMAND]
```

| Command | Description |
|---------|-------------|
| _(none)_ | Launch interactive REPL |
| `demo` | Run built-in demo |
| `status` | System status overview |
| `model [show\|list\|set]` | Manage LLM providers |
| `session [list\|show\|resume\|delete]` | Manage plans |
| `skills [list\|show\|install-builtin]` | Skill management |
| `config [set\|get\|list]` | Configuration management |
| `git [status\|commit\|pr]` | Git integration |
| `doctor [--fix]` | Diagnose issues |
| `logs [--follow]` | View runtime logs |
| `mcp-server` | Start MCP protocol server |

---

## Security

| Protection | Implementation |
|------------|----------------|
| Command Blocklist | 20+ dangerous commands blocked |
| Path Sandbox | `path_jail` prevents path traversal |
| SSRF Protection | Blocks localhost and internal IPs |
| Timeout Protection | 30s limits for commands/HTTP/browser |
| Environment Sanitization | Only safe env vars preserved |
| Output Truncation | Limits on command output and file reads |

---

## Building

```bash
# Development build
cargo build

# Release build (recommended)
cargo build --release

# With GUI support
cargo build --release --features gui
```

---

## Testing

```bash
# Run all tests
cargo test

# Run demo
cargo run --release demo
```

---

## License

MIT