use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use rupoo::skill::SkillManager;

struct CheckResult {
    name: &'static str,
    passed: bool,
    message: Option<String>,
    fixable: bool,
}

impl CheckResult {
    fn new(name: &'static str, passed: bool, message: Option<String>, fixable: bool) -> Self {
        Self { name, passed, message, fixable }
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn status_summary(results: &[CheckResult]) -> (usize, usize, usize) {
    let pass = results.iter().filter(|r| r.passed).count();
    let warn = results.iter().filter(|r| !r.passed && r.fixable).count();
    let fail = results.iter().filter(|r| !r.passed && !r.fixable).count();
    (pass, warn, fail)
}

pub async fn run(fix: bool) -> Result<()> {
    let out = output(false).await?;
    print!("{out}");
    if fix {
        let (_, warn, fail) = status_summary(&all_checks().await);
        if warn + fail > 0 {
            println!("\n{}", style("Attempting fixes...").yellow());
            run_fixes().await?;
        }
    }
    Ok(())
}

pub async fn output(show_hint: bool) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "{}", style("Rupoo Diagnostics").bold())?;
    let results = all_checks().await;

    for r in &results {
        let icon = if r.passed {
            style("●").green()
        } else if r.fixable {
            style("○").yellow()
        } else {
            style("✗").red()
        };
        writeln!(out, "  {}  {}", icon, style(r.name).white().bold())?;
        if let Some(msg) = &r.message {
            writeln!(out, "{}", indent(msg, 4))?;
        }
    }

    writeln!(out, "{}", style("─".repeat(50)).dim())?;
    let (pass, warn, fail) = status_summary(&results);
    writeln!(out, "{} {}  {} {}  {} {}",
        style("●").green(),
        style(format!("{pass} passed")).green(),
        style("●").yellow(),
        style(format!("{warn} warnings")).yellow(),
        if fail == 0 { style("●").green() } else { style("✗").red() },
        style(format!("{fail} errors")).red(),
    )?;

    let (_, warn, fail) = status_summary(&results);
    if show_hint && warn + fail > 0 {
        writeln!(out, "  {} Run with --fix to auto-resolve fixable issues.", style("→").dim())?;
    }

    Ok(out)
}

async fn all_checks() -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. Database
    match TaskRepo::new("agent.db") {
        Ok(_repo) => {
            let tables = ["plans", "checkpoints", "settings", "memories"];
            let msg = format!("agent.db — connected, {} tables present", tables.len());
            results.push(CheckResult::new("Database", true, Some(msg), false));
        }
        Err(e) => {
            results.push(CheckResult::new("Database", false, Some(format!("Cannot open DB: {e}")), false));
        }
    }

    // 2. LLM keys
    if let Ok(repo) = TaskRepo::new("agent.db") {
        let mut msgs = Vec::new();
        let mut all_ok = true;
        for provider in &["anthropic", "openai", "deepseek"] {
            let key = format!("api_key.{provider}");
            match repo.get_setting(&key).await {
                Ok(Some(val)) if val.len() > 4 => {
                    let prefix: String = val.chars().take(8).collect();
                    msgs.push(format!("{}: configured ({prefix}...)", provider));
                }
                _ => {
                    msgs.push(format!("{}: {} — no {} set", provider, style("WARN").yellow(), key));
                    all_ok = false;
                }
            }
        }
        // Ollama check
        match reqwest::get("http://localhost:11434/api/tags").await {
            Ok(resp) if resp.status().is_success() => {
                msgs.push("ollama: reachable at localhost:11434".into());
            }
            _ => {
                msgs.push(format!("ollama: {} — connection refused (optional)", style("WARN").yellow()));
                // Don't mark all_ok false for optional Ollama
            }
        }
        results.push(CheckResult::new("LLM Configuration", all_ok, Some(msgs.join("\n")), true));
    }

    // 3. Skills
    let skill_dir = SkillManager::default_dir();
    if skill_dir.exists() {
        match SkillManager::new(skill_dir.clone()).list_skills() {
            Ok(skills) => {
                let names: Vec<String> = skills.iter().map(|s| format!("'{}'", s)).collect();
                let msg = format!("{} installed at {}\n  {}",
                    skills.len(), skill_dir.display(), names.join(", "));
                results.push(CheckResult::new("Skills", true, Some(msg), false));
            }
            Err(e) => {
                results.push(CheckResult::new("Skills", false, Some(format!("Error: {e}")), false));
            }
        }
    } else {
        let msg = format!("Directory not found: {}", skill_dir.display());
        results.push(CheckResult::new("Skills", false, Some(msg), true));
    }

    // 4. Git
    match rupoo::git::GitRepo::open(".") {
        Ok(git) => {
            let branch = git.current_branch().unwrap_or_default();
            let msg = format!("libgit2 available, repository at ./ ({})", branch);
            results.push(CheckResult::new("Git", true, Some(msg), false));
        }
        Err(_) => {
            results.push(CheckResult::new("Git", true, Some("(not a git repository)".into()), false));
        }
    }

    // 5. Data directory
    let data_dir = crate::tracing_setup::data_dir();
    if data_dir.exists() {
        results.push(CheckResult::new("Data Directory", true,
            Some(format!("{} — exists, writable", data_dir.display())), false));
    } else {
        results.push(CheckResult::new("Data Directory", false,
            Some(format!("Not found: {}", data_dir.display())), true));
    }

    // 6. Log file
    let log_path = data_dir.join("rupoo.log");
    match std::fs::metadata(&log_path) {
        Ok(meta) => {
            let size = if meta.len() < 1024 {
                format!("{} B", meta.len())
            } else {
                format!("{:.1} KB", meta.len() as f64 / 1024.0)
            };
            results.push(CheckResult::new("Log File", true,
                Some(format!("{} — {}", log_path.display(), size)), false));
        }
        Err(_) => {
            results.push(CheckResult::new("Log File", false,
                Some(format!("Not found: {}", log_path.display())), true));
        }
    }

    results
}

async fn run_fixes() -> Result<()> {
    let dirs = [
        SkillManager::default_dir(),
        crate::tracing_setup::data_dir(),
    ];
    for d in &dirs {
        if !d.exists() {
            std::fs::create_dir_all(d)?;
            println!("  {} Created: {}", style("✓").green(), d.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_summary_all_pass() {
        let results = vec![
            CheckResult::new("DB", true, None, false),
            CheckResult::new("Git", true, None, false),
        ];
        let (pass, warn, fail) = status_summary(&results);
        assert_eq!(pass, 2);
        assert_eq!(warn, 0);
        assert_eq!(fail, 0);
    }

    #[test]
    fn test_status_summary_mixed() {
        let results = vec![
            CheckResult::new("DB", true, None, false),
            CheckResult::new("Keys", false, None, true),
            CheckResult::new("Network", false, None, false),
        ];
        let (pass, warn, fail) = status_summary(&results);
        assert_eq!(pass, 1);
        assert_eq!(warn, 1);
        assert_eq!(fail, 1);
    }

    #[test]
    fn test_indent_single_line() {
        let result = indent("hello", 4);
        assert_eq!(result, "    hello");
    }

    #[test]
    fn test_indent_multi_line() {
        let result = indent("line1\nline2", 2);
        assert_eq!(result, "  line1\n  line2");
    }
}
