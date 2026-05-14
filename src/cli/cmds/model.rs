use std::sync::Arc;
use anyhow::Result;
use console::style;
use rupoo::db::TaskRepo;

const PROVIDER_KEYS: &[(&str, &str, &str, &str)] = &[
    ("anthropic", "claude-sonnet-4-20250514", "api_key.anthropic", ""),
    ("openai", "gpt-4o", "api_key.openai", "base_url.openai"),
    ("ollama", "llama3.2", "", ""),
];

pub async fn run(db_path: &str, action: Option<crate::ModelAction>) -> Result<()> {
    let repo = Arc::new(TaskRepo::new(db_path)?);
    let action = action.unwrap_or(crate::ModelAction::Show);

    match action {
        crate::ModelAction::Show => cmd_show(&repo).await?,
        crate::ModelAction::List => cmd_list(&repo).await?,
        crate::ModelAction::Set { target } => cmd_set(&repo, target.as_deref()).await?,
    }
    Ok(())
}

async fn cmd_show(repo: &TaskRepo) -> Result<()> {
    let provider = repo.get_setting("active_provider").await?
        .unwrap_or_else(|| "none".into());
    let model = repo.get_setting(&format!("model.{provider}")).await?
        .or_else(|| provider_default_model(&provider).map(|s| s.to_string()))
        .unwrap_or_else(|| "(unknown)".into());
    let api_key = repo.get_setting(&format!("api_key.{provider}")).await?;

    println!("{}", style("Current LLM Configuration:").bold());
    let icon = if api_key.is_some() { "●" } else { "○" };
    let key_display = render_key_status(api_key.as_deref());
    println!("  {}  {}  {} / {}",
        icon,
        style("Provider:").cyan(),
        style(&provider).white(),
        style(&model).dim(),
    );
    println!("  {}  {}  {}",
        style("      "), // align with icon width
        style("API Key:").cyan(),
        key_display,
    );
    Ok(())
}

async fn cmd_list(repo: &TaskRepo) -> Result<()> {
    println!("{:<12} {:<30} {:<22}  Status",
        style("Provider").bold(),
        style("Default Model").bold(),
        style("Config Key").bold(),
    );
    println!("{}", style("─".repeat(76)).dim());

    for (name, model, key, _) in PROVIDER_KEYS {
        let key_val = repo.get_setting(key).await?;
        let status = if key.is_empty() {
            style("local-only").dim().to_string()
        } else if key_val.is_some() {
            style("● configured").green().to_string()
        } else {
            style("○ not set").yellow().to_string()
        };
        println!("{:<12} {:<30} {:<22}  {}",
            name, model, key, status,
        );
    }
    Ok(())
}

async fn cmd_set(repo: &TaskRepo, target: Option<&str>) -> Result<()> {
    let target = match target {
        Some(t) => t.to_owned(),
        None => {
            let input = show_interactive_picker(repo).await?;
            if input.is_empty() {
                return Ok(());
            }
            input
        }
    };

    let (provider, model) = parse_target(&target)
        .ok_or_else(|| anyhow::anyhow!("Invalid format. Use: <provider> or <provider>/<model>"))?;

    if !PROVIDER_KEYS.iter().any(|(n, _, _, _)| *n == provider) {
        anyhow::bail!("Unknown provider '{provider}'. Valid: {}",
            PROVIDER_KEYS.iter().map(|(n,_,_,_)| *n).collect::<Vec<_>>().join(", "));
    }

    repo.set_setting("active_provider", &provider).await?;

    match model {
        Some(m) => {
            repo.set_setting(&format!("model.{provider}"), m).await?;
            println!("{} Provider switched to: {}", style("✓").green(), provider);
            println!("{} Model set to: {}", style("✓").green(), m);
        }
        None => {
            let default_model = provider_default_model(provider)
                .ok_or_else(|| anyhow::anyhow!("No default model for {provider}"))?;
            println!("{} Provider switched to: {} ({})", style("✓").green(), provider, default_model);
        }
    }

    let key_name = format!("api_key.{provider}");
    if repo.get_setting(&key_name).await?.is_none() {
        println!("  {} Tip: set API key with: rupoo config set {} <key>",
            style("ℹ").yellow(),
            key_name,
        );
    }
    Ok(())
}

async fn show_interactive_picker(repo: &TaskRepo) -> Result<String> {
    let current = repo.get_setting("active_provider").await?.unwrap_or_default();
    println!("{}", style("Select a provider:").bold());
    for (name, model, _, _) in PROVIDER_KEYS {
        let marker = if *name == current { "❯" } else { " " };
        println!("  {} {:<12} → {}  {}",
            style(marker).green(),
            style(name).white(),
            style(model).dim(),
            if *name == current { style("(current)").dim() } else { style("").dim() },
        );
    }
    println!();
    println!("Enter provider name or press Enter to cancel: ");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();
    Ok(input)
}

// -- Pure helpers --

fn provider_default_model(name: &str) -> Option<&'static str> {
    PROVIDER_KEYS.iter()
        .find(|(n, _, _, _)| *n == name)
        .map(|(_, m, _, _)| *m)
}

fn render_key_status(key: Option<&str>) -> String {
    match key {
        Some(k) if k.len() > 8 => {
            let prefix: String = k.chars().take(8).collect();
            format!("{} {} ({})", style("●").green(), style(prefix).dim(), style("set").dim())
        }
        Some(_) => format!("{} set", style("●").green()),
        None => format!("{} not set", style("○").yellow()),
    }
}

fn parse_target(target: &str) -> Option<(&str, Option<&str>)> {
    let parts: Vec<&str> = target.splitn(2, '/').collect();
    match parts.len() {
        1 if !parts[0].is_empty() => Some((parts[0], None)),
        2 if !parts[0].is_empty() && !parts[1].is_empty() && !parts[1].contains('/') => Some((parts[0], Some(parts[1]))),
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
            assert_eq!(found, Some(*default_model), "model for {name} should match PROVIDER_KEYS");
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
