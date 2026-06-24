pub mod audit_logger;
pub mod compliance;
pub use self::compliance::ComplianceChecker;
pub use self::compliance::ComplianceResult;
pub mod confidence;
pub mod circuit_breaker;

#[cfg(test)]
mod test_data_types;

use std::sync::Arc;

use async_trait::async_trait;
use tracing::warn;

use crate::error::{AgentError, AgentResult};
use crate::supervisor::audit_logger::{AuditEvent, AuditEventType, AuditLogger};
use crate::supervisor::circuit_breaker::CircuitBreaker;
use crate::supervisor::confidence::ConfidenceChecker;

/// 待拦截的动作
#[derive(Debug, Clone)]
pub struct Action {
    /// 动作类型标识
    pub action_type: String,
    /// 动作的上下文描述
    pub description: String,
    /// 关联数据
    pub payload: serde_json::Value,
}

impl Action {
    pub fn new(action_type: &str, description: &str) -> Self {
        Self {
            action_type: action_type.to_string(),
            description: description.to_string(),
            payload: serde_json::Value::Null,
        }
    }

    #[must_use]
    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }
}

/// 执行元信息——置信度、工具名、调用次数等
#[derive(Debug, Clone, Default)]
pub struct ExecutionMeta {
    pub tool_name: Option<String>,
    pub confidence: Option<f64>,
    pub action_count: u64,
}

