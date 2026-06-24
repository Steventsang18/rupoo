use async_trait::async_trait;
use crate::error::AgentResult;
use crate::execution::{ExecutionEngine, ValidationResult};

pub struct ExecutionEngineImpl;

#[async_trait]
impl ExecutionEngine for ExecutionEngineImpl {
    async fn validate_input(
        &self,
        _tool_name: &str,
        _params: &serde_json::Value,
    ) -> AgentResult<ValidationResult> {
        Ok(ValidationResult::passed())
    }

    async fn validate_output(
        &self,
        _tool_name: &str,
        _result: &str,
        _expected: Option<&str>,
    ) -> AgentResult<ValidationResult> {
        Ok(ValidationResult::passed())
    }
}
