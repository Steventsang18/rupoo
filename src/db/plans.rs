//! Plan and Checkpoint CRUD operations
//!
//! Split from db.rs (Phase 1 Step 2)

use tracing::info;

use crate::error::{AgentError, AgentResult};
use crate::task::{
    Checkpoint, CheckpointStatus, Plan, PlanStatus, Step, StepStatus,
};

use super::TaskRepo;

// ---------------------------------------------------------------------------
// Status string helpers (store as plain text, not JSON-encoded)
// ---------------------------------------------------------------------------

fn plan_status_to_str(s: &PlanStatus) -> &'static str {
    match s {
        PlanStatus::Pending => "Pending",
        PlanStatus::Running => "Running",
        PlanStatus::Completed => "Completed",
        PlanStatus::Failed => "Failed",
        PlanStatus::WaitingForInput => "WaitingForInput",
    }
}

fn str_to_plan_status(s: &str) -> AgentResult<PlanStatus> {
    match s {
        "Pending" => Ok(PlanStatus::Pending),
        "Running" => Ok(PlanStatus::Running),
        "Completed" => Ok(PlanStatus::Completed),
        "Failed" => Ok(PlanStatus::Failed),
        "WaitingForInput" => Ok(PlanStatus::WaitingForInput),
        other => Err(AgentError::Other(format!("unknown plan status: {other}"))),
    }
}

// ---------------------------------------------------------------------------
// Checkpoint status helper
// ---------------------------------------------------------------------------

fn checkpoint_status_to_str(s: &CheckpointStatus) -> &'static str {
    match s {
        CheckpointStatus::Running => "Running",
        CheckpointStatus::Completed => "Completed",
        CheckpointStatus::Failed => "Failed",
    }
}

// ---------------------------------------------------------------------------
// Plan/Checkpoint impl
// ---------------------------------------------------------------------------

impl TaskRepo {
    // ---------------------------------------------------------------------------
    // Plan CRUD
    // ---------------------------------------------------------------------------