impl ExecutionMeta {
    #[must_use]
    pub fn with_confidence(confidence: f64) -> Self {
        Self {
            confidence: Some(confidence),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_tool(name: &str) -> Self {
        Self {
            tool_name: Some(name.to_string()),
            ..Default::default()
        }
    }
}

/// 监督层 Trait——三道闸门串行拦截
#[async_trait]
pub trait Supervisor: Send + Sync {
    /// 三道闸门串行执行
    async fn intercept(&self, action: &Action, meta: &ExecutionMeta) -> AgentResult<()>;
}

/// 监督层默认实现——三道闸门串行
pub struct SupervisorImpl {
    compliance: ComplianceChecker,
    confidence: ConfidenceChecker,
    circuit_breaker: CircuitBreaker,
    audit_logger: Arc<dyn AuditLogger>,
}

impl SupervisorImpl {
    pub fn new(
        compliance: ComplianceChecker,
        confidence: ConfidenceChecker,
        circuit_breaker: CircuitBreaker,
        audit_logger: Arc<dyn AuditLogger>,
    ) -> Self {
        Self {
            compliance,
            confidence,
            circuit_breaker,
            audit_logger,
        }
    }

    /// 从 SafetyContext + 默认配置构建
    pub fn from_safety_ctx(ctx: &crate::safety::SafetyContext) -> AgentResult<Self> {
        let compliance = ComplianceChecker::from_safety_ctx(ctx);
        let confidence = ConfidenceChecker::default();
        let circuit_breaker =
            CircuitBreaker::new(crate::supervisor::circuit_breaker::BreakerConfig::default());
        let audit_logger = Arc::new(
            crate::supervisor::audit_logger::SqliteAuditLogger::new()?,
        );
        Ok(Self::new(compliance, confidence, circuit_breaker, audit_logger))
    }
}

#[async_trait]
impl Supervisor for SupervisorImpl {
    async fn intercept(&self, action: &Action, meta: &ExecutionMeta) -> AgentResult<()> {
        // 闸门1: 合规校验
        let compliance = self.compliance.check(action)?;
        if !compliance.allowed {
            self.audit_logger
                .record(AuditEvent::new_blocked(
                    AuditEventType::ComplianceCheck,
                    "supervisor",
                    &compliance.reason,
                ))
                .await
                .map_err(|e| {
                    warn!("audit log write failed: {}", e);
                })
                .ok();
            return Err(AgentError::Safety(compliance.reason));
        }
        self.audit_logger
            .record(AuditEvent::new(
                AuditEventType::ComplianceCheck,
                "supervisor",
                &serde_json::json!({"action": action.action_type, "result": "passed"}),
            ))
            .await
            .map_err(|e| warn!("audit log write failed: {}", e))
            .ok();

        // 闸门2: 置信度拦截
        if let Err(e) = self.confidence.check(meta) {
            self.audit_logger
                .record(AuditEvent::new_blocked(
                    AuditEventType::ConfidenceCheck,
                    "supervisor",
                    &e.to_string(),
                ))
                .await
                .map_err(|e| warn!("audit log write failed: {}", e))
                .ok();
            return Err(e);
        }
        self.audit_logger
            .record(AuditEvent::new(
                AuditEventType::ConfidenceCheck,
                "supervisor",
                &serde_json::json!({"confidence": meta.confidence, "result": "passed"}),
            ))
            .await
            .map_err(|e| warn!("audit log write failed: {}", e))
            .ok();

        // 闸门3: 熔断器
        if let Err(e) = self.circuit_breaker.check() {
            self.audit_logger
                .record(AuditEvent::new_blocked(
                    AuditEventType::CircuitBreakerCheck,
                    "supervisor",
                    &e.to_string(),
                ))
                .await
                .map_err(|e| warn!("audit log write failed: {}", e))
                .ok();
            return Err(e);
        }
        self.audit_logger
            .record(AuditEvent::new(
                AuditEventType::CircuitBreakerCheck,
                "supervisor",
                &serde_json::json!({"state": "closed", "result": "passed"}),
            ))
            .await
            .map_err(|e| warn!("audit log write failed: {}", e))
            .ok();

        // 全部通过
        self.audit_logger
            .record(AuditEvent::new(
                AuditEventType::ActionApproved,
                "supervisor",
                &serde_json::json!({"action": action.action_type, "description": action.description}),
            ))
            .await
            .map_err(|e| warn!("audit log write failed: {}", e))
            .ok();

        Ok(())
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::supervisor::audit_logger::SqliteAuditLogger;
    use crate::supervisor::circuit_breaker::BreakerConfig;

    #[tokio::test]
    async fn test_supervisor_approves_safe_action() {
        let compliance = ComplianceChecker::new(vec!["sudo".to_string()], vec![]);
        let confidence = ConfidenceChecker::default();
        let breaker = CircuitBreaker::new(BreakerConfig::default());
        let audit = Arc::new(SqliteAuditLogger::new().unwrap());
        let supervisor = SupervisorImpl::new(compliance, confidence, breaker, audit);
        let action = Action::new("echo", "echo hello");
        let meta = ExecutionMeta::with_confidence(0.95);
        assert!(supervisor.intercept(&action, &meta).await.is_ok());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_forbidden_command() {
        let compliance = ComplianceChecker::new(vec!["sudo".to_string()], vec![]);
        let supervisor = SupervisorImpl::new(
            compliance,
            ConfidenceChecker::default(),
            CircuitBreaker::new(BreakerConfig::default()),
            Arc::new(SqliteAuditLogger::new().unwrap()),
        );
        assert!(supervisor
            .intercept(
                &Action::new("sudo", "sudo rm -rf /"),
                &ExecutionMeta::default(),
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_low_confidence() {
        let compliance = ComplianceChecker::new(vec![], vec![]);
        let supervisor = SupervisorImpl::new(
            compliance,
            ConfidenceChecker::default(),
            CircuitBreaker::new(BreakerConfig::default()),
            Arc::new(SqliteAuditLogger::new().unwrap()),
        );
        let meta = ExecutionMeta::with_confidence(0.3);
        assert!(supervisor
            .intercept(&Action::new("echo", "echo"), &meta)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_supervisor_blocks_open_breaker() {
        let breaker = CircuitBreaker::new(BreakerConfig {
            failure_threshold: 1,
            open_duration_secs: 30,
            half_open_max_requests: 1,
            max_rate_per_sec: 100,
        });
        breaker.record_failure();
        let supervisor = SupervisorImpl::new(
            ComplianceChecker::new(vec![], vec![]),
            ConfidenceChecker::default(),
            breaker,
            Arc::new(SqliteAuditLogger::new().unwrap()),
        );
        assert!(supervisor
            .intercept(
                &Action::new("echo", "echo"),
                &ExecutionMeta::with_confidence(0.95),
            )
            .await
            .is_err());
    }
}
