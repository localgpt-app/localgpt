//! `localgpt` — a `.world` package's command line, the shim that gives
//! any MCP agent tools against the world an app has open, and deterministic
//! screenplay-to-previs (`localgpt previs`).
//!
//! There is no chat, daemon, model provider or key store in this binary.
//! Coding agents stay outside and edit a world through its API or this shim.

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
        Commands::Previs(args) => crate::cli::previs::run(args),
        #[cfg(feature = "audio")]
        Commands::Audio { command } => crate::cli::audio::run(command).await,
        Commands::Completion(args) => crate::cli::completion::run(args),
    }
}
