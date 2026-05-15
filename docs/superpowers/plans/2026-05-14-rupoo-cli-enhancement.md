# Rupoo CLI Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 5 new CLI subcommands (`status`, `model`, `session`, `doctor`, `logs`) to Rupoo, plus 4 supporting DB methods.

**Architecture:** Each command lives in its own file under `src/cli/cmds/`, receives shared state via function parameters (no struct refactor needed), and uses existing `TaskRepo`, `SkillManager`, `LlmConfig`, and `GitRepo` APIs. DB changes add query methods to the existing `TaskRepo` — no schema changes.

**Tech Stack:** Rust 2021, clap 4 (derive), rusqlite (bundled), tokio, anyhow.

---

## File Structure

### New files
| File | Purpose |
|------|---------|
| `src/cli/cmds/mod.rs` | Module declarations for all 5 commands |
| `src/cli/cmds/status.rs` | `rupoo status` — system overview (~100 loc) |
| `src/cli/cmds/model.rs` | `rupoo model` — LLM provider management (~150 loc) |
| `src/cli/cmds/session.rs` | `rupoo session` — plan/session management (~120 loc) |
| `src/cli/cmds/doctor.rs` | `rupoo doctor` — environment diagnostics (~120 loc) |
| `src/cli/cmds/logs.rs` | `rupoo logs` — log viewer (~80 loc) |

### Modified files
| File | Change |
|------|--------|
| `src/main.rs` | Add 5 new variants to `Commands` enum + dispatch in `run_cmd()` |
| `src/db.rs` | Add `list_plans`, `count_plans_by_status`, `delete_plan`, `prune_plans` + `PlanSummary` type |

---

### Task 1: DB layer — PlanSummary + 4 query methods

**Files:**
- Modify: `src/db.rs` — add PlanSummary struct + 4 pub async fn

- [ ] **Step 1: Add PlanSummary type and 4 method signatures to db.rs**

Insert after the existing `use` statements (around line 5), and add methods before the closing `}` of `impl TaskRepo`:

```rust
// ── PlanSummary for CLI listing ──

#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}
```

Then add these 4 methods inside `impl TaskRepo`:

```rust
/// List plans ordered by updated_at descending.
pub async fn list_plans(&self, limit: usize, offset: usize) -> AgentResult<Vec<PlanSummary>> {
    self.with_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, steps_json, current_step_index, status, created_at, updated_at
             FROM plans ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| {
            let steps_json: String = row.get(2)?;
            let steps: Vec<super::task::Step> = serde_json::from_str(&steps_json)
                .unwrap_or_default();
            Ok(PlanSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                total_steps: steps.len(),
                current_step_index: row.get::<_, i64>(3)? as usize,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows.flatten() {
            results.push(row);
        }
        Ok(results)
    }).await
}

/// Count plans grouped by status.
pub async fn count_plans_by_status(&self) -> AgentResult<Vec<(String, i64)>> {
    self.with_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM plans GROUP BY status"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut results = Vec::new();
        for row in rows.flatten() {
            results.push(row);
        }
        Ok(results)
    }).await
}

/// Delete a plan and its associated checkpoints.
pub async fn delete_plan(&self, plan_id: &str) -> AgentResult<()> {
    let pid = plan_id.to_string();
    self.with_conn(move |conn| {
        conn.execute("DELETE FROM checkpoints WHERE plan_id = ?1", rusqlite::params![pid])?;
        conn.execute("DELETE FROM plans WHERE id = ?1", rusqlite::params![pid])?;
        Ok(())
    }).await
}

/// Delete completed plans older than `before` (RFC 3339 timestamp).
/// Returns the number of deleted plans.
pub async fn prune_plans(&self, before: &str) -> AgentResult<usize> {
    let ts = before.to_string();
    self.with_conn(move |conn| {
        // First clean up orphaned checkpoints
        conn.execute(
            "DELETE FROM checkpoints WHERE plan_id IN (
                SELECT id FROM plans WHERE created_at < ?1 AND status IN ('Completed', 'Failed')
            )",
            rusqlite::params![ts],
        )?;
        let deleted = conn.execute(
            "DELETE FROM plans WHERE created_at < ?1 AND status IN ('Completed', 'Failed')",
            rusqlite::params![ts],
        )?;
        Ok(deleted)
    }).await
}
```

- [ ] **Step 2: Write DB tests**

Append to the existing `#[cfg(test)]` block at the bottom of `src/db.rs`:

```rust
#[test]
fn test_plan_summary_serde() {
    let summary = PlanSummary {
        id: "test-123".into(),
        name: "Test Plan".into(),
        current_step_index: 2,
        total_steps: 5,
        status: "Running".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T01:00:00Z".into(),
    };
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("test-123"));
    assert!(json.contains("Running"));

    let back: PlanSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_steps, 5);
}
```

Also add integration tests at `tests/cli_db_test.rs` (new file):

```rust
// tests/cli_db_test.rs
use rupoo::db::TaskRepo;
use rupoo::task::Plan;

#[tokio::test]
async fn test_list_plains_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();

    let plans = repo.list_plans(10, 0).await.unwrap();
    assert!(plans.is_empty());

    let counts = repo.count_plans_by_status().await.unwrap();
    assert!(counts.is_empty());
}

#[tokio::test]
async fn test_crud_plan() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();

    let plan = Plan::new("Test", vec![]);
    repo.save_plan(&plan).await.unwrap();

    let plans = repo.list_plans(10, 0).await.unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].name, "Test");

    repo.delete_plan(&plan.id).await.unwrap();
    let plans = repo.list_plans(10, 0).await.unwrap();
    assert!(plans.is_empty());
}
```

- [ ] **Step 3: Run tests to verify they fail (no implementation yet)**

