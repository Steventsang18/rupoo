//! Orchestrator integration test — mock full-stack pipeline verification.
//!
//! Uses mock implementations for all 5 layers to verify the Orchestrator
//! pipeline executes correctly end-to-end without real LLM dependencies.

use std::sync::Arc;

use async_trait::async_trait;
use rupoo::cognitive::goal::{AgentGoal, AuthLevel};
use rupoo::cognitive::CognitiveEngine;
use rupoo::context::ConversationContext;
use rupoo::db::TaskRepo;
use rupoo::error::AgentResult;
use rupoo::execution::{ExecutionEngine, ValidationResult};
use rupoo::memory::{MemoryStorage, MemorySystem, MemorySystemBridge};
use rupoo::orchestrator::Orchestrator;
use rupoo::planning::{ExecutionPlan, PlanScore, Planner};
use rupoo::supervisor::{Action, ExecutionMeta, Supervisor};
use rupoo::task::{MemoryEntry, Step, StepStatus};

// ---------------------------------------------------------------------------
// Mock CognitiveEngine — returns a fixed goal
// ---------------------------------------------------------------------------

struct MockCognitive;
#[async_trait]
impl CognitiveEngine for MockCognitive {
    async fn parse(&self, raw: &str, _ctx: &ConversationContext) -> AgentResult<AgentGoal> {
        Ok(AgentGoal::new(raw, "mock parsed goal"))
    }

    async fn decompose(&self, _goal: &AgentGoal) -> AgentResult<Vec<AgentGoal>> {
        Ok(vec![])
    }

    async fn check_boundary(&self, _goal: &AgentGoal) -> AgentResult<AuthLevel> {
        Ok(AuthLevel::FullAuto)
    }
}

// ---------------------------------------------------------------------------
// Mock Planner — returns a plan with a single Think step
// ---------------------------------------------------------------------------

struct MockPlanner;
#[async_trait]
impl Planner for MockPlanner {
    async fn generate_alternatives(
        &self,
        _goal: &AgentGoal,
        count: usize,
    ) -> AgentResult<Vec<ExecutionPlan>> {
        let plan = ExecutionPlan::new(
            "mock-goal",
            "mock execution plan",
            vec![
                Step::Think {
                    id: "step-1".to_string(),
                    instruction: "analyze the problem".to_string(),
                    status: StepStatus::Pending,
                    output: None,
                },
                Step::Think {
                    id: "step-2".to_string(),
                    instruction: "complete the task".to_string(),
                    status: StepStatus::Pending,
                    output: None,
                },
            ],
        );
        Ok(vec![plan; count.min(3)])
    }

    async fn score(&self, _plan: &ExecutionPlan) -> AgentResult<PlanScore> {
        Ok(PlanScore {
            success_probability: 0.85,
            resource_cost: 0.3,
            risk_level: 0.1,
            weighted_total: 0.8,
            scoring_log: vec![],
        })
    }

