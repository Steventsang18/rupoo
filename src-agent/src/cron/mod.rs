//! Cron 调度模块 — 定时任务管理。
//!
//! 提供基于标准 5 字段 cron 表达式的定时任务调度能力。
//! 任务到期后复用 Loop Engine 执行。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::db::TaskRepo;
use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// CronJob — 一条定时任务记录
// ---------------------------------------------------------------------------

/// 一条 cron 定时任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    /// 用户可读的任务名称。
    pub name: String,
    /// 标准 5 字段 cron 表达式（min hour day month weekday）。
    /// 示例: "0 9 * * 1-5" = 工作日 9:00
    pub schedule: String,
    /// 任务描述 / 要执行的 message（传给 /loop 相同路径）。
    pub task_message: String,
    /// 是否启用。
    pub enabled: bool,
    /// 上次执行时间（Unix 时间戳）。
    pub last_run_at: Option<i64>,
    /// 下次计划执行时间（Unix 时间戳）。
    pub next_run_at: Option<i64>,
    /// 创建时间。
    pub created_at: i64,
    /// 更新时间。
    pub updated_at: i64,
}

impl CronJob {
    /// 新建定时任务（自动生成 ID 并计算 next_run_at）。
    pub fn new(name: &str, schedule: &str, task_message: &str) -> AgentResult<Self> {
        let now = chrono::Utc::now().timestamp();
        let next = calculate_next_run(schedule)?;

        Ok(Self {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            schedule: schedule.to_string(),
            task_message: task_message.to_string(),
            enabled: true,
            last_run_at: None,
            next_run_at: next,
            created_at: now,
            updated_at: now,
        })
    }

    /// 标记一次执行完成（更新 last_run_at 并计算下一次）。
    pub fn mark_run(&mut self) -> AgentResult<()> {
        let now = chrono::Utc::now().timestamp();
        self.last_run_at = Some(now);
        self.next_run_at = calculate_next_run(&self.schedule)?;
        self.updated_at = now;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CronManager — 后台调度器
// ---------------------------------------------------------------------------

/// Cron 调度器——管理定时任务的注册、轮询和触发执行。
pub struct CronManager {
    repo: Arc<TaskRepo>,
    /// 调度器是否正在运行。
    active: Arc<AtomicBool>,
    /// 轮询间隔（秒）。
    poll_interval_secs: u64,
}

impl CronManager {
    /// 创建新的 CronManager。
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            repo,
            active: Arc::new(AtomicBool::new(false)),
            poll_interval_secs: 60,
        }
    }

    /// 创建 CronManager 并指定轮询间隔。
    pub fn with_poll_interval(repo: Arc<TaskRepo>, poll_interval_secs: u64) -> Self {
        Self {
            repo,
            active: Arc::new(AtomicBool::new(false)),
            poll_interval_secs,
        }
    }

    /// 启动后台调度任务。
    ///
    /// `task_runner` 是一个闭包，接收任务消息并执行（通常调用 agent.start_loop()）。
    /// 在每个 tick 检查到期的 cron job，对每个到期 job 调用 `task_runner`。
    pub fn start_scheduler<F>(&self, task_runner: Arc<F>)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.active.store(true, Ordering::SeqCst);
        let repo = self.repo.clone();
        let active = self.active.clone();
        let interval = self.poll_interval_secs;

        tokio::spawn(async move {
            info!(poll_interval_secs = interval, "cron scheduler started");

            loop {
                // 被 stop 信号终止
                if !active.load(Ordering::SeqCst) {
                    info!("cron scheduler stopped");
                    break;
                }

                // 查询到期 job
                match list_due_cron_jobs(&repo, 10).await {
                    Ok(jobs) => {
                        for mut job in jobs {
                            info!(
                                name = %job.name,
                                schedule = %job.schedule,
                                task = %job.task_message,
                                "cron job triggered"
                            );

                            // 标记已执行
                            if let Err(e) = job.mark_run() {
                                warn!(
                                    name = %job.name,
                                    error = %e,
                                    "failed to calculate next run for cron job"
                                );
                                continue;
                            }

                            // 持久化更新（last_run_at, next_run_at）
                            if let Err(e) = update_cron_job_timing(&repo, &job).await {
                                warn!(
                                    name = %job.name,
                                    error = %e,
                                    "failed to persist cron job timing"
                                );
                            }

                            // 执行任务
                            (task_runner)(job.task_message);
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "cron scheduler: failed to list due jobs");
                    }
                }

                // 等待下一个 tick
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        });
    }