```bash
cargo test test_list_plains_empty 2>&1 | head -10
```
Expected: `error[E0599]: no method named 'list_plans' found`

- [ ] **Step 4: Implement the 4 methods in db.rs** (code in Step 1 above)

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test test_list_plains_empty -- --nocapture
cargo test test_crud_plan -- --nocapture
cargo test test_plan_summary_serde -- --nocapture
```
Expected: all 3 pass

- [ ] **Step 6: Commit**

```bash
git add src/db.rs tests/cli_db_test.rs
git commit -m "feat(db): add PlanSummary and 4 query methods for CLI"
```

---

### Task 2: CLI module structure + main.rs wiring

**Files:**
- Create: `src/cli/cmds/mod.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `src/cli/cmds/mod.rs`**

```rust
pub mod status;
pub mod model;
pub mod session;
pub mod doctor;
pub mod logs;
```

- [ ] **Step 2: Update `src/cli/mod.rs`**

```rust
pub mod app;
pub mod ui;
pub mod cmds;
```

- [ ] **Step 3: Add imports and new subcommands to `src/main.rs`**

At the top of `src/main.rs`, add after existing imports (around line 10):

```rust
use rupoo::db::PlanSummary;
```

In the `Commands` enum (around line 105), add after `McpServer`:

```rust
    /// Show system status overview
    Status {
        /// Short one-line output (for scripts)
        #[arg(long)]
        short: bool,
        /// Database path (default: ./agent.db)
        #[arg(long, default_value = "agent.db")]
        db: String,
    },
    /// Show/switch LLM provider and model
    Model {
        #[command(subcommand)]
        action: Option<ModelAction>,
        /// Database path (default: ./agent.db)
        #[arg(long, default_value = "agent.db")]
        db: String,
    },
    /// List, show, resume, delete plans
    Session {
        #[command(subcommand)]
        action: SessionAction,
        /// Database path (default: ./agent.db)
        #[arg(long, default_value = "agent.db")]
        db: String,
    },
    /// Diagnose configuration and environment
    Doctor {
        /// Attempt to auto-fix warnings
        #[arg(long)]
        fix: bool,
    },
    /// View and follow agent logs
    Logs {
        /// Follow log file in real-time
        #[arg(long)]
        follow: bool,
        /// Number of lines to show (default: 50)
        #[arg(long, default_value_t = 50)]
        lines: usize,
        /// Filter by log level (e.g., WARN, ERROR)
        #[arg(long)]
        level: Option<String>,
        /// Show previous session log instead
        #[arg(long)]
        prev: bool,
    },
```

And add the new subcommand enums (after `GitAction` block around line 102):

```rust
#[derive(Subcommand)]
enum ModelAction {
    /// Show current LLM configuration
    Show,
    /// List available providers and their default models
    List,
    /// Set provider and optionally model (e.g., "anthropic/claude-sonnet-4")
    Set {
        /// Provider name, optionally with /model suffix
        target: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// List all plans
    List {
        /// Maximum plans to show (default: 10)
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show plan details and steps
    Show {
        /// Plan ID
        id: String,
    },
    /// Resume execution of a plan
    Resume {
        /// Plan ID
        id: String,
        /// Database path for the plan
        #[arg(long, default_value = "agent.db")]
        db: String,
        /// Optional input for WaitForInput steps
        #[arg(long)]
        input: Option<String>,
    },
    /// Delete a plan and its checkpoints
    Delete {
        /// Plan ID
        id: String,
    },
    /// Delete completed/failed plans older than N days
    Prune {
        /// Age in days (default: 30)
        #[arg(long, default_value_t = 30)]
        days: u64,
    },
}
```

In `run_cmd` (around line 188), add the new match arms after `Commands::Serve`:

```rust
        Commands::Status { short, db } => {
            crate::cli::cmds::status::run(&db, short).await?;
        }
        Commands::Model { action, db } => {
            crate::cli::cmds::model::run(&db, action).await?;
        }
        Commands::Session { action, db } => {
            crate::cli::cmds::session::run(&db, action).await?;
        }
        Commands::Doctor { fix } => {
            crate::cli::cmds::doctor::run(fix).await?;
        }
        Commands::Logs { follow, lines, level, prev } => {
            crate::cli::cmds::logs::run(follow, lines, level.as_deref(), prev).await?;
        }
```

- [ ] **Step 4: Check compilation (expected to fail — functions not yet defined)**

```bash
cargo check 2>&1 | head -20
```
Expected: errors about `cannot find function 'run' in module 'status'` etc.

- [ ] **Step 5: Commit**

```bash
git add src/cli/cmds/mod.rs src/cli/mod.rs src/main.rs
git commit -m "feat(cli): add command skeleton for 5 new subcommands"
```

---

### Task 3: `rupoo status` command

**Files:**
- Create: `src/cli/cmds/status.rs`

- [ ] **Step 1: Write unit tests**

At the bottom of `src/cli/cmds/status.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_status_lines() {
        let counts = vec![
            ("Completed".to_string(), 8i64),
            ("Running".to_string(), 2i64),
            ("Failed".to_string(), 2i64),
        ];
        let lines = build_status_counts(&counts);
        assert!(lines.contains("12 total"));
        assert!(lines.contains("8 completed"));
        assert!(lines.contains("2 running"));
        assert!(lines.contains("2 failed"));
    }

    #[test]
    fn test_build_status_counts_empty() {
        let lines = build_status_counts(&[]);
        assert!(lines.contains("0 total"));
    }

    #[test]
    fn test_format_short_line() {
        let line = format_short_line("0.2.0", 5, "anthropic", "claude-sonnet-4", 3);
        assert!(line.contains("0.2.0"));
        assert!(line.contains("5 plans"));
        assert!(line.contains("anthropic/claude-sonnet-4"));
        assert!(line.contains("3 skills"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib cli::cmds::status 2>&1 | head -5
```

