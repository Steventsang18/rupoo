use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::task::{PlanStatus, Step, StepStatus};
use chrono::Utc;
use std::sync::Arc;

pub async fn run(db_path: &str, action: crate::SessionAction) -> Result<()> {
    let repo = Arc::new(TaskRepo::new(db_path)?);

    match action {
        crate::SessionAction::List { limit } => cmd_list(&repo, limit).await?,
        crate::SessionAction::Show { id } => cmd_show(&repo, &id).await?,
        crate::SessionAction::Resume { id, .. } => cmd_resume(&id).await?,
        crate::SessionAction::Delete { id } => cmd_delete(&repo, &id).await?,
        crate::SessionAction::Prune { days } => cmd_prune(&repo, days).await?,
    }
    Ok(())
}

async fn cmd_list(repo: &TaskRepo, limit: usize) -> Result<()> {
    let plans = repo.list_plans(limit, 0).await?;
    if plans.is_empty() {
        println!("{} No plans found.", style("ℹ").yellow());
        return Ok(());
    }

    println!(
        "{:<10} {:<30} {:<8} {:<16} {:<22}",
        style("ID").bold(),
        style("Name").bold(),
        style("Steps").bold(),
        style("Status").bold(),
        style("Updated").bold(),
    );
    println!("{}", style("─".repeat(86)).dim());

    for p in &plans {
        let short_id: String = p.id.chars().take(8).collect();
        let status = match p.status.as_str() {
            "Completed" => style("● Completed").green(),
            "Running" => style("● Running").yellow(),
            "Failed" => style("● Failed").red(),
            "Pending" => style("○ Pending").dim(),
            s => style(s).dim(),
        };
        println!(
            "{:<10} {:<30} {:<8} {:<16} {:<22}",
            style(short_id).dim(),
            &p.name,
            format!("{}/{}", p.current_step_index, p.total_steps),
            status,
            style(&p.updated_at).dim(),
        );
    }
    Ok(())
}

async fn cmd_show(repo: &TaskRepo, plan_id: &str) -> Result<()> {
    let plan = repo
        .load_plan(plan_id)
        .await
        .map_err(|_| anyhow::anyhow!("Plan not found: {}", plan_id))?;

    let status_icon = match &plan.status {
        PlanStatus::Completed => style("✓ Completed").green(),
        PlanStatus::Running => style("▶ Running").yellow(),
        PlanStatus::Failed => style("✗ Failed").red(),
        PlanStatus::Pending => style("· Pending").dim(),
        PlanStatus::WaitingForInput => style("⊘ Waiting").cyan(),
    };

    println!(
        "{} {}  {}",
        style("Plan:").cyan().bold(),
        style(&plan.name).white().bold(),
        style(&plan.id).dim()
    );
    println!(
        "{}  {}  {}/{} steps",
        style("Status:").dim(),
        status_icon,
        plan.current_step_index,
        plan.steps.len(),
    );
    println!();
    println!("{}", style("Steps:").dim());
    for (i, step) in plan.steps.iter().enumerate() {
        println!(
            "  {} [{}] {}",
            step_icon(step.status()),
            i,
            step_label(step),
        );
    }
    Ok(())
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
        Step::Think {
            instruction, ..
        } => format!("Think: {}", instruction),
        Step::ToolCall {
            tool_name, params, ..
        } => format!("Tool: {} ({})", tool_name, params),
        Step::WaitForInput {
            prompt, ..
        } => format!("Wait: {}", prompt),
        Step::Finish {
            summary, ..
        } => format!("Finish: {}", summary),
        Step::Exec {
            command, ..
        } => format!("Exec: {}", command),
        Step::HttpRequest {
            url, method, ..
        } => format!("HTTP: {:?} {}", method, url),
        Step::BrowserAction {
            action, ..
        } => format!("Browser: {:?}", action),
    }
}

async fn cmd_resume(plan_id: &str) -> Result<()> {
    println!("Use: rupoo run --task {} --db agent.db", plan_id);
    println!("  or from TUI: /run {}", plan_id);
    Ok(())
}

async fn cmd_delete(repo: &TaskRepo, plan_id: &str) -> Result<()> {
    repo.delete_plan(plan_id).await?;
    println!("{} Plan {} deleted.", style("✓").green(), plan_id);
    Ok(())
}

async fn cmd_prune(repo: &TaskRepo, days: u64) -> Result<()> {
    let before = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
    let deleted = repo.prune_plans(&before).await?;
    println!(
        "{} Pruned {} completed/failed plans older than {} days.",
        style("✓").green(),
        deleted,
        days
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_icon_pending() {
        assert_eq!(step_icon(&StepStatus::Pending), "·");
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
