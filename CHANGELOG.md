# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-06-27

### 🏗️ Architecture Overhaul — Five-Layer Pipeline

#### Trait-Based Core Architecture

Introduced formal trait definitions for all core layers, enabling mock testing and future provider swaps:

- **`CognitiveEngine`** trait — `parse()` → `decompose()` → `check_boundary()` with `AgentGoal`, `AuthLevel`, `GoalConstraint` types
- **`Planner`** trait — `generate_alternatives()` → `score()` → `select_best()` with `ExecutionPlan`, `PlanScore` types
- **`ExecutionEngine`** trait — step validation with `ExecutionMeta`, type-aware input checking, replan trigger detection
- **`MemorySystem`** / **`MemoryStorage`** traits — unified interface over short-term, long-term, episodic stores

#### CognitiveEngine Implementation

- **`CognitiveEngineImpl`**: LLM-powered goal parsing with structured JSON output extraction
- **Safety boundary detection**: Interactive safety assessment merging goal constraints with `SafetyContext` rules
- **Task decomposition**: LLM-driven goal decomposition into up to 5 sub-goals with rationale tracking
- **Forbidden-command / high-risk-keyword detection** integrated into pipeline layer 1

#### Three-Gate Supervisor (Pipeline Layer 3)

- **Gate 1 — ComplianceChecker**: Forbidden-command filtering, approval-required detection
- **Gate 2 — ConfidenceChecker**: Semantic confidence threshold evaluation
- **Gate 3 — CircuitBreaker**: Configurable failure threshold with Half-Open → Open probe recovery
- **`SupervisorImpl`** : Serial 3-gate intercept with rate limiting
- **`SqliteAuditLogger`**: Persistent audit event logging with `tempfile`-based test isolation
- **Integration tests**: 4 full-path supervisor integration tests

#### Memory System Bridge

- **`MemorySystemBridge`**: Wraps legacy `MemoryStore` behind new `MemorySystem` trait
- **Hybrid recall**: Merges short-term + long-term + episodic results with dedup
- **`ShortTermMemory`**: In-memory cache with capacity-based LRU eviction
- **Long-term / Episodic adapters**: Delegate to `MemoryStore`'s FTS5 backend via `LegacyStorageAdapter`
- **Send+Sync safety**: Verified thread-safe bridge architecture

#### Five-Layer Orchestrator

- **`Orchestrator`**: Assembles CognitiveEngine → Planner → ExecutionEngine → Supervisor → MemorySystem into a single pipeline
- **Step-type-aware validation**: Replaces empty-JSON validation with type-specific parameter extraction
- **Real replanning**: `Replanner` marks failure point, inserts `Think` step for re-assessment, preserves remaining steps with context
- **Full mock integration test**: End-to-end pipeline test with mock implementations for all 5 layers

### 🐛 Bug Fixes

#### Vector Store

- **`remove()` memory leak**: Fixed — embeddings now drained from flat array on doc removal (`65cab86`)
- **IndexMap migration**: Switched from `HashMap` to `IndexMap` for stable iteration order, ensuring embedding array stays in sync with document indices
- **Honest labeling**: Module docs updated to reflect O(n) brute-force status; `hnswx` dependency reserved but not yet wired

#### Supervisor

- **`SqliteAuditLogger::new()` unwrap removal**: Replaced bare `unwrap()` with proper `?` propagation (`3853de1`)
- **Dead code cleanup**: Removed stale fields, fixed test naming consistency (`0e5d892`, `18a65ed`)
- **`#[must_use]` annotations**: Added to `ComplianceResult`, moved to dedicated `compliance.rs` module (`749ef22`)

#### Loop Engine

- **`execute_plan_inner` declared as placeholder**: Added runtime warning `warn!()` logging on each invocation
- **Daemon mode fallback**: Added not-implemented warning with graceful fallback to standard loop

#### Orchestrator

- **Step-type-aware validation**: Replaces empty `{}` JSON validation with per-type parameter extraction (ToolCall, Exec, HttpRequest, BrowserAction)
- **Real replanning implementation**: Replaces `continue` placeholder with `Replanner` that inserts `Think` steps and preserves context
- **Mock full-stack integration tests**: 4 tests covering pipeline success, empty-plans error, supervisor blocking, and memory bridge integration

### ♻️ Code Quality

- **clippy zero-warnings**: `cargo clippy --fix` automated + 6 manual fixes across the codebase (`0c58d68`)
- **Safety context alignment**: `SafetyContext::from_config()` merges config-file forbidden_commands with runtime defaults (`178d18a`)
- **Dependency cleanup**: Removed unused/redundant dependencies; documented `hnswx` as reserved-but-unwired (`178d18a`)
- **`cargo fmt`** applied project-wide

### 🧪 Testing

- **Total: 275 tests** (239 unit + 23 binary + 4 integration + 9 doc) — all passing
- **4 new orchestrator integration tests**: Full pipeline, supervisor blocking, empty plans, memory bridge
- **Memory system tests**: Bridge short-term/long-term/episodic store/retrieve, hybrid recall with dedup, Send+Sync compile check
- **Supervisor integration tests**: 4 full-path tests with `tempfile`-backed audit logger
- **Cognitive engine tests**: Goal parsing, safety assessment, boundary checking

### 🔧 Technical Changes