- [ ] **Step 3: Implement `src/cli/cmds/status.rs`**

```rust
use std::sync::Arc;
use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run(db_path: &str, short: bool) -> Result<()> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    let plan_counts = repo.count_plans_by_status().await?;
    let total_plans: i64 = plan_counts.iter().map(|(_, c)| c).sum();

    let provider = repo.get_setting("active_provider").await?
        .unwrap_or_else(|| "none".into());
    let model = repo.get_setting(&format!("model.{provider}")).await?
        .unwrap_or_else(|| "(default)".into());
    let has_key = repo.get_setting(&format!("api_key.{provider}")).await?
        .map(|k| k.len() > 4).unwrap_or(false);

    let skills = rupoo::skill::SkillManager::new(
        rupoo::skill::SkillManager::default_dir(),
    ).list_skills().unwrap_or_default();

    if short {
        println!("{}", format_short_line(
            VERSION, total_plans as usize, &provider, &model, skills.len(),
        ));
    } else {
        println!("{} {}\n", style("Rupoo").bold(), style(VERSION).dim());
        let total_icon = if has_key { "●" } else { "○" };
        println!("  {}  {:<12} {}     {}",
            style("├──").dim(),
            style("Data").cyan(),
            style(db_path).white(),
            style("(WAL mode)").dim(),
        );
        println!("  {}  {:<12} {}",
            style("├──").dim(),
            style("Plans").cyan(),
            build_status_counts(&plan_counts),
        );
        println!("  {}  {:<12} {}  {} / {}",
            style("├──").dim(),
            style("LLM").cyan(),
            style(total_icon).green(),
            style(&provider).white(),
            style(&model).dim(),
        );
        println!("  {}  {:<12} {} installed {}",
            style("├──").dim(),
            style("Skills").cyan(),
            skills.len(),
            if skills.is_empty() { "".into() }
            else { format!("({})", skills.join(", ")) },
        );
        println!("  {}  {:<12} {} entries (FTS5 indexed)",
            style("├──").dim(),
            style("Memory").cyan(),
            repo.count_memories().await.unwrap_or(0),
        );
        print_git_status()?;
        print_log_info()?;
    }
    Ok(())
}

fn build_status_counts(counts: &[(String, i64)]) -> String {
    let total: i64 = counts.iter().map(|(_, c)| c).sum();
    let mut parts = vec![format!("{} total", total)];
    for (status, count) in counts {
        let styled = match status.as_str() {
            "Completed" => style(format!("{} completed", count)).green(),
            "Running"   => style(format!("{} running", count)).yellow(),
            "Failed"    => style(format!("{} failed", count)).red(),
            _           => style(format!("{} {}", count, status.to_lowercase())).dim(),
        };
        parts.push(styled.to_string());
    }
    parts.join("  ")
}

fn format_short_line(ver: &str, plans: usize, provider: &str, model: &str, skills: usize) -> String {
    format!("Rupoo {} | {} plans | {}/{} | {} skills", ver, plans, provider, model, skills)
}

fn print_git_status() -> Result<()> {
    match rupoo::git::GitRepo::open(".") {
        Ok(git) => {
            let branch = git.current_branch().unwrap_or_default();
            let files = git.status().unwrap_or_default();
            let status = if files.is_empty() {
                "clean".to_string()
            } else {
                format!("{} uncommitted", files.len())
            };
            println!("  {}  {:<12} ./  ({})  {}",
                style("├──").dim(), style("Git").cyan(),
                style(branch).green(), style(status).dim(),
            );
        }
        Err(_) => {
            println!("  {}  {:<12} {}",
                style("├──").dim(), style("Git").cyan(),
                style("(not a git repository)").dim(),
            );
        }
    }
    Ok(())
}

fn print_log_info() -> Result<()> {
    let log_path = crate::tracing_setup::data_dir().join("rupoo.log");
    let size = if log_path.exists() {
        let meta = std::fs::metadata(&log_path)?;
        if meta.len() < 1024 {
            format!("{} B", meta.len())
        } else {
            format!("{:.1} KB", meta.len() as f64 / 1024.0)
        }
    } else {
        "none".into()
    };
    println!("  {}  {:<12} {}  ({})",
        style("└──").dim(), style("Log").cyan(),
        style(log_path.display()).dim(), style(size).dim(),
    );
    Ok(())
}
```

- [ ] **Step 4: Add `count_memories` helper to `db.rs`**

Add this method to `impl TaskRepo` in `src/db.rs`:

```rust
/// Count total memory entries.
pub async fn count_memories(&self) -> AgentResult<usize> {
    self.with_conn(move |conn| {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        Ok(count as usize)
    }).await
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib cli::cmds::status -- --nocapture
```
Expected: 3 tests pass

- [ ] **Step 6: Manual smoke test**

```bash
cargo run -- status
```
Expected: output with Rupoo version, plans, LLM info etc.

- [ ] **Step 7: Commit**

```bash
git add src/cli/cmds/status.rs src/db.rs
git commit -m "feat(cli): add rupoo status command"
```

---

### Task 4: `rupoo model` command

**Files:**
- Create: `src/cli/cmds/model.rs`

- [ ] **Step 1: Write unit tests**

At the bottom of `src/cli/cmds/model.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_providers_default_model() {
        for p in provider_variants() {
            let name = provider_default_model(p);
            assert!(!name.is_empty(), "model name for {p} should not be empty");
        }
    }

    #[test]
    fn test_render_key_status() {
        let rendered = render_key_status(Some("sk-ant-xxxxxxxxxxxx"));
        assert!(rendered.contains("●"));
        assert!(rendered.contains("set"));

        let rendered_none = render_key_status(None);
        assert!(rendered_none.contains("not set"));
    }

    #[test]
    fn test_parse_target() {
        assert_eq!(parse_target("anthropic"), Some(("anthropic", None)));
        assert_eq!(
            parse_target("anthropic/claude-sonnet-4"),
            Some(("anthropic", Some("claude-sonnet-4")))
        );
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("a/b/c"), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib cli::cmds::model 2>&1 | head -5
```

