//! Supervisor audit logger — SQLite-backed audit event storage.
//!
//! Uses a dedicated `audit_events` table (not the `settings` key-value store)
//! for efficient SQL-level filtering and TTL-based cleanup.

use crate::error::{AgentError, AgentResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 审计事件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditEventType {
    ComplianceCheck,
    ConfidenceCheck,
    CircuitBreakerCheck,
    ActionApproved,
    ActionBlocked,
    ActionPaused,
    ToolCall,
    ToolResult,
    GoalParsed,
    PlanSelected,
    ReplanTriggered,
    TaskCompleted,
}

impl AuditEventType {
    /// Canonical string representation for DB storage.
    fn as_str(&self) -> &'static str {
        match self {
            Self::ComplianceCheck => "ComplianceCheck",
            Self::ConfidenceCheck => "ConfidenceCheck",
            Self::CircuitBreakerCheck => "CircuitBreakerCheck",
            Self::ActionApproved => "ActionApproved",
            Self::ActionBlocked => "ActionBlocked",
            Self::ActionPaused => "ActionPaused",
            Self::ToolCall => "ToolCall",
            Self::ToolResult => "ToolResult",
            Self::GoalParsed => "GoalParsed",
            Self::PlanSelected => "PlanSelected",
            Self::ReplanTriggered => "ReplanTriggered",
            Self::TaskCompleted => "TaskCompleted",
        }
    }
}

/// 审计结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditResult {
    Passed,
    Blocked,
    Paused,
}

impl AuditResult {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "Passed",
            Self::Blocked => "Blocked",
            Self::Paused => "Paused",
        }
    }
}

/// 全链路审计事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: AuditEventType,
    pub layer: String,
    pub action_id: String,
    pub actor: String,
    pub detail: serde_json::Value,
    pub result: AuditResult,
}

impl AuditEvent {
    pub fn new(event_type: AuditEventType, layer: &str, detail: &serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            layer: layer.to_string(),
            action_id: uuid::Uuid::new_v4().to_string(),
            actor: "agent".to_string(),
            detail: detail.clone(),
            result: AuditResult::Passed,
        }
    }

    pub fn new_blocked(event_type: AuditEventType, layer: &str, reason: &str) -> Self {
        let mut event = Self::new(event_type, layer, &serde_json::json!({"reason": reason}));
        event.result = AuditResult::Blocked;
        event
    }
}

/// 审计日志存储 Trait
#[async_trait]
pub trait AuditLogger: Send + Sync {
    async fn record(&self, event: AuditEvent) -> AgentResult<()>;
    async fn query_by_type(
        &self,
        event_type: AuditEventType,
        limit: usize,
    ) -> AgentResult<Vec<AuditEvent>>;
    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>>;
    async fn count_events(&self) -> AgentResult<usize>;
    /// Delete audit events older than `max_age_days`.
    async fn cleanup(&self, max_age_days: u32) -> AgentResult<u64>;
}

/// SQLite 实现的审计日志 — uses dedicated `audit_events` table.
pub struct SqliteAuditLogger {
    repo: std::sync::Arc<crate::db::TaskRepo>,
}

impl SqliteAuditLogger {
    /// Create a logger that opens the default database at `RUPOO_HOME/agent.db`.
    pub fn new() -> AgentResult<Self> {
        let path = crate::config::rupoo_home().join("agent.db");
        let repo = std::sync::Arc::new(
            crate::db::TaskRepo::new(path.to_str().unwrap_or(":memory:"))
                .map_err(|_| AgentError::Config("无法打开审计日志数据库".to_string()))?,
        );
        Ok(Self { repo })
    }

