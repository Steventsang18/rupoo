use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::error::AgentResult;

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