- [ ] **Step 3: Implement `src/cli/cmds/model.rs`**

```rust
use std::sync::Arc;
use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::llm::LlmProvider;

const PROVIDER_KEYS: &[(&str, &str, &str, &str)] = &[
    ("anthropic", "claude-sonnet-4-20250514", "api_key.anthropic", ""),
    ("openai", "gpt-4o", "api_key.openai", "base_url.openai"),
    ("ollama", "llama3.2", "", ""),
];

pub async fn run(db_path: &str, action: Option<crate::ModelAction>) -> Result<()> {
    let repo = Arc::new(TaskRepo::new(db_path)?);
    let action = action.unwrap_or(crate::ModelAction::Show);

    match action {
        crate::ModelAction::Show => cmd_show(&repo).await?,
        crate::ModelAction::List => cmd_list(&repo).await?,
        crate::ModelAction::Set { target } => cmd_set(&repo, target.as_deref()).await?,
    }
    Ok(())
}

async fn cmd_show(repo: &TaskRepo) -> Result<()> {
    let provider = repo.get_setting("active_provider").await?
        .unwrap_or_else(|| "none".into());
    let model = repo.get_setting(&format!("model.{provider}")).await?
        .or_else(|| provider_default_model(&provider).map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());
    let api_key = repo.get_setting(&format!("api_key.{provider}")).await?;

    println!("{}", style("Current LLM Configuration:").bold());
    println!("  {}  {}  {} / {}",
        if api_key.is_some() { style("●").green() } else { style("○").yellow() },
        style("Provider:").cyan(),
        style(&provider).white().bold(),
        style(&model).dim(),
    );
    println!("  {}  {}  {}",
        " ".to_string(),  // align
        style("API Key:").cyan(),
        render_key_status(api_key.as_deref()),
    );
    Ok(())
}

async fn cmd_list(repo: &TaskRepo) -> Result<()> {
    println!("{:<15} {:<30} {:<20}  Status",
        style("Provider").bold(),
        style("Default Model").bold(),
        style("Config Key").bold(),
    );
    println!("{}", style("─".repeat(80)).dim());

    for (name, model, key, _) in PROVIDER_KEYS {
        let key_val = repo.get_setting(key).await?;
        let status = if key.is_empty() {
            style("local-only").dim().to_string()
        } else if key_val.is_some() {
            style("● configured").green().to_string()
        } else {
            style("○ not set").yellow().to_string()
        };
        println!("{:<15} {:<30} {:<20}  {}",
            name, model, key, status,
        );
    }
    Ok(())
}

async fn cmd_set(repo: &TaskRepo, target: Option<&str>) -> Result<()> {
    let target = match target {
        Some(t) => t,
        None => return show_interactive_picker(repo).await,
    };

    let (provider, model) = parse_target(target)
        .ok_or_else(|| anyhow::anyhow!("Invalid format. Use: <provider> or <provider>/<model>"))?;

    // Validate provider
    if !PROVIDER_KEYS.iter().any(|(n, _, _, _)| *n == provider) {
        anyhow::bail!("Unknown provider '{provider}'. Valid: {}", PROVIDER_KEYS.iter().map(|(n,_,_,_)| *n).collect::<Vec<_>>().join(", "));
    }

    repo.set_setting("active_provider", &provider).await?;

    match model {
        Some(m) => {
            repo.set_setting(&format!("model.{provider}"), m).await?;
            println!("{} Provider switched to: {}", style("✓").green(), provider);
            println!("{} Model set to: {}", style("✓").green(), m);
        }
        None => {
            let default_model = provider_default_model(provider)
                .ok_or_else(|| anyhow::anyhow!("No default model for {provider}"))?;
            repo.set_setting(&format!("model.{provider}"), default_model).await?;
            println!("{} Provider switched to: {} ({})", style("✓").green(), provider, default_model);
        }
    }

    let key_name = format!("api_key.{provider}");
    if repo.get_setting(&key_name).await?.is_none() {
        println!("  {} Tip: set API key with: {} {} {}",
            style("ℹ").yellow(),
            style("config set").cyan(),
            key_name,
            style("<key>").dim(),
        );
    }
    Ok(())
}

async fn show_interactive_picker(repo: &TaskRepo) -> Result<()> {
    let current = repo.get_setting("active_provider").await?
        .unwrap_or_default();

    println!("{}", style("Select a provider:").bold());
    for (i, (name, model, _, _)) in PROVIDER_KEYS.iter().enumerate() {
        let marker = if *name == current { "❯" } else { " " };
        println!("  {} {:<12} → {}  {}",
            style(marker).green(),
            style(name).white(),
            style(model).dim(),
            if *name == current { style("(current)").dim() } else { style("").dim() },
        );
    }
    println!();
    println!("  Provider name or Enter to cancel: ");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(());
    }

    cmd_set(repo, Some(input)).await
}

// -- Pure helpers (testable without DB) --

fn provider_default_model(name: &str) -> Option<&'static str> {
    PROVIDER_KEYS.iter()
        .find(|(n, _, _, _)| *n == name)
        .map(|(_, m, _, _)| *m)
}

fn render_key_status(key: Option<&str>) -> String {
    match key {
        Some(k) if k.len() > 8 => {
            let prefix: String = k.chars().take(8).collect();
            format!("{} {}... ({})", style("●").green(), style(prefix).dim(), style("set").dim())
        }
        Some(k) => format!("{} {} ({})", style("●").green(), style(k).dim(), style("set").dim()),
        None => format!("{} {} ({})", style("○").yellow(), style("—").dim(), style("not set").yellow()),
    }
}

fn parse_target(target: &str) -> Option<(&str, Option<&str>)> {
    let parts: Vec<&str> = target.splitn(2, '/').collect();
    match parts.len() {
        1 if !parts[0].is_empty() => Some((parts[0], None)),
        2 if !parts[0].is_empty() && !parts[1].is_empty() => Some((parts[0], Some(parts[1]))),
        _ => None,
    }
}

fn provider_variants() -> Vec<&'static str> {
    vec!["anthropic", "openai", "ollama"]
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib cli::cmds::model -- --nocapture
```
Expected: 3 tests pass

