#[cfg(feature = "audio")]
pub mod audio;
pub mod completion;
pub mod gen_cmd;
pub mod previs;
pub mod world;
pub mod world_api;
pub mod world_link;
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

    /// The optional audio models, which this build leaves out: it says where a build with them is
    #[cfg(not(feature = "audio"))]
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Audio {
        /// Whatever was asked of the audio models
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },

    /// Generate shell completion scripts
    Completion(completion::CompletionArgs),
}

/// What `localgpt audio` answers in a build without the `audio` feature,
/// which is what npm and a plain `cargo install localgpt` give: an agent
/// whose MCP configuration starts `localgpt audio mcp` reads this in place
/// of a server that never comes up. An Intel Mac has its own answer, since
/// no build has the models there and a `cargo install` asking for them fails.
#[cfg(not(feature = "audio"))]
pub const NO_AUDIO: &str = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    NO_AUDIO_ON_AN_INTEL_MAC
} else {
    NO_AUDIO_IN_THIS_BUILD
};

#[cfg(not(feature = "audio"))]
const NO_AUDIO_IN_THIS_BUILD: &str = "this localgpt was built without the audio models.

They run on ONNX Runtime, which has no build for an Intel Mac or for a Linux
older than glibc 2.38, so a build has them only when asked:

    cargo install localgpt --features audio

On an Apple-silicon Mac, the localgpt inside LocalGPT.app has them.
https://localgpt.app/docs/audio-models/#platforms";

#[cfg(not(feature = "audio"))]
const NO_AUDIO_ON_AN_INTEL_MAC: &str = "this localgpt was built without the audio models.

An Intel Mac cannot run them: they run on ONNX Runtime, which has no build
for one. Where they do run:
https://localgpt.app/docs/audio-models/#platforms";

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

    /// Without the feature, anything asked of `audio` still parses, flags
    /// and `--help` included, so every way in reaches the one answer: where
    /// a build with the models is.
    #[cfg(not(feature = "audio"))]
    #[test]
    fn audio_without_the_feature_takes_anything_and_says_where_it_is() {
        for asked in [
            &["localgpt", "audio"][..],
            &["localgpt", "audio", "mcp"],
            &["localgpt", "audio", "install", "--all"],
            &["localgpt", "audio", "--help"],
            &["localgpt", "audio", "notes", "a.wav", "--json"],
        ] {
            let cli = Cli::try_parse_from(asked).unwrap_or_else(|e| panic!("{asked:?}: {e}"));
            assert!(
                matches!(cli.command, Commands::Audio { .. }),
                "{asked:?} is not the audio command"
            );
        }
        // Both answers open with the words npm.yml's install job looks for
        // on every platform, and end at the page that says where they run.
        for said in [NO_AUDIO_IN_THIS_BUILD, NO_AUDIO_ON_AN_INTEL_MAC] {
            assert!(said.starts_with("this localgpt was built without the audio models."));
            assert!(said.ends_with("https://localgpt.app/docs/audio-models/#platforms"));
        }
        assert!(NO_AUDIO_IN_THIS_BUILD.contains("cargo install localgpt --features audio"));
        assert!(NO_AUDIO_IN_THIS_BUILD.contains("LocalGPT.app"));
        // Neither is to be had on an Intel Mac, so its answer names neither.
        assert!(!NO_AUDIO_ON_AN_INTEL_MAC.contains("cargo install"));
        assert!(!NO_AUDIO_ON_AN_INTEL_MAC.contains("LocalGPT.app"));
        let intel_mac = cfg!(all(target_os = "macos", target_arch = "x86_64"));
        assert_eq!(NO_AUDIO == NO_AUDIO_ON_AN_INTEL_MAC, intel_mac);
    }
}
