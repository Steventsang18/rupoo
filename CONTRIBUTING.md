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

# Run with a specific model provider
ANTHROPIC_API_KEY=sk-ant-... cargo run --release
```

## Project Structure

```
src/
├── main.rs          # CLI entry point, command dispatch, engine bootstrap
├── lib.rs           # Crate root, re-exports shared types
├── agent.rs         # Agent state machine: 7 step types, crash recovery
├── db.rs            # SQLite (WAL + FTS5): Plan CRUD, checkpoints, memories
├── llm.rs           # LLM gateway: unified interface for Anthropic/OpenAI/Ollama
├── cli/
│   ├── mod.rs       # TUI event loop, AgentUiBridge thread, run_tui_with_agent
│   ├── app.rs       # App state, InputMode routing, apply_agent_event
│   ├── ui.rs        # ratatui render: 3-column layout, bubbles, code blocks
│   ├── handlers.rs  # Input mode strategies (Chat/Thinking/Approval/Palette)
│   └── cmds/        # CLI subcommands: status, model, session, doctor, logs...
├── safety.rs        # Sandboxing: path_jail, command blacklist, SSRF protection
├── mcp.rs           # MCP tool dispatcher + JSON-RPC client
├── mcp_server.rs    # MCP protocol server (JSON-RPC over stdio)
├── rig_tools.rs     # Built-in tools: Echo, FileRead, FileWrite, ListDir
├── skill.rs         # Skill system: JSON files, auto-learn from plans
├── memory.rs        # Long-term memory with FTS5 full-text search
├── git.rs           # Git integration via git2 + gh CLI
├── task.rs          # Step/Plan/Checkpoint type definitions
├── shared.rs        # Shared types between agent core and TUI (AgentToTui, etc.)
└── error.rs         # Unified error type
```

## Architecture Notes

### Agent ↔ TUI Communication

The TUI runs on the main thread in a synchronous event loop. The agent engine runs in a separate `AgentUiBridge` thread. Communication uses `crossbeam-channel`:

- **TUI → Agent**: `TuiToAgent { SubmitMessage, ApproveTool, DenyTool }`
- **Agent → TUI**: `AgentToTui { Message, Thinking, Idle, TokenUpdate, RequestApproval }`

### TextArea Serialization

`tui_textarea::TextArea` cannot be serialized. The pattern used:

```rust
// App state holds both:
pub struct RupooApp {
    pub input: TextArea<'static>,   // runtime only, NOT serialized
    #[serde(skip)]
    pub input_text: String,         // mirror for serde
}
```

On save: copy `input.lines()` → `input_text`. On restore: `TextArea::from(input_text.split('\n'))`.

### TTY Requirement

The TUI requires a TTY (`enable_raw_mode()`). Subcommands (`status`, `doctor`, `logs`) work in pipes.

## Pull Request Process

1. **Fork** the repository and create a feature branch from `main`.
2. **Run tests**: `cargo test --lib && cargo test`
3. **Run clippy**: `cargo clippy -- -D warnings`
4. **Format**: `cargo fmt`
5. **Commit** with a clear message describing the change.
6. **Open a PR** targeting `main`.

## Reporting Bugs

Please include:

- Rust version (`rustc --version`)
- Platform (macOS/Linux/Windows + version)
- Steps to reproduce
- Expected vs actual behavior

## Code Style

- 4-space indentation, no tabs
- `rustfmt` for formatting
- `clippy` with deny-by-default warnings
- Document public API with doc comments (`///`)
- Prefer `thiserror` for error types, `anyhow` for propagation
