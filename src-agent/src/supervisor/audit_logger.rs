use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::{AgentError, AgentResult};

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

/// 审计结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditResult {
    Passed,
    Blocked,
    Paused,
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
    async fn query_by_type(&self, event_type: AuditEventType, limit: usize) -> AgentResult<Vec<AuditEvent>>;
    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>>;
    async fn count_events(&self) -> AgentResult<usize>;
}

/// SQLite 实现的审计日志
pub struct SqliteAuditLogger {
    repo: std::sync::Arc<crate::db::TaskRepo>,
}

impl SqliteAuditLogger {
    pub fn new() -> AgentResult<Self> {
        let path = crate::config::rupoo_home().join("agent.db");
        let repo = std::sync::Arc::new(
            crate::db::TaskRepo::new(path.to_str().unwrap_or(":memory:"))
                .map_err(|_| AgentError::Config("无法打开审计日志数据库".to_string()))?,
        );
        Ok(Self { repo })
    }

    pub fn with_repo(repo: std::sync::Arc<crate::db::TaskRepo>) -> Self {
        Self { repo }
    }
}


#[async_trait]
impl AuditLogger for SqliteAuditLogger {
    async fn record(&self, event: AuditEvent) -> AgentResult<()> {
        let key = format!("audit_{}", event.action_id);
        let json = serde_json::to_string(&event)?;
        let key_c = key.clone();
        let json_c = json.clone();
        self.repo
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = ?2",
                    rusqlite::params![key_c, json_c],
                )?;
                Ok(())
            })
            .await
    }

    async fn query_by_type(
        &self,
        event_type: AuditEventType,
        limit: usize,
    ) -> AgentResult<Vec<AuditEvent>> {
        let all = self.repo.list_settings().await?;
        let mut events = Vec::new();
        for (key, val) in &all {
            if key.starts_with("audit_") {
                if let Ok(event) = serde_json::from_str::<AuditEvent>(val) {
                    if event.event_type == event_type {
                        events.push(event);
                        if events.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
        Ok(events)
    }

    async fn query_blocked(&self, limit: usize) -> AgentResult<Vec<AuditEvent>> {
        let all = self.repo.list_settings().await?;
        let mut events = Vec::new();
        for (key, val) in &all {
            if key.starts_with("audit_") {
                if let Ok(event) = serde_json::from_str::<AuditEvent>(val) {
                    if event.result == AuditResult::Blocked {
                        events.push(event);
                        if events.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
        Ok(events)
    }

    async fn count_events(&self) -> AgentResult<usize> {
        let all = self.repo.list_settings().await?;
        Ok(all.iter().filter(|(k, _)| k.starts_with("audit_")).count())
    }
}