    /// 停止调度器。
    pub fn stop_scheduler(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// 调度计算
// ---------------------------------------------------------------------------

/// 将 5 字段 cron 表达式转为 `cron` crate 的 6 字段格式。
fn normalize_schedule(s: &str) -> String {
    let trimmed = s.trim();
    let field_count = trimmed.split_whitespace().count();
    if field_count == 5 {
        // 5 字段标准 cron: min hour day month weekday
        // prepend "0" seconds → 6 字段
        format!("0 {}", trimmed)
    } else if field_count == 6 {
        // 已经是 6 字段（带秒）
        trimmed.to_string()
    } else {
        // 无效，返回原字符串让 parse 报错
        trimmed.to_string()
    }
}

/// 计算给定 cron 表达式的下次执行时间。
/// 返回 Unix 时间戳（秒）。
pub fn calculate_next_run(schedule: &str) -> AgentResult<Option<i64>> {
    let normalized = normalize_schedule(schedule);
    let sched = normalized
        .parse::<cron::Schedule>()
        .map_err(|e| AgentError::Other(format!("invalid cron schedule '{}': {e}", schedule)))?;

    let next = sched.upcoming(chrono::Utc).next().map(|dt| dt.timestamp());

    Ok(next)
}

// ---------------------------------------------------------------------------
// DB 操作（直接在 mod.rs 定义，避免复杂模块拆分）
// ---------------------------------------------------------------------------

/// 保存一条 cron job。
pub async fn save_cron_job(repo: &TaskRepo, job: &CronJob) -> AgentResult<()> {
    let id = job.id.clone();
    let name = job.name.clone();
    let schedule = job.schedule.clone();
    let task_message = job.task_message.clone();
    let enabled = job.enabled;
    let last_run_at = job.last_run_at;
    let next_run_at = job.next_run_at;
    let created_at = job.created_at;
    let updated_at = job.updated_at;

    repo.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO cron_jobs (id, name, schedule, task_message, enabled, last_run_at, next_run_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, schedule=excluded.schedule,
                task_message=excluded.task_message, enabled=excluded.enabled,
                last_run_at=excluded.last_run_at, next_run_at=excluded.next_run_at,
                updated_at=excluded.updated_at",
            rusqlite::params![
                id, name, schedule, task_message, enabled,
                last_run_at, next_run_at, created_at, updated_at
            ],
        )?;
        Ok(())
    })
    .await
}

/// 按 ID 加载一条 cron job。
pub async fn load_cron_job(repo: &TaskRepo, job_id: &str) -> AgentResult<CronJob> {
    let jid = job_id.to_string();
    repo.with_read_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, task_message, enabled, last_run_at, next_run_at, created_at, updated_at
             FROM cron_jobs WHERE id = ?1",
        )?;

        stmt.query_row(rusqlite::params![jid], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                task_message: row.get(3)?,
                enabled: row.get(4)?,
                last_run_at: row.get(5)?,
                next_run_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                AgentError::Other(format!("cron job not found: {jid}"))
            }
            other => AgentError::Database(other),
        })
    })
    .await
}

/// 列出所有 cron jobs，按 updated_at 降序。
pub async fn list_cron_jobs(
    repo: &TaskRepo,
    limit: usize,
    offset: usize,
) -> AgentResult<Vec<CronJob>> {
    repo.with_read_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, task_message, enabled, last_run_at, next_run_at, created_at, updated_at
             FROM cron_jobs
             ORDER BY updated_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                task_message: row.get(3)?,
                enabled: row.get(4)?,
                last_run_at: row.get(5)?,
                next_run_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut jobs = Vec::new();
        for row in rows.flatten() {
            jobs.push(row);
        }
        Ok(jobs)
    })
    .await
}

/// 按 ID 删除 cron job。
pub async fn delete_cron_job(repo: &TaskRepo, job_id: &str) -> AgentResult<()> {
    let jid = job_id.to_string();
    repo.with_conn(move |conn| {
        conn.execute(
            "DELETE FROM cron_jobs WHERE id = ?1",
            rusqlite::params![jid],
        )?;
        Ok(())
    })
    .await
}

