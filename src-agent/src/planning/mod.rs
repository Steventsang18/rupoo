pub mod planner;
pub mod scorer;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::cognitive::goal::AgentGoal;
use crate::error::AgentResult;
use crate::task::Step;

/// 执行方案（带量化数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub id: String,
    pub goal_id: String,
    pub name: String,
    pub steps: Vec<Step>,
    pub estimated_cost: PlanCost,
    pub score: Option<PlanScore>,
}

/// 预估执行成本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCost {
    /// 预估 token 消耗
    pub estimated_tokens: u64,
    /// 预估执行时间（秒）
    pub estimated_duration_secs: u64,
    /// 外部 API 调用次数
    pub external_api_calls: u32,
    /// 最高风险等级的工具
    pub max_tool_risk: String,
}

impl Default for PlanCost {
    fn default() -> Self {
        Self {
            estimated_tokens: 1000,
            estimated_duration_secs: 30,
            external_api_calls: 0,
            max_tool_risk: "safe".to_string(),
        }
    }
}

/// 三维加权评分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanScore {
    /// 执行成功率评估（0.0-1.0）
    pub success_probability: f64,
    /// 资源开销评分（越大越贵）
    pub resource_cost: f64,
    /// 业务风险等级（0.0-1.0）
    pub risk_level: f64,
    /// 加权总分
    pub weighted_total: f64,
    /// 评分明细日志（用于复盘）
    pub scoring_log: Vec<String>,
}

impl ExecutionPlan {
    pub fn new(goal_id: &str, name: &str, steps: Vec<Step>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            goal_id: goal_id.to_string(),
            name: name.to_string(),
            steps,
            estimated_cost: PlanCost::default(),
            score: None,
        }
    }

    pub fn with_cost(mut self, cost: PlanCost) -> Self {
        self.estimated_cost = cost;
        self
    }
}

/// 规划层 Trait——多方案生成 + 加权择优
#[async_trait]
pub trait Planner: Send + Sync {
    /// 并行生成 N 个候选方案
    async fn generate_alternatives(
        &self,
        goal: &AgentGoal,
        n: usize,
    ) -> AgentResult<Vec<ExecutionPlan>>;

    /// 对一个方案做三维评分
    async fn score(&self, plan: &ExecutionPlan) -> AgentResult<PlanScore>;

    /// 择优：返回最优方案 + 备选方案列表
    async fn select_best(
        &self,
        candidates: Vec<ExecutionPlan>,
    ) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_plan_construction() {
        let plan = ExecutionPlan::new("goal-1", "fast-path", vec![]);
        assert_eq!(plan.goal_id, "goal-1");
        assert_eq!(plan.name, "fast-path");
        assert!(plan.score.is_none());
    }

    #[test]
    fn test_plan_score_fields() {
        let score = PlanScore {
            success_probability: 0.9,
            resource_cost: 0.3,
            risk_level: 0.1,
            weighted_total: 0.85,
            scoring_log: vec!["高成功概率".to_string()],
        };
        assert!((score.weighted_total - 0.85).abs() < 1e-10);
    }

    #[test]
    fn test_plan_cost_default() {
        let cost = PlanCost::default();
        assert_eq!(cost.estimated_tokens, 1000);
        assert_eq!(cost.max_tool_risk, "safe");
    }
}
