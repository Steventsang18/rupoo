use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;
use std::sync::Arc;

const PROVIDER_KEYS: &[(&str, &str, &str, &str)] = &[
    (
        "anthropic",
        "claude-sonnet-4-20250514",
        "api_key.anthropic",
        "",
    ),
    ("openai", "gpt-4o", "api_key.openai", "base_url.openai"),
    ("ollama", "llama3.2", "", ""),
];

pub async fn run(db_path: &str, action: Option<crate::main_cli::ModelAction>) -> Result<()> {
    let out = output(db_path, action).await?;
    print!("{out}");
    Ok(())
}

pub async fn output(db_path: &str, action: Option<crate::main_cli::ModelAction>) -> Result<String> {
    let repo = Arc::new(TaskRepo::new(db_path)?);
    let action = action.unwrap_or(crate::main_cli::ModelAction::Show);

    match action {
        crate::main_cli::ModelAction::Show => cmd_show_string(&repo).await,
        crate::main_cli::ModelAction::List => cmd_list_string(&repo).await,
        crate::main_cli::ModelAction::Set { target } => {
            cmd_set_string(&repo, target.as_deref()).await
        }
    }
}

async fn cmd_show_string(repo: &TaskRepo) -> Result<String> {
    use std::fmt::Write;
    let provider = repo
        .get_setting("active_provider")
        .await?
        .unwrap_or_else(|| "none".into());
    let model = repo
        .get_setting(&format!("model.{provider}"))
        .await?
        .or_else(|| provider_default_model(&provider).map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());
    let api_key = repo.get_setting(&format!("api_key.{provider}")).await?;

    let mut out = String::new();
    writeln!(out, "{}", style("Current LLM Configuration:").bold())?;
    let icon = if api_key.is_some() { "●" } else { "○" };
    let key_display = render_key_status(api_key.as_deref());
    writeln!(
        out,
        "  {}  {}  {} / {}",
        icon,
        style("Provider:").cyan(),
        style(&provider).white(),
        style(&model).dim(),
    )?;
    writeln!(
        out,
        "  {}  {}  {}",
        style("      "),
        style("API Key:").cyan(),
        key_display,
    )?;
    Ok(out)
}

async fn cmd_list_string(repo: &TaskRepo) -> Result<String> {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(
        out,
        "{:<12} {:<30} {:<22}  Status",
        style("Provider").bold(),
        style("Default Model").bold(),
        style("Config Key").bold(),
    )?;
    writeln!(out, "{}", style("─".repeat(76)).dim())?;

    for (name, model, key, _) in PROVIDER_KEYS {
        let key_val = repo.get_setting(key).await?;
        let status = if key.is_empty() {
            style("local-only").dim().to_string()
        } else if key_val.is_some() {
            style("● configured").green().to_string()
        } else {
            style("○ not set").yellow().to_string()
        };
        writeln!(out, "{:<12} {:<30} {:<22}  {}", name, model, key, status,)?;
    }
    Ok(out)
}

async fn cmd_set_string(repo: &TaskRepo, target: Option<&str>) -> Result<String> {
    use std::fmt::Write;
    let target = match target {
        Some(t) => t.to_owned(),
        None => {
            return Ok(
                "  Usage: /model set <provider> (interactive picker not available in TUI)".into(),
            )
        }
    };

    let (provider, model) = parse_target(&target)
        .ok_or_else(|| anyhow::anyhow!("Invalid format. Use: <provider> or <provider>/<model>"))?;

    if !PROVIDER_KEYS.iter().any(|(n, _, _, _)| *n == provider) {
        anyhow::bail!(
            "Unknown provider '{provider}'. Valid: {}",
            PROVIDER_KEYS
                .iter()
                .map(|(n, _, _, _)| *n)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    repo.set_setting("active_provider", provider).await?;

    let mut out = String::new();
    match model {
        Some(m) => {
            repo.set_setting(&format!("model.{provider}"), m).await?;
            writeln!(
                out,
                "{} Provider switched to: {}",
                style("✓").green(),
                provider
            )?;
            writeln!(out, "{} Model set to: {}", style("✓").green(), m)?;
        }
        None => {
            let default_model = provider_default_model(provider)
                .ok_or_else(|| anyhow::anyhow!("No default model for {provider}"))?;
            writeln!(
                out,
                "{} Provider switched to: {} ({})",
                style("✓").green(),
                provider,
                default_model
            )?;
        }
    }

    let key_name = format!("api_key.{provider}");
    if repo.get_setting(&key_name).await?.is_none() {
        writeln!(
            out,
            "  {} Tip: set API key with: rupoo config set {} <key>",
            style("ℹ").yellow(),
            key_name,
        )?;
    }
    Ok(out)
}

// -- Pure helpers --

fn provider_default_model(name: &str) -> Option<&'static str> {
    PROVIDER_KEYS
        .iter()
        .find(|(n, _, _, _)| *n == name)
        .map(|(_, m, _, _)| *m)
}

fn render_key_status(key: Option<&str>) -> String {
    match key {
        Some(k) if k.len() > 8 => {
            let prefix: String = k.chars().take(8).collect();
            format!(
                "{} {} ({})",
                style("●").green(),
                style(prefix).dim(),
                style("set").dim()
            )
        }
        Some(_) => format!("{} set", style("●").green()),
        None => format!("{} not set", style("○").yellow()),
    }
}

fn parse_target(target: &str) -> Option<(&str, Option<&str>)> {
    let parts: Vec<&str> = target.splitn(2, '/').collect();
    match parts.len() {
        1 if !parts[0].is_empty() => Some((parts[0], None)),
        2 if !parts[0].is_empty() && !parts[1].is_empty() && !parts[1].contains('/') => {
            Some((parts[0], Some(parts[1])))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_default_model_exists() {
        for (name, default_model, _, _) in PROVIDER_KEYS {
            let found = provider_default_model(name);
            assert_eq!(
                found,
                Some(*default_model),
                "model for {name} should match PROVIDER_KEYS"
            );
        }
    }

    #[test]
    fn test_render_key_status_set() {
        let rendered = render_key_status(Some("sk-ant-xxxxxxxxxxxx"));
        assert!(rendered.contains("●"));
        assert!(rendered.contains("set"));
    }

    #[test]
    fn test_render_key_status_none() {
        let rendered = render_key_status(None);
        assert!(rendered.contains("○"));
        assert!(rendered.contains("not set"));
    }

    #[test]
    fn test_parse_target() {
        assert_eq!(parse_target("anthropic"), Some(("anthropic", None)));
        assert_eq!(
            parse_target("anthropic/claude-sonnet-4"),
            Some(("anthropic", Some("claude-sonnet-4")))
        );
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("a/b/c"), None);
    }
}
