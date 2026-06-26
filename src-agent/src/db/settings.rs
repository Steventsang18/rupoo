//! Settings, Memory, ConversationHistory, and UISession CRUD operations
//!
//! Split from db.rs (Phase 1 Step 2)

use crate::error::{AgentError, AgentResult};
use crate::llm::ConversationHistory;
use crate::task::MemoryEntry;
use tracing::warn;

use super::TaskRepo;

/// Valid configuration keys for `set_setting`.
const VALID_CONFIG_KEYS: &[&str] = &[
    "api_key.anthropic",
    "api_key.openai",
    "api_key.deepseek",
    "model.anthropic",
    "model.openai",
    "model.deepseek",
    "model.ollama",
    "base_url.openai",
    "base_url.deepseek",
    "ollama.base_url",
    "active_provider",
    "approve_all",
    "default_timeout_secs",
    "browser_path",
    "max_turns",
    "theme",
];

/// Check if a config key is valid, returning a suggestion for close matches.
fn validate_config_key(key: &str) -> AgentResult<()> {
    if VALID_CONFIG_KEYS.contains(&key) {
        return Ok(());
    }
    // Find the closest match using simple edit distance
    let best = VALID_CONFIG_KEYS
        .iter()
        .filter_map(|valid| {
            let dist = levenshtein_distance(key, valid);
            if dist <= 3 {
                Some((dist, *valid))
            } else {
                None
            }
        })
        .min_by_key(|(d, _)| *d);

    match best {
        Some((_, suggestion)) => Err(AgentError::Config(format!(
            "unknown config key '{}'. Did you mean '{}'?",
            key, suggestion
        ))),
        None => Err(AgentError::Config(format!(
            "unknown config key '{}'. Valid keys: {}",
            key,
            VALID_CONFIG_KEYS.join(", ")
        ))),
    }
}

