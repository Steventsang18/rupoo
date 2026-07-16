# CLI 轻量化 / 快速 / 安全 优化总结（2026-07-14）

本轮围绕 `src-agent` CLI（TUI）做了三波"更轻、更快、更安全"的优化，全部改动均 `cargo check --tests` 通过。以下为逐项记录、收益与验证。

---

## 总览

| 波次 | 优化项 | 目标 | 状态 |
|---|---|---|---|
| 2 | Session 消息懒加载 | 快（启动） | ✅ |
| 2 | `git2` → `git` CLI 改写 | 轻 + 安全 | ✅ |
| 3 | `persist_sessions` 后台 writer | 快 + 轻 | ✅ |
| 3 | 流式未完成行实时出字 | 体感（快/顺） | ✅ |
| 3 | `build_engine` settings 批量读取 | 快（启动） | ✅ |

> 调查中证伪的候选项：原「轻命令拆分」经代码核实**不成立**——`build_engine` 仅在 TUI 分支调用，`status/doctor/model/session/logs` 各自只 `TaskRepo::new`，并未初始化 LLM/Embedding，故无拆分收益，已删除该项。

---

## 第二波

### A. Session 消息懒加载

**文件**：`src/cli/app.rs`、`src/cli/mod.rs`

**问题**：`ReplSession::new` 启动即 `serde_json::from_str` 解析**全部**会话的消息，多/长会话时启动开销 O(所有会话消息)。

**改动**：
- `RupooApp` 新增 `session_raw: HashMap<String, String>`，存每个会话原始 `messages_json`。
- 启动仅解析 **active** 会话；其余仅存 raw。
- 新增 `get_session_messages(id)`：未缓存时从 raw 惰性解析并写回缓存。
- `switch_session`（app.rs）、`/switch` 命令（mod.rs）改用 `get_session_messages` 按需加载。

**正确性**：切换时先 `session_messages.insert(old, current)` 再惰性取 new，非激活会话不会被陈旧 raw 覆盖，与原「启动全解析」语义一致，不回退持久化行为。

---

### B. `git2` → `git` CLI 改写 —— 「轻/安全」头号大项

**文件**：`src/git.rs`、`src-agent/Cargo.toml`、`src/cli/cmds/doctor.rs`

**问题**：`git2` 依赖 `vendored-libgit2 + vendored-openssl`，引入整套 C 依赖（OpenSSL + libgit2），增大二进制体积与攻击面。

**改动**：`GitRepo` 全部本地操作改为 `std::process::Command("git")`：
- `open` → `git rev-parse --show-toplevel`
- `current_branch` → `git symbolic-ref --short HEAD`（失败回退读 `.git/HEAD`）
- `status` → `git status --porcelain`（XY 码映射为原分类词）
- `commit_all` → `git add -A` + `git commit -m` + `git rev-parse --short HEAD`
- `create_gh_pr` 本就用 `gh` CLI，未动
- 对外 API 签名（open/current_branch/status/commit_all/commit_with_task_ref/create_gh_pr）**不变**，调用方零改动
- `Cargo.toml` 删除 `git2` 依赖；`doctor.rs` 文案 `libgit2 available` → `git CLI available`
- `describe_status(git2::Status)` 改为 `describe_status(x: char, y: char)`，配套 6 个单测覆盖 clean/staged_new/modified/untracked/deleted/open

**收益（已验证）**：
- `cargo tree -i openssl` 与 `cargo tree -i libgit2-sys` 均确认**已从依赖树移除**。
- 二进制删掉 vendored OpenSSL + libgit2 两套 C 依赖，**只剩 rustls 一套 TLS**（瘦身 + 缩小攻击面）。
- `cargo test --lib git::` 6 项全过。

---

## 第三波

### 1. `persist_sessions` 常驻后台 writer + 无界通道

**文件**：`src/cli/app.rs`、`src/cli/mod.rs`

**问题**：每次提交/切换都 `std::thread::spawn` 孵一个线程，且 `clone` 全量 `sessions` Vec。

