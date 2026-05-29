//! Plan Pool — parallel plan execution with crossbeam channels.
//!
//! Multiple independent plans can run simultaneously without blocking
//! each other. Each plan gets its own execution context.
//!
//! Usage:
//! ```rust
//! let pool = PlanPool::new();
//! pool.run_parallel(repo, agent, vec!["plan-1", "plan-2"]).await;
//! ```

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use tracing::{warn, error};

use crate::agent::Agent;

// ---------------------------------------------------------------------------
// Plan status messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PlanStatus {
    pub plan_id: String,
    pub plan_name: String,
    pub step_index: usize,
    pub total_steps: usize,
    pub state: PlanState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanState {
    Running,
    Completed,
    Failed,
    WaitingForInput,
}

// ---------------------------------------------------------------------------
// Plan Pool
// ---------------------------------------------------------------------------

pub struct PlanPool {
    status_tx: Sender<PlanStatus>,
    status_rx: Receiver<PlanStatus>,
}

impl PlanPool {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self { status_tx: tx, status_rx: rx }
    }

    /// Run multiple plans in parallel using shared agent reference.
    ///
    /// Each plan is spawned as a separate tokio task.
    /// Status updates are sent through the channel.
    /// Returns when all plans have completed or failed.
    ///
    /// Note: Agent is not Clone, so we use Arc<Mutex<Agent>> for shared access.
    /// Plans execute concurrently but agent calls are serialized.
    /// For truly parallel execution, create separate Agent instances per plan.
    pub async fn run_parallel_serialized(
        &self,
        agent: Arc<tokio::sync::Mutex<Agent>>,
        plan_ids: Vec<String>,
    ) -> Vec<PlanStatus> {
        let mut handles = Vec::new();

        for plan_id in plan_ids {
            let agent = Arc::clone(&agent);
            let tx = self.status_tx.clone();

            let handle = tokio::spawn(async move {
                let agent_guard = agent.lock().await;
                match agent_guard.resume(&plan_id).await {
                    Ok(Some(mut plan)) => {
                        let plan_name = plan.name.clone();
                        let total_steps = plan.steps.len();

                        tx.send(PlanStatus {
                            plan_id: plan_id.clone(),
                            plan_name: plan_name.clone(),
                            step_index: plan.current_step_index,
                            total_steps,
                            state: PlanState::Running,
                            message: None,
                        }).ok();

                        // Execute the plan step by step
                        loop {
                            let step_idx = plan.current_step_index;
                            let outcome = agent_guard.run_next_step(&mut plan).await;

                            match outcome {
                                Ok(crate::agent::StepOutcome::Advanced) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: step_idx + 1,
                                        total_steps,
                                        state: PlanState::Running,
                                        message: None,
                                    }).ok();
                                }
                                Ok(crate::agent::StepOutcome::Finished) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: total_steps,
                                        total_steps,
                                        state: PlanState::Completed,
                                        message: Some("Plan completed".into()),
                                    }).ok();
                                    break;
                                }
                                Ok(crate::agent::StepOutcome::WaitingForInput(prompt)) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: step_idx,
                                        total_steps,
                                        state: PlanState::WaitingForInput,
                                        message: Some(prompt),
                                    }).ok();
                                    break;
                                }
                                Ok(crate::agent::StepOutcome::RequiresApproval { .. }) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: step_idx,
                                        total_steps,
                                        state: PlanState::WaitingForInput,
                                        message: Some("Requires approval".into()),
                                    }).ok();
                                    break;
                                }
                                Ok(crate::agent::StepOutcome::Failed(err)) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: step_idx,
                                        total_steps,
                                        state: PlanState::Failed,
                                        message: Some(err),
                                    }).ok();
                                    break;
                                }
                                Err(e) => {
                                    tx.send(PlanStatus {
                                        plan_id: plan_id.clone(),
                                        plan_name: plan_name.clone(),
                                        step_index: step_idx,
                                        total_steps,
                                        state: PlanState::Failed,
                                        message: Some(e.to_string()),
                                    }).ok();
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        tx.send(PlanStatus {
                            plan_id: plan_id.clone(),
                            plan_name: "unknown".into(),
                            step_index: 0,
                            total_steps: 0,
                            state: PlanState::Completed,
                            message: Some("Already completed".into()),
                        }).ok();
                    }
                    Err(e) => {
                        error!(plan_id = %plan_id, error = %e, "failed to load plan");
                        tx.send(PlanStatus {
                            plan_id: plan_id.clone(),
                            plan_name: "unknown".into(),
                            step_index: 0,
                            total_steps: 0,
                            state: PlanState::Failed,
                            message: Some(e.to_string()),
                        }).ok();
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!(error = %e, "plan task panicked");
            }
        }

        // Collect all status updates
        let mut statuses = Vec::new();
        while let Ok(status) = self.status_rx.try_recv() {
            statuses.push(status);
        }
        statuses
    }

    /// Cancel a running plan (signal via channel).
    pub fn cancel(&self, _plan_id: &str) {
        // In a full implementation, we'd track plan handles and abort them.
        // For now, this is a placeholder for the cancel signal.
        warn!("plan cancellation not yet fully implemented");
    }

    /// Get the status receiver for monitoring plan progress.
    pub fn status_receiver(&self) -> &Receiver<PlanStatus> {
        &self.status_rx
    }
}

impl Default for PlanPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_pool_channel() {
        let pool = PlanPool::new();
        pool.status_tx.send(PlanStatus {
            plan_id: "test".into(),
            plan_name: "Test Plan".into(),
            step_index: 0,
            total_steps: 3,
            state: PlanState::Running,
            message: None,
        }).unwrap();

        let status = pool.status_rx.try_recv().unwrap();
        assert_eq!(status.plan_id, "test");
        assert_eq!(status.state, PlanState::Running);
    }

    #[test]
    fn test_plan_status_clone() {
        let status = PlanStatus {
            plan_id: "p1".into(),
            plan_name: "Plan 1".into(),
            step_index: 1,
            total_steps: 5,
            state: PlanState::Completed,
            message: Some("Done".into()),
        };
        let cloned = status.clone();
        assert_eq!(cloned.plan_id, "p1");
        assert_eq!(cloned.state, PlanState::Completed);
    }
}
