use anyhow::Result;
use chrono::Utc;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::task::{PlanStatus, Step, StepStatus};
use std::sync::Arc;

pub async fn run(db_path: &str, action: crate::main_cli::SessionAction) -> Result<()> {
    let out = output(db_path, action).await?;
    print!("{out}");
    Ok(())
}

pub async fn output(db_path: &str, action: crate::main_cli::SessionAction) -> Result<String> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    match action {
        crate::main_cli::SessionAction::List { limit } => cmd_list_string(&repo, limit).await,
        crate::main_cli::SessionAction::Show { id } => cmd_show_string(&repo, &id).await,
        crate::main_cli::SessionAction::Resume { id, .. } => cmd_resume_string(&id).await,
        crate::main_cli::SessionAction::Delete { id } => cmd_delete_string(&repo, &id).await,
        crate::main_cli::SessionAction::Prune { days } => cmd_prune_string(&repo, days).await,
    }
}

async fn cmd_list_string(repo: &TaskRepo, limit: usize) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    let plans = repo.list_plans(limit, 0).await?;
    if plans.is_empty() {
        writeln!(out, "{} No plans found.", style("ℹ").yellow())?;
        return Ok(out);
    }

    writeln!(
        out,
        "{:<10} {:<30} {:<8} {:<16} {:<22}",
        style("ID").bold(),
        style("Name").bold(),
        style("Steps").bold(),
        style("Status").bold(),
        style("Updated").bold(),
    )?;
    writeln!(out, "{}", style("─".repeat(86)).dim())?;

    for p in &plans {
        let short_id: String = p.id.chars().take(8).collect();
        let status = match p.status.as_str() {
            "Completed" => style("● Completed").green(),
            "Running" => style("● Running").yellow(),
            "Failed" => style("● Failed").red(),
            "Pending" => style("○ Pending").dim(),
            s => style(s).dim(),
        };
        writeln!(
            out,
            "{:<10} {:<30} {:<8} {:<16} {:<22}",
            style(short_id).dim(),
            &p.name,
            format!("{}/{}", p.current_step_index, p.total_steps),
            status,
            style(&p.updated_at).dim(),
        )?;
    }
    Ok(out)
}

async fn cmd_show_string(repo: &TaskRepo, plan_id: &str) -> Result<String> {
    use std::fmt::Write;
    let plan = repo
        .load_plan(plan_id)
        .await
        .map_err(|_| anyhow::anyhow!("Plan not found: {}", plan_id))?;

    let mut out = String::new();
    let status_icon = match &plan.status {
        PlanStatus::Completed => style("✓ Completed").green(),
        PlanStatus::Running => style("▶ Running").yellow(),
        PlanStatus::Failed => style("✗ Failed").red(),
        PlanStatus::Pending => style("· Pending").dim(),
        PlanStatus::WaitingForInput => style("⊘ Waiting").cyan(),
    };

    writeln!(
        out,
        "{} {}  {}",
        style("Plan:").cyan().bold(),
        style(&plan.name).white().bold(),
        style(&plan.id).dim()
    )?;
    writeln!(
        out,
        "{}  {}  {}/{} steps",
        style("Status:").dim(),
        status_icon,
        plan.current_step_index,
        plan.steps.len(),
    )?;
    writeln!(out)?;
    writeln!(out, "{}", style("Steps:").dim())?;
    for (i, step) in plan.steps.iter().enumerate() {
        writeln!(
            out,
            "  {} [{}] {}",
            step_icon(step.status()),
            i,
            step_label(step),
        )?;
    }
    Ok(out)
}

fn step_icon(status: &StepStatus) -> String {
    match status {
        StepStatus::Completed => style("✓").green().to_string(),
        StepStatus::Running => style("▶").yellow().to_string(),
        StepStatus::Failed => style("✗").red().to_string(),
        StepStatus::Pending => style("·").dim().to_string(),
        StepStatus::WaitingForInput => style("⊘").cyan().to_string(),
    }
}

fn step_label(step: &Step) -> String {
    match step {
        Step::Think { instruction, .. } => format!("Think: {}", instruction),
        Step::ToolCall {
            tool_name, params, ..
        } => format!("Tool: {} ({})", tool_name, params),
        Step::WaitForInput { prompt, .. } => format!("Wait: {}", prompt),
        Step::Finish { summary, .. } => format!("Finish: {}", summary),
        Step::Exec { command, .. } => format!("Exec: {}", command),
        Step::HttpRequest { url, method, .. } => format!("HTTP: {:?} {}", method, url),
        Step::BrowserAction { action, .. } => format!("Browser: {:?}", action),
    }
}

async fn cmd_resume_string(plan_id: &str) -> Result<String> {
    Ok(format!(
        "Use: rupoo run --task {} --db agent.db\n  or from TUI: /run {}",
        plan_id, plan_id
    ))
}

async fn cmd_delete_string(repo: &TaskRepo, plan_id: &str) -> Result<String> {
    use std::fmt::Write;
    repo.delete_plan(plan_id).await?;
    let mut out = String::new();
    writeln!(out, "{} Plan {} deleted.", style("✓").green(), plan_id)?;
    Ok(out)
}

async fn cmd_prune_string(repo: &TaskRepo, days: u64) -> Result<String> {
    use std::fmt::Write;
    let before = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let deleted = repo.prune_plans(&before).await?;
    let mut out = String::new();
    writeln!(
        out,
        "{} Pruned {} completed/failed plans older than {} days.",
        style("✓").green(),
        deleted,
        days
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_icon_pending() {
        let icon = step_icon(&StepStatus::Pending);
        assert!(
            icon.contains("·"),
            "Expected icon to contain '·', got: {:?}",
            icon
        );
    }

    #[test]
    fn test_step_icon_completed() {
        let icon = step_icon(&StepStatus::Completed);
        assert!(icon.contains("✓"));
    }

    #[test]
    fn test_step_label_think() {
        let step = Step::Think {
            id: "1".into(),
            instruction: "Test analysis".into(),
            status: StepStatus::Pending,
            output: None,
        };
        let label = step_label(&step);
        assert!(label.contains("Think"));
        assert!(label.contains("Test analysis"));
    }

    #[test]
    fn test_step_label_finish() {
        let step = Step::Finish {
            id: "2".into(),
            summary: "All done".into(),
            status: StepStatus::Pending,
        };
        let label = step_label(&step);
        assert!(label.contains("Finish"));
        assert!(label.contains("All done"));
    }
}
