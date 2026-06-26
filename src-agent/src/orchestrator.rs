use std::sync::Arc;
use tracing::{info, warn};

use crate::cognitive::goal::{AgentGoal, AuthLevel};
use crate::cognitive::CognitiveEngine;
use crate::error::{AgentError, AgentResult};
use crate::execution::ExecutionEngine;
use crate::memory::MemorySystem;
use crate::planning::{ExecutionPlan, Planner};
use crate::supervisor::{Action, ExecutionMeta, Supervisor};
use crate::task::Step;

/// 五层编排器——认知→规划→执行→记忆→监督
pub struct Orchestrator {
    pub cognitive: Box<dyn CognitiveEngine>,
    pub planner: Box<dyn Planner>,
    pub execution: Box<dyn ExecutionEngine>,
    pub memory: Arc<dyn MemorySystem>,
    pub supervisor: Box<dyn Supervisor>,
}

impl Orchestrator {
    pub fn new(
        cognitive: Box<dyn CognitiveEngine>,
        planner: Box<dyn Planner>,
        execution: Box<dyn ExecutionEngine>,
        memory: Arc<dyn MemorySystem>,
        supervisor: Box<dyn Supervisor>,
    ) -> Self {
        Self {
            cognitive,
            planner,
            execution,
            memory,
            supervisor,
        }
    }

    /// 五层闭环执行管线
    pub async fn execute(&self, raw_instruction: &str) -> AgentResult<()> {
        // ======== 第0层：监督拦截（前置） ========
        let init_action = Action::new("execute_task", raw_instruction);
        self.supervisor
            .intercept(&init_action, &ExecutionMeta::default())
            .await?;

        // ======== 第1层：认知层——目标还原 ========
        info!("[认知层] 解析用户指令: {}", raw_instruction);
        let context = crate::context::ConversationContext::collect();
        let goal = self.cognitive.parse(raw_instruction, &context).await?;
        info!("[认知层] 目标: {}", goal.primary_objective);

        // 边界校验
        let auth_level = self.cognitive.check_boundary(&goal).await?;
        if auth_level == AuthLevel::Forbidden {
            return Err(AgentError::Safety("目标被安全策略禁止执行".into()));
        }
        if auth_level == AuthLevel::RequiresReview {
            info!("[认知层] 目标需要人工复核");
            // 当前版本：RequiresReview 仍继续，由监督层拦截实际动作
        }

        // ======== 第2层：规划层——多方案择优 ========
        info!("[规划层] 生成候选方案...");
        let mut candidates = self.planner.generate_alternatives(&goal, 3).await?;
        if candidates.is_empty() {
            return Err(AgentError::Other("无法生成任何执行方案".into()));
        }

        // 为每个候选方案打分
        for plan in &mut candidates {
            let score = self.planner.score(plan).await?;
            plan.score = Some(score);
        }

        // 择优
        let (best_plan, fallbacks) = self.planner.select_best(candidates).await?;
        info!(
            "[规划层] 选择方案: {} (成功率={:.2}, 风险={:.2})",
            best_plan.name,
            best_plan
                .score
                .as_ref()
                .map(|s| s.success_probability)
                .unwrap_or(0.0),
            best_plan
                .score
                .as_ref()
                .map(|s| s.risk_level)
                .unwrap_or(0.0),
        );
        info!("[规划层] {} 个备选方案", fallbacks.len());

        // ======== 第3层：执行层——带监督的逐步执行 ========
        let mut plan = best_plan.clone();
        let mut step_offset = 0;

        while step_offset < plan.steps.len() {
            let i = step_offset;
            let step = &plan.steps[i];
            let step_label = format!("step {}/{}", i + 1, plan.steps.len());
            let step_action = Action::new("execute_step", &step_label);
            let meta = ExecutionMeta::with_tool(&format!("{:?}", std::mem::discriminant(step)));

            // 每步执行前监督拦截
            self.supervisor.intercept(&step_action, &meta).await?;

            // 根据步骤类型提取入参
            let input_params = match step {
                Step::ToolCall {
                    tool_name, params, ..
                } => {
                    serde_json::json!({ "tool": tool_name, "params": params })
                }
                Step::Exec { command, args, .. } => {
                    serde_json::json!({ "command": command, "args": args })
                }
                Step::HttpRequest { method, url, .. } => {
                    serde_json::json!({ "method": method, "url": url })
                }
                _ => serde_json::json!({}), // Think, WaitForInput, Finish
            };

            // 入参校验
            let validation = self.execution.validate_input("step", &input_params).await?;

            if validation.trigger_replan {
                warn!("[执行层] 步骤 {} 入参校验失败，触发重规划", i);

                // 收集失败步骤信息后重规划
                let revised = self.planner.generate_alternatives(&goal, 1).await?;

                if let Some(new_plan) = revised.into_iter().next() {
                    info!("[执行层] 重规划完成，新方案: {}", new_plan.name);
                    plan = new_plan;
                    step_offset = 0; // 从头执行新方案
                    continue;
                } else {
                    warn!("[执行层] 重规划失败，跳过当前步骤继续");
                    step_offset += 1;
                    continue;
                }
            }

            info!("[执行层] {} 校验通过", step_label);
            // 实际执行由现存的 Agent::run_next_step 或 ToolExecutor 处理
            // 编排器当前是调度层，实际 step 执行由 agent 完成
            step_offset += 1;
        }

        // ======== 第4层：记忆层——经验沉淀 ========
        info!("[记忆层] 沉淀经验: {}", goal.primary_objective);
        // 情景记忆存储
        if let Err(e) = self.remember_execution(&goal, &best_plan).await {
            warn!("[记忆层] 情景记忆存储失败: {}", e);
        }

        info!("[编排器] 任务执行完成: {}", goal.primary_objective);
        Ok(())
    }

