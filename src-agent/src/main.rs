use clap::{Parser, Subcommand};

mod cli;
mod tracing_setup;

mod executor;
mod main_cli;

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
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
        /// Input to provide if the plan is waiting for user input
        #[arg(long)]
        input: Option<String>,
    },
    /// Run the built-in demo plan
    Demo {
        /// Database path (default: $RUPOO_HOME/agent.db)
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
    Gui {
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Start MCP protocol server over stdio
    McpServer,
    /// Show system status overview
    Status {
        /// Short one-line output (for scripts)
        #[arg(long)]
        short: bool,
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Show/switch LLM provider and model
    Model {
        #[command(subcommand)]
        action: Option<main_cli::ModelAction>,
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// List, show, resume, delete plans
    Session {
        #[command(subcommand)]
        action: main_cli::SessionAction,
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Loop engineering: start, manage adaptive iterative loops
    Loops {
        #[command(subcommand)]
        action: main_cli::LoopAction,
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Manage cron jobs for scheduled task execution
    Cron {
        #[command(subcommand)]
        action: Option<main_cli::CronAction>,
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
    },
    /// Manage and install external tools (MCP servers, search engines)
    Tools {
        #[command(subcommand)]
        action: main_cli::ToolsAction,
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
    /// Start channel daemon (Feishu, DingTalk, etc.)
    Serve {
        /// Database path (default: $RUPOO_HOME/agent.db)
        #[arg(long)]
        db: Option<String>,
        /// Path to rupoo config.toml (default: $RUPOO_HOME/config.toml)
        #[arg(long)]
        config: Option<String>,
        /// Run as daemon in background (auto-daemonize)
        #[arg(short, long)]
        daemon: bool,
    },
    /// Configure channels (Feishu, DingTalk, etc.)
    Channel {
        #[command(subcommand)]
        action: main_cli::ChannelAction,
    },
    /// 停止后台服务
    ServeStop,
    /// 查看后台服务状态
    ServeStatus,
    /// 接入飞书通道
    Feishu,
    /// 接入钉钉通道
    Dingtalk,
    /// 查看已配置通道
    Channels,
    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, elvish, powershell)
        shell: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_setup::init_logging(cli.verbose);

    match cli.command {
        None => {
            // Build the agent engine, then launch the three-panel TUI
            let data_dir = tracing_setup::data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("agent.db");
            let (repo, agent, tool_executor) =
                rupoo::build_engine::build_engine(db_path.to_str().unwrap_or("agent.db")).await?;

            // Capture tokio handle on the main async thread (not inside spawn_blocking)
            let handle = tokio::runtime::Handle::current();

            let err_msg = tokio::task::spawn_blocking(move || {
                crate::cli::run_tui_with_agent(repo, agent, tool_executor, handle)
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
