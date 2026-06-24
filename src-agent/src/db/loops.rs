//! Loop and LoopRun CRUD operations
//!
//! Part of Loop Engineering (Phase A)
//! Follows the same patterns as plans.rs

use tracing::info;

use crate::error::{AgentError, AgentResult};

use super::TaskRepo;

// ---------------------------------------------------------------------------
// Status string helpers — store as plain text, matching plans.rs conventions
// ---------------------------------------------------------------------------

fn loop_status_to_str(status: &crate::loop_engine::LoopStatus) -> &'static str {
    match status {
        crate::loop_engine::LoopStatus::Pending => "Pending",
        crate::loop_engine::LoopStatus::Running => "Running",
        crate::loop_engine::LoopStatus::StepComplete => "StepComplete",
        crate::loop_engine::LoopStatus::Evaluating => "Evaluating",
        crate::loop_engine::LoopStatus::WaitingForApproval => "WaitingForApproval",
        crate::loop_engine::LoopStatus::WaitingForInput => "WaitingForInput",
        crate::loop_engine::LoopStatus::Decomposing => "Decomposing",
        crate::loop_engine::LoopStatus::Paused => "Paused",
        crate::loop_engine::LoopStatus::Completed => "Completed",
        crate::loop_engine::LoopStatus::Failed => "Failed",
        crate::loop_engine::LoopStatus::BudgetExceeded => "BudgetExceeded",
        crate::loop_engine::LoopStatus::TimedOut => "TimedOut",
        crate::loop_engine::LoopStatus::Cancelled => "Cancelled",
    }
}

fn str_to_loop_status(s: &str) -> AgentResult<crate::loop_engine::LoopStatus> {
    match s {
        "Pending" => Ok(crate::loop_engine::LoopStatus::Pending),
        "Running" => Ok(crate::loop_engine::LoopStatus::Running),
        "StepComplete" => Ok(crate::loop_engine::LoopStatus::StepComplete),
        "Evaluating" => Ok(crate::loop_engine::LoopStatus::Evaluating),
        "WaitingForApproval" => Ok(crate::loop_engine::LoopStatus::WaitingForApproval),
        "WaitingForInput" => Ok(crate::loop_engine::LoopStatus::WaitingForInput),
        "Decomposing" => Ok(crate::loop_engine::LoopStatus::Decomposing),
        "Paused" => Ok(crate::loop_engine::LoopStatus::Paused),
        "Completed" => Ok(crate::loop_engine::LoopStatus::Completed),
        "Failed" => Ok(crate::loop_engine::LoopStatus::Failed),
        "BudgetExceeded" => Ok(crate::loop_engine::LoopStatus::BudgetExceeded),
        "TimedOut" => Ok(crate::loop_engine::LoopStatus::TimedOut),
        "Cancelled" => Ok(crate::loop_engine::LoopStatus::Cancelled),
        other => Err(AgentError::Other(format!("unknown loop status: {other}"))),
    }
}

fn loop_run_status_to_str(status: &crate::loop_engine::LoopRunStatus) -> &'static str {
    match status {
        crate::loop_engine::LoopRunStatus::Running => "Running",
        crate::loop_engine::LoopRunStatus::Completed => "Completed",
        crate::loop_engine::LoopRunStatus::Failed => "Failed",
    }
}

