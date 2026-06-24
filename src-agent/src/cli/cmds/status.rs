use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run(db_path: &str, short: bool) -> Result<()> {
    let out = output(db_path, short).await?;
    print!("{out}");
    Ok(())
}

pub async fn output(db_path: &str, short: bool) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    let repo = Arc::new(TaskRepo::new(db_path)?);

    let plan_counts = repo.count_plans_by_status().await?;
    let total_plans: i64 = plan_counts.iter().map(|(_, c)| c).sum();

    let provider = repo
        .get_setting("active_provider")
        .await?
        .unwrap_or_else(|| "none".into());
    let model_key = format!("model.{provider}");
    let model = repo
        .get_setting(&model_key)
        .await?
        .unwrap_or_else(|| "(default)".into());
    let key_key = format!("api_key.{provider}");
    let has_key = repo
        .get_setting(&key_key)
        .await?
        .map(|k| k.len() > 4)
        .unwrap_or(false);

    let skills = rupoo::skill::SkillManager::new(rupoo::skill::SkillManager::default_dir())
        .list_skills()
        .unwrap_or_default();

    if short {
        writeln!(
            out,
            "{}",
            format_short_line(
                VERSION,
                total_plans as usize,
                &provider,
                &model,
                skills.len(),
            )
        )?;
    } else {
        writeln!(out, "{} {}", style("Rupoo").bold(), style(VERSION).dim())?;

        writeln!(
            out,
            "  {}  {:<12} {}     {}",
            style("├──").dim(),
            style("Data").cyan(),
            style(db_path).white(),
            style("(WAL mode)").dim(),
        )?;
        writeln!(
            out,
            "  {}  {:<12} {}",
            style("├──").dim(),
            style("Plans").cyan(),
            build_status_counts(&plan_counts),
        )?;
        writeln!(
            out,
            "  {}  {:<12} {}  {} / {}",
            style("├──").dim(),
            style("LLM").cyan(),
            if has_key {
                style("●").green()
            } else {
                style("○").yellow()
            },
            style(&provider).white(),
            style(&model).dim(),
        )?;
        writeln!(
            out,
            "  {}  {:<12} {} installed {}",
            style("├──").dim(),
            style("Skills").cyan(),
            skills.len(),
            if skills.is_empty() {
                String::new()
            } else {
                format!("({})", skills.join(", "))
            },
        )?;
        writeln!(
            out,
            "  {}  {:<12} {} {}",
            style("├──").dim(),
            style("Memory").cyan(),
            style("●").green(),
            style("entries (FTS5 indexed)").dim(),
        )?;
        out.push_str(&git_status_line()?);
        out.push_str(&log_info_line()?);
    }
    Ok(out)
}

fn build_status_counts(counts: &[(String, i64)]) -> String {
    let total: i64 = counts.iter().map(|(_, c)| c).sum();
    let mut parts = vec![format!("{} total", total)];
    for (status, count) in counts {
        let styled = match status.as_str() {
            "Completed" => style(format!("{} completed", count)).green(),
            "Running" => style(format!("{} running", count)).yellow(),
            "Failed" => style(format!("{} failed", count)).red(),
            _ => style(format!("{} {}", count, status.to_lowercase())).dim(),
        };
        parts.push(styled.to_string());
    }
    parts.join("  ")
}

fn format_short_line(
    ver: &str,
    plans: usize,
    provider: &str,
    model: &str,
    skills: usize,
) -> String {
    format!(
        "Rupoo {} | {} plans | {}/{} | {} skills",
        ver, plans, provider, model, skills
    )
}

fn git_status_line() -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    match rupoo::git::GitRepo::open(".") {
        Ok(git) => {
            let branch = git.current_branch().unwrap_or_default();
            let files = git.status().unwrap_or_default();
            let status = if files.is_empty() {
                "clean".to_string()
            } else {
                format!("{} uncommitted", files.len())
            };
            writeln!(
                out,
                "  {}  {:<12} {}  {}",
                style("├──").dim(),
                style("Git").cyan(),
                style(branch).green(),
                style(status).dim(),
            )?;
        }
        Err(_) => {
            writeln!(
                out,
                "  {}  {:<12} {}",
                style("├──").dim(),
                style("Git").cyan(),
                style("(not a git repository)").dim(),
            )?;
        }
    }
    Ok(out)
}

fn log_info_line() -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    let log_path = crate::tracing_setup::data_dir().join("rupoo.log");
    let size = if log_path.exists() {
        let meta = std::fs::metadata(&log_path)?;
        if meta.len() < 1024 {
            format!("{} B", meta.len())
        } else {
            format!("{:.1} KB", meta.len() as f64 / 1024.0)
        }
    } else {
        "none".into()
    };
    writeln!(
        out,
        "  {}  {:<12} {}  ({})",
        style("└──").dim(),
        style("Log").cyan(),
        style(log_path.display()).dim(),
        style(size).dim(),
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_status_counts() {
        let counts = vec![
            ("Completed".to_string(), 8i64),
            ("Running".to_string(), 2i64),
            ("Failed".to_string(), 2i64),
        ];
        let lines = build_status_counts(&counts);
        assert!(lines.contains("12 total"));
        assert!(lines.contains("8 completed"));
        assert!(lines.contains("2 running"));
        assert!(lines.contains("2 failed"));
    }

    #[test]
    fn test_build_status_counts_empty() {
        let lines = build_status_counts(&[]);
        assert!(lines.contains("0 total"));
    }

    #[test]
    fn test_format_short_line() {
        let line = format_short_line("0.2.0", 5, "anthropic", "claude-sonnet-4", 3);
        assert!(line.contains("0.2.0"));
        assert!(line.contains("5 plans"));
        assert!(line.contains("anthropic/claude-sonnet-4"));
        assert!(line.contains("3 skills"));
    }
}