| Commit | Change |
|--------|--------|
| `cd515fe` | Define `CognitiveEngine` trait, `AgentGoal`, `AuthLevel` types |
| `167dbb9` | Define `Planner` trait, `PlanScore`, `ExecutionPlan` types |
| `a43cd38` | Define `ExecutionEngine` trait, validation types |
| `ac9484f` | Define `MemoryStorage` / `MemorySystem` traits, `ShortTermMemory` |
| `4a74d35` | Define `Supervisor` trait, `AuditEvent` types |
| `749ef22` | Add `#[must_use]`, move `ComplianceResult` to `compliance.rs` |
| `b4562b6` | Implement `ComplianceChecker` as gate 1 |
| `0faa7f7` | Implement `ConfidenceChecker` as gate 2 |
| `432bdc5` | Implement `CircuitBreaker` as gate 3 |
| `31472a1` | `SupervisorImpl` with 3-gate intercept + integration tests |
| `3853de1` | Fix `SqliteAuditLogger::new()` unwrap |
| `7da3c03` | Five-layer `Orchestrator` pipeline |
| `0211593` | `MemorySystemBridge` wrapping legacy `MemoryStore` |
| `47e829f` | Agent `memory_system` field for trait-based access |
| `da743c1` | Step-type-aware validation + real replanning |
| `e2bd171` | Mock full-stack orchestrator integration tests |
| `65cab86` | Fix `remove()` memory leak, IndexMap migration |
| `0c58d68` | Clippy zero-warnings (auto + manual) |
| `32699e2` | Loop engine placeholder warnings + daemon fallback |
| `178d18a` | Phase 6: safety alignment + dependency cleanup |

### 📈 Statistics

| Metric | Value |
|--------|-------|
| Commits | 20 |
| Files Changed | ~40 |
| Tests Added | ~44 |
| Tests Total | **275** (all passing) |
| Clippy | ✅ Zero warnings across all targets |

---

## [0.4.1] - 2026-06-10

### 🧠 Agent Core Intelligence

#### Context Management System

- **Unified Context Object**: Added `context.rs` module for comprehensive conversation context management
- **Token Budget Control**: Implemented intelligent token management with configurable limits
- **System Resource Awareness**: Real-time monitoring of CPU cores and memory usage
- **User Behavior Tracking**: Profile-based behavior analysis for personalized responses

#### Tool Call Intelligence

- **Smart Tool Selection**: Added `tool_selector.rs` module with intelligent tool recommendation engine
- **Risk Assessment**: Five-level risk classification (Critical/High/Medium/Low/Safe)
- **Automatic Risk Mitigation**: Critical operations blocked by default, high-risk requires user approval
- **Parallel Execution Planning**: Dependency-aware tool call batching for improved efficiency
- **Dangerous Pattern Detection**: Identifies suspicious tool call sequences (e.g., file_read + shell_exec)

#### Security Policy Enhancement

- **Default Deny Principle**: Implemented strict security model with explicit permissions
- **Role-Based Access Control**: Admin/User/Guest/Custom role definitions
- **Audit Logging**: Comprehensive security event tracking
- **Path/Host Blocking**: Protection against sensitive system access

### ✨ Improvements

- **Memory System**: Optimized vector storage operations (IndexMap migration for stable iteration)
- **Deep Search**: Faster semantic search responses with improved embedding alignment
- **User Experience**: Significantly improved search responsiveness
- **Environment Signals**: Enhanced system context awareness
- **Vector Store**: Brute-force O(n) search optimized with IndexMap for stable document-embedding alignment

### 📚 Documentation

- **rustdoc Enhancement**: Added comprehensive documentation for core modules
- **Code Quality**: Cleaned up unused imports and variables
- **Benchmark Tool**: Added `examples/vector_search_benchmark.rs` for performance testing

### 🔧 Technical Changes

- Refactored `VectorStore` with IndexMap for stable iteration order
- New module: `src/context.rs` — Conversation context management
- New module: `src/tool_selector.rs` — Intelligent tool selection engine
- Enhanced: `src/security_policy.rs` — Advanced risk assessment
- Enhanced: `src/signal.rs` — System resource monitoring
- Enhanced: `src/agent.rs` — Integrated tool intelligence
- `hnswx` dependency reserved but not yet integrated (planned for future ANN index support)

### 📈 Statistics

| Metric | Value |
|--------|-------|
| Files Changed | 12 |
| Lines Added | +2,145 |
| Lines Removed | -186 |
| Tests Added | 32 |
| New Modules | 2 |

---

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

### Upgrading from v0.4.x to v0.5.0

No breaking changes. The new five-layer pipeline traits (`CognitiveEngine`, `Planner`,
`ExecutionEngine`, `Supervisor`, `MemorySystem`) are additive — existing `Agent` and
`MemoryStore` usage remains unchanged. The `MemorySystemBridge` provides backward
compatibility between new and legacy memory paths.

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

### Planned for v0.6.0

- [ ] HNSW ANN Index — upgrade vector search from O(n) brute-force to O(log n)
- [ ] Orchestrator-Agent Integration — wire five-layer pipeline into Agent main loop
- [ ] Web UI Dashboard
- [ ] Cloud Sync
- [ ] Team Collaboration

---

## Contributing

If you find any issues or have suggestions, please open an issue or pull request on GitHub.

- GitHub Issues: https://github.com/Steventsang18/rupoo/issues
- Discussions: https://github.com/Steventsang18/rupoo/discussions
