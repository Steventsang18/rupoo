use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 权限级别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum AuthLevel {
    /// 全自动执行，无需审批
    #[default]
    FullAuto,
    /// 需要人工复核
    RequiresReview,
    /// 禁止执行
    Forbidden,
}

/// 目标约束条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConstraint {
    /// 约束字段名（如 "time", "resource", "permission"）
    pub field: String,
    /// 约束描述
    pub description: String,
    /// 严重程度
    pub severity: ConstraintSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConstraintSeverity {
    /// 建议性约束
    Suggestion,
    /// 硬性约束
    Required,
    /// 安全约束——违反则禁止执行
    Security,
}

/// 结构化业务目标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGoal {
    /// 目标唯一标识
    pub id: String,
    /// 用户原始指令（保留审计溯源）
    pub raw_instruction: String,
    /// 提炼后的核心目标描述
    pub primary_objective: String,
    /// 清晰的成功标准列表
    pub success_criteria: Vec<String>,
    /// 约束条件（时间、资源、权限边界）
    pub constraints: Vec<GoalConstraint>,
    /// 拆解后的子目标（如果有）
    pub sub_goals: Vec<AgentGoal>,
    /// 权限等级需求
    pub required_auth_level: AuthLevel,
    /// 扩展属性（用于灵活的场景特定信息）
    pub metadata: HashMap<String, String>,
}

impl AgentGoal {
    pub fn new(raw_instruction: &str, primary_objective: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            raw_instruction: raw_instruction.to_string(),
            primary_objective: primary_objective.to_string(),
            success_criteria: Vec::new(),
            constraints: Vec::new(),
            sub_goals: Vec::new(),
            required_auth_level: AuthLevel::FullAuto,
            metadata: HashMap::new(),
        }
    }

    /// 添加一个成功标准
    pub fn with_criterion(mut self, criterion: &str) -> Self {
        self.success_criteria.push(criterion.to_string());
        self
    }

    /// 添加一个约束
    pub fn with_constraint(
        mut self,
        field: &str,
        desc: &str,
        severity: ConstraintSeverity,
    ) -> Self {
        self.constraints.push(GoalConstraint {
            field: field.to_string(),
            description: desc.to_string(),
            severity,
        });
        self
    }

    /// 设置权限等级
    pub fn with_auth(mut self, level: AuthLevel) -> Self {
        self.required_auth_level = level;
        self
    }
}