    /// Save a plan to the database.
    pub async fn save_plan(&self, plan: &Plan) -> AgentResult<()> {
        let id = plan.id.clone();
        let name = plan.name.clone();
        let steps_json = serde_json::to_string(&plan.steps)?;
        let step_index = plan.current_step_index;
        let status = plan_status_to_str(&plan.status);
        let created_at = plan.created_at.to_rfc3339();
        let updated_at = plan.updated_at.to_rfc3339();

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO plans (id, name, steps_json, current_step_index, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, name, steps_json, step_index, status, created_at, updated_at],
            )?;
            Ok(())
        })
        .await
    }

    /// Load a plan by ID.
    pub async fn load_plan(&self, plan_id: &str) -> AgentResult<Plan> {
        let pid = plan_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, steps_json, current_step_index, status, created_at, updated_at
                 FROM plans WHERE id = ?1",
            )?;

            let row = stmt
                .query_row(rusqlite::params![pid], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, usize>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AgentError::PlanNotFound(pid.clone())
                    }
                    other => AgentError::Database(other),
                })?;

            let (id, name, steps_json, current_step_index, status_str, created_at_str, updated_at_str) = row;

            let steps: Vec<Step> = serde_json::from_str(&steps_json)?;
            let status = str_to_plan_status(&status_str)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| AgentError::Other(format!("parse created_at: {e}")))?
                .with_timezone(&chrono::Utc);
            let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map_err(|e| AgentError::Other(format!("parse updated_at: {e}")))?
                .with_timezone(&chrono::Utc);

            Ok(Plan {
                id,
                name,
                steps,
                current_step_index,
                status,
                created_at,
                updated_at,
            })
        })
        .await
    }

    /// Atomically update a plan's step index, status, and the step's own status
    /// **and** insert a checkpoint — all in a single transaction.
    pub async fn record_step_completion(
        &self,
        plan_id: &str,
        step_index: usize,
        step_status: StepStatus,
        output: Option<String>,
    ) -> AgentResult<()> {
        let pid = plan_id.to_string();
        let checkpoint_id = uuid::Uuid::new_v4().to_string();
        let checkpoint_created = chrono::Utc::now().to_rfc3339();
        let ckpt_status = match &step_status {
            StepStatus::Completed => "Completed",
            StepStatus::Failed => "Failed",
            StepStatus::Running => "Running",
            StepStatus::WaitingForInput => "Pending", // waiter resumes later
            _ => "Completed",
        };

        let output_json = output.clone();

        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;

            // 1. Load current plan data
            let (steps_json, _old_idx, _old_status): (String, usize, String) = tx
                .query_row(
                    "SELECT steps_json, current_step_index, status FROM plans WHERE id = ?1",
                    rusqlite::params![pid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AgentError::PlanNotFound(pid.clone())
                    }
                    other => AgentError::Database(other),
                })?;

            // 2. Update the step's status inside the JSON
            let mut steps: Vec<Step> = serde_json::from_str(&steps_json)?;
            // 3. Determine new plan-level status and next step index
            // (must happen before we move step_status into set_status)
            let new_index = step_index + 1;
            let next_step = steps.get(new_index);
            let step_is_waiting = matches!(step_status, StepStatus::WaitingForInput);
            let plan_status: &str = if step_is_waiting {
                "WaitingForInput"
            } else if next_step.is_none() || step_index >= steps.len() - 1 {
                if output_json.as_deref() == Some("__FAILED__") {
                    "Failed"
                } else {
                    "Completed"
                }
            } else if next_step.is_some_and(|s| s.is_waiting()) {
                "WaitingForInput"
            } else {
                "Running"
            };
            if let Some(step) = steps.get_mut(step_index) {
                step.set_status(step_status);
            }
            let new_steps_json = serde_json::to_string(&steps)?;
            let now = chrono::Utc::now().to_rfc3339();

            // 4. Update plan row
            tx.execute(
                "UPDATE plans SET steps_json = ?1, current_step_index = ?2, status = ?3, updated_at = ?4 WHERE id = ?5",
                rusqlite::params![new_steps_json, new_index, plan_status, now, pid],
            )?;

            // 5. Insert checkpoint atomically
            tx.execute(
                "INSERT INTO checkpoints (id, plan_id, step_index, status, output, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    checkpoint_id,
                    pid,
                    step_index,
                    ckpt_status,
                    output_json,
                    checkpoint_created,
                ],
            )?;

            tx.commit()?;

            info!(
                plan_id = %pid,
                step = step_index,
                plan_status,
                "checkpoint committed"
            );

            Ok(())
        })
        .await
    }

    /// Non-transactional step status update (used for intermediate progress).
    pub async fn update_step_progress(
        &self,
        plan_id: &str,
        step_index: usize,
        step_status: StepStatus,
    ) -> AgentResult<()> {
        let pid = plan_id.to_string();
        self.with_conn(move |conn| {
            let (steps_json,): (String,) = conn
                .query_row(
                    "SELECT steps_json FROM plans WHERE id = ?1",
                    rusqlite::params![pid],
                    |row| Ok((row.get(0)?,)),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AgentError::PlanNotFound(pid.clone())
                    }
                    other => AgentError::Database(other),
                })?;

            let mut steps: Vec<Step> = serde_json::from_str(&steps_json)?;
            if let Some(step) = steps.get_mut(step_index) {
                step.set_status(step_status);
            }

            let new_steps_json = serde_json::to_string(&steps)?;
            let now = chrono::Utc::now().to_rfc3339();

            conn.execute(
                "UPDATE plans SET steps_json = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![new_steps_json, now, pid],
            )?;

            Ok(())
        })
        .await
    }

    // ---------------------------------------------------------------------------
    // Checkpoint queries
    // ---------------------------------------------------------------------------

    /// Get the last checkpoint for a plan.
    pub async fn get_last_checkpoint(&self, plan_id: &str) -> AgentResult<Option<Checkpoint>> {
        let pid = plan_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, plan_id, step_index, status, output, created_at
                 FROM checkpoints
                 WHERE plan_id = ?1
                 ORDER BY step_index DESC, created_at DESC
                 LIMIT 1",
            )?;

            let result = stmt.query_row(rusqlite::params![pid], |row| {
                // Extract raw strings first (within the rusqlite closure where errors convert)
                let id: String = row.get(0)?;
                let cplan_id: String = row.get(1)?;
                let step_index: usize = row.get(2)?;
                let status: String = row.get(3)?;
                let output: Option<String> = row.get(4)?;
                let created_at: String = row.get(5)?;
                Ok((id, cplan_id, step_index, status, output, created_at))
            });

            match result {
                Ok((id, cplan_id, step_index, status_str, output, created_at_str)) => {
                    let status = match status_str.as_str() {
                        "Completed" => CheckpointStatus::Completed,
                        "Running" => CheckpointStatus::Running,
                        _ => CheckpointStatus::Failed,
                    };
                    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .map_err(|e| {
                            AgentError::Other(format!("parse checkpoint date: {e}"))
                        })?
                        .with_timezone(&chrono::Utc);
                    Ok(Some(Checkpoint {
                        id,
                        plan_id: cplan_id,
                        step_index,
                        status,
                        output,
                        created_at,
                    }))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(AgentError::Database(e)),
            }
        })
        .await
    }

    /// Reset any plans that were left in Running status (crash recovery).
    pub async fn reset_running_plans_to_pending(&self) -> AgentResult<Vec<String>> {
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, steps_json FROM plans WHERE status = 'Running'",
            )?;

            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();

            let mut plan_ids = Vec::new();
            for (plan_id, steps_json) in &rows {
                let mut steps: Vec<Step> =
                    serde_json::from_str(steps_json).unwrap_or_default();
                for step in &mut steps {
                    if *step.status() == StepStatus::Running {
                        step.set_status(StepStatus::Pending);
                    }
                }
                let new_json = serde_json::to_string(&steps).unwrap_or_default();
                let now = chrono::Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE plans SET steps_json = ?1, status = 'Pending', updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![new_json, now, plan_id],
                )?;
                plan_ids.push(plan_id.clone());
            }

            Ok(plan_ids)
        })
        .await
    }

    /// Save a checkpoint (standalone, no plan-update coupling).
    pub async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> AgentResult<()> {
        let id = checkpoint.id.clone();
        let plan_id = checkpoint.plan_id.clone();
        let step_index = checkpoint.step_index;
        let status = checkpoint_status_to_str(&checkpoint.status);
        let output = checkpoint.output.clone();
        let created_at = checkpoint.created_at.to_rfc3339();

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO checkpoints (id, plan_id, step_index, status, output, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![id, plan_id, step_index, status, output, created_at],
            )?;
            Ok(())
        })
        .await
    }

    // ---------------------------------------------------------------------------
    // Plan listing, counting, deletion, pruning
    // ---------------------------------------------------------------------------

    /// List plans ordered by updated_at descending.
    pub async fn list_plans(&self, limit: usize, offset: usize) -> AgentResult<Vec<super::PlanSummary>> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, steps_json, current_step_index, status, created_at, updated_at
                 FROM plans
                 ORDER BY updated_at DESC
                 LIMIT ?1 OFFSET ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let steps_json: String = row.get(2)?;
                let current_step_index: usize = row.get(3)?;
                let status: String = row.get(4)?;
                let created_at: String = row.get(5)?;
                let updated_at: String = row.get(6)?;

                let total_steps = serde_json::from_str::<Vec<Step>>(&steps_json)
                    .map(|s| s.len())
                    .unwrap_or(0);

                Ok(super::PlanSummary {
                    id,
                    name,
                    current_step_index,
                    total_steps,
                    status,
                    created_at,
                    updated_at,
                })
            })?;

            let mut results = Vec::new();
            for row in rows.flatten() {
                results.push(row);
            }
            Ok(results)
        })
        .await
    }

    /// Count plans grouped by status.
    pub async fn count_plans_by_status(&self) -> AgentResult<Vec<(String, i64)>> {
        self.with_read_conn(move |conn| {
            let mut stmt =
                conn.prepare("SELECT status, COUNT(*) as cnt FROM plans GROUP BY status")?;

            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;

            let mut results = Vec::new();
            for row in rows.flatten() {
                results.push(row);
            }
            Ok(results)
        })
        .await
    }

    /// Delete a plan and its associated checkpoints.
    pub async fn delete_plan(&self, plan_id: &str) -> AgentResult<()> {
        let pid = plan_id.to_string();
        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM checkpoints WHERE plan_id = ?1",
                rusqlite::params![pid],
            )?;
            tx.execute(
                "DELETE FROM plans WHERE id = ?1",
                rusqlite::params![pid],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Delete completed/failed plans older than `before` (RFC 3339 timestamp).
    /// Returns number of deleted plans.
    pub async fn prune_plans(&self, before: &str) -> AgentResult<usize> {
        let before = before.to_string();
        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;

            // Delete associated checkpoints for matching plans
            tx.execute(
                "DELETE FROM checkpoints WHERE plan_id IN (
                     SELECT id FROM plans WHERE (status = 'Completed' OR status = 'Failed') AND updated_at < ?1
                 )",
                rusqlite::params![before],
            )?;

            let deleted = tx.execute(
                "DELETE FROM plans WHERE (status = 'Completed' OR status = 'Failed') AND updated_at < ?1",
                rusqlite::params![before],
            )?;

            tx.commit()?;
            Ok(deleted)
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::db::tests::repo;
    use crate::db::PlanSummary;
    use crate::task::{finish_step, think_step, Plan, PlanStatus, StepStatus};

    #[tokio::test]
    async fn test_save_and_load_plan() {
        let repo = repo();
        let steps = vec![think_step("analyze"), finish_step("done")];
        let plan = Plan::new("test", steps);
        let id = plan.id.clone();

        repo.save_plan(&plan).await.unwrap();
        let loaded = repo.load_plan(&id).await.unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.steps.len(), 2);
    }

    #[tokio::test]
    async fn test_record_step_completion_updates_checkpoint() {
        let repo = repo();
        let steps = vec![think_step("step1"), think_step("step2"), finish_step("done")];
        let plan = Plan::new("checkpoint-test", steps);
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        repo.record_step_completion(&id, 0, StepStatus::Completed, None)
            .await
            .unwrap();

        let ckpt = repo.get_last_checkpoint(&id).await.unwrap().unwrap();
        assert_eq!(ckpt.step_index, 0);
    }

    #[tokio::test]
    async fn test_reset_running_plans() {
        let repo = repo();
        let steps = vec![think_step("work"), finish_step("done")];
        let mut plan = Plan::new("crash-recovery", steps);
        plan.status = PlanStatus::Running;
        let id = plan.id.clone();
        repo.save_plan(&plan).await.unwrap();

        let ids = repo.reset_running_plans_to_pending().await.unwrap();
        assert!(ids.contains(&id));

        let reloaded = repo.load_plan(&id).await.unwrap();
        assert_eq!(reloaded.status, PlanStatus::Pending);
    }

    #[test]
    fn test_plan_summary_serde() {
        let summary = PlanSummary {
            id: "test-123".into(),
            name: "Test Plan".into(),
            current_step_index: 2,
            total_steps: 5,
            status: "Running".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T01:00:00Z".into(),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("test-123"));
        assert!(json.contains("Running"));
        let back: PlanSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_steps, 5);
    }
}