/// Simple Levenshtein distance for config key suggestions.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for (i, ac) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b.chars().enumerate() {
            curr[j + 1] = if ac == bc {
                prev[j]
            } else {
                1 + prev[j].min(curr[j]).min(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

// ---------------------------------------------------------------------------
// Conversation History impl
// ---------------------------------------------------------------------------

impl TaskRepo {
    // ---------------------------------------------------------------------------
    // Conversation History persistence for Chat Mode
    // ---------------------------------------------------------------------------

    /// Save conversation history for a session.
    ///
    /// Serialized with a schema version wrapper: `{"v":1,"data":{...}}`
    /// so that future schema changes can be migrated gracefully.
    pub async fn save_conversation_history(
        &self,
        session_id: &str,
        history: &ConversationHistory,
    ) -> AgentResult<()> {
        const HISTORY_SCHEMA_VERSION: u32 = 1;

        let sid = session_id.to_string();
        let data = serde_json::to_string(history)?;
        let wrapper = serde_json::json!({
            "v": HISTORY_SCHEMA_VERSION,
            "data": serde_json::from_str::<serde_json::Value>(&data)?,
        });
        let history_json = serde_json::to_string(&wrapper)?;
        let now = chrono::Utc::now().to_rfc3339();

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO conversation_histories (session_id, history_json, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(session_id) DO UPDATE SET
                   history_json = excluded.history_json,
                   updated_at = excluded.updated_at",
                rusqlite::params![sid, history_json, now],
            )?;
            Ok(())
        })
        .await
    }

    /// Load conversation history for a session.
    ///
    /// Supports versioned format `{"v":N,"data":{...}}` and falls back to
    /// raw deserialization for legacy (unversioned) records.
    pub async fn load_conversation_history(
        &self,
        session_id: &str,
    ) -> AgentResult<Option<ConversationHistory>> {
        let sid = session_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn
                .prepare("SELECT history_json FROM conversation_histories WHERE session_id = ?1")?;

            let result = stmt
                .query_row(rusqlite::params![sid], |row| row.get::<_, String>(0))
                .ok();

            match result {
                Some(json) => {
                    let history = Self::deserialize_history(&json)?;
                    Ok(Some(history))
                }
                None => Ok(None),
            }
        })
        .await
    }

    /// Deserialize conversation history with schema version support.
    ///
    /// - Versioned format: `{"v":1,"data":{...}}` → parse by version
    /// - Legacy format: raw ConversationHistory JSON → treat as v1
    fn deserialize_history(json: &str) -> AgentResult<ConversationHistory> {
        // Try versioned format first
        if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(json) {
            if let Some(version) = wrapper.get("v").and_then(|v| v.as_u64()) {
                let data = wrapper.get("data").ok_or_else(|| {
                    AgentError::Other("history_json: missing 'data' field".into())
                })?;
                match version {
                    1 => {
                        return serde_json::from_value(data.clone())
                            .map_err(|e| AgentError::Other(format!("parse history v1: {e}")))
                    }
                    v => {
                        return Err(AgentError::Other(format!(
                            "history_json: unsupported schema version {v}"
                        )))
                    }
                }
            }
            // No "v" field → legacy format, fall through to direct deserialization
        }
        // Legacy: raw ConversationHistory JSON (no wrapper)
        serde_json::from_str(json)
            .map_err(|e| AgentError::Other(format!("parse history (legacy): {e}")))
    }

    // ---------------------------------------------------------------------------
    // Settings (key-value store for API keys, preferences)
    // ---------------------------------------------------------------------------

    /// Set a configuration value.
    pub async fn set_setting(&self, key: &str, value: &str) -> AgentResult<()> {
        validate_config_key(key)?;
        let key = key.to_string();
        let value = value.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?2",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    /// Get a configuration value by key.
    pub async fn get_setting(&self, key: &str) -> AgentResult<Option<String>> {
        let key = key.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let result = stmt
                .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
                .ok();
            Ok(result)
        })
        .await
    }

    /// List all settings keys.
    pub async fn list_settings(&self) -> AgentResult<Vec<(String, String)>> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut results = Vec::new();
            for row in rows.flatten() {
                results.push(row);
            }
            Ok(results)
        })
        .await
    }

    /// Delete a setting.
    pub async fn delete_setting(&self, key: &str) -> AgentResult<()> {
        let key = key.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![key],
            )?;
            Ok(())
        })
        .await
    }

    // ---------------------------------------------------------------------------
    // UI Session operations
    // ---------------------------------------------------------------------------

    /// Save UI session.
    #[allow(clippy::empty_line_after_doc_comments)]
    pub async fn save_ui_session(
        &self,
        id: &str,
        label: &str,
        messages_json: &str,
        is_active: bool,
    ) -> AgentResult<()> {
        let id = id.to_string();
        let label = label.to_string();
        let messages_json = messages_json.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let active: i32 = if is_active { 1 } else { 0 };
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO ui_sessions (id, label, messages_json, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   label = excluded.label,
                   messages_json = excluded.messages_json,
                   is_active = excluded.is_active,
                   updated_at = excluded.updated_at",
                rusqlite::params![id, label, messages_json, active, now, now],
            )?;
            Ok(())
        })
        .await
    }

    /// Load all UI sessions.
    pub async fn load_ui_sessions(&self) -> AgentResult<Vec<(String, String, String, bool)>> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, label, messages_json, is_active FROM ui_sessions ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let label: String = row.get(1)?;
                let messages_json: String = row.get(2)?;
                let active: i32 = row.get(3)?;
                Ok((id, label, messages_json, active != 0))
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
        .await
    }

    /// Delete a UI session.
    pub async fn delete_ui_session(&self, id: &str) -> AgentResult<()> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM ui_sessions WHERE id = ?",
                rusqlite::params![id],
            )?;
            Ok(())
        })
        .await
    }

    // ---------------------------------------------------------------------------
    // Memory operations (FTS5 long-term memory)
    // ---------------------------------------------------------------------------

    /// Store a memory entry with FTS5 full-text indexing.
    pub async fn store_memory(
        &self,
        content: &str,
        tags: &[&str],
        source: &str,
    ) -> AgentResult<String> {
        let mem_id = uuid::Uuid::new_v4().to_string();
        let content = content.to_string();
        let tags_json = serde_json::to_string(tags)?;
        let source = source.to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let mem_id_clone = mem_id.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO memories (content, tags, source, created_at, updated_at, content_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![content, tags_json, source, now, now, mem_id],
            )?;
            Ok(())
        })
        .await?;

        Ok(mem_id_clone)
    }

    /// Full-text search across stored memories. Returns results sorted by
    /// relevance (BM25 ranking).
    pub async fn search_memories(
        &self,
        query: &str,
        limit: usize,
    ) -> AgentResult<Vec<MemoryEntry>> {
        let query = query.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT content_id, content, tags, source, created_at, updated_at
                 FROM memories
                 WHERE memories MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
                let tags_str: String = row.get(2)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_else(|e| {
                    warn!(tags_str = %tags_str, error = %e, "failed to deserialize tags, using empty vec");
                    Vec::new()
                });
                Ok(MemoryEntry {
                    id: row.get::<_, String>(0)?,
                    content: row.get(1)?,
                    tags,
                    source: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows.flatten() {
                results.push(row);
            }
            Ok(results)
        })
        .await
    }

    /// Retrieve recent memories without a search query (e.g., for context injection).
    pub async fn recent_memories(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT content_id, content, tags, source, created_at, updated_at
                 FROM memories
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )?;

            let rows = stmt.query_map([limit as i64], |row| {
                let tags_str: String = row.get(2)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_else(|e| {
                    warn!(tags_str = %tags_str, error = %e, "failed to deserialize tags, using empty vec");
                    Vec::new()
                });
                Ok(MemoryEntry {
                    id: row.get::<_, String>(0)?,
                    content: row.get(1)?,
                    tags,
                    source: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows.flatten() {
                results.push(row);
            }
            Ok(results)
        })
        .await
    }

    /// Count total memory entries.
    pub async fn count_memories(&self) -> AgentResult<usize> {
        self.with_read_conn(move |conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
            Ok(count as usize)
        })
        .await
    }

    /// Get a specific memory entry by ID.
    pub async fn get_memory(&self, id: &str) -> AgentResult<Option<MemoryEntry>> {
        let id = id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT content_id, content, tags, source, created_at, updated_at
                 FROM memories
                 WHERE content_id = ?1",
            )?;

            let result = stmt
                .query_row(rusqlite::params![id], |row| {
                    let tags_str: String = row.get(2)?;
                    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_else(|e| {
                        warn!(tags_str = %tags_str, error = %e, "failed to deserialize tags, using empty vec");
                        Vec::new()
                    });
                    Ok(MemoryEntry {
                        id: row.get::<_, String>(0)?,
                        content: row.get(1)?,
                        tags,
                        source: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })
                .ok();

            Ok(result)
        })
        .await
    }

    /// Delete a memory entry by content_id.
    pub async fn delete_memory(&self, id: &str) -> AgentResult<()> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let affected = conn.execute(
                "DELETE FROM memories WHERE content_id = ?1",
                rusqlite::params![id],
            )?;
            if affected == 0 {
                warn!(memory_id = %id, "delete_memory: no matching entry found");
            }
            Ok(())
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::tests::repo;
    use crate::error::AgentError;
    use crate::llm::ConversationHistory;

    #[tokio::test]
    async fn test_conversation_history_persistence() {
        let repo = repo();
        let mut history = ConversationHistory::new(10);
        history.push_user("Hello");
        history.push_assistant("Hi there!");

        repo.save_conversation_history("session-1", &history)
            .await
            .unwrap();

        let loaded = repo
            .load_conversation_history("session-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.message_count(), 2);

        // Non-existent session returns None
        let none = repo.load_conversation_history("nonexistent").await.unwrap();
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn test_conversation_history_schema_version() {
        let repo = repo();

        // Save — should produce versioned JSON {"v":1,"data":{...}}
        let mut history = ConversationHistory::new(5);
        history.push_user("test");
        repo.save_conversation_history("vtest", &history)
            .await
            .unwrap();

        // Verify the stored JSON has the wrapper
        let json: String = repo
            .with_read_conn(move |conn| {
                conn.query_row(
                    "SELECT history_json FROM conversation_histories WHERE session_id = ?1",
                    rusqlite::params!["vtest"],
                    |row| row.get(0),
                )
                .map_err(|e| AgentError::Other(e.to_string()))
            })
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["v"], 1);
        assert!(val.get("data").is_some());

        // Load — should parse versioned format correctly
        let loaded = repo
            .load_conversation_history("vtest")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.message_count(), 1);
    }

    #[tokio::test]
    async fn test_conversation_history_legacy_fallback() {
        let repo = repo();

        // Insert a raw (legacy) history JSON without schema wrapper
        let raw_json = serde_json::to_string(&ConversationHistory::new(10)).unwrap();
        repo.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO conversation_histories (session_id, history_json, updated_at)
                 VALUES (?1, ?2, '2026-01-01T00:00:00Z')",
                rusqlite::params!["legacy-session", raw_json],
            )
            .map_err(|e| AgentError::Other(e.to_string()))
        })
        .await
        .unwrap();

        // Load — should fall back to direct deserialization
        let loaded = repo
            .load_conversation_history("legacy-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.message_count(), 0);
    }
}