    async fn select_best(
        &self,
        mut candidates: Vec<ExecutionPlan>,
    ) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)> {
        candidates.sort_by(|a, b| {
            let sa = a.score.as_ref().map(|s| s.weighted_total).unwrap_or(0.0);
            let sb = b.score.as_ref().map(|s| s.weighted_total).unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut iter = candidates.into_iter();
        let best = iter
            .next()
            .ok_or_else(|| rupoo::error::AgentError::Other("no plans".into()))?;
        Ok((best, iter.collect()))
    }
}

// ---------------------------------------------------------------------------
// Mock ExecutionEngine — always passes validation
// ---------------------------------------------------------------------------

struct MockExecutionEngine;
#[async_trait]
impl ExecutionEngine for MockExecutionEngine {
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

// ---------------------------------------------------------------------------
// Mock Supervisor — always allows
// ---------------------------------------------------------------------------

struct MockSupervisor;
#[async_trait]
impl Supervisor for MockSupervisor {
    async fn intercept(&self, _action: &Action, _meta: &ExecutionMeta) -> AgentResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock MemoryStorage for MemorySystem
// ---------------------------------------------------------------------------

struct MockMemoryStorage;
#[async_trait]
impl MemoryStorage for MockMemoryStorage {
    async fn store(&self, _entry: MemoryEntry) -> AgentResult<()> {
        Ok(())
    }
    async fn retrieve(&self, _query: &str, _limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        Ok(vec![])
    }
    async fn delete(&self, _id: &str) -> AgentResult<()> {
        Ok(())
    }
    async fn count(&self) -> AgentResult<usize> {
        Ok(0)
    }
}

struct MockMemorySystem;
#[async_trait]
impl MemorySystem for MockMemorySystem {
    fn short_term(&self) -> &dyn MemoryStorage {
        &MockMemoryStorage
    }
    fn long_term(&self) -> &dyn MemoryStorage {
        &MockMemoryStorage
    }
    fn episodic(&self) -> &dyn MemoryStorage {
        &MockMemoryStorage
    }
    async fn hybrid_recall(&self, _query: &str, _limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_orchestrator_full_pipeline_succeeds() {
    let orch = Orchestrator::new(
        Box::new(MockCognitive),
        Box::new(MockPlanner),
        Box::new(MockExecutionEngine),
        Arc::new(MockMemorySystem),
        Box::new(MockSupervisor),
    );

    let result = orch.execute("test instruction for integration test").await;
    assert!(
        result.is_ok(),
        "orchestrator pipeline should succeed: {:?}",
        result
    );
}

#[tokio::test]
async fn test_orchestrator_with_memory_system_bridge() {
    let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
    let memory = Arc::new(MemorySystemBridge::new(repo));

    let orch = Orchestrator::new(
        Box::new(MockCognitive),
        Box::new(MockPlanner),
        Box::new(MockExecutionEngine),
        memory,
        Box::new(MockSupervisor),
    );

    let result = orch.execute("test with real bridge").await;
    assert!(
        result.is_ok(),
        "pipeline with MemorySystemBridge should succeed: {:?}",
        result
    );
}

#[tokio::test]
async fn test_orchestrator_supervisor_blocks_forbidden_action() {
    struct BlockingSupervisor;
    #[async_trait]
    impl Supervisor for BlockingSupervisor {
        async fn intercept(&self, _action: &Action, _meta: &ExecutionMeta) -> AgentResult<()> {
            Err(rupoo::error::AgentError::Safety("mock block".to_string()))
        }
    }

    let orch = Orchestrator::new(
        Box::new(MockCognitive),
        Box::new(MockPlanner),
        Box::new(MockExecutionEngine),
        Arc::new(MockMemorySystem),
        Box::new(BlockingSupervisor),
    );

    let result = orch.execute("should be blocked").await;
    assert!(result.is_err(), "blocked action should return error");
    assert!(result.unwrap_err().to_string().contains("mock block"));
}

#[tokio::test]
async fn test_orchestrator_empty_plans_return_error() {
    struct EmptyPlanner;
    #[async_trait]
    impl Planner for EmptyPlanner {
        async fn generate_alternatives(
            &self,
            _goal: &AgentGoal,
            _count: usize,
        ) -> AgentResult<Vec<ExecutionPlan>> {
            Ok(vec![])
        }
        async fn score(&self, _plan: &ExecutionPlan) -> AgentResult<PlanScore> {
            Ok(PlanScore {
                success_probability: 0.0,
                resource_cost: 0.0,
                risk_level: 0.0,
                weighted_total: 0.0,
                scoring_log: vec![],
            })
        }
        async fn select_best(
            &self,
            _candidates: Vec<ExecutionPlan>,
        ) -> AgentResult<(ExecutionPlan, Vec<ExecutionPlan>)> {
            Err(rupoo::error::AgentError::Other("no plans available".into()))
        }
    }

    let orch = Orchestrator::new(
        Box::new(MockCognitive),
        Box::new(EmptyPlanner),
        Box::new(MockExecutionEngine),
        Arc::new(MockMemorySystem),
        Box::new(MockSupervisor),
    );

    let result = orch.execute("should fail").await;
    assert!(result.is_err(), "empty plans should cause error");
}
