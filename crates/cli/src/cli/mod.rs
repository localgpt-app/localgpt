#[cfg(feature = "audio")]
pub mod audio;
pub mod completion;
pub mod previs;
pub mod world;
pub mod world_api;
pub mod world_mcp;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "localgpt")]
#[command(
    author,
    version,
    about = "A .world package's command line, the MCP shim for an open world, and deterministic screenplay-to-previs"
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

    /// Stage a screenplay (.fountain, .fdx, .pdf, or .md) as per-scene .world packages, shotlist.csv, and board.html
    Previs(previs::PrevisArgs),

    /// The optional audio models: list, install and remove them, run them, serve them to an agent
    #[cfg(feature = "audio")]
    Audio {
        #[command(subcommand)]
        command: audio::AudioCommands,
    },

    /// Generate shell completion scripts
    Completion(completion::CompletionArgs),
}
