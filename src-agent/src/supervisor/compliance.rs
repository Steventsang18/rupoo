use std::collections::HashSet;
use tracing::warn;

use crate::error::AgentResult;
use crate::supervisor::Action;

/// 合规校验结果
#[derive(Debug, Clone)]
pub struct ComplianceResult {
    pub allowed: bool,
    pub reason: String,
}

/// 合规校验器——检查动作是否越权
#[derive(Debug, Clone)]
pub struct ComplianceChecker {
    /// 永久禁止的命令
    forbidden_commands: HashSet<String>,
    /// 需要审批的工具
    approval_required_tools: HashSet<String>,
}

impl ComplianceChecker {
    pub fn new(
        forbidden: Vec<String>,
        approval_required: Vec<String>,
    ) -> Self {
        Self {
            forbidden_commands: forbidden.into_iter().collect(),
            approval_required_tools: approval_required.into_iter().collect(),
        }
    }

    /// 从 SafetyContext 构建（保持向后兼容）
    pub fn from_safety_ctx(ctx: &crate::safety::SafetyContext) -> Self {
        // 提取 forbidden_commands
        let cb = ctx.forbidden_commands();
        let forbidden: Vec<String> = cb.into_iter().map(|s| s.to_lowercase()).collect();

        // needs_approval 使用字符串匹配，这里提取所有审批需要的工具前缀
        let mut approval: Vec<String> = Vec::new();
        // 常用的审批工具列表
        for t in &["delete_file", "rm", "remove", "exec", "run_command",
            "bash", "sh", "zsh", "sudo", "reboot", "shutdown",
            "http_delete", "http_post", "python", "python3", "perl", "ruby", "node"] {
            approval.push(t.to_string());
        }

        Self::new(forbidden, approval)
    }

    /// 单次合规校验
    pub fn check(&self, action: &Action) -> AgentResult<ComplianceResult> {
        // 提取 base command，与 is_forbidden()/needs_approval() 保持一致的 split 逻辑
        let base = action.action_type.split_whitespace().next().unwrap_or(&action.action_type).to_lowercase();

        // 检查禁止命令
        if self.is_forbidden(&action.action_type) {
            warn!(command = %base, "blocked forbidden command");
            return Ok(ComplianceResult {
                allowed: false,
                reason: format!("命令 '{}' 被安全策略禁止", base),
            });
        }

        // 检查是否需要审批——当前阶段放行，审批逻辑由外部控制
        if self.needs_approval(&action.action_type) {
            // 这里返回 allowed=true 但标记需要审批的信号
            // 实际审批在编排器层面由 loop_engine 的 autonomy level 控制
            return Ok(ComplianceResult {
                allowed: true,
                reason: format!("工具 '{}' 需要审批，已放行至下一闸门", base),
            });
        }

        Ok(ComplianceResult {
            allowed: true,
            reason: "通过合规校验".to_string(),
        })
    }

    /// 检查命令是否在禁止列表中（供 SafetyContext 调用）
    pub fn is_forbidden(&self, command: &str) -> bool {
        let base = command.split_whitespace().next().unwrap_or(command).to_lowercase();
        self.forbidden_commands.contains(&base)
    }

    pub fn needs_approval(&self, tool_name: &str) -> bool {
        let lower = tool_name.split_whitespace().next().unwrap_or(tool_name).to_lowercase();
        self.approval_required_tools.contains(&lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::Action;

    #[test]
    fn test_forbidden_command_blocked() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string(), "rm".to_string()],
            vec!["bash".to_string()],
        );
        let action = Action::new("sudo", "sudo rm -rf /");
        let result = checker.check(&action).unwrap();
        assert!(!result.allowed);
    }

    #[test]
    fn test_default_allow_passes() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string()],
            vec![],
        );
        let action = Action::new("echo", "echo hello");
        let result = checker.check(&action).unwrap();
        assert!(result.allowed);
    }

    #[test]
    fn test_needs_approval_returns_true() {
        let checker = ComplianceChecker::new(
            vec![],
            vec!["bash".to_string(), "sh".to_string()],
        );
        assert!(checker.needs_approval("bash -c 'ls'"));
        assert!(!checker.needs_approval("echo hello"));
    }

    #[test]
    fn test_is_forbidden() {
        let checker = ComplianceChecker::new(
            vec!["sudo".to_string(), "rm".to_string()],
            vec![],
        );
        assert!(checker.is_forbidden("sudo"));
        assert!(!checker.is_forbidden("ls"));
    }

    #[test]
    fn test_empty_forbidden_allows_all() {
        let checker = ComplianceChecker::new(
            vec![],
            vec![],
        );
        let action = Action::new("any_command", "anything");
        let result = checker.check(&action).unwrap();
        assert!(result.allowed);
    }
}
