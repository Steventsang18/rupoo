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

# Run with verbose output
cargo test -- --nocapture

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
├── db.rs                # SQLite (WAL + FTS5): Plan CRUD, checkpoints, memories
├── llm/                 # LLM module
│   ├── gateway.rs       # LLM gateway: unified interface
│   ├── router.rs       # Provider routing
│   └── providers.rs    # Anthropic/OpenAI/DeepSeek/Ollama
├── memory.rs            # Long-term memory with FTS5 + Vector hybrid search
├── memory_cache.rs      # LRU cache for memory
├── vector_store.rs      # Vector storage and retrieval
├── embedding.rs         # Embedding service for vector generation
├── safety.rs            # Sandboxing: path_jail, command blacklist, SSRF
├── mcp.rs               # MCP tool dispatcher + JSON-RPC client
├── mcp_server.rs        # MCP protocol server (JSON-RPC over stdio)
├── skill.rs             # Skill system: JSON files, auto-learn from plans
├── git.rs               # Git integration via git2 + gh CLI
├── task.rs              # Step/Plan/Checkpoint type definitions
├── shared.rs            # Shared types between agent core and REPL
├── error.rs             # Unified error type (AgentError)
├── retry.rs             # Retry mechanism
├── strings.rs           # String utilities
├── tracing_setup.rs     # Logging configuration
├── cli/
│   ├── mod.rs           # REPL event loop, Agent bridge thread
│   ├── app.rs           # App state, session management
│   ├── bridge.rs        # Agent ↔ REPL bridge (crossbeam channels)
│   ├── chat_mode.rs     # Chat Mode handler
│   ├── plan_mode.rs     # Plan Mode: interactive step execution
│   ├── approval.rs      # Tool approval workflow
│   ├── output.rs        # Output formatting
│   ├── markdown.rs      # Markdown renderer
│   ├── enhanced_ui.rs   # Enhanced UI components
│   ├── shortcuts.rs     # Keyboard shortcuts handler
│   ├── completion.rs    # Auto-completion system
│   ├── commands.rs      # Command registry system
│   ├── theme.rs         # Theme system: Dark/Light/Monokai
│   └── cmds/            # CLI subcommands
└── tools/
    ├── browser.rs       # Browser automation
    ├── search.rs        # Web search integration
    ├── network.rs       # HTTP request tool
    └── terminal.rs      # Terminal command execution
```

## Architecture Notes

### Agent ↔ REPL Communication

The REPL runs on the main thread with rustyline for input. The agent engine runs in a separate bridge thread. Communication uses `crossbeam-channel`:

- **REPL → Agent**: `TuiToAgent { SubmitMessage, ApproveTool, ApproveAll, DenyTool, Cancel }`
- **Agent → REPL**: `AgentToTui { Message, Thinking, Idle, TokenUpdate, RequestApproval }`

### Memory System Architecture

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

Key components:
- **MemoryStore**: High-level memory operations with hybrid search
- **MemoryCache**: LRU cache for fast repeated queries
- **VectorStore**: Vector storage using SQLite
- **EmbeddingService**: Generates embeddings for semantic search

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
5. **Update documentation** if adding new features
6. **Commit** with a clear message describing the change.
7. **Open a PR** targeting `master`.

## Reporting Bugs

Please include:

- Rust version (`rustc --version`)
- Platform (macOS/Linux/Windows + version)
- Steps to reproduce
- Expected vs actual behavior
- Memory/Deep Search related: include search queries and results

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with verbose output
cargo test -- --nocapture

# Run benchmarks
cargo bench
```

### Writing Tests

Tests are located in:
- Unit tests: alongside source files
- Integration tests: `tests/` directory

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_feature() {
        // Test implementation
    }
}
```

## Making a Release

Releases are managed via GitHub Releases with Git tags and a pre-built binary.

### Step-by-step

```bash
# 1. Ensure the version is up to date in Cargo.toml
# Update version in:
# - [package] version
# - [package.metadata.bundle] version

# 2. Run final checks
cargo test --lib && cargo test
cargo clippy -- -D warnings
cargo fmt --check

# 3. Build the release binary
cargo build --release

# 4. Tag the release
git tag -a v0.4.0 -m "v0.4.0 — Memory System Enhancement + Hybrid Search"

# 5. Push the tag
git push origin v0.4.0

# 6. Create the GitHub Release with binary attachment
gh release create v0.4.0 \
  --title "v0.4.0 — Memory System Enhancement" \
  --notes "## Release Notes

### ✨ Features
- Hybrid Search (FTS5 + Vector)
- Memory Toggle Control
- Deep Search Toggle

### 📦 Binary
- Build from source with \`cargo build --release\`
" \
  target/release/rupoo
```

### Versioning

This project follows **Semantic Versioning** (SemVer):

- **Patch** (0.4.x): Bug fixes, minor improvements — backward compatible
- **Minor** (0.x.0): New features, non-breaking API changes
- **Major** (x.0.0): Breaking changes, significant architectural shifts

## Code Style

- 4-space indentation, no tabs
- `rustfmt` for formatting
- `clippy` with deny-by-default warnings
- Document public API with doc comments (`///`)
- Prefer `thiserror` for error types, `anyhow` for propagation

## Feature Flags

The project uses Cargo feature flags:

| Flag | Description |
|------|-------------|
| `gui` | Enable GUI mode with system tray |

## Security

When contributing code that handles:
- User input
- File system operations
- Network requests
- Memory storage

Please ensure:
- Input validation
- Path sandboxing
- SSRF protection
- Memory encryption (if applicable)

## Performance

For performance-critical code:
- Use benchmarks in `benches/bench.rs`
- Profile with `cargo flamegraph`
- Consider caching strategies
- Document time/space complexity

## Documentation

When adding new features:

1. Update **README.md** with feature overview
2. Add to **docs/USER_GUIDE.md** with usage examples
3. Add to **CHANGELOG.md** with version and date
4. Add code comments for complex logic

## Getting Help

- **GitHub Issues**: https://github.com/Steventsang18/rupoo/issues
- **Discussions**: https://github.com/Steventsang18/rupoo/discussions
