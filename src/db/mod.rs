//! Database module for Rupoo - Task Repository
//!
//! Split from db.rs (Phase 1 Step 2):
//! - mod.rs: TaskRepo struct + new() + PlanSummary
//! - plans.rs: Plan/Checkpoint CRUD operations
//! - settings.rs: Settings/Memory/ConversationHistory/UISession CRUD

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

pub mod plans;
pub mod settings;

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
        // - cache_size=10000: increase page cache (each page is ~4KB)
        // - temp_store=MEMORY: use memory for temporary tables
        // - journal_size_limit=104857600: limit WAL file size to 100MB
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-10000;
             PRAGMA temp_store=MEMORY;
             PRAGMA journal_size_limit=104857600;
             PRAGMA foreign_keys=ON;",
        )?;
        conn.execute_batch(
            "
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
            ",
        )?;
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
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|poisoned| {
                error!("mutex poisoned, recovering");
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
}
