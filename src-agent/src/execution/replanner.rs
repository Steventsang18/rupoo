//! 执行层重规划（P1 落地，原为空桩）。
//!
//! 当 `ValidationResult::trigger_replan` 为真（Critical 偏差）时，由本模块产出
//! 一个修正方案：在失败方案之后追加一个「诊断 + 重试」步骤，使 agent 在下一轮
//! 针对偏差修正。纯函数、确定性、可单测；不依赖 LLM。

use crate::error::AgentResult;
use crate::execution::DataDiscrepancy;
use crate::planning::ExecutionPlan;
use crate::task::{think_step, Step};
use async_trait::async_trait;

/// 重规划器 Trait。
#[async_trait]
pub trait Replanner: Send + Sync {
    /// 基于失败方案与触发重规划的偏差，产出修正方案。
    async fn revise(
        &self,
        failed: &ExecutionPlan,
        discrepancy: &DataDiscrepancy,
    ) -> AgentResult<ExecutionPlan>;
}

/// 默认重规划实现：追加诊断 + 重试步骤。
pub struct ReplannerImpl;

#[async_trait]
impl Replanner for ReplannerImpl {
    async fn revise(
        &self,
        failed: &ExecutionPlan,
        discrepancy: &DataDiscrepancy,
    ) -> AgentResult<ExecutionPlan> {
        let mut steps: Vec<Step> = failed.steps.clone();
        let diagnostic = format!(
            "重规划：字段 '{}' 出现严重偏差（期望 {}, 实际 {}）。请诊断根因并修正后重试。",
            discrepancy.field, discrepancy.expected, discrepancy.actual
        );
        steps.push(think_step(&diagnostic));
        Ok(ExecutionPlan::new(
            &failed.goal_id,
            &format!("{}-revised", failed.name),
            steps,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::DiscrepancySeverity;
    use crate::task::think_step;

    #[tokio::test]
    async fn test_revise_appends_diagnostic_step() {
        let plan = ExecutionPlan::new("g", "p", vec![think_step("step1")]);
        let disc = DataDiscrepancy {
            field: "result".to_string(),
            expected: serde_json::json!("a"),
            actual: serde_json::json!("b"),
            severity: DiscrepancySeverity::Critical,
        };
        let revised = ReplannerImpl.revise(&plan, &disc).await.unwrap();
        assert_eq!(revised.steps.len(), 2);
        assert_eq!(revised.name, "p-revised");
        assert_eq!(revised.goal_id, "g");
    }
}
