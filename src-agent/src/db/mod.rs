//! Database module for Rupoo - Task Repository
//!
//! Split from db.rs (Phase 1 Step 2):
//! - mod.rs: TaskRepo struct + new() + PlanSummary
//! - plans.rs: Plan/Checkpoint CRUD operations
//! - settings.rs: Settings/Memory/ConversationHistory/UISession CRUD

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

pub mod loops;
pub mod plans;
pub mod settings;

// ---------------------------------------------------------------------------
// Schema migrations
// ---------------------------------------------------------------------------

/// Current database schema version (stored in SQLite `PRAGMA user_version`).
///
/// Every release that changes the schema MUST bump this constant and append
/// a matching migration to [`MIGRATIONS`]. Old databases are upgraded
/// automatically and transactionally on first open.
pub const SCHEMA_VERSION: i64 = 1;

/// Ordered list of schema migrations. `MIGRATIONS[i]` upgrades the database
/// from version `i` to version `i + 1`.
///
/// Migration 1 is the historical full-schema DDL. It uses `IF NOT EXISTS`
/// everywhere so that databases created before versioning (v0.6.3 and older)
/// upgrade cleanly without touching existing data.
const MIGRATIONS: &[&str] = &[
    // ── Migration 1: initial schema (plans, checkpoints, loops, cron, …) ──
    r#"
            CREATE TABLE IF NOT EXISTS plans (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                steps_json  TEXT NOT NULL,
                current_step_index INTEGER NOT NULL DEFAULT 0,
                status      TEXT NOT NULL DEFAULT 'Pending',
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS checkpoints (
                id          TEXT PRIMARY KEY,
                plan_id     TEXT NOT NULL,
                step_index  INTEGER NOT NULL,
                status      TEXT NOT NULL,
                output      TEXT,
                created_at  TEXT NOT NULL,
                FOREIGN KEY (plan_id) REFERENCES plans(id)
            );

            CREATE INDEX IF NOT EXISTS idx_checkpoints_plan
                ON checkpoints(plan_id, step_index);

            -- Loop Engineering tables
            CREATE TABLE IF NOT EXISTS loops (
                id              TEXT PRIMARY KEY,
                goal            TEXT NOT NULL,
                status          TEXT NOT NULL DEFAULT 'Pending',
                config_json     TEXT NOT NULL,
                current_run_id  TEXT,
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS loop_runs (
                id              TEXT PRIMARY KEY,
                loop_id         TEXT NOT NULL,
                iteration       INTEGER NOT NULL,
                plan_id         TEXT,
                status          TEXT NOT NULL,
                evaluation_json TEXT,
                decision        TEXT,
                token_usage_json TEXT,
                started_at      INTEGER NOT NULL,
                finished_at     INTEGER,
                UNIQUE(loop_id, iteration),
                FOREIGN KEY (loop_id) REFERENCES loops(id)
            );

            CREATE INDEX IF NOT EXISTS idx_loop_runs_loop_id
                ON loop_runs(loop_id);
            CREATE INDEX IF NOT EXISTS idx_loop_runs_status
                ON loop_runs(status);
            CREATE INDEX IF NOT EXISTS idx_loops_status
                ON loops(status);

            -- Cron scheduling table
            CREATE TABLE IF NOT EXISTS cron_jobs (
                id              TEXT PRIMARY KEY,
                name            TEXT NOT NULL,
                schedule        TEXT NOT NULL,
                task_message    TEXT NOT NULL,
                enabled         INTEGER NOT NULL DEFAULT 1,
                last_run_at     INTEGER,
                next_run_at     INTEGER,
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_cron_jobs_next_run
                ON cron_jobs(enabled, next_run_at);

            -- Key-value settings store (API keys, preferences)
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- UI session history for chat UI
            CREATE TABLE IF NOT EXISTS ui_sessions (
                id            TEXT PRIMARY KEY,
                label         TEXT NOT NULL,
                messages_json TEXT NOT NULL,
                is_active     INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );

            -- Conversation histories for multi-turn Chat Mode
            CREATE TABLE IF NOT EXISTS conversation_histories (
                session_id    TEXT PRIMARY KEY,
                history_json  TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );

            -- FTS5-based memory store for long-term memory
            -- content_id stores the UUID (rowid is auto-increment integer)
            CREATE VIRTUAL TABLE IF NOT EXISTS memories USING fts5(
                content,
                tags,
                source     UNINDEXED,
                created_at UNINDEXED,
                updated_at UNINDEXED,
                content_id UNINDEXED,
                tokenize='unicode61'
            );

            -- Performance optimization: composite indexes
            -- Note: FTS5 virtual tables cannot have additional indexes
            -- Plans: index for status + created_at queries
            CREATE INDEX IF NOT EXISTS idx_plans_status_created
                ON plans(status, created_at DESC);

            -- UI Sessions: index for active sessions + updated_at
            CREATE INDEX IF NOT EXISTS idx_sessions_active
                ON ui_sessions(is_active, updated_at DESC);

            -- Conversation histories: index for updated_at
            CREATE INDEX IF NOT EXISTS idx_conversations_updated
                ON conversation_histories(updated_at DESC);

            -- Audit events (supervisor subsystem)
            CREATE TABLE IF NOT EXISTS audit_events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type  TEXT NOT NULL,
                result      TEXT NOT NULL,
                timestamp   TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_audit_events_type
                ON audit_events(event_type, timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_events_result
                ON audit_events(result, timestamp DESC);
            "#,
];

/// Apply all pending migrations to `conn`, atomically per migration, and
/// record the new schema version in `PRAGMA user_version`.
///
/// A database whose version is NEWER than this build is rejected: the user
/// must upgrade rupoo first (downgrading would risk data loss).
fn apply_migrations(conn: &rusqlite::Connection) -> AgentResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(AgentError::Other(format!(
            "database schema v{version} is newer than rupoo supports (v{SCHEMA_VERSION}); upgrade rupoo first"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let target = (i + 1) as i64;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {target};"))?;
        tx.commit()?;
        info!(schema_version = target, "database migration applied");
    }
    Ok(())
}

// Re-export for convenience
// PlanSummary is defined directly in this module — do not re-export from plans.rs

// ---------------------------------------------------------------------------
// TaskRepo - Core database repository
// ---------------------------------------------------------------------------

pub struct TaskRepo {
    /// Write connection with mutex protection.
    conn: Arc<Mutex<rusqlite::Connection>>,
    /// Database path for spawning read connections.
    db_path: String,
}

// ---------------------------------------------------------------------------
// PlanSummary (lightweight, no full Plan deserialization)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub current_step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl TaskRepo {
    /// Open (or create) the database at `db_path` and ensure tables exist.
    ///
    /// For file-based databases (not `:memory:`), restricts file permissions
    /// to owner-only (0o600 on Unix) to protect stored API keys and settings.
    ///
    /// Uses WAL mode for better concurrent read performance:
    /// - Write operations use the main connection with mutex protection
    /// - Read operations use a separate read-only connection for concurrent access
    pub fn new(db_path: &str) -> AgentResult<Self> {
        // Restrict file permissions before opening — protects stored API keys
        if db_path != ":memory:" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let path = std::path::Path::new(db_path);
                if path.exists() {
                    let perms = std::fs::Permissions::from_mode(0o600);
                    std::fs::set_permissions(path, perms)?;
                }
            }
        }

        let conn = rusqlite::Connection::open(db_path)?;
        // Enable WAL mode for better concurrent read performance
        // Additional PRAGMA optimizations for better performance:
        // - synchronous=NORMAL: balances safety and performance
        // - cache_size=64000: increase page cache to 64MB (each page is ~4KB)
        // - temp_store=MEMORY: use memory for temporary tables
        // - journal_size_limit=104857600: limit WAL file size to 100MB
        // - mmap_size=30000000000: enable memory-mapped I/O for large databases
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-64000;
             PRAGMA temp_store=MEMORY;
             PRAGMA journal_size_limit=104857600;
             PRAGMA mmap_size=30000000000;
             PRAGMA foreign_keys=ON;",
        )?;
        apply_migrations(&conn)?;
        info!(db_path, "database initialized");

        // For in-memory DB, we just open a new connection (they share memory)
        // For file-based DBs, we don't need to keep a persistent read connection
        // since rusqlite Connection is not Send+Sync. Instead, we spawn new connections
        // per read operation which is still efficient due to WAL mode.

        // Ensure restrictive permissions after creation (new files inherit umask)
        if db_path != ":memory:" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let path = std::path::Path::new(db_path);
                let perms = std::fs::Permissions::from_mode(0o600);
                let _ = std::fs::set_permissions(path, perms);
            }
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: db_path.to_string(),
        })
    }

    // ---------------------------------------------------------------------------
    // Internal helper: run a closure on the write connection via spawn_blocking
    // ---------------------------------------------------------------------------

    pub(crate) async fn with_conn<F, T>(&self, f: F) -> AgentResult<T>
    where
        F: FnOnce(&rusqlite::Connection) -> AgentResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|poisoned| {
                tracing::warn!(
                    db = %db_path,
                    "SQLite write mutex poisoned — another thread panicked while holding it; recovering"
                );
                poisoned.into_inner()
            });
            f(&guard)
        })
        .await
        .map_err(|e| AgentError::Join(e.to_string()))?
    }

    // ---------------------------------------------------------------------------
    // Internal helper: run a closure on a read connection (concurrent reads)
    // For file-based DBs, we spawn a new connection per read operation (WAL mode).
    // For in-memory DBs, we fall back to the main connection.
    // ---------------------------------------------------------------------------

    pub(crate) async fn with_read_conn<F, T>(&self, f: F) -> AgentResult<T>
    where
        F: FnOnce(&rusqlite::Connection) -> AgentResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let db_path = self.db_path.clone();

        // For in-memory databases, use the main connection (fallback to with_conn)
        if db_path == ":memory:" {
            return self.with_conn(f).await;
        }

        // For file-based DBs, open a new read-only connection
        tokio::task::spawn_blocking(move || {
            let read_conn = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|e| AgentError::Other(format!("failed to open read connection: {e}")))?;
            f(&read_conn)
        })
        .await
        .map_err(|e| AgentError::Join(e.to_string()))?
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an in-memory TaskRepo for testing.
    pub(super) fn repo() -> TaskRepo {
        TaskRepo::new(":memory:").unwrap()
    }

    fn schema_version(repo: &TaskRepo) -> i64 {
        let conn = repo.conn.lock().unwrap();
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_db_reaches_latest_schema_version() {
        assert_eq!(schema_version(&repo()), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_existing_db_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let p = path.to_str().unwrap();

        // First open runs migration 1; re-opens must be no-ops.
        TaskRepo::new(p).unwrap();
        TaskRepo::new(p).unwrap();
        let repo = TaskRepo::new(p).unwrap();
        assert_eq!(schema_version(&repo), SCHEMA_VERSION);
    }

    #[test]
    fn legacy_db_without_version_upgrades_to_latest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");

        // Simulate a pre-versioning database: full schema already created by
        // the old `CREATE TABLE IF NOT EXISTS` bootstrap, user_version = 0.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0, "legacy db must start unversioned");
        drop(conn);

        let repo = TaskRepo::new(path.to_str().unwrap()).unwrap();
        assert_eq!(schema_version(&repo), SCHEMA_VERSION);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(conn);

        let err = match TaskRepo::new(path.to_str().unwrap()) {
            Ok(_) => panic!("expected schema rejection"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("newer"), "got: {err}");
    }
}
