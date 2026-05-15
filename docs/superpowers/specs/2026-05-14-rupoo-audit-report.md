# Rupoo 项目代码审查报告

> 日期: 2026-05-14
> 范围: 全项目代码审查（~7,774 行 Rust）
> 审查方法: 静态分析 + clippy + 逻辑推理

---

## 汇总

| 类型 | 数量 | 严重程度 |
|------|------|----------|
| Bug（行为不一致） | 2 | 🔴 高 |
| 架构缺陷 | 2 | 🟡 中 |
| 代码异味 | 5 | 🟢 低 |
| clippy 警告 | 2 | 🟢 低 |
| 待清理 | 3 | 🟢 低 |

---

## 🔴 Bug（高优先级）

### B1: TUI 与 CLI 子命令使用不同的数据库文件

**位置:** `src/main.rs:371` vs `src/main.rs:201-243`

TUI 入口:
```rust
// run_repl() 中
let db_path = data_dir.join("agent.db");  // → ~/.rupoo/agent.db
```

CLI 子命令默认值:
```rust
Status  { #[arg(long, default_value = "agent.db")] db: String }  // → ./agent.db
Model   { #[arg(long, default_value = "agent.db")] db: String }
Session { #[arg(long, default_value = "agent.db")] db: String }
```

**影响:** 用户在 TUI 中创建的计划/配置，用 `rupoo status` 或 `rupoo session list` 看不到。这是两个完全独立的数据库。

**验证:**
```bash
# 确认存在两个不同的 agent.db
ls -la agent.db ~/.rupoo/agent.db
# 它们有不同的大小和内容
sqlite3 agent.db "SELECT COUNT(*) FROM plans;"
sqlite3 ~/.rupoo/agent.db "SELECT COUNT(*) FROM plans;"  # 结果不同
```

**修复方案:** 统一默认路径。所有 CLI 子命令默认路径改为读取 `crate::tracing_setup::data_dir().join("agent.db")`，或至少保证 TUI 和 CLI 使用同一个默认值。

```rust
// 在 main.rs 中添加常量
const DEFAULT_DB: &str = "agent.db"; // 或使用函数

// 或者更彻底的方案：CLI 默认使用 ~/.rupoo/agent.db
```

---

### B2: `doctor` 命令硬编码 `./agent.db` 且缺少 `--db` 参数

**位置:** `src/cli/cmds/doctor.rs:51,78`

```rust
match TaskRepo::new("agent.db") {   // 硬编码，不是 ~/.rupoo/agent.db
```

而其他命令 (`status`, `model`, `session`) 都有 `--db` 参数。

**影响:** `rupoo doctor` 检查的数据库与 TUI 实际使用的不同，可能给出假阳性结果。

**验证:**
```bash
# 创建测试数据库演示问题
touch /tmp/test.db
cargo run -- doctor  # 检查 ./agent.db
# 但 TUI 用 ~/.rupoo/agent.db
```

**修复方案:**
1. 为 `doctor` 添加 `--db` 参数（与其他命令一致）
2. 默认值与其他命令保持一致

---

## 🟡 架构缺陷（中优先级）

### A1: `doctor` 重复打开数据库

**位置:** `src/cli/cmds/doctor.rs:51,78`

数据库检查 (`Database` 项) 中打开一次，LLM 配置检查中又打开一次。`TaskRepo::new` 每次都会建立新的 SQLite 连接。

```rust
// 第 51 行：第一次打开
match TaskRepo::new("agent.db") {
    Ok(repo) => { ... }
}
// 第 78 行：第二次打开
if let Ok(repo) = TaskRepo::new("agent.db") {
    // 再次查询 settings 表
}
```

**修复方案:** 将打开的 repo 实例传入 `all_checks`，或先在 `run()` 中打开一次再共享。

```rust
pub async fn run(fix: bool) -> Result<()> {
    let repo = TaskRepo::new("agent.db").ok();
    println!("{}", style("Rupoo Diagnostics").bold());
    let results = all_checks(repo.as_ref()).await;
    // ...
}

async fn all_checks(repo: Option<&TaskRepo>) -> Vec<CheckResult> {
    // 使用传入的 repo，避免重复打开
}
```

---

### A2: `..` 在安全沙箱允许路径中

**位置:** `src/safety.rs:51-52`

```rust
allowed_paths: vec![
    PathBuf::from("."),
    PathBuf::from(".."),   // ← 允许父目录
],
```

**分析:** `path_jail::join` 可能能防止路径穿越，但将 `..` 作为 jail root 在概念上是危险的。如果某个代码路径没有经过 `apply_file_jail`，`..` 会使文件访问的范围隐式等同于整个文件系统。

**修复方案:** 移除 `..`，或将其替换为明确的父目录绝对路径。

```rust
allowed_paths: vec![
    PathBuf::from("."),
    // 移除 ".."
],
```

---

## 🟢 代码异味（低优先级）

### C1: `status` 命令不显示实际记忆数量

**位置:** `src/cli/cmds/status.rs:60-64`

```rust
println!("  {}  {:<12} {} {}",
    style("├──").dim(),
    style("Memory").cyan(),
    style("●").green(),
    style("entries (FTS5 indexed)").dim(),  // 没显示具体数字
);
```

**修复方案:** 调用 `repo.count_memories()` 并显示实际数量。

---

### C2: `doctor` 技能列表输出过长

