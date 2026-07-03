# Rupoo — AI-powered Terminal Assistant

[中文版本](README_CN.md) | English

Rupoo is a terminal-based AI assistant with a native REPL interface, featuring syntax-highlighted code blocks, Markdown rendering, theme switching, and Claude Code–style tool call display — driven by a triple-mode agent engine (Chat + Plan + Loop).

```
Version:   0.5.0          Language: Rust 2021
Lines:     ~48,000        Tests:    275 ✅
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
| **Triple-Mode Agent** | Chat Mode + Plan Mode + Loop Mode |
| **Loop Engineering** | Adaptive iterative execution: execute → evaluate → correct → repeat |
| **Recursive Decomposition** | Auto-break complex goals into independent sub-tasks |
| **7 Step Types** | Think, ToolCall, WaitForInput, Exec, HttpRequest, BrowserAction, Finish |
| **Long-term Memory** | SQLite FTS5 full-text search + Vector semantic search |
| **Hybrid Search** | Combines FTS5 keyword matching + Vector semantic understanding |
| **Memory Toggle** | Enable/disable memory with `/memory on/off` |
| **Deep Search Toggle** | Enable/disable hybrid search with `/deep on/off` |
| **Skill System** | JSON-based skills with auto-learning capability |
| **Crash Recovery** | Heartbeat checkpoints with transactional atomicity |
| **Five-Layer Pipeline** | Orchestrator with Cognitive → Planner → Supervisor → Execution → Memory layers |
| **Trait-Based Architecture** | All core layers defined by traits for mock testing and provider swaps |
| **Cognitive Engine** | LLM-powered goal parsing with safety boundary detection and task decomposition |
| **Supervisor 3-Gate** | Compliance checker → Confidence checker → Circuit breaker serial intercept |
| **Memory System Bridge** | Unified `MemorySystem` trait bridging legacy store with new architecture |

| **IM Channel Integration** | Feishu (飞书) / DingTalk (钉钉) channel support with WebSocket persistent connection |

---

## 🔌 Channel Integration

Rupoo can run as an IM bot alongside the terminal CLI, supporting Feishu and DingTalk.

```bash
# One-click configuration (auto-validate + write config)
rupoo feishu          # Setup Feishu channel
rupoo dingtalk        # Setup DingTalk channel  
rupoo channels        # List configured channels

# Start service
rupoo serve           # Foreground
rupoo serve -d        # Background daemon
rupoo serve-stop      # Stop daemon
rupoo serve-status    # Check daemon status
```

### Channel Features

| Feature | Description |
|---------|-------------|
| **WebSocket Persist Connection** | Real-time event subscription via long connection |
| **Auto Reconnect** | Exponential backoff (2s → 60s max) |
| **Session Persistence** | LRU-cached conversation history per sender |
| **System Prompt Isolation** | `[agents.feishu]` / `[agents.dingtalk]` profile in config.toml |
| **Slash Commands** | `/new`, `/help`, `/status`, `/search <keyword>` |
| **Memory Source Tagging** | CLI memories (`source=agent`) isolated from channel memories (`source=channel`) |
| **Cross-Source Memory Query** | `/search` searches across all memory sources |
| **Rich Reactions** | 🔨 for code tasks, 👀 for chat, ✅ on completion |
| **Graceful Shutdown** | SIGTERM/SIGINT handles ongoing tasks |
| **Daemon Mode** | Background process with PID management |

---

---

## 🚀 Quick Start

### Download Pre-built Binary

Download the archive for your platform from the [latest release](https://github.com/Steventsang18/rupoo/releases/latest):

| Platform | Download |
|----------|----------|
| 🍎 **macOS Apple Silicon** (M1/M2/M3/M4) | `rupoo-v0.5.0-aarch64-apple-darwin.tar.gz` |
| 🍎 **macOS Intel** | `rupoo-v0.5.0-x86_64-apple-darwin.tar.gz` |
| 🐧 **Linux x86_64** | `rupoo-v0.5.0-x86_64-unknown-linux-gnu.tar.gz` |
| 🐧 **Linux ARM64** (Raspberry Pi, AWS Graviton) | `rupoo-v0.5.0-aarch64-unknown-linux-gnu.tar.gz` |
| 🪟 **Windows x86_64** | `rupoo-v0.5.0-x86_64-pc-windows-msvc.zip` |

#### macOS

```bash
# Replace <file> with your platform's archive
tar xzf rupoo-v0.5.0-aarch64-apple-darwin.tar.gz
mv rupoo /usr/local/bin/
# Verify
rupoo --help
```

> ⚠️ If you see "rupoo cannot be opened because the developer cannot be verified", run:
> `xattr -d com.apple.quarantine /usr/local/bin/rupoo`

#### Linux

```bash
# x86_64
tar xzf rupoo-v0.5.0-x86_64-unknown-linux-gnu.tar.gz
sudo mv rupoo /usr/local/bin/