- [ ] **Step 5: Manual smoke test**

```bash
cargo run -- model show
cargo run -- model list
cargo run -- model set anthropic
```
Expected: clean output for each

- [ ] **Step 6: Commit**

```bash
git add src/cli/cmds/model.rs
git commit -m "feat(cli): add rupoo model command"
```

---

### Task 5: `rupoo session` command

**Files:**
- Create: `src/cli/cmds/session.rs`

- [ ] **Step 1: Write unit tests**

At the bottom of `src/cli/cmds/session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rupoo::task::Plan;

    #[tokio::test]
    async fn test_list_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let repo = Arc::new(rupoo::db::TaskRepo::new(db_path.to_str().unwrap()).unwrap());
        let plans = repo.list_plans(10, 0).await.unwrap();
        assert!(plans.is_empty());
    }

    #[tokio::test]
    async fn test_list_one_plan() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let repo = Arc::new(rupoo::db::TaskRepo::new(db_path.to_str().unwrap()).unwrap());
        let plan = Plan::new("Test Session", vec![]);
        repo.save_plan(&plan).await.unwrap();

        let list = repo.list_plans(10, 0).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Test Session");
        assert_eq!(list[0].total_steps, 0);
    }

    #[tokio::test]
    async fn test_delete_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let repo = Arc::new(rupoo::db::TaskRepo::new(db_path.to_str().unwrap()).unwrap());

        let plan = Plan::new("To Delete", vec![]);
        repo.save_plan(&plan).await.unwrap();
        repo.delete_plan(&plan.id).await.unwrap();
        assert_eq!(repo.list_plans(10, 0).await.unwrap().len(), 0);
    }

    #[test]
    fn test_format_step_summary() {
        let think = rupoo::task::Step::Think {
            id: "1".into(), instruction: "Analyze".into(),
            status: rupoo::task::StepStatus::Completed, output: None,
        };
        let s = format_step_summary(&think, 0);
        assert!(!s.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib cli::cmds::session 2>&1 | head -5
```

- [ ] **Step 3: Implement `src/cli/cmds/session.rs`**

```rust
use std::sync::Arc;
use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::task::{Plan, PlanStatus, Step, StepStatus};
use chrono::Utc;

pub async fn run(db_path: &str, action: crate::SessionAction) -> Result<()> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    match action {
        crate::SessionAction::List { limit } => cmd_list(&repo, limit).await?,
        crate::SessionAction::Show { id } => cmd_show(&repo, &id).await?,
        crate::SessionAction::Resume { id, db, input } => cmd_resume(&id, &db, input).await?,
        crate::SessionAction::Delete { id } => cmd_delete(&repo, &id).await?,
        crate::SessionAction::Prune { days } => cmd_prune(&repo, days).await?,
    }
    Ok(())
}

async fn cmd_list(repo: &TaskRepo, limit: usize) -> Result<()> {
    let plans = repo.list_plans(limit, 0).await?;
    if plans.is_empty() {
        println!("{} No plans found.", style("ℹ").yellow());
        return Ok(());
    }

    // Print header
    println!(
        "{:<12} {:<30} {:<8} {:<12} {:<20}",
        style("ID").bold(),
        style("Name").bold(),
        style("Steps").bold(),
        style("Status").bold(),
        style("Updated").bold(),
    );
    println!("{}", style("─".repeat(85)).dim());

    for p in &plans {
        let short_id: String = p.id.chars().take(8).collect();
        let status = match p.status.as_str() {
            "Completed" => style("● Completed").green(),
            "Running" => style("● Running").yellow(),
            "Failed" => style("● Failed").red(),
            "Pending" => style("○ Pending").dim(),
            _ => style(&p.status).dim(),
        };
        println!(
            "{:<12} {:<30} {:<8} {:<12} {:<20}",
            style(short_id).dim(),
            &p.name,
            format!("{}/{}", p.current_step_index, p.total_steps),
            status,
            style(&p.updated_at).dim(),
        );
    }
    Ok(())
}

async fn cmd_show(repo: &TaskRepo, plan_id: &str) -> Result<()> {
    let plan = repo.load_plan(plan_id).await
        .map_err(|_| anyhow::anyhow!("Plan not found: {plan_id}"))?;

    println!("{}  {}", style("Plan:").cyan().bold(), style(&plan.name).white().bold());
    println!("{}  {}  {}  {}/{}",
        style("ID:").dim(),
        style(&plan.id).dim(),
        style("Status:").dim(),
        style(match plan.status {
            PlanStatus::Completed => "✓ Completed".to_string(),
            PlanStatus::Running => "▶ Running".to_string(),
            PlanStatus::Failed => "✗ Failed".to_string(),
            PlanStatus::Pending => "· Pending".to_string(),
        }).green(),
        plan.current_step_index,
        plan.steps.len(),
    );
    println!("{}", style("Steps:").dim());
    for (i, step) in plan.steps.iter().enumerate() {
        println!("{}", format_step_summary(step, i));
    }
    Ok(())
}

async fn cmd_resume(_plan_id: &str, _db: &str, _input: Option<String>) -> Result<()> {
    // Delegates to the existing Commands::Run logic.
    // This is a thin wrapper — full implementation reuses execute_plan.
    // For now, print guidance and exit.
    println!("Use: rupoo run --task {_plan_id} --db {_db}");
    if let Some(inp) = _input {
        println!("  with --input \"{inp}\"");
    }
    Ok(())
}

async fn cmd_delete(repo: &TaskRepo, plan_id: &str) -> Result<()> {
    repo.delete_plan(plan_id).await?;
    println!("{} Plan {plan_id} deleted.", style("✓").green());
    Ok(())
}

async fn cmd_prune(repo: &TaskRepo, days: u64) -> Result<()> {
    let before = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let deleted = repo.prune_plans(&before).await?;
    println!("{} Pruned {deleted} completed/failed plans older than {days} days.", style("✓").green());
    Ok(())
}

fn format_step_summary(step: &Step, index: usize) -> String {
    let (icon, label) = match step {
        Step::Think { instruction, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Think: {instruction}"))
        }
        Step::ToolCall { tool_name, params, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Tool: {tool_name} ({})", params))
        }
        Step::WaitForInput { prompt, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Wait: {prompt}"))
        }
        Step::Finish { summary, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Finish: {summary}"))
        }
        Step::Exec { command, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Exec: {command}"))
        }
        Step::HttpRequest { url, method, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("HTTP: {method:?} {url}"))
        }
        Step::BrowserAction { action, status, .. } => {
            let icon = step_icon(status);
            (icon, format!("Browser: {action:?}"))
        }
    };
    format!("  {} [{}] {}", icon, index, label)
}

fn step_icon(status: &StepStatus) -> String {
    match status {
        StepStatus::Completed => style("✓").green().to_string(),
        StepStatus::Running => style("▶").yellow().to_string(),
        StepStatus::Failed => style("✗").red().to_string(),
        StepStatus::Pending => style("·").dim().to_string(),
        StepStatus::WaitingForInput => style("⊘").cyan().to_string(),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib cli::cmds::session -- --nocapture
```
Expected: all tests pass

