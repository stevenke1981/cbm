use cbrlm::cli;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "cbrlm", version, about = "Codebase RLM Memory MCP — Rust knowledge graph server")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Enable HTTP graph UI (also CBRLM_UI=1)
    #[arg(long, default_value_t = false)]
    ui: bool,

    /// HTTP UI port (also CBRLM_PORT)
    #[arg(long, default_value_t = 9749)]
    port: u16,
}

#[derive(Subcommand)]
enum Command {
    /// Run a single MCP tool from CLI
    Cli {
        tool: String,
        #[arg(long)]
        json: bool,
        args: Option<String>,
    },
    /// Install binary and configure MCP for coding agents
    Install {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        all: bool,
    },
    /// Remove MCP integration and hooks
    Uninstall {
        #[arg(long)]
        dry_run: bool,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        keep_binary: bool,
    },
    /// PreToolUse graph augmenter (reads hook JSON from stdin)
    HookAugment,
    /// Print SessionStart reminder to stdout
    HookSessionStart,
    /// Config utilities
    Config {
        action: String,
    },
    /// HTTP graph UI only (no MCP stdio)
    Ui {
        #[arg(long, default_value_t = 9749)]
        port: u16,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("cbrlm=info".parse().unwrap()))
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    let result = match args.command {
        Some(Command::Cli { tool, json, args }) => cli::run_cli(&tool, args.as_deref(), json),
        Some(Command::Install { dry_run, force, yes, all }) => {
            cli::run_install(cbrlm::install::InstallOptions {
                dry_run,
                force,
                yes,
                all_agents: all,
                binary: None,
            })
        }
        Some(Command::Uninstall { dry_run, yes, all, keep_binary }) => {
            cli::run_uninstall(cbrlm::install::UninstallOptions {
                dry_run,
                yes,
                all_agents: all,
                keep_binary,
            })
        }
        Some(Command::HookAugment) => {
            cli::run_hook_augment();
            Ok(())
        }
        Some(Command::HookSessionStart) => {
            cli::run_hook_session_start();
            Ok(())
        }
        Some(Command::Config { action }) => cli::run_config(&action),
        Some(Command::Ui { port }) => cli::run_ui_server(port),
        None => cli::run_mcp_server(cbrlm::http::UiConfig::from_env_and_args(args.ui, args.port)),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}