**位置:** `src/cli/cmds/doctor.rs:127`

当 `~/.skills/` 下有 56 个技能文件时，会全部打印在一行，造成超长输出。

**验证:**
```bash
cargo run -- doctor | grep "Skills"
# 输出: 56 installed at /Users/xxx/.skills
#       'auto-085cb0a2', 'auto-0a7c3fae', ... (56个名字挤在一行)
```

**修复方案:** 截断显示，例如只显示前 5 个 + "and N more"：

```rust
let display: Vec<String> = if names.len() > 5 {
    let mut truncated: Vec<String> = names.iter().take(5).cloned().collect();
    truncated.push(format!("... and {} more", names.len() - 5));
    truncated
} else {
    names
};
```

---

### C3: `allow(dead_code)` 隐藏了未使用的代码

**位置:** `src/git.rs:11`, `src/mcp.rs:32,34,283,315`

有 5 处 `#[allow(dead_code)]`，掩盖了未使用的字段和方法：

| 位置 | 屏蔽了什么 |
|------|-----------|
| `git.rs` 某字段 | 未使用 |
| `mcp.rs:ToolDispatchEntry.name` | 未使用 |
| `mcp.rs:ToolDispatchEntry.description` | 未使用 |
| `mcp.rs:283` | 未使用的函数 |
| `mcp.rs:315` | 未使用的函数 |

**修复方案:** 逐个审查这些条目，如果确实不需要则删除，需要则添加 `#[allow(dead_code)]` 的注释说明。

---

### C4: `chrono` 重复声明为 dev-dependency

**位置:** `Cargo.toml:19, Cargo.toml:56`

```toml
[dependencies]
chrono = { version = "0.4", features = ["serde"] }  # 第 19 行

[dev-dependencies]
chrono = "0.4"  # 第 56 行 — 多余
```

`[dev-dependencies]` 中的 `chrono` 是多余的，因为集成测试二进制可以自动使用 `[dependencies]` 中的 crate。

**修复方案:** 删除 `[dev-dependencies]` 中的 `chrono` 行。

---

### C5: Cargo.toml 版本号与里程碑不一致

**位置:** `Cargo.toml:3` vs `MILESTONE-v0.2.md`

```toml
version = "0.1.0"  # Cargo.toml 显示 0.1.0
```

但 `MILESTONE-v0.2.md` 标题为 "Rupoo 里程碑 v0.2"。

**验证:**
```bash
cargo run -- status --short
# 输出: Rupoo 0.1.0 | ...  (而不是 0.2.0)
```

**修复方案:** 统一版本号为 `0.2.0`。

---

## ⚠️ Clippy 警告

### L1: 不需要的引用

**位置:** `src/cli/cmds/model.rs:93`

```rust
repo.set_setting("active_provider", &provider).await?;
//                                   ^^^^^^^^^ help: change this to: `provider`
```

### L2: `match` 可以替换为 `if let`

**位置:** `src/main.rs:523`

```rust
match &event {
    Event::Key(key) => { ... }
    _ => {}
}
// 建议改为 if let Event::Key(key) = &event { ... }
```

**自动修复:**
```bash
cargo clippy --fix --bin "rupoo" -p rupoo
```

---

## 测试覆盖缺口

| 模块 | 测试数 | 备注 |
|------|--------|------|
| agent | 8 | 覆盖完整，含崩溃恢复 |
| cli/status | 3 | 格式化函数 ✅，缺少集成测试 |
| cli/model | 4 | 格式化 + 解析 ✅，缺少 set/show 集成测试 |
| cli/session | 4 | 格式化 ✅，缺少 show/delete/prune 集成测试 |
| cli/doctor | 4 | 工具函数 ✅，缺少运行时检查测试 |
| cli/logs | 4 | 过滤函数 ✅，缺少文件读取集成测试 |
| mcp_server | 4 | 覆盖完整 |
| safety | 5 | 覆盖完整 |
| browser | 2 | 基础覆盖 |
| network | 2 | 基础覆盖 |

新增的 5 个 CLI 命令**缺少集成测试**——所有测试都是纯逻辑测试（格式化、解析），没有测试真实 DB 交互或文件读写的场景。

---

## 推荐修复优先级

### 立即修复（P0）
1. **B1** — TUI 与 CLI 的 DB 路径统一
2. **B2** — doctor 添加 `--db` 参数

### 本周修复（P1）
3. **A1** — doctor 减少重复 DB 打开
4. **C1** — status 显示实际记忆数量
5. **C5** — 版本号统一为 0.2.0
6. **L1 + L2** — clippy 警告自动修复

### 下次迭代（P2）
7. **C2** — doctor 技能列表截断
8. **A2** — safety 中评估 `..` 的必要性
9. **C3** — 清理 dead_code
10. **C4** — 清理多余 dev-dependency

---

## 如何验证修复

每条修复都可以独立验证：

```bash
# B1: 统一 DB 路径后，TUI 和 CLI 看到相同的计划数
cargo run -- session list
# 应与 TUI 中的计划列表一致

# B2: doctor 现在接受 --db 参数
cargo run -- doctor --db ~/.rupoo/agent.db

# C5: 版本号一致
cargo run -- status --short
# 应输出 Rupoo 0.2.0

# L1+L2: 零 clippy 警告
cargo clippy 2>&1 | grep "warning:" | wc -l
# 应输出 0
```
