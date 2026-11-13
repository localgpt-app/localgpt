#[cfg(feature = "audio")]
pub mod audio;
pub mod completion;
pub mod gen_cmd;
pub mod previs;
pub mod world;
pub mod world_api;
pub mod world_mcp;

use std::ffi::OsString;

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

    /// Drive Gen's 3D world builder (`localgpt-app gen` / `localgpt-gen`): MCP server, live canvas, sessions
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Gen {
        /// Arguments forwarded to `localgpt-app gen` (or `localgpt-gen`)
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_subcommand_forwards_flags_including_help_and_version() {
        let cli = Cli::try_parse_from(["localgpt", "gen", "--help"]).unwrap();
        match cli.command {
            Commands::Gen { args } => assert_eq!(args, [OsString::from("--help")]),
            _ => panic!("expected gen"),
        }

        let cli = Cli::try_parse_from(["localgpt", "gen", "--version"]).unwrap();
        match cli.command {
            Commands::Gen { args } => assert_eq!(args, [OsString::from("--version")]),
            _ => panic!("expected gen"),
        }

        let cli = Cli::try_parse_from([
            "localgpt",
            "gen",
            "mcp-server",
            "--headless",
            "--mcp-http",
            "8765",
        ])
        .unwrap();
        match cli.command {
            Commands::Gen { args } => assert_eq!(
                args,
                [
                    OsString::from("mcp-server"),
                    OsString::from("--headless"),
                    OsString::from("--mcp-http"),
                    OsString::from("8765"),
                ]
            ),
            _ => panic!("expected gen"),
        }
    }
}