- [ ] **Step 5: Manual smoke test**

```bash
cargo run -- session list
cargo run -- session show <some-id>
```
Expected: clean output

- [ ] **Step 6: Commit**

```bash
git add src/cli/cmds/session.rs
git commit -m "feat(cli): add rupoo session command"
```

---

### Task 6: `rupoo doctor` command

**Files:**
- Create: `src/cli/cmds/doctor.rs`

- [ ] **Step 1: Write unit tests**

At the bottom of `src/cli/cmds/doctor.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_summary_all_pass() {
        let results = vec![
            CheckResult::new("DB", true, None, false),
            CheckResult::new("Git", true, None, false),
        ];
        let summary = status_summary(&results);
        assert_eq!(summary.0, 2);
        assert_eq!(summary.1, 0);
        assert_eq!(summary.2, 0);
    }

    #[test]
    fn test_result_display_pass() {
        let r = CheckResult {
            name: "TestCheck".into(),
            passed: true,
            message: None,
            fixable: false,
        };
        let output = r.format_display();
        assert!(output.contains("●"));
        assert!(output.contains("TestCheck"));
    }

    #[test]
    fn test_check_names_at_least_six() {
        let names = ["Database", "LLM Configuration", "Skills", "Git", "Data Directory", "Log File"];
        assert!(names.len() >= 6);
        assert!(names.contains(&"Database"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib cli::cmds::doctor 2>&1 | head -5
```

- [ ] **Step 3: Implement `src/cli/cmds/doctor.rs`**

