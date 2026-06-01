use clap::{Parser, Subcommand};

mod cli;
mod tracing_setup;

mod main_cli;
mod build_engine;
mod executor;

/// Rupoo — AI-powered assistant for your terminal.
/// Run without subcommands to enter interactive mode.
#[derive(Parser)]
#[command(name = "rupoo", version = env!("CARGO_PKG_VERSION"), about)]
struct Cli {
    /// Show debug-level logs on stderr.
    #[arg(long, global = true)]
    verbose: bool,

    /// Optional subcommand. If omitted, enters interactive TUI.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single plan by ID
    Run {
        /// Plan ID to execute
        #[arg(long)]
        task: String,
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
        /// Input to provide if the plan is waiting for user input
        #[arg(long)]
        input: Option<String>,
    },
    /// Run the built-in demo plan
    Demo {
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Manage skills
    Skills {
        #[command(subcommand)]
        action: main_cli::SkillAction,
    },
    /// Git integration: status, commit, PR
    Git {
        #[command(subcommand)]
        action: main_cli::GitAction,
    },
    /// Manage configuration (API keys, model settings)
    Config {
        #[command(subcommand)]
        action: main_cli::ConfigAction,
    },
    /// Launch the desktop GUI
    /// Start MCP protocol server over stdio
    McpServer,
    /// Show system status overview
    Status {
        /// Short one-line output (for scripts)
        #[arg(long)]
        short: bool,
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Show/switch LLM provider and model
    Model {
        #[command(subcommand)]
        action: Option<main_cli::ModelAction>,
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// List, show, resume, delete plans
    Session {
        #[command(subcommand)]
        action: main_cli::SessionAction,
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Diagnose configuration and environment
    Doctor {
        /// Attempt to auto-fix warnings
        #[arg(long)]
        fix: bool,
    },
    /// View and follow agent logs
    Logs {
        /// Follow log file in real-time
        #[arg(long)]
        follow: bool,
        /// Number of lines to show (default: 50)
        #[arg(long, default_value_t = 50)]
        lines: usize,
        /// Filter by log level (e.g., WARN, ERROR)
        #[arg(long)]
        level: Option<String>,
        /// Show previous session log instead
        #[arg(long)]
        prev: bool,
    },
    /// Start in server mode (placeholder for future daemon)
    Serve {
        /// Database path (default: ~/.rupoo/agent.db)
        #[arg(long)]
        db: Option<String>,
        /// Port to listen on
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, elvish, powershell)
        shell: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_setup::init_logging(cli.verbose);

    match cli.command {
        None => {
            // Build the agent engine, then launch the three-panel TUI
            let data_dir = tracing_setup::data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("agent.db");
            let (repo, agent, tool_executor, llm_router) =
                build_engine::build_engine(db_path.to_str().unwrap_or("agent.db")).await?;

            // Capture tokio handle on the main async thread (not inside spawn_blocking)
            let handle = tokio::runtime::Handle::current();

            let err_msg = tokio::task::spawn_blocking(move || {
                crate::cli::run_tui_with_agent(repo, agent, tool_executor, handle, llm_router)
            })
            .await
            .map_err(|e| anyhow::anyhow!("TUI task failed: {e}"))?;

            if let Err(e) = err_msg {
                // Make sure stderr is flushed so user sees the panic message
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "\nrupoo error: {}", e);
                anyhow::bail!("TUI error: {}", e);
            }
        }
        Some(cmd) => main_cli::run_cmd(cmd).await?,
    }
    Ok(())
}