fn str_to_loop_run_status(s: &str) -> AgentResult<crate::loop_engine::LoopRunStatus> {
    match s {
        "Running" => Ok(crate::loop_engine::LoopRunStatus::Running),
        "Completed" => Ok(crate::loop_engine::LoopRunStatus::Completed),
        "Failed" => Ok(crate::loop_engine::LoopRunStatus::Failed),
        other => Err(AgentError::Other(format!(
            "unknown loop run status: {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// LoopRepo — Loop and LoopRun persistence
// ---------------------------------------------------------------------------

impl TaskRepo {
    // ---------------------------------------------------------------------------
    // Loop CRUD
    // ---------------------------------------------------------------------------

    /// Save (insert) a new Loop.
    pub async fn save_loop(&self, l: &crate::loop_engine::Loop) -> AgentResult<()> {
        let id = l.id.clone();
        let goal = l.goal.clone();
        let status = loop_status_to_str(&l.status);
        let config_json = serde_json::to_string(&l.config)?;
        let current_run_id = l.current_run_id.clone();
        let created_at = l.created_at;
        let updated_at = l.updated_at;

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO loops (id, goal, status, config_json, current_run_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, goal, status, config_json, current_run_id, created_at, updated_at],
            )?;
            Ok(())
        })
        .await
    }

    /// Load a Loop by ID.
    pub async fn load_loop(&self, loop_id: &str) -> AgentResult<crate::loop_engine::Loop> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, goal, status, config_json, current_run_id, created_at, updated_at
                 FROM loops WHERE id = ?1",
            )?;

            let row = stmt
                .query_row(rusqlite::params![lid], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                })
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AgentError::Other(format!("loop not found: {lid}"))
                    }
                    other => AgentError::Database(other),
                })?;

            let (id, goal, status_str, config_json, current_run_id, created_at, updated_at) = row;

            let status = str_to_loop_status(&status_str)?;
            let config: crate::loop_engine::LoopConfig = serde_json::from_str(&config_json)?;

            Ok(crate::loop_engine::Loop {
                id,
                goal,
                status,
                config,
                current_run_id,
                created_at,
                updated_at,
            })
        })
        .await
    }

    /// Update a Loop's status and current_run_id atomically.
    pub async fn update_loop_status(
        &self,
        loop_id: &str,
        status: &crate::loop_engine::LoopStatus,
        current_run_id: Option<&str>,
    ) -> AgentResult<()> {
        let lid = loop_id.to_string();
        let status_str = loop_status_to_str(status);
        let run_id = current_run_id.map(|s| s.to_string());
        let now = chrono::Utc::now().timestamp();

        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE loops SET status = ?1, current_run_id = ?2, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![status_str, run_id, now, lid],
            )?;
            Ok(())
        })
        .await
    }

    /// List loops, ordered by updated_at descending.
    pub async fn list_loops(
        &self,
        limit: usize,
        offset: usize,
    ) -> AgentResult<Vec<crate::loop_engine::Loop>> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, goal, status, config_json, current_run_id, created_at, updated_at
                 FROM loops
                 ORDER BY updated_at DESC
                 LIMIT ?1 OFFSET ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;

            let mut results = Vec::new();
            for row in rows {
                if let Ok((id, goal, status_str, config_json, current_run_id, created_at, updated_at)) = row {
                    let status = str_to_loop_status(&status_str).unwrap_or(crate::loop_engine::LoopStatus::Failed);
                    let config: crate::loop_engine::LoopConfig =
                        serde_json::from_str(&config_json).unwrap_or_default();
                    results.push(crate::loop_engine::Loop {
                        id,
                        goal,
                        status,
                        config,
                        current_run_id,
                        created_at,
                        updated_at,
                    });
                }
            }
            Ok(results)
        })
        .await
    }

    /// List loops filtered by status.
    pub async fn list_loops_by_status(
        &self,
        status: &crate::loop_engine::LoopStatus,
    ) -> AgentResult<Vec<crate::loop_engine::Loop>> {
        let status_str = loop_status_to_str(status).to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, goal, status, config_json, current_run_id, created_at, updated_at
                 FROM loops WHERE status = ?1
                 ORDER BY updated_at DESC",
            )?;

            let rows = stmt.query_map(rusqlite::params![status_str], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })?;

            let mut results = Vec::new();
            for row in rows {
                if let Ok((id, goal, s, config_json, current_run_id, created_at, updated_at)) = row {
                    let status = str_to_loop_status(&s).unwrap_or(crate::loop_engine::LoopStatus::Failed);
                    let config: crate::loop_engine::LoopConfig =
                        serde_json::from_str(&config_json).unwrap_or_default();
                    results.push(crate::loop_engine::Loop {
                        id,
                        goal,
                        status,
                        config,
                        current_run_id,
                        created_at,
                        updated_at,
                    });
                }
            }
            Ok(results)
        })
        .await
    }

    /// Delete a Loop and its associated LoopRuns.
    pub async fn delete_loop(&self, loop_id: &str) -> AgentResult<()> {
        let lid = loop_id.to_string();
        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM loop_runs WHERE loop_id = ?1",
                rusqlite::params![lid],
            )?;
            tx.execute("DELETE FROM loops WHERE id = ?1", rusqlite::params![lid])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    // ---------------------------------------------------------------------------
    // LoopRun CRUD
    // ---------------------------------------------------------------------------

    /// Save (insert) a new LoopRun. Uses INSERT OR REPLACE for upsert semantics
    /// on the UNIQUE(loop_id, iteration) constraint.
    pub async fn save_loop_run(&self, run: &crate::loop_engine::LoopRun) -> AgentResult<()> {
        let id = run.id.clone();
        let loop_id = run.loop_id.clone();
        let iteration = run.iteration;
        let plan_id = run.plan_id.clone();
        let status = loop_run_status_to_str(&run.status);
        let evaluation_json = run
            .evaluation
            .as_ref()
            .map(|e| serde_json::to_string(e))
            .transpose()?;
        let decision = run.decision.as_ref().map(|d| {
            match d {
                crate::loop_engine::LoopDecision::Done => "Done",
                crate::loop_engine::LoopDecision::Continue => "Continue",
                crate::loop_engine::LoopDecision::Decompose => "Decompose",
                crate::loop_engine::LoopDecision::Impossible => "Impossible",
            }
            .to_string()
        });
        let token_usage_json = run
            .token_usage
            .as_ref()
            .map(|t| serde_json::to_string(t))
            .transpose()?;
        let started_at = run.started_at;
        let finished_at = run.finished_at;

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO loop_runs (id, loop_id, iteration, plan_id, status, evaluation_json, decision, token_usage_json, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![id, loop_id, iteration, plan_id, status, evaluation_json, decision, token_usage_json, started_at, finished_at],
            )?;
            Ok(())
        })
        .await
    }

    /// Load a LoopRun by ID.
    pub async fn load_loop_run(
        &self,
        run_id: &str,
    ) -> AgentResult<crate::loop_engine::LoopRun> {
        let rid = run_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, loop_id, iteration, plan_id, status, evaluation_json, decision, token_usage_json, started_at, finished_at
                 FROM loop_runs WHERE id = ?1",
            )?;

            let row = stmt
                .query_row(rusqlite::params![rid], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                    ))
                })
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        AgentError::Other(format!("loop run not found: {rid}"))
                    }
                    other => AgentError::Database(other),
                })?;

            let (
                id,
                loop_id,
                iteration,
                plan_id,
                status_str,
                evaluation_json,
                decision_str,
                token_usage_json,
                started_at,
                finished_at,
            ) = row;

            let status = str_to_loop_run_status(&status_str)?;
            let evaluation: Option<crate::loop_engine::EvaluationResult> = evaluation_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?;
            let decision = decision_str.as_deref().map(|d| match d {
                "Done" => crate::loop_engine::LoopDecision::Done,
                "Continue" => crate::loop_engine::LoopDecision::Continue,
                "Decompose" => crate::loop_engine::LoopDecision::Decompose,
                "Impossible" => crate::loop_engine::LoopDecision::Impossible,
                _ => crate::loop_engine::LoopDecision::Continue,
            });
            let token_usage: Option<crate::llm::TokenUsage> = token_usage_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?;

            Ok(crate::loop_engine::LoopRun {
                id,
                loop_id,
                iteration,
                plan_id,
                status,
                evaluation,
                decision,
                token_usage,
                started_at,
                finished_at,
            })
        })
        .await
    }

    /// Get the latest LoopRun for a Loop (highest iteration number).
    pub async fn get_latest_loop_run(
        &self,
        loop_id: &str,
    ) -> AgentResult<Option<crate::loop_engine::LoopRun>> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, loop_id, iteration, plan_id, status, evaluation_json, decision, token_usage_json, started_at, finished_at
                 FROM loop_runs
                 WHERE loop_id = ?1
                 ORDER BY iteration DESC
                 LIMIT 1",
            )?;

            let result = stmt.query_row(rusqlite::params![lid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                ))
            });

            match result {
                Ok((id, loop_id, iteration, plan_id, status_str, evaluation_json, decision_str, token_usage_json, started_at, finished_at)) => {
                    let status = str_to_loop_run_status(&status_str)?;
                    let evaluation: Option<crate::loop_engine::EvaluationResult> = evaluation_json
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()?;
                    let decision = decision_str.as_deref().map(|d| match d {
                        "Done" => crate::loop_engine::LoopDecision::Done,
                        "Continue" => crate::loop_engine::LoopDecision::Continue,
                        "Decompose" => crate::loop_engine::LoopDecision::Decompose,
                        "Impossible" => crate::loop_engine::LoopDecision::Impossible,
                        _ => crate::loop_engine::LoopDecision::Continue,
                    });
                    let token_usage: Option<crate::llm::TokenUsage> = token_usage_json
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()?;

                    Ok(Some(crate::loop_engine::LoopRun {
                        id,
                        loop_id,
                        iteration,
                        plan_id,
                        status,
                        evaluation,
                        decision,
                        token_usage,
                        started_at,
                        finished_at,
                    }))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(AgentError::Database(e)),
            }
        })
        .await
    }

    /// Update a LoopRun's status, evaluation, decision, and finished_at.
    pub async fn update_loop_run_result(
        &self,
        run_id: &str,
        status: &crate::loop_engine::LoopRunStatus,
        evaluation: Option<&crate::loop_engine::EvaluationResult>,
        decision: Option<&crate::loop_engine::LoopDecision>,
        token_usage: Option<&crate::llm::TokenUsage>,
    ) -> AgentResult<()> {
        let rid = run_id.to_string();
        let status_str = loop_run_status_to_str(status);
        let evaluation_json = evaluation.map(|e| serde_json::to_string(e)).transpose()?;
        let decision_str = decision.map(|d| {
            match d {
                crate::loop_engine::LoopDecision::Done => "Done",
                crate::loop_engine::LoopDecision::Continue => "Continue",
                crate::loop_engine::LoopDecision::Decompose => "Decompose",
                crate::loop_engine::LoopDecision::Impossible => "Impossible",
            }
            .to_string()
        });
        let token_json = token_usage.map(|t| serde_json::to_string(t)).transpose()?;
        let now = chrono::Utc::now().timestamp();

        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE loop_runs SET status = ?1, evaluation_json = ?2, decision = ?3, token_usage_json = ?4, finished_at = ?5
                 WHERE id = ?6",
                rusqlite::params![status_str, evaluation_json, decision_str, token_json, now, rid],
            )?;
            Ok(())
        })
        .await
    }

    /// Count total loop_runs for a Loop.
    pub async fn count_loop_runs(&self, loop_id: &str) -> AgentResult<u32> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM loop_runs WHERE loop_id = ?1",
                rusqlite::params![lid],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        })
        .await
    }

    /// Get recent evaluation decisions for a Loop (for oscillation detection).
    /// Returns the last `n` decisions ordered by iteration DESC.
    pub async fn recent_loop_decisions(
        &self,
        loop_id: &str,
        n: usize,
    ) -> AgentResult<Vec<crate::loop_engine::LoopDecision>> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT decision FROM loop_runs
                 WHERE loop_id = ?1 AND decision IS NOT NULL
                 ORDER BY iteration DESC
                 LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![lid, n as i64], |row| {
                row.get::<_, Option<String>>(0)
            })?;

            let mut results = Vec::new();
            for row in rows.flatten() {
                if let Some(d) = row {
                    let decision = match d.as_str() {
                        "Done" => crate::loop_engine::LoopDecision::Done,
                        "Continue" => crate::loop_engine::LoopDecision::Continue,
                        "Decompose" => crate::loop_engine::LoopDecision::Decompose,
                        "Impossible" => crate::loop_engine::LoopDecision::Impossible,
                        _ => continue,
                    };
                    results.push(decision);
                }
            }
            // Reverse to get chronological order (oldest first)
            results.reverse();
            Ok(results)
        })
        .await
    }

    /// Get recent unmet counts for a Loop (for stall detection).
    /// Returns the unmet count for the last `n` runs that have evaluations.
    pub async fn recent_unmet_counts(
        &self,
        loop_id: &str,
        n: usize,
    ) -> AgentResult<Vec<usize>> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT evaluation_json FROM loop_runs
                 WHERE loop_id = ?1 AND evaluation_json IS NOT NULL
                 ORDER BY iteration DESC
                 LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![lid, n as i64], |row| {
                row.get::<_, Option<String>>(0)
            })?;

            let mut counts = Vec::new();
            for row in rows.flatten() {
                if let Some(json_str) = row {
                    if let Ok(eval) =
                        serde_json::from_str::<crate::loop_engine::EvaluationResult>(&json_str)
                    {
                        counts.push(eval.unmet.len());
                    }
                }
            }
            counts.reverse(); // chronological order
            Ok(counts)
        })
        .await
    }

    /// Reset any loops that were left in "Running" or "Evaluating" status (crash recovery).
    pub async fn reset_running_loops_to_paused(&self) -> AgentResult<Vec<String>> {
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM loops WHERE status IN ('Running', 'Evaluating', 'Decomposing', 'StepComplete')",
            )?;

            let ids: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();

            let now = chrono::Utc::now().timestamp();
            for lid in &ids {
                conn.execute(
                    "UPDATE loops SET status = 'Paused', updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![now, lid],
                )?;
            }

            info!(count = ids.len(), "reset running loops to paused");
            Ok(ids)
        })
        .await
    }

    /// Load a LoopRun by (loop_id, iteration) composite key.
    pub async fn load_loop_run_by_iteration(
        &self,
        loop_id: &str,
        iteration: u32,
    ) -> AgentResult<Option<crate::loop_engine::EvaluationResult>> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT evaluation_json FROM loop_runs WHERE loop_id = ?1 AND iteration = ?2",
            )?;

            let result = stmt.query_row(rusqlite::params![lid, iteration], |row| {
                row.get::<_, Option<String>>(0)
            });

            match result {
                Ok(Some(json_str)) => {
                    let eval: crate::loop_engine::EvaluationResult =
                        serde_json::from_str(&json_str).map_err(|e| {
                            AgentError::Other(format!("parse evaluation: {e}"))
                        })?;
                    Ok(Some(eval))
                }
                Ok(None) => Ok(None),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(AgentError::Database(e)),
            }
        })
        .await
    }

    /// Sum token usage across all LoopRuns for a given loop.
    pub async fn sum_loop_run_tokens(&self, loop_id: &str) -> AgentResult<u64> {
        let lid = loop_id.to_string();
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT token_usage_json FROM loop_runs WHERE loop_id = ?1 AND token_usage_json IS NOT NULL",
            )?;

            let rows = stmt.query_map(rusqlite::params![lid], |row| {
                row.get::<_, String>(0)
            })?;

            let mut total: u64 = 0;
            for row in rows.flatten() {
                if let Ok(usage) = serde_json::from_str::<crate::llm::TokenUsage>(&row) {
                    total += usage.total() as u64;
                }
            }
            Ok(total)
        })
        .await
    }

    /// Delete loop_runs older than `before` for completed/failed/cancelled loops.
    pub async fn prune_loops(&self, before_timestamp: i64) -> AgentResult<usize> {
        self.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;

            // Delete loop_runs for old, terminal loops
            tx.execute(
                "DELETE FROM loop_runs WHERE loop_id IN (
                     SELECT id FROM loops
                     WHERE status IN ('Completed', 'Failed', 'Cancelled')
                     AND updated_at < ?1
                 )",
                rusqlite::params![before_timestamp],
            )?;

            let deleted = tx.execute(
                "DELETE FROM loops WHERE status IN ('Completed', 'Failed', 'Cancelled') AND updated_at < ?1",
                rusqlite::params![before_timestamp],
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
    use super::super::tests::repo;

    // Helper: create a minimal LoopConfig for testing
    fn test_loop_config() -> crate::loop_engine::LoopConfig {
        crate::loop_engine::LoopConfig::default()
    }

    // Helper: create a test Loop
    fn test_loop() -> crate::loop_engine::Loop {
        crate::loop_engine::Loop::new("test goal", test_loop_config())
    }

    #[tokio::test]
    async fn test_save_and_load_loop() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();
        let loaded = repo.load_loop(&lid).await.unwrap();

        assert_eq!(loaded.goal, "test goal");
        assert_eq!(loaded.status, crate::loop_engine::LoopStatus::Pending);
        assert_eq!(loaded.config.max_iterations, 10);
    }

    #[tokio::test]
    async fn test_update_loop_status() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();
        repo.update_loop_status(&lid, &crate::loop_engine::LoopStatus::Running, None)
            .await
            .unwrap();

        let updated = repo.load_loop(&lid).await.unwrap();
        assert_eq!(updated.status, crate::loop_engine::LoopStatus::Running);
    }

    #[tokio::test]
    async fn test_save_and_load_loop_run() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();

        let run = crate::loop_engine::LoopRun::new(&lid, 0, "plan-123");
        let rid = run.id.clone();

        repo.save_loop_run(&run).await.unwrap();
        let loaded = repo.load_loop_run(&rid).await.unwrap();

        assert_eq!(loaded.loop_id, lid);
        assert_eq!(loaded.iteration, 0);
        assert_eq!(loaded.plan_id, "plan-123");
    }

    #[tokio::test]
    async fn test_loop_run_upsert() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();

        // First insert
        let run = crate::loop_engine::LoopRun::new(&lid, 1, "plan-a");
        repo.save_loop_run(&run).await.unwrap();

        // Should upsert (same loop_id + iteration, INSERT OR REPLACE)
        let run2 = crate::loop_engine::LoopRun {
            id: uuid::Uuid::new_v4().to_string(), // different ID
            ..run.clone()
        };
        repo.save_loop_run(&run2).await.unwrap();

        // Should still be exactly 1 run for this loop
        let count = repo.count_loop_runs(&lid).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_latest_loop_run() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();

        let run0 = crate::loop_engine::LoopRun::new(&lid, 0, "plan-0");
        let run1 = crate::loop_engine::LoopRun::new(&lid, 1, "plan-1");

        repo.save_loop_run(&run0).await.unwrap();
        repo.save_loop_run(&run1).await.unwrap();

        let latest = repo.get_latest_loop_run(&lid).await.unwrap().unwrap();
        assert_eq!(latest.iteration, 1);
    }

    #[tokio::test]
    async fn test_update_loop_run_result() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();

        let run = crate::loop_engine::LoopRun::new(&lid, 0, "plan-x");
        let rid = run.id.clone();
        repo.save_loop_run(&run).await.unwrap();

        let eval = crate::loop_engine::EvaluationResult {
            verdict: crate::loop_engine::LoopDecision::Done,
            confidence: 0.95,
            met: vec!["done".into()],
            unmet: vec![],
            new_issues: vec![],
            next_action: String::new(),
        };

        repo.update_loop_run_result(
            &rid,
            &crate::loop_engine::LoopRunStatus::Completed,
            Some(&eval),
            Some(&crate::loop_engine::LoopDecision::Done),
            None,
        )
        .await
        .unwrap();

        let loaded = repo.load_loop_run(&rid).await.unwrap();
        assert!(loaded.evaluation.is_some());
        assert_eq!(loaded.decision, Some(crate::loop_engine::LoopDecision::Done));
    }

    #[tokio::test]
    async fn test_reset_running_loops() {
        let repo = repo();
        let mut l = test_loop();
        l.status = crate::loop_engine::LoopStatus::Running;
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();

        let ids = repo.reset_running_loops_to_paused().await.unwrap();
        assert!(ids.contains(&lid));

        let reloaded = repo.load_loop(&lid).await.unwrap();
        assert_eq!(reloaded.status, crate::loop_engine::LoopStatus::Paused);
    }

    #[tokio::test]
    async fn test_delete_loop_cascades() {
        let repo = repo();
        let l = test_loop();
        let lid = l.id.clone();

        repo.save_loop(&l).await.unwrap();
        let run = crate::loop_engine::LoopRun::new(&lid, 0, "plan-y");
        repo.save_loop_run(&run).await.unwrap();

        repo.delete_loop(&lid).await.unwrap();

        // Both should be gone
        assert!(repo.load_loop(&lid).await.is_err());
        assert!(repo.load_loop_run(&run.id).await.is_err());
    }
}
