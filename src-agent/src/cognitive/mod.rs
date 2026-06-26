pub mod engine;
pub mod goal;

use crate::cognitive::goal::{AgentGoal, AuthLevel};
use crate::context::ConversationContext;
use crate::error::AgentResult;
use async_trait::async_trait;

/// 认知层 Trait——将原始指令解析为结构化目标
#[async_trait]
pub trait CognitiveEngine: Send + Sync {
    /// 将原始指令解析为结构化 AgentGoal
    async fn parse(&self, raw: &str, context: &ConversationContext) -> AgentResult<AgentGoal>;

    /// 拆解复杂目标为独立子目标
    async fn decompose(&self, goal: &AgentGoal) -> AgentResult<Vec<AgentGoal>>;

    /// 前置边界校验——检查目标是否超出 Agent 权限边界
    async fn check_boundary(&self, goal: &AgentGoal) -> AgentResult<AuthLevel>;
}

#[cfg(test)]
mod tests {
    use crate::cognitive::goal::{AgentGoal, AuthLevel, ConstraintSeverity};

    #[test]
    fn test_agent_goal_construction() {
        let goal = AgentGoal::new("帮我优化数据库", "优化数据库查询性能")
            .with_criterion("延迟降低50%")
            .with_auth(AuthLevel::RequiresReview);
        assert_eq!(goal.primary_objective, "优化数据库查询性能");
        assert_eq!(goal.success_criteria.len(), 1);
        assert_eq!(goal.required_auth_level, AuthLevel::RequiresReview);
    }

    #[test]
    fn test_agent_goal_with_constraint() {
        let goal = AgentGoal::new("deploy", "部署到生产").with_constraint(
            "time",
            "必须在非工作时间",
            ConstraintSeverity::Required,
        );
        assert_eq!(goal.constraints.len(), 1);
        assert_eq!(goal.constraints[0].field, "time");
    }

    #[test]
    fn test_auth_level_default() {
        assert_eq!(AuthLevel::default(), AuthLevel::FullAuto);
    }

    #[test]
    fn test_agent_goal_id_is_unique() {
        let g1 = AgentGoal::new("a", "a");
        let g2 = AgentGoal::new("a", "a");
        assert_ne!(g1.id, g2.id);
    }
}
