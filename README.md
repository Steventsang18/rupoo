# Rupoo — AI-powered Terminal Assistant

[中文版本](README_CN.md) | English

Rupoo is a terminal-based AI assistant with a native REPL interface, featuring syntax-highlighted code blocks, Markdown rendering, theme switching, and Claude Code–style tool call display — all driven by a dual-mode agent engine (Chat + Plan).

```
Version:   0.4.0          Language: Rust 2021
Lines:     ~26,000        Tests:    110 ✅
Interface: Native REPL    LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:        SQLite (FTS5)  Memory:   Hybrid Search (FTS5 + Vector)
Safety:    path_jail sandbox + SSRF protection
```

---

## ✨ Features

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
| **Long-term Memory** | SQLite FTS5 full-text search + Vector semantic search |
| **Hybrid Search** | Combines FTS5 keyword matching + Vector semantic understanding |
| **Memory Toggle** | Enable/disable memory with `/memory on/off` |
| **Deep Search Toggle** | Enable/disable hybrid search with `/deep on/off` |
| **Skill System** | JSON-based skills with auto-learning capability |
| **Crash Recovery** | Heartbeat checkpoints with transactional atomicity |

---

## 🚀 Quick Start

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

---

## 🎯 New Features in v0.4.0

### Memory System

```bash
# View memory status
/memory

# Enable/disable memory
/memory on
/memory off

# List recent memories
/memory list

# Search memories
/memory search <keyword>
```

### Deep Search (Hybrid Search)

Deep Search combines FTS5 full-text search with vector semantic search for better relevance.

```bash
# Check deep search status
/deep

# Enable deep search (FTS5 + Vector)
/deep on

# Disable deep search (FTS5 only)
/deep off
```

#### How Hybrid Search Works

```
User Query
    │
    ├──► FTS5 Search (keyword matching)
    │         │ Fast, exact keyword matches
    │
    └──► Vector Search (semantic understanding)
              │ Understands intent and meaning

Combined Results (RRF ranking)
```

---

## ⌨️ Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `↑` / `↓` | Navigate history |
| `Ctrl+R` | Incremental search |
| `Ctrl+C` | Cancel operation |
| `Ctrl+D` | Exit |
| `Ctrl+L` | Clear screen |
| `Tab` | Auto-complete commands |
| `Ctrl+N` | New session |

---

## 📝 REPL Commands

| Command | Description |
|---------|-------------|
| `/new` | New conversation |
| `/model` | Switch LLM model |
| `/plan` | Enter Plan Mode |
| `/memory` | Memory management |
| `/memory on/off` | Enable/disable memory |
| `/memory list` | List recent memories |
| `/memory search <query>` | Search memories |
| `/deep` | Deep search status |
| `/deep on/off` | Enable/disable hybrid search |
| `/theme dark\|light\|monokai` | Switch theme |
| `?` or `/help` | Show help |

---

## 🔧 CLI Commands

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

## 🔒 Security

| Protection | Implementation |
|------------|----------------|
| Command Blocklist | 20+ dangerous commands blocked |
| Path Sandbox | `path_jail` prevents path traversal |
| SSRF Protection | Blocks localhost and internal IPs |
| Timeout Protection | 30s limits for commands/HTTP/browser |
| Environment Sanitization | Only safe env vars preserved |
| Output Truncation | Limits on command output and file reads |

---

## 🏗️ Building

```bash
# Development build
cargo build

# Release build (recommended)
cargo build --release

# With GUI support
cargo build --release --features gui
```

---

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run benchmarks
cargo bench
```

---

## 📊 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       User Layer (CLI/TUI)                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐   │
│  │  Chat    │  │  Plan    │  │ Commands │  │   Memory     │   │
│  │  Mode    │  │  Mode    │  │  System  │  │   System     │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └──────┬───────┘   │
└───────┼─────────────┼─────────────┼────────────────┼───────────┘
        │             │             │                │
┌───────▼─────────────▼─────────────▼────────────────▼───────────┐
│                      Agent Core Layer                          │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Agent + Memory System + LLM Gateway                    │   │
│  │  ┌──────────┐ ┌─────────────┐ ┌─────────────────────┐  │   │
│  │  │ TaskRepo │ │ MemoryStore  │ │ MemoryCache (LRU)   │  │   │
│  │  └──────────┘ └─────────────┘ └──────────────────────┘  │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 📈 Performance

| Metric | Target | Status |
|--------|--------|--------|
| Cold start | < 2s | ✅ |
| LLM call latency | < 10s | ✅ |
| Tool execution timeout | < 5s | ✅ |
| Memory search response | < 100ms | ✅ |
| Signal compression | < 50ms | ✅ |

---

## 📚 Documentation

- [User Guide](docs/USER_GUIDE.md) - Comprehensive user documentation
- [Performance](docs/PERFORMANCE.md) - Performance optimization details
- [CONTRIBUTING.md](CONTRIBUTING.md) - Contribution guidelines

---

## 📝 Changelog

See [CHANGELOG.md](CHANGELOG.md) for detailed version history.

---

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🙏 Acknowledgments

- [rig-core](https://github.com/gregpr07/rig) - LLM agent framework
- [syntect](https://github.com/trishume/syntect) - Syntax highlighting
- [rustyline](https://github.com/kknghk/rustyline) - Readline implementation
