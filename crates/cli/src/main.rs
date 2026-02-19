use anyhow::Result;
use clap::Parser;

mod cli;
#[cfg(feature = "desktop")]
mod desktop;
mod tools;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    // argv[0] dispatch: if re-exec'd as "localgpt-sandbox", enter sandbox child path
    // immediately — before Tokio, Clap, or any other initialization.
    #[cfg(unix)]
    if let Some(arg0) = std::env::args_os().next()
        && arg0.to_string_lossy().ends_with("localgpt-sandbox")
    {
        localgpt_sandbox::sandbox_child_main();
    }

    let cli = Cli::parse();

    // Handle Gen mode specially — Bevy must own the main thread (no tokio runtime here)
    #[cfg(feature = "gen")]
    if let Commands::Gen(args) = cli.command {
        // Initialize logging before handing off to Bevy
        let log_level = if cli.verbose { "debug" } else { "info" };
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
            )
            .with_writer(std::io::stderr)
            .init();
        return crate::cli::gen3d::run(args, &cli.agent);
    }

    // Handle daemon start/restart specially - must fork BEFORE starting Tokio runtime
    #[cfg(unix)]
    if let Commands::Daemon(ref args) = cli.command {
        match args.command {
            crate::cli::daemon::DaemonCommands::Start { foreground: false } => {
                // Do the fork synchronously, then start Tokio in the child
                return crate::cli::daemon::daemonize_and_run(&cli.agent);
            }
            crate::cli::daemon::DaemonCommands::Restart { foreground: false } => {
                // Stop first (synchronously), then fork and start
                crate::cli::daemon::stop_sync()?;
                return crate::cli::daemon::daemonize_and_run(&cli.agent);
            }
            _ => {}
        }
    }

    // For all other commands, start the async runtime normally
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> Result<()> {
    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        Commands::Chat(args) => crate::cli::chat::run(args, &cli.agent).await,
        Commands::Ask(args) => crate::cli::ask::run(args, &cli.agent).await,
        #[cfg(feature = "desktop")]
        Commands::Desktop(args) => crate::cli::desktop::run(args, &cli.agent),
        #[cfg(feature = "gen")]
        Commands::Gen(_) => unreachable!("Gen is handled before tokio runtime starts"),
        Commands::Daemon(args) => crate::cli::daemon::run(args, &cli.agent).await,
        Commands::Memory(args) => crate::cli::memory::run(args, &cli.agent).await,
        Commands::Config(args) => crate::cli::config::run(args).await,
        Commands::Paths => crate::cli::paths::run(),
        Commands::Md(args) => crate::cli::md::run(args).await,
        Commands::Sandbox(args) => crate::cli::sandbox::run(args).await,
        Commands::Search(args) => crate::cli::search::run(args).await,
    }
}