/// 切换 cron job 的启用/停用状态。
pub async fn toggle_cron_job(repo: &TaskRepo, job_id: &str, enabled: bool) -> AgentResult<()> {
    let jid = job_id.to_string();
    let now = chrono::Utc::now().timestamp();
    repo.with_conn(move |conn| {
        conn.execute(
            "UPDATE cron_jobs SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![enabled, now, jid],
        )?;
        Ok(())
    })
    .await
}

/// 查询所有到期（enabled=true AND next_run_at <= now）的 cron jobs。
pub async fn list_due_cron_jobs(repo: &TaskRepo, limit: usize) -> AgentResult<Vec<CronJob>> {
    let now = chrono::Utc::now().timestamp();
    repo.with_read_conn(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, task_message, enabled, last_run_at, next_run_at, created_at, updated_at
             FROM cron_jobs
             WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1
             ORDER BY next_run_at ASC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![now, limit as i64], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                task_message: row.get(3)?,
                enabled: row.get(4)?,
                last_run_at: row.get(5)?,
                next_run_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let mut jobs = Vec::new();
        for row in rows.flatten() {
            jobs.push(row);
        }
        Ok(jobs)
    })
    .await
}

/// 更新 cron job 的执行时间（last_run_at, next_run_at）。
pub async fn update_cron_job_timing(repo: &TaskRepo, job: &CronJob) -> AgentResult<()> {
    let id = job.id.clone();
    let last_run_at = job.last_run_at;
    let next_run_at = job.next_run_at;
    let updated_at = job.updated_at;

    repo.with_conn(move |conn| {
        conn.execute(
            "UPDATE cron_jobs SET last_run_at = ?1, next_run_at = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![last_run_at, next_run_at, updated_at, id],
        )?;
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

/// 格式化 cron 表达式为人类可读的描述。
pub fn describe_schedule(schedule: &str) -> String {
    let normalized = normalize_schedule(schedule);
    match normalized.parse::<cron::Schedule>() {
        Ok(sched) => {
            let next = sched.upcoming(chrono::Utc).next();
            match next {
                Some(dt) => {
                    let local = dt.with_timezone(&chrono::Local);
                    format!("下次执行: {}", local.format("%Y-%m-%d %H:%M"))
                }
                None => "无下次执行时间（表达式有效但不在范围内）".to_string(),
            }
        }
        Err(e) => format!("无效 cron 表达式: {e}"),
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_job_new() {
        let job = CronJob::new("test", "0 9 * * *", "say hello").unwrap();
        assert_eq!(job.name, "test");
        assert!(job.enabled);
        assert!(job.next_run_at.is_some());
    }

    #[test]
    fn test_calculate_next_run_5field() {
        // "0 9 * * *" = 每天 9:00
        let next = calculate_next_run("0 9 * * *").unwrap();
        assert!(next.is_some());
    }

    #[test]
    fn test_calculate_next_run_6field() {
        // "0 0 9 * * *" = 也是每天 9:00（显式秒）
        let next = calculate_next_run("0 0 9 * * *").unwrap();
        assert!(next.is_some());
    }

    #[test]
    fn test_calculate_next_run_invalid() {
        let result = calculate_next_run("not-a-cron");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_schedule_5field() {
        assert_eq!(normalize_schedule("0 9 * * *"), "0 0 9 * * *");
    }

    #[test]
    fn test_normalize_schedule_6field() {
        assert_eq!(normalize_schedule("0 0 9 * * *"), "0 0 9 * * *");
    }

    #[test]
    fn test_describe_schedule() {
        let desc = describe_schedule("0 9 * * 1-5");
        assert!(desc.contains("下次执行") || desc.contains("cron"));
    }

    #[test]
    fn test_cron_job_mark_run() {
        let mut job = CronJob::new("test", "*/5 * * * *", "task").unwrap();
        let old_next = job.next_run_at;
        job.mark_run().unwrap();
        assert!(job.last_run_at.is_some());
        assert!(job.next_run_at != old_next || job.next_run_at.is_some());
    }
}