    /// 将执行记录存储为情景记忆
    async fn remember_execution(&self, goal: &AgentGoal, plan: &ExecutionPlan) -> AgentResult<()> {
        use crate::task::MemoryEntry;

        let tags = vec![
            "execution".to_string(),
            "case".to_string(),
            goal.primary_objective
                .split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string(),
        ];

        let summary = format!(
            "目标: {}\n方案: {}\n步骤数: {}",
            goal.primary_objective,
            plan.name,
            plan.steps.len(),
        );

        let entry = MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            content: summary,
            tags,
            source: "orchestrator".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        self.memory.episodic().store(entry).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::goal::AgentGoal;
    use crate::memory::traits::MemoryStorage;
    use async_trait::async_trait;

    /// 模拟认知层——用于测试
    struct MockCognitive;
    #[async_trait]
    impl CognitiveEngine for MockCognitive {
        async fn parse(
            &self,
            raw: &str,
            _ctx: &crate::context::ConversationContext,
        ) -> AgentResult<AgentGoal> {
            Ok(AgentGoal::new(raw, "模拟目标"))
        }
        async fn decompose(&self, _goal: &AgentGoal) -> AgentResult<Vec<AgentGoal>> {
            Ok(vec![])
        }
        async fn check_boundary(&self, _goal: &AgentGoal) -> AgentResult<AuthLevel> {
            Ok(AuthLevel::FullAuto)
        }
    }

    /// 监督层 mock——始终放行
    struct MockSupervisor;
    #[async_trait]
    impl Supervisor for MockSupervisor {
        async fn intercept(&self, _action: &Action, _meta: &ExecutionMeta) -> AgentResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_orchestrator_creates() {
        let cognitive = Box::new(MockCognitive);
        let planner = Box::new(crate::planning::planner::PlannerImpl);
        let execution = Box::new(crate::execution::validator::ExecutionEngineImpl);
        let memory = Arc::new(MockMemorySystem);
        let supervisor = Box::new(MockSupervisor);

        let orch = Orchestrator::new(cognitive, planner, execution, memory, supervisor);
        let result = orch.execute("测试指令").await;
        // 因为规划器返回空方案，预计失败
        assert!(result.is_err());
    }

    /// 模拟三层记忆系统
    struct MockMemorySystem;
    #[async_trait]
    impl MemorySystem for MockMemorySystem {
        fn short_term(&self) -> &dyn MemoryStorage {
            &MockStorage
        }
        fn long_term(&self) -> &dyn MemoryStorage {
            &MockStorage
        }
        fn episodic(&self) -> &dyn MemoryStorage {
            &MockStorage
        }
        async fn hybrid_recall(
            &self,
            _query: &str,
            _limit: usize,
        ) -> AgentResult<Vec<crate::task::MemoryEntry>> {
            Ok(vec![])
        }
    }

    struct MockStorage;
    #[async_trait]
    impl MemoryStorage for MockStorage {
        async fn store(&self, _entry: crate::task::MemoryEntry) -> AgentResult<()> {
            Ok(())
        }
        async fn retrieve(
            &self,
            _query: &str,
            _limit: usize,
        ) -> AgentResult<Vec<crate::task::MemoryEntry>> {
            Ok(vec![])
        }
        async fn delete(&self, _id: &str) -> AgentResult<()> {
            Ok(())
        }
        async fn count(&self) -> AgentResult<usize> {
            Ok(0)
        }
    }
}