**改动**：
- `RupooApp` 新增 `persister: Option<crossbeam_channel::Sender<PersistMsg>>`（`PersistMsg{id,label,json}`）。（`RupooApp` 无 `Clone` derive，可直接存 `Sender`，无需 `Arc`。）
- 新增 `init_persister(&mut self)`（幂等）：建 `crossbeam_channel::unbounded` 通道，孵**一个**常驻 worker 线程（`while let Ok(msg) = rx.recv()` → `handle.block_on(repo.save_ui_session(...))`）。
- `persist_sessions` 重写：有 writer 则只序列化 active 会话 json 后 `tx.send`（不再 clone 整个 `sessions` Vec）；无 writer（如测试）回退原 spawn-per-call，行为不退化。
- `ReplSession::new` 在 `set_repo` 后调 `init_persister()`，TUI 路径只孵一个 writer。

**收益**：消除每次提交/切换的 spawn + 全量 clone；改为单线程 + 无界通道。通道 FIFO 保证写入顺序，比原「每次 spawn」的竞态更严格（最后状态必赢）。

---

### 2. 流式「未完成行」实时出字

**文件**：`src/cli/markdown.rs`、`src/cli/mod.rs`

**问题**：`render_stream_chunk` 用 `buffer.rfind('\n')` 只渲染**完整行**，未完成行留在 buffer，要等下一个 `\n` 或 `flush_stream` 才显示——长段落时用户看到「卡住」，是体感「慢」的主因。

**改动**：
- `StreamState` 新增 `partial_rendered: bool`。
- `render_stream_chunk` 重写：①每帧开头若上帧渲染过未完成行，`print!("\r\x1b[2K")` 擦除；②完整行循环不变；③循环后若还有未完成内容且非代码块，按**显示列宽**（`unicode_width`，中文算 2 列）截断到 `width-1`，着色后 `print!("\r\x1b[2K{content}▌")` 单行不换行 + `stdout().flush()`。
- 新增 `truncate_display_cols`：按 Unicode 显示宽度截断，避免超宽触发终端自动折行导致清除残留。
- `flush_stream` 开头先擦除未完成行再 flush 剩余 buffer。
- 新增 `StreamState::take(&mut self)`：重置前擦除未完成行；`mod.rs` 9 处 `self.stream_state = StreamState::new()` 改为 `self.stream_state.take()`，防止打断/切换时残留带 `▌` 的半行。

**正确性闭环**：未完成行永远是「最后打印、不换行」的行，下一帧开头的 `\r\x1b[2K` 恰好清除它；完整行一旦提交永不被清除，无画面错乱。

**限制**：code block 内流式仍不实时（闭合才一次性高亮显示），属预期降级；`▌` 光标依赖终端字形支持。

---

### 3. `build_engine` settings 批量读取

**文件**：`src/db/settings.rs`、`src/build_engine.rs`

**问题**：引擎初始化时为每个 provider 逐个 `get_setting("api_key.{p}" / "model.{p}" / "base_url.{p}")` await，外加 `active_provider`，最坏 ~13 次顺序 DB 往返。

**改动**：
- `settings.rs` 新增 `get_settings(&self, keys: &[String]) -> HashMap<String, String>`：拼 `WHERE key IN (?,?,...)` 单条 SQL（`rusqlite::params_from_iter`），返回 `key→value` map；空 keys 返回空 map。
- `build_engine` 先一次性取回 `active_provider` + 4 provider × `{api_key, model, base_url}` 共 13 个 key（**1 次查询**），之后全部改为 `settings_map.get(...)` 内存查找，不再有 `.await` DB 往返。

**收益**：启动期 settings 读取从最多 ~13 次顺序查询降为 1 次；余下纯内存查找。属低收益（单次启动省十几毫秒级），改动有界、零行为变化。

---

## 验证方式

- 全部改动：`cargo check --tests` 通过（无 error / 无新增 warning）。
- `git.rs` 改写：`cargo test --lib git::` 6 项全过。
- 依赖瘦身：`cargo tree -i openssl` / `cargo tree -i libgit2-sys` 均确认已移除。
- TUI 交互 / 集成：因难自动化未跑；逻辑与既有 `save_ui_session`（`ON CONFLICT(id) DO UPDATE` upsert）、`flush_stream`、`StreamState` 复位语义自洽。

## 后续可选

- 为 `StreamState` 增量重绘、`git.rs` CLI 改写补更多单元测试。
- 提交（commit）本轮改动。