# ARM64 (e.g. Raspberry Pi)
tar xzf rupoo-v0.5.0-aarch64-unknown-linux-gnu.tar.gz
sudo mv rupoo /usr/local/bin/
```

#### Windows

1. Download `rupoo-v0.5.0-x86_64-pc-windows-msvc.zip`
2. Extract the archive
3. Move `rupoo.exe` to a directory in your `PATH` (e.g., `C:\Windows\System32\` or create a custom path)
4. Open a new terminal and run `rupoo --help`

> 💡 Or use PowerShell: `Expand-Archive rupoo-v0.5.0-x86_64-pc-windows-msvc.zip -DestinationPath C:\Users\YourName\bin`

### Install from Source

```bash
# Prerequisites: Rust toolchain (https://rustup.rs)
cargo install --path src-agent

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

## 🎯 New in v0.5.0 — Five-Layer Pipeline Architecture

v0.5.0 introduces a major architecture overhaul: a formal five-layer pipeline that replaces
the monolithic agent core with well-defined, independently-testable layers.

### Five-Layer Orchestrator

```
User Input
    │
    ├─ Layer 1: Cognitive Engine  ── parse goal, safety check, decompose
    ├─ Layer 2: Planner           ── generate alternatives, score, select best
    ├─ Layer 3: Supervisor        ── 3-gate intercept (compliance → confidence → circuit-breaker)
    ├─ Layer 4: Execution Engine  ── validate input, run steps, detect replan
    └─ Layer 5: Memory System     ── short-term / long-term / episodic recall
```

### Trait-Based Architecture

Each layer is defined by a trait, enabling mock testing and future provider swaps:

- **`CognitiveEngine`** — parse raw instructions into `AgentGoal`, detect safety boundary violations, decompose complex goals
- **`Planner`** — generate alternative execution plans, score by success probability / cost / risk
- **`Supervisor`** — three-gate serial intercept: ComplianceChecker → ConfidenceChecker → CircuitBreaker
- **`ExecutionEngine`** — step validation with type-aware parameter checking and replan triggers
- **`MemorySystem`** — unified trait over short-term, long-term, and episodic stores with hybrid recall

### Supervisor 3-Gate Protection

```
Action → Gate 1: Compliance (forbidden-command filter)
       → Gate 2: Confidence (semantic confidence threshold)
       → Gate 3: Circuit Breaker (failure-rate threshold)
       → Approved / Blocked
```

### Memory System Bridge

The `MemorySystemBridge` wraps the legacy `MemoryStore` behind the new `MemorySystem`
trait, providing backward compatibility while enabling the unified recall path.

### Quality & Safety

- **239 unit tests** + **4 integration tests** + **9 doc tests** = **275 total**
- **Clippy clean** — zero warnings across all targets
- **Hygiene fixes**: memory leak patched (vector store `remove()`), placeholder implementations emit runtime warnings, `clippy --fix` applied project-wide
- **Safety alignment**: `SafetyContext` now reads config file defaults and merges with runtime rules

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
| `/loop <goal>` | Start adaptive iterative loop |
| `/loop status <id>` | Show loop status |
| `/loop list` | List all loops |
| `/loop pause\|resume\|cancel` | Manage running loops |
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
| `loops [start\|status\|list\|pause\|resume\|cancel]` | Loop engineering |
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
User Layer:  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
             │  Chat    │  │  Plan    │  │  Loop    │  │ Commands │  │  Skills  │
             │  Mode    │  │  Mode    │  │  Mode    │  │  System  │  │  System  │
             └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘
                  │             │             │             │             │
         ┌────────┴─────────────┴─────────────┴─────────────┴─────────────┴────────┐
         │                           Orchestrator                                  │
         │  ┌──────────────────────────────────────────────────────────────────┐   │
         │  │ Layer 1: CognitiveEngine  (parse → safety-scan → decompose)     │   │
         │  │ Layer 2: Planner          (generate → score → select)           │   │
         │  │ Layer 3: Supervisor       (compliance → confidence → breaker)   │   │
         │  │ Layer 4: ExecutionEngine  (validate → execute → replan)         │   │
         │  │ Layer 5: MemorySystem     (short-term / long-term / episodic)   │   │
         │  └──────────────────────────────────────────────────────────────────┘   │
         └───────────────────────────────┬──────────────────────────────────────────┘
                                         │
         ┌───────────────────────────────▼──────────────────────────────────────────┐
         │                         Agent Core Layer                                │
         │  ┌──────────┐ ┌──────────────┐ ┌────────────────┐ ┌──────────────────┐ │
         │  │ Agent    │ │ LoopEngine   │ │ MemorySystem   │ │ LLM Gateway      │ │
         │  │ (bridge) │ │ (chat+plan)  │ │ Bridge + Store  │ │ (multi-provider) │ │
         │  └──────────┘ └──────────────┘ └────────────────┘ └──────────────────┘ │
         └──────────────────────────────────────────────────────────────────────────┘
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
