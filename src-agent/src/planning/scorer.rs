//! 评分器：基于方案步骤与预估成本的三维启发式评分（P1 落地，原为空桩）。
//!
//! 纯函数、确定性、可单测，不依赖 LLM。被 `planning::planner::PlannerImpl::score`
//! 调用，使规划层择优选出的方案有可解释的分数依据。

use crate::planning::{ExecutionPlan, PlanCost, PlanScore};
use crate::task::Step;

/// 单步工具风险表（0.0 安全 ~ 1.0 高危）。
fn step_risk(step: &Step) -> f64 {
    match step {
        Step::ToolCall { tool_name, .. } => match tool_name.as_str() {
            "shell_exec" | "check_output" => 0.6,
            "file_write" | "file_edit" => 0.4,
            "code_search" | "file_read" | "list_directory" => 0.2,
            "run_tests" | "diff_check" => 0.15,
            "web_search" | "web_http" | "browser" => 0.3,
            _ => 0.35,
        },
        Step::Exec { .. } => 0.6,
        Step::HttpRequest { .. } | Step::BrowserAction { .. } => 0.3,
        Step::Think { .. } | Step::Finish { .. } => 0.05,
        Step::WaitForInput { .. } => 0.1,
    }
}

/// 三维加权评分器。
pub struct PlanScorer;

impl PlanScorer {
    pub fn new() -> Self {
        Self
    }

    /// 对一个方案做三维评分。
    pub fn score_plan(&self, plan: &ExecutionPlan) -> PlanScore {
        let steps = &plan.steps;
        let risk_level = if steps.is_empty() {
            0.5
        } else {
            steps.iter().map(step_risk).sum::<f64>() / steps.len() as f64
        };

        let cost: &PlanCost = &plan.estimated_cost;
        let token_cost = (cost.estimated_tokens as f64 / 5000.0).clamp(0.0, 1.0);
        let api_cost = (cost.external_api_calls as f64 / 10.0).clamp(0.0, 1.0);
        let resource_cost = (0.7 * token_cost + 0.3 * api_cost).clamp(0.0, 1.0);

        // 步骤越多、风险越高，成功率越低。
        let complexity_penalty = (steps.len() as f64 * 0.03).clamp(0.0, 0.4);
        let success_probability = (1.0 - 0.5 * risk_level - complexity_penalty).clamp(0.0, 1.0);

        let weighted_total =
            0.5 * success_probability + 0.2 * (1.0 - resource_cost) + 0.3 * (1.0 - risk_level);

        let scoring_log = vec![
            format!(
                "steps={}, risk={risk_level:.2}, cost={resource_cost:.2}",
                steps.len()
            ),
            format!("success_prob={success_probability:.2}, weighted={weighted_total:.2}"),
        ];

        PlanScore {
            success_probability,
            resource_cost,
            risk_level,
            weighted_total,
            scoring_log,
        }
    }
}

impl Default for PlanScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{think_step, tool_call_step};

    #[test]
    fn test_score_empty_plan_is_moderate() {
        let plan = ExecutionPlan::new("g", "empty", vec![]);
        let s = PlanScorer::new().score_plan(&plan);
        assert!((0.0..=1.0).contains(&s.weighted_total));
        assert!(!s.scoring_log.is_empty());
    }

    #[test]
    fn test_score_shell_heavy_plan_has_higher_risk() {
        let safe = ExecutionPlan::new(
            "g",
            "safe",
            vec![
                think_step("think"),
                tool_call_step("file_read", serde_json::json!({})),
            ],
        );
        let risky = ExecutionPlan::new(
            "g",
            "risky",
            vec![tool_call_step("shell_exec", serde_json::json!({}))],
        );
        let s_safe = PlanScorer::new().score_plan(&safe);
        let s_risky = PlanScorer::new().score_plan(&risky);
        assert!(s_risky.risk_level > s_safe.risk_level);
        assert!(s_risky.weighted_total < s_safe.weighted_total);
    }
}
