use clap::{Parser, Subcommand};

mod cli;

/// Re-export the logging module from the library crate so existing
/// `crate::tracing_setup::*` paths (including cli/) keep working.
pub mod tracing_setup {
    pub use rupoo::tracing_setup::*;
}

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
    /// Check for and install the latest release from GitHub
    Update {
        /// Only check whether an update is available, do not install
        #[arg(long)]
        check: bool,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() -> miette::Result<()> {
    // Register a global panic hook to capture crash info before exit.
    // Instead of dumping a raw backtrace to stderr, the hook writes a
    // structured crash report to a file under the data directory and
    // logs it via tracing so the CLI/TUI can surface a friendly message.
    std::panic::set_hook(Box::new(|info| {
        // Try to get a human-readable description.
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let report = format!(
            "CRASH [{location}] {msg}\nBacktrace:\n{}",
            std::backtrace::Backtrace::capture()
        );
        // Emit to the tracing subscriber so it appears in structured logs.
        tracing::error!(crash_location = %location, "{msg}");
        // Also write to a dedicated crash file under the data directory.
        if let Ok(dir) = crate::tracing_setup::data_dir_str() {
            let crash_file = std::path::PathBuf::from(&dir).join("crash.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&crash_file)
            {
                use std::io::Write;
                let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
                let _ = writeln!(f, "[{ts}] {report}");
            }
        }
        // Still print a concise message to stderr so the user knows something went wrong.
        eprintln!(
            "\n💥 rupoo crashed: {msg}\n   at {location}\n   Crash details saved to {{data_dir}}/crash.log\n   Please report this at https://github.com/Steventsang18/rupoo/issues\n"
        );
    }));

    let cli = Cli::parse();
    crate::tracing_setup::init_logging(cli.verbose);

    match cli.command {
        None => {
            // Build the agent engine, then launch the three-panel TUI
            let data_dir = crate::tracing_setup::data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("agent.db");
            let (repo, agent, tool_executor) =
                rupoo::build_engine::build_engine(db_path.to_str().unwrap_or("agent.db"))
                    .await
                    .map_err(|e| miette::miette!("build engine: {}", e))?;

            // Capture tokio handle on the main async thread (not inside spawn_blocking)
            let handle = tokio::runtime::Handle::current();

            let err_msg = tokio::task::spawn_blocking(move || {
                crate::cli::run_tui_with_agent(repo, agent, tool_executor, handle)
            })
            .await
            .map_err(|e| miette::miette!("TUI task failed: {e}"))?;

            if let Err(e) = err_msg {
                // Make sure stderr is flushed so user sees the panic message
                use std::io::Write;
                let _ = writeln!(std::io::stderr(), "\nrupoo error: {}", e);
                miette::bail!("TUI error: {}", e);
            }
        }
        // Handled separately because self-update is a bin-level concern.
        Some(Commands::Update { check }) => match check {
            true => match rupoo::updater::check() {
                Ok(true) => {
                    println!("A newer release is available. Run `rupoo update` to install it.")
                }
                Ok(false) => println!("You are running the latest version."),
                Err(e) => miette::bail!("update check failed: {}", e),
            },
            false => match rupoo::updater::update() {
                Ok(rupoo::updater::UpdateOutcome::Updated(v)) => {
                    println!("Updated to v{v}. Restart rupoo to use the new version.")
                }
                Ok(rupoo::updater::UpdateOutcome::UpToDate(v)) => {
                    println!("Already running the latest version (v{v}).")
                }
                Err(e) => miette::bail!("update failed: {}", e),
            },
        },
        Some(cmd) => main_cli::run_cmd(cmd)
            .await
            .map_err(|e| miette::miette!("{}", e))?,
    }
    Ok(())
}