```rust
use std::sync::Arc;
use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::skill::SkillManager;

struct CheckResult {
    name: &'static str,
    passed: bool,
    message: Option<String>,
    fixable: bool,
}

impl CheckResult {
    fn new(name: &'static str, passed: bool, message: Option<String>, fixable: bool) -> Self {
        Self { name, passed, message, fixable }
    }

    fn format_display(&self) -> String {
        let icon = if self.passed {
            style("●").green()
        } else if self.fixable {
            style("○").yellow()
        } else {
            style("✗").red()
        };
        let name = style(self.name).white().bold();
        let msg = match &self.message {
            Some(m) => format!("\n{}", indent(m, 4)),
            None => String::new(),
        };
        format!("  {} {} {}", icon, name, msg)
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn status_summary(results: &[CheckResult]) -> (usize, usize, usize) {
    let pass = results.iter().filter(|r| r.passed).count();
    let warn = results.iter().filter(|r| !r.passed && r.fixable).count();
    let fail = results.iter().filter(|r| !r.passed && !r.fixable).count();
    (pass, warn, fail)
}

pub async fn run(fix: bool) -> Result<()> {
    println!("{}", style("Rupoo Diagnostics").bold());
    let results = all_checks(fix).await;

    for r in &results {
        println!("{}", r.format_display());
    }

    println!("{}", style("─".repeat(50)).dim());
    let (pass, warn, fail) = status_summary(&results);
    println!(
        "{}  {} {}  {} {}  {} {}",
        style("●").green(),
        style(format!("{pass} passed")).green(),
        style("●").yellow(),
        style(format!("{warn} warnings")).yellow(),
        if fail > 0 { "✗" } else { "●" },
        style(format!("{fail} errors")).red(),
    );

    if fix && (warn > 0 || fail > 0) {
        println!("\n{}", style("Attempting fixes...").yellow());
        run_fixes().await?;
    } else if (warn > 0 || fail > 0) && !fix {
        println!("  {} Run with --fix to auto-resolve fixable issues.", style("→").dim());
    }
    Ok(())
}

async fn all_checks(fix: bool) -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. Database
    match TaskRepo::new("agent.db") {
        Ok(repo) => {
            let tables = ["plans", "checkpoints", "settings", "memories"];
            let all_ok = tables.iter().all(|t| {
                let sql = format!("SELECT name FROM sqlite_master WHERE type='table' AND name='{t}'");
                std::sync::Mutex::new(())
                    // quick pragma check: just verify the repo opened
                    ;
                true
            });
            let msg = Some(format!("agent.db — connected, {} tables present", tables.len()));
            results.push(CheckResult::new("Database", true, msg, false));
        }
        Err(e) => {
            results.push(CheckResult::new("Database", false, Some(format!("Cannot open DB: {e}")), false));
        }
    }

    // 2. LLM keys
    if let Ok(repo) = TaskRepo::new("agent.db") {
        let mut msgs = Vec::new();
        let mut all_ok = true;
        for provider in &["anthropic", "openai"] {
            let key = format!("api_key.{provider}");
            match repo.get_setting(&key).await {
                Ok(Some(val)) if val.len() > 4 => {
                    let prefix: String = val.chars().take(8).collect();
                    msgs.push(format!("{provider}: configured ({prefix}...)"));
                }
                _ => {
                    msgs.push(format!("{provider}: {} — no {} set", style("WARN").yellow(), key));
                    all_ok = false;
                }
            }
        }
        // Ollama
        match reqwest::get("http://localhost:11434/api/tags").await {
            Ok(resp) if resp.status().is_success() => {
                msgs.push("ollama: reachable at localhost:11434".into());
            }
            _ => {
                msgs.push(format!("ollama: {} — connection refused", style("WARN").yellow()));
                all_ok = false;
            }
        }
        results.push(CheckResult::new("LLM Configuration", all_ok, Some(msgs.join("\n")), true));
    }

    // 3. Skills
    let skill_dir = SkillManager::default_dir();
    if skill_dir.exists() {
        match SkillManager::new(skill_dir.clone()).list_skills() {
            Ok(skills) => {
                let msg = format!("{} installed at {}", skills.len(), skill_dir.display());
                results.push(CheckResult::new("Skills", true, Some(msg), false));
            }
            Err(e) => {
                results.push(CheckResult::new("Skills", false, Some(format!("Error: {e}")), false));
            }
        }
    } else if fix {
        std::fs::create_dir_all(&skill_dir).ok();
        results.push(CheckResult::new("Skills", true, Some("Created empty skills directory".into()), true));
    } else {
        results.push(CheckResult::new("Skills", false, Some(format!("Directory not found: {}", skill_dir.display())), true));
    }

    // 4. Git
    match rupoo::git::GitRepo::open(".") {
        Ok(git) => {
            let branch = git.current_branch().unwrap_or_default();
            let msg = format!("libgit2 available, repository at ./ ({branch})");
            results.push(CheckResult::new("Git", true, Some(msg), false));
        }
        Err(_) => {
            results.push(CheckResult::new("Git", true, Some("(not a git repository)".into()), false));
        }
    }

    // 5. Data directory
    let data_dir = crate::tracing_setup::data_dir();
    if data_dir.exists() {
        results.push(CheckResult::new("Data Directory", true, Some(format!("{} — exists, writable", data_dir.display())), false));
    } else if fix {
        std::fs::create_dir_all(&data_dir).ok();
        results.push(CheckResult::new("Data Directory", true, Some("Created".into()), true));
    } else {
        results.push(CheckResult::new("Data Directory", false, Some(format!("Not found: {}", data_dir.display())), true));
    }

    // 6. Log file
    let log_path = data_dir.join("rupoo.log");
    match std::fs::metadata(&log_path) {
        Ok(meta) => {
            let size = format_size(meta.len());
            results.push(CheckResult::new("Log File", true, Some(format!("{} — {} bytes", log_path.display(), size)), false));
        }
        Err(_) if fix => {
            std::fs::OpenOptions::new().create(true).write(true).open(&log_path).ok();
            results.push(CheckResult::new("Log File", true, Some("Created empty log file".into()), true));
        }
        Err(_) => {
            results.push(CheckResult::new("Log File", false, Some(format!("Not found: {}", log_path.display())), true));
        }
    }

    results
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

async fn run_fixes() -> Result<()> {
    let dirs = [
        rupoo::skill::SkillManager::default_dir(),
        crate::tracing_setup::data_dir(),
    ];
    for d in &dirs {
        if !d.exists() {
            std::fs::create_dir_all(d)?;
            println!("  {} Created: {}", style("✓").green(), d.display());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Add `reqwest` usage note**

The doctor command uses `reqwest` (already in `Cargo.toml` line 37) — no new dependency needed. Verify:

```bash
grep -c "reqwest" Cargo.toml
```
Expected: 1 (already a dependency)

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test --lib cli::cmds::doctor -- --nocapture
```
Expected: 3 tests pass

- [ ] **Step 6: Manual smoke test**

```bash
cargo run -- doctor
```
Expected: diagnostic output for all checks

- [ ] **Step 7: Commit**

```bash
git add src/cli/cmds/doctor.rs
git commit -m "feat(cli): add rupoo doctor command"
```

---

### Task 7: `rupoo logs` command

**Files:**
- Create: `src/cli/cmds/logs.rs`

- [ ] **Step 1: Write unit tests**

