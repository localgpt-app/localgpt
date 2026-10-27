//! `localgpt` — a `.world` package's command line, and the shim that gives
//! any MCP agent tools against the world an app has open.
//!
//! The assistant this binary was through 0.4.x — chat, tui, daemon, memory,
//! heartbeat and cron, the sandbox, the APIs, the bridges — went in 0.5.0:
//! the coding agents are the better harness, and a world is edited from
//! outside the window (`docs/rfcs/authoring/the-harness-goes.md`).
//! `cargo install localgpt --version 0.4.2` keeps it.

use anyhow::Result;
use clap::Parser;

mod cli;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> Result<()> {
    // Logs go to stderr: stdout is the transport while `world mcp` serves.
    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Commands::World { command } => crate::cli::world::run(command).await,
        Commands::Completion(args) => crate::cli::completion::run(args),
    }
}
