use async_trait::async_trait;
use crate::cognitive::goal::AgentGoal;
use crate::error::AgentResult;
use crate::planning::{ExecutionPlan, PlanScore, Planner};

/// 规划器默认实现（桩，Task 2.2 填充）
pub struct PlannerImpl;

#[async_trait]
impl Planner for PlannerImpl {
    async fn generate_alternatives(
        &self,
        _goal: &AgentGoal,
        _n: usize,
    ) -> AgentResult<Vec<ExecutionPlan>> {
        Ok(Vec::new())
    }

    async fn score(&self, _plan: &ExecutionPlan) -> AgentResult<PlanScore> {
        Ok(PlanScore {
            success_probability: 0.5,
            resource_cost: 0.5,
            risk_level: 0.5,
            weighted_total: 0.5,
            scoring_log: vec!["默认评分（桩实现）".to_string()],
        })
    }

    async fn select_best(
        &self,
        candidates: Vec<ExecutionPlan>,
    ) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)> {
        if candidates.is_empty() {
            return Err(crate::error::AgentError::Other("无候选方案".to_string()));
        }
        let mut sorted = candidates;
        sorted.sort_by(|a, b| b.score.as_ref().map(|s| s.weighted_total).unwrap_or(0.0)
            .partial_cmp(&a.score.as_ref().map(|s| s.weighted_total).unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal));
        let best = sorted.remove(0);
        Ok((best, sorted))
    }
}