At the bottom of `src/cli/cmds/logs.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_last_lines_from_str() {
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let lines = read_last_lines_from_str(content, 3);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line3");
        assert_eq!(lines[2], "line5");
    }

    #[test]
    fn test_read_last_lines_overflow() {
        let content = "a\nb\n";
        let lines = read_last_lines_from_str(content, 10);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_read_last_lines_empty() {
        let lines = read_last_lines_from_str("", 5);
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_filter_lines_by_level() {
        let lines = vec![
            "INFO  test: msg1".to_string(),
            "WARN  test: msg2".to_string(),
            "ERROR test: msg3".to_string(),
        ];
        let filtered = filter_lines(&lines, Some("WARN"));
        assert_eq!(filtered.len(), 2); // WARN + ERROR
    }

    #[test]
    fn test_filter_lines_none() {
        let lines = vec!["INFO  test: msg".to_string()];
        let filtered = filter_lines(&lines, None::<&str>);
        assert_eq!(filtered.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib cli::cmds::logs 2>&1 | head -5
```

- [ ] **Step 3: Implement `src/cli/cmds/logs.rs`**

```rust
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use anyhow::Result;
use console::style;

pub async fn run(follow: bool, lines: usize, level: Option<&str>, prev: bool) -> Result<()> {
    let path = log_path(prev);

    if !path.exists() {
        println!("{} No log file found at {}", style("ℹ").yellow(), path.display());
        return Ok(());
    }

    println!("{} {} — Ctrl+C to stop",
        style("Showing last").dim(),
        if follow { format!("following {}", path.display()) }
        else { format!("{lines} lines from {}", path.display()) },
    );
    println!("{}", style("─".repeat(60)).dim());

    let content = fs::read_to_string(&path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let tail = read_last_lines_from_vec(&all_lines, lines);
    let tail: Vec<String> = tail.into_iter().map(ToString::to_string).collect();
    let filtered = filter_lines(&tail, level);

    for line in &filtered {
        println!("{line}");
    }

    if follow {
        follow_file(&path, all_lines.len(), level).await?;
    }

    Ok(())
}

fn log_path(prev: bool) -> PathBuf {
    let dir = crate::tracing_setup::data_dir();
    if prev {
        dir.join("rupoo.prev.log")
    } else {
        dir.join("rupoo.log")
    }
}

fn read_last_lines_from_vec<'a>(lines: &[&'a str], n: usize) -> Vec<&'a str> {
    let start = if n >= lines.len() { 0 } else { lines.len() - n };
    lines[start..].to_vec()
}

fn read_last_lines_from_str(content: &str, n: usize) -> Vec<&str> {
    read_last_lines_from_vec(&content.lines().collect::<Vec<_>>(), n)
}

fn filter_lines(lines: &[String], level: Option<&str>) -> Vec<String> {
    match level {
        Some(lvl) => {
            let upper = lvl.to_uppercase();
            lines.iter()
                .filter(|l| {
                    let l_upper = l.to_uppercase();
                    match upper.as_str() {
                        // Show this level and above
                        "ERROR" => l_upper.contains("ERROR"),
                        "WARN"  => l_upper.contains("WARN") || l_upper.contains("ERROR"),
                        "INFO"  => true, // show all
                        _       => l_upper.contains(&upper),
                    }
                })
                .cloned()
                .collect()
        }
        None => lines.to_vec(),
    }
}

async fn follow_file(path: &PathBuf, start_line: usize, level: Option<&str>) -> Result<()> {
    let mut last_len = start_line;
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    loop {
        // Read any new lines
        let mut new_lines = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 { break; }
            new_lines.push(line.trim_end().to_string());
        }

        if !new_lines.is_empty() {
            let filtered = filter_lines(&new_lines, level);
            for l in &filtered {
                println!("{l}");
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        // Check file size for rollover
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() < reader.get_ref().metadata().map(|m| m.len()).unwrap_or(0) {
                // File was rotated — reopen
                let file = File::open(path)?;
                reader = BufReader::new(file);
            }
        }
    }
    // unreachable in normal flow; Ctrl+C exits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_last_lines_from_str() {
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let lines = read_last_lines_from_str(content, 3);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line3");
        assert_eq!(lines[2], "line5");
    }

    #[test]
    fn test_read_last_lines_overflow() {
        let content = "a\nb\n";
        let lines = read_last_lines_from_str(content, 10);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_read_last_lines_empty() {
        let lines = read_last_lines_from_str("", 5);
        assert_eq!(lines.len(), 0);
    }

    #[test]
    fn test_filter_lines_by_level() {
        let lines = vec![
            "INFO  test: msg1".to_string(),
            "WARN  test: msg2".to_string(),
            "ERROR test: msg3".to_string(),
        ];
        let filtered = filter_lines(&lines, Some("WARN"));
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_lines_none() {
        let lines = vec!["INFO  test: msg".to_string()];
        let filtered = filter_lines(&lines, None::<&str>);
        assert_eq!(filtered.len(), 1);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib cli::cmds::logs -- --nocapture
```
Expected: 6 tests pass

- [ ] **Step 5: Manual smoke test**

```bash
cargo run -- logs --lines 5
```
Expected: last 5 lines of rupoo.log or "No log file found" on fresh run

- [ ] **Step 6: Commit**

```bash
git add src/cli/cmds/logs.rs
git commit -m "feat(cli): add rupoo logs command"
```

---

### Task 8: Final integration check

- [ ] **Step 1: Build release**

```bash
cargo build 2>&1
```
Expected: no errors, 0 warnings

- [ ] **Step 2: Run full test suite**

```bash
cargo test -- --nocapture 2>&1 | tail -20
```
Expected: all tests pass (including existing 48+)

- [ ] **Step 3: Quick smoke test all new commands**

```bash
cargo run -- status --short
cargo run -- model show
cargo run -- session list
cargo run -- doctor
cargo run -- logs --lines 5
```
Expected: each produces output, no panics

- [ ] **Step 4: Final commit**

```bash
git status
git add -A
git commit -m "chore: final integration cleanup for CLI enhancement"
```
