pub mod completion;
pub mod world;
pub mod world_mcp;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "localgpt")]
#[command(
    author,
    version,
    about = "A .world package's command line, and the shim that gives any agent tools against an open world"
)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Log at debug level, to stderr
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Work on a `.world` package: commit batches, read the log, verify, serve it to an agent
    World {
        #[command(subcommand)]
        command: world::WorldCommands,
    },

    /// Generate shell completion scripts
    Completion(completion::CompletionArgs),
}
