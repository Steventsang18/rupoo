# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-06-06

### 🎉 Major Features

#### Memory System Enhancement

- **Hybrid Search Architecture**: Implemented hybrid search combining FTS5 full-text search with vector semantic search
- **Memory Toggle Control**: Added `/memory on/off` commands to enable/disable memory feature
- **Memory Management Commands**:
  - `/memory` - View memory status
  - `/memory list` - List recent memories
  - `/memory search <query>` - Search memories with keywords
- **RRF (Reciprocal Rank Fusion)**: Implemented RRF algorithm for combining search results
- **Cache Invalidation**: Added automatic cache invalidation when memory is updated

#### Deep Search (Hybrid Search)

- **Deep Search Toggle**: Added `/deep on/off` commands
- **Vector Embedding Service**: Integrated embedding service for semantic search
- **Hybrid Search Configuration**: Configurable weights for FTS5 and vector search
- **Status Display**: Added deep search status indicator in status bar

### ✨ Improvements

#### CLI Enhancement

- **Enhanced Command System**: Improved command registration and lookup
- **UI Components**: Added new UI components (enhanced_ui, shortcuts, completion)
- **History Management**: Enhanced command history search and management

#### Agent Core

- **Error Handling**: Improved error types and handling
- **Safety Improvements**: Enhanced security context and permission controls
- **LLM Gateway**: Optimized router and provider management
- **Thread Safety**: Improved atomic operations for feature flags

### 🐛 Bug Fixes

- Fixed memory cache invalidation issues
- Fixed embedding service configuration
- Fixed hybrid search result ranking

### 📚 Documentation

- **README.md**: Updated with new features (Memory, Deep Search)
- **USER_GUIDE.md**: Comprehensive user guide with new features
- **PERFORMANCE.md**: Performance optimization documentation
- **CONTRIBUTING.md**: Contribution guidelines

### 🔧 Technical Changes

#### New Modules

| Module | Description |
|--------|-------------|
| `src/embedding.rs` | Embedding service for vector generation |
| `src/vector_store.rs` | Vector storage and retrieval |
| `src/retry.rs` | Retry mechanism for failed operations |
| `src/strings.rs` | String utilities |
| `src/cli/enhanced_ui.rs` | Enhanced UI components |
| `src/cli/shortcuts.rs` | Keyboard shortcuts handler |
| `src/cli/completion.rs` | Auto-completion system |

#### API Changes

- `Agent::set_memory_enabled(bool)` - Enable/disable memory
- `Agent::set_hybrid_search_enabled(bool)` - Enable/disable deep search
- `Agent::remember(content, tags)` - Store memory
- `Agent::recall(query, limit)` - Retrieve memories
- `Agent::memory_count()` - Get memory count

### 📊 Statistics

| Metric | Value |
|--------|-------|
| Files Changed | 38 |
| Lines Added | +5,646 |
| Lines Removed | -698 |
| Tests Added | 110 |
| New Modules | 7 |

---

## [0.3.1] - 2026-05-30

### 🐛 Bug Fixes

- Fixed node_modules tracking issue
- Minor documentation updates

---

## [0.3.0] - 2026-05-26

### ✨ New Features

#### Phase 1-5 Optimization Updates

- **LLM Gateway Enhancement**: Improved routing and fallback mechanism
- **Thinking Process**: Enhanced AI reasoning display
- **Extension System**: Framework for extensible plugins
- **TUI Rendering**: Differential rendering for better performance
- **CI/CD Pipeline**: Automated testing and deployment

### 🔧 Technical Changes

- Refactored LLM gateway architecture
- Improved error handling and recovery
- Enhanced logging system

---

## [0.2.0] - 2026-05-12

### ✨ Features

- **Dual-Mode Agent**: Chat Mode + Plan Mode
- **Plan Execution Engine**: Multi-step task execution
- **Checkpoint System**: Crash recovery support
- **MCP Protocol**: Model Context Protocol support

### 📝 Documentation

- Added MILESTONE-v0.2.md
- Added PROJECT-AUDIT-REPORT.md

---

## [0.1.0] - 2026-05-01

### 🎉 Initial Release

- Native REPL interface
- Basic chat functionality
- SQLite database integration
- Safety sandbox
- Command execution tools

---

## Migration Guides

### Upgrading from v0.3.x to v0.4.0

#### New Commands

The following commands are now available:

```bash
# Memory management
/memory           # View memory status
/memory on        # Enable memory
/memory off       # Disable memory
/memory list      # List memories
/memory search    # Search memories

# Deep search
/deep             # View status
/deep on          # Enable hybrid search
/deep off         # Disable hybrid search
```

#### Configuration

No configuration changes required. Memory system works with existing setup.

#### Performance

- Memory search now supports hybrid mode (FTS5 + Vector)
- Deep search disabled by default for faster responses
- Enable `/deep on` for semantic understanding

---

## Deprecation Notices

None at this time.

---

## Security Advisories

None at this time.

---

## Coming Soon

### Planned for v0.5.0

- [ ] Web UI Dashboard
- [ ] Cloud Sync
- [ ] Team Collaboration
- [ ] Plugin Marketplace
- [ ] Enhanced Analytics

---

## Contributing

If you find any issues or have suggestions, please open an issue or pull request on GitHub.

- GitHub Issues: https://github.com/Steventsang18/rupoo/issues
- Discussions: https://github.com/Steventsang18/rupoo/discussions
