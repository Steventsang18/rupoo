use crate::cognitive::goal::AgentGoal;
use crate::error::AgentResult;
use crate::planning::{ExecutionPlan, PlanScore, Planner};
use crate::task::think_step;
use async_trait::async_trait;

/// 规划器默认实现（桩，Task 2.2 填充）
pub struct PlannerImpl;

#[async_trait]
impl Planner for PlannerImpl {
    async fn generate_alternatives(
        &self,
        goal: &AgentGoal,
        n: usize,
    ) -> AgentResult<Vec<ExecutionPlan>> {
        // 离线兜底：无 LLM 时无法拆解多方案，生成 n 个「先分析目标」的回退方案，
        // 保证规划层可继续择优（真实多方案应由 LLM 在认知层产出并注入）。
        let mut plans = Vec::with_capacity(n.max(1));
        for k in 0..n.max(1) {
            plans.push(ExecutionPlan::new(
                goal.id.as_str(),
                &format!("fallback-{}", k),
                vec![think_step(&format!(
                    "分析目标并制定执行步骤：{}",
                    goal.primary_objective
                ))],
            ));
        }
        Ok(plans)
    }

    async fn score(&self, plan: &ExecutionPlan) -> AgentResult<PlanScore> {
        Ok(crate::planning::scorer::PlanScorer::new().score_plan(plan))
    }

    async fn select_best(
        &self,
        candidates: Vec<ExecutionPlan>,
    ) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)> {
        if candidates.is_empty() {
            return Err(crate::error::AgentError::Other("无候选方案".to_string()));
        }
        let mut sorted = candidates;
        sorted.sort_by(|a, b| {
            b.score
                .as_ref()
                .map(|s| s.weighted_total)
                .unwrap_or(0.0)
                .partial_cmp(&a.score.as_ref().map(|s| s.weighted_total).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let best = sorted.remove(0);
        Ok((best, sorted))
    }
}