    /// Create a logger backed by an existing TaskRepo (for testing).
    pub fn with_repo(repo: std::sync::Arc<crate::db::TaskRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl AuditLogger for SqliteAuditLogger {
    /// Record an audit event into the `audit_events` table.
    ///
    /// # Preconditions
    /// - The `audit_events` table must exist (created by `TaskRepo::new`).
    ///
    /// # Postconditions
    /// - One row is inserted into `audit_events`.
    async fn record(&self, event: AuditEvent) -> AgentResult<()> {
        let event_type = event.event_type.as_str().to_string();
        let result = event.result.as_str().to_string();
        let timestamp = event.timestamp.to_rfc3339();
        let payload = serde_json::to_string(&event)?;

        self.repo
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO audit_events (event_type, result, timestamp, payload_json)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![event_type, result, timestamp, payload],
                )?;
                Ok(())
            })
            .await
    }

    /// Query audit events by type, ordered by timestamp descending.
    ///
    /// Uses SQL WHERE clause for efficient filtering (no full-table scan).
    async fn query_by_type(
        &self,
        event_type: AuditEventType,
        limit: usize,
    ) -> AgentResult<Vec<AuditEvent>> {
        let type_str = event_type.as_str().to_string();
        self.repo
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT payload_json FROM audit_events
                     WHERE event_type = ?1
                     ORDER BY timestamp DESC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(rusqlite::params![type_str, limit as i64], |row| {
                    row.get::<_, String>(0)
                })?;
                let mut events = Vec::new();
                for row in rows.flatten() {
                    if let Ok(event) = serde_json::from_str::<AuditEvent>(&row) {
                        events.push(event);
                    }
                }
                Ok(events)
            })
            .await
    }

    /// Query blocked audit events, ordered by timestamp descending.
    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>> {
        self.repo
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT payload_json FROM audit_events
                     WHERE result = 'Blocked'
                     ORDER BY timestamp DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
                    row.get::<_, String>(0)
                })?;
                let mut events = Vec::new();
                for row in rows.flatten() {
                    if let Ok(event) = serde_json::from_str::<AuditEvent>(&row) {
                        events.push(event);
                    }
                }
                Ok(events)
            })
            .await
    }

    /// Count total audit events.
    async fn count_events(&self) -> AgentResult<usize> {
        self.repo
            .with_read_conn(move |conn| {
                let count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
                Ok(count as usize)
            })
            .await
    }

    /// Delete audit events older than `max_age_days`.
    ///
    /// # Returns
    /// The number of deleted rows.
    async fn cleanup(&self, max_age_days: u32) -> AgentResult<u64> {
        let cutoff = (Utc::now() - chrono::Duration::days(max_age_days as i64)).to_rfc3339();
        self.repo
            .with_conn(move |conn| {
                let deleted = conn.execute(
                    "DELETE FROM audit_events WHERE timestamp < ?1",
                    rusqlite::params![cutoff],
                )?;
                Ok(deleted as u64)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskRepo;
    use std::sync::Arc;

    fn test_repo() -> Arc<TaskRepo> {
        Arc::new(TaskRepo::new(":memory:").expect("in-memory repo"))
    }

    #[tokio::test]
    async fn test_record_and_count() {
        let logger = SqliteAuditLogger::with_repo(test_repo());
        assert_eq!(logger.count_events().await.unwrap(), 0);

        let event = AuditEvent::new(
            AuditEventType::ToolCall,
            "test-layer",
            &serde_json::json!({"tool": "read_file"}),
        );
        logger.record(event).await.unwrap();
        assert_eq!(logger.count_events().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_query_by_type() {
        let logger = SqliteAuditLogger::with_repo(test_repo());

        // Record events of different types
        logger
            .record(AuditEvent::new(
                AuditEventType::ToolCall,
                "layer",
                &serde_json::json!({}),
            ))
            .await
            .unwrap();
        logger
            .record(AuditEvent::new(
                AuditEventType::ComplianceCheck,
                "layer",
                &serde_json::json!({}),
            ))
            .await
            .unwrap();
        logger
            .record(AuditEvent::new(
                AuditEventType::ToolCall,
                "layer",
                &serde_json::json!({}),
            ))
            .await
            .unwrap();

        let tool_calls = logger
            .query_by_type(AuditEventType::ToolCall, 10)
            .await
            .unwrap();
        assert_eq!(tool_calls.len(), 2, "should find 2 ToolCall events");

        let compliance = logger
            .query_by_type(AuditEventType::ComplianceCheck, 10)
            .await
            .unwrap();
        assert_eq!(compliance.len(), 1, "should find 1 ComplianceCheck event");
    }

    #[tokio::test]
    async fn test_query_blocked() {
        let logger = SqliteAuditLogger::with_repo(test_repo());

        logger
            .record(AuditEvent::new(
                AuditEventType::ActionBlocked,
                "layer",
                &serde_json::json!({}),
            ))
            .await
            .unwrap();
        logger
            .record(AuditEvent::new_blocked(
                AuditEventType::ActionBlocked,
                "layer",
                "forbidden command",
            ))
            .await
            .unwrap();

        let blocked = logger.query_blocked(10).await.unwrap();
        assert_eq!(blocked.len(), 1, "should find 1 blocked event");
        assert_eq!(blocked[0].result, AuditResult::Blocked);
    }

    #[tokio::test]
    async fn test_cleanup_removes_old_events() {
        let logger = SqliteAuditLogger::with_repo(test_repo());

        // Insert a recent event (timestamp = now)
        logger
            .record(AuditEvent::new(
                AuditEventType::ToolCall,
                "layer",
                &serde_json::json!({}),
            ))
            .await
            .unwrap();

        // Manually insert an old event (year 2020)
        let repo = logger.repo.clone();
        repo.with_conn(|conn| {
            conn.execute(
                "INSERT INTO audit_events (event_type, result, timestamp, payload_json)
                 VALUES ('ToolCall', 'Passed', '2020-01-01T00:00:00Z', ?1)",
                rusqlite::params![serde_json::to_string(&AuditEvent::new(
                    AuditEventType::ToolCall,
                    "layer",
                    &serde_json::json!({}),
                ))?],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        assert_eq!(logger.count_events().await.unwrap(), 2);

        // cleanup(1) removes events older than 1 day — only the 2020 event
        let deleted = logger.cleanup(1).await.unwrap();
        assert_eq!(deleted, 1, "should delete 1 old event");
        assert_eq!(logger.count_events().await.unwrap(), 1);

        // cleanup(365) should NOT remove the recent event
        let deleted = logger.cleanup(365).await.unwrap();
        assert_eq!(deleted, 0, "recent event should survive");
        assert_eq!(logger.count_events().await.unwrap(), 1);
    }
}
