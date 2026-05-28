# Contributing to Rupoo

Thank you for your interest in contributing to Rupoo!

## Development Setup

```bash
# Clone the repository
git clone https://github.com/Steventsang18/rupoo
cd rupoo

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build the project
cargo build

# Run tests
cargo test

# If project path contains non-ASCII characters:
CARGO_TARGET_DIR=/tmp/rupoo-target cargo build --release

# Run with a specific model provider
ANTHROPIC_API_KEY=sk-ant-... cargo run --release
```

## Project Structure

```
src/
├── main.rs              # Binary entry, feature-gated dispatch (GUI vs CLI)
├── main_cli.rs          # CLI entry point, command dispatch
├── lib.rs               # Crate root, re-exports shared types
├── build_engine.rs      # Engine bootstrap (LLM gateway + DB + agent)
├── agent.rs             # Agent state machine: 7 step types, crash recovery
├── db.rs                # SQLite (WAL + FTS5): Plan CRUD, checkpoints, memories, themes
├── llm.rs               # LLM gateway: unified interface for Anthropic/OpenAI/Ollama
├── executor.rs          # Step executor dispatch
├── cli/
│   ├── mod.rs           # REPL event loop, Agent bridge thread
│   ├── app.rs           # App state, session management, message routing
│   ├── bridge.rs        # Agent ↔ REPL bridge (crossbeam channels)
│   ├── chat_mode.rs     # Chat Mode handler
│   ├── plan_mode.rs     # Plan Mode: interactive step execution
│   ├── approval.rs      # Tool approval workflow
│   ├── output.rs        # Output formatting: chat bubbles, tool cards, thinking chain
│   ├── markdown.rs      # Markdown renderer: tables, blockquotes, task lists, links
│   ├── theme.rs         # Theme system: Dark/Light/Monokai with 12 RGB constants
│   ├── handlers.rs      # Input mode strategies
│   └── cmds/            # CLI subcommands: status, model, session, doctor, logs...
├── tools/
│   ├── browser.rs       # Browser automation (Navigate/Screenshot/Click/GetText)
│   ├── search.rs        # Web search integration
│   ├── network.rs       # HTTP request tool
│   └── terminal.rs      # Terminal command execution
├── safety.rs            # Sandboxing: path_jail, command blacklist, SSRF protection
├── mcp.rs               # MCP tool dispatcher + JSON-RPC client
├── mcp_server.rs        # MCP protocol server (JSON-RPC over stdio)
├── rig_tools.rs         # Built-in tools: Echo, FileRead, FileWrite, ListDir
├── skill.rs             # Skill system: JSON files, auto-learn from plans
├── memory.rs            # Long-term memory with FTS5 full-text search
├── git.rs               # Git integration via git2 + gh CLI
├── task.rs              # Step/Plan/Checkpoint type definitions
├── shared.rs            # Shared types between agent core and REPL
├── error.rs             # Unified error type
├── tracing_setup.rs     # Logging configuration
├── gui.rs               # GUI mode (feature-gated)
└── tray.rs              # System tray (feature-gated)
```

## Architecture Notes

### Agent ↔ REPL Communication

The REPL runs on the main thread with rustyline for input. The agent engine runs in a separate bridge thread. Communication uses `crossbeam-channel`:

- **REPL → Agent**: `TuiToAgent { SubmitMessage, ApproveTool, ApproveAll, DenyTool, Cancel }`
- **Agent → REPL**: `AgentToTui { Message, Thinking, Idle, TokenUpdate, RequestApproval }`

### Theme System

Themes are stored in SQLite and loaded at startup via `OnceLock + RwLock`. All output modules read colors from `theme::current()`. Three built-in themes: Dark (GitHub Dark Dimmed + Catppuccin Mocha), Light (GitHub Light), Monokai.

### Streaming Code Block Rendering

Two-phase rendering eliminates flicker:
1. **Stream phase**: Fast `│` placeholders with plain-colored text
2. **Completion phase**: Erase block, rewrite with syntect highlighting + line numbers

### Terminal Requirements

The REPL requires a TTY for rustyline. Subcommands (`status`, `doctor`, `logs`) work in pipes. The green blinking bar cursor uses DECSCUSR escape sequences.

## Pull Request Process

1. **Fork** the repository and create a feature branch from `master`.
2. **Run tests**: `cargo test --lib && cargo test`
3. **Run clippy**: `cargo clippy -- -D warnings`
4. **Format**: `cargo fmt`
5. **Commit** with a clear message describing the change.
6. **Open a PR** targeting `master`.

## Reporting Bugs

Please include:

- Rust version (`rustc --version`)
- Platform (macOS/Linux/Windows + version)
- Steps to reproduce
- Expected vs actual behavior

## Making a Release

Releases are managed via GitHub Releases with Git tags and a pre-built binary.

### Step-by-step

```bash
# 1. Ensure the version is up to date in Cargo.toml

# 2. Run final checks
cargo test --lib && cargo test
cargo clippy -- -D warnings
cargo fmt --check

# 3. Build the release binary
cargo build --release

# 4. Tag the release
git tag -a v0.3.0 -m "v0.3.0 — AI-powered Terminal Assistant"

# 5. Push the tag
git push origin v0.3.0

# 6. Create the GitHub Release with binary attachment
gh release create v0.3.0 \
  --title "v0.3.0 — AI-powered Terminal Assistant" \
  --notes "## Release Notes

### ✨ Features
- ... (list key changes since last release)

### 📦 Binary
- Build from source with \`cargo build --release\`
" \
  target/release/rupoo
```

### Versioning

This project follows **Semantic Versioning** (SemVer):

- **Patch** (0.3.x): Bug fixes, minor improvements — backward compatible
- **Minor** (0.x.0): New features, non-breaking API changes
- **Major** (x.0.0): Breaking changes, significant architectural shifts

## Code Style

- 4-space indentation, no tabs
- `rustfmt` for formatting
- `clippy` with deny-by-default warnings
- Document public API with doc comments (`///`)
- Prefer `thiserror` for error types, `anyhow` for propagation
