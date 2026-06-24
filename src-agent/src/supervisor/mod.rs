pub mod audit_logger;
pub mod compliance;
pub use self::compliance::ComplianceChecker;
pub use self::compliance::ComplianceResult;
pub mod confidence;
pub mod circuit_breaker;

#[cfg(test)]
mod test_data_types;

use async_trait::async_trait;
use crate::error::AgentResult;

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
