use anyhow::Result;
use console::style;
use rupoo::cron::{self, CronJob};
use rupoo::db::TaskRepo;
use std::sync::Arc;

pub async fn run(db_path: &str, action: Option<crate::main_cli::CronAction>) -> Result<()> {
    let out = match action {
        Some(a) => handle_action(db_path, a).await?,
        None => cmd_list_string(db_path).await?,
    };
    print!("{out}");
    Ok(())
}

async fn handle_action(db_path: &str, action: crate::main_cli::CronAction) -> Result<String> {
    match action {
        crate::main_cli::CronAction::Add {
            name,
            schedule,
            task,
        } => cmd_add_string(db_path, &name, &schedule, &task).await,
        crate::main_cli::CronAction::List => cmd_list_string(db_path).await,
        crate::main_cli::CronAction::Remove { id } => cmd_remove_string(db_path, &id).await,
        crate::main_cli::CronAction::Pause { id } => cmd_toggle_string(db_path, &id, false).await,
        crate::main_cli::CronAction::Resume { id } => cmd_toggle_string(db_path, &id, true).await,
    }
}

async fn repo(db_path: &str) -> Result<Arc<TaskRepo>> {
    Ok(Arc::new(TaskRepo::new(db_path)?))
}

async fn cmd_add_string(db_path: &str, name: &str, schedule: &str, task: &str) -> Result<String> {
    let repo = repo(db_path).await?;

    // Validate cron expression before creating the job
    let next = cron::calculate_next_run(schedule)
        .map_err(|e| anyhow::anyhow!("Invalid cron schedule: {e}"))?;

    let job = CronJob::new(name, schedule, task)?;
    cron::save_cron_job(&repo, &job).await?;

    let desc = match next {
        Some(ts) => {
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "unknown".to_string());
            format!("下次执行: {dt}")
        }
        None => "无下次执行时间".to_string(),
    };

    Ok(format!(
        "{} Cron job '{}' added ({desc})",
        style("✓").green(),
        name,
    ))
}

async fn cmd_list_string(db_path: &str) -> Result<String> {
    let repo = repo(db_path).await?;
    let jobs = cron::list_cron_jobs(&repo, 100, 0).await?;

    if jobs.is_empty() {
        return Ok(format!(
            "{} 没有定时任务。使用 `rupoo cron add` 添加一个。\n",
            style("ℹ").cyan(),
        ));
    }

    use std::fmt::Write;
    let mut out = String::new();

    writeln!(
        out,
        "{:<12} {:<24} {:<14} {:<10}  {}",
        style("Name").bold(),
        style("Schedule").bold(),
        style("Next Run").bold(),
        style("Status").bold(),
        style("Task").bold(),
    )?;
    writeln!(out, "{}", style("─".repeat(78)).dim())?;

    let now = chrono::Utc::now().timestamp();
    for job in &jobs {
        let status = if job.enabled {
            match job.next_run_at {
                Some(ts) if ts <= now => style("● due").yellow().to_string(),
                Some(_) => style("● active").green().to_string(),
                None => style("○ none").dim().to_string(),
            }
        } else {
            style("○ paused").dim().to_string()
        };

        let next_str = match job.next_run_at {
            Some(ts) => chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "unknown".to_string()),
            None => "-".to_string(),
        };

        let _id_short = &job.id[..8.min(job.id.len())];
        writeln!(
            out,
            "{:<12} {:<24} {:<14} {:<10}  {}",
            job.name,
            job.schedule,
            next_str,
            status,
            &job.task_message[..40.min(job.task_message.len())],
        )?;
    }
    writeln!(out, "{}", style("─".repeat(78)).dim())?;

    Ok(out)
}

async fn cmd_remove_string(db_path: &str, job_id: &str) -> Result<String> {
    let repo = repo(db_path).await?;
    cron::delete_cron_job(&repo, job_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to remove cron job: {e}"))?;
    Ok(format!("{} Cron job removed", style("✓").green()))
}

async fn cmd_toggle_string(db_path: &str, job_id: &str, enabled: bool) -> Result<String> {
    let repo = repo(db_path).await?;
    cron::toggle_cron_job(&repo, job_id, enabled)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update cron job: {e}"))?;
    let action = if enabled { "resumed" } else { "paused" };
    Ok(format!("{} Cron job {action}", style("✓").green()))
}
