pub mod replanner;
pub mod validator;

use crate::error::AgentResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 数据差异严重程度
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiscrepancySeverity {
    /// 轻微偏差，不影响执行
    Minor,
    /// 明显偏差，记录但不中断
    Warning,
    /// 严重偏差，需要触发重规划
    Critical,
}

/// 数据差异记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDiscrepancy {
    pub field: String,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
    pub severity: DiscrepancySeverity,
}

/// 校验结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub passed: bool,
    pub discrepancies: Vec<DataDiscrepancy>,
    /// 是否触发重规划
    pub trigger_replan: bool,
}

impl ValidationResult {
    /// 通过验证
    pub fn passed() -> Self {
        Self {
            passed: true,
            discrepancies: Vec::new(),
            trigger_replan: false,
        }
    }

    /// 有差异
    pub fn with_discrepancy(
        field: &str,
        expected: serde_json::Value,
        actual: serde_json::Value,
        severity: DiscrepancySeverity,
    ) -> Self {
        let trigger = severity == DiscrepancySeverity::Critical;
        Self {
            passed: !trigger,
            discrepancies: vec![DataDiscrepancy {
                field: field.to_string(),
                expected,
                actual,
                severity,
            }],
            trigger_replan: trigger,
        }
    }
}

/// 执行层 Trait——工具调度 + 数据校验
#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// 调用前入参合法性校验
    async fn validate_input(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> AgentResult<ValidationResult>;

    /// 调用后多数据源置信度比对
    async fn validate_output(
        &self,
        tool_name: &str,
        result: &str,
        expected: Option<&str>,
    ) -> AgentResult<ValidationResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_passed() {
        let r = ValidationResult::passed();
        assert!(r.passed);
        assert!(!r.trigger_replan);
        assert!(r.discrepancies.is_empty());
    }

    #[test]
    fn test_validation_critical_triggers_replan() {
        let r = ValidationResult::with_discrepancy(
            "result",
            serde_json::json!("expected"),
            serde_json::json!("unexpected"),
            DiscrepancySeverity::Critical,
        );
        assert!(!r.passed);
        assert!(r.trigger_replan);
    }

    #[test]
    fn test_validation_minor_does_not_trigger() {
        let r = ValidationResult::with_discrepancy(
            "latency",
            serde_json::json!("100ms"),
            serde_json::json!("110ms"),
            DiscrepancySeverity::Minor,
        );
        assert!(r.passed);
        assert!(!r.trigger_replan);
    }
}
