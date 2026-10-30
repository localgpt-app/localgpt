//! CLI subcommand for LocalGPT Gen — delegates to `localgpt-gen` binary.
//!
//! Gen mode lives in a separate binary (`localgpt-gen`) to avoid pulling
//! Bevy's 200+ transitive deps into the CLI. This module finds and spawns
//! that binary, passing through all arguments.

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct GenArgs {
    #[command(subcommand)]
    pub command: Option<GenCommands>,

    /// Initial prompt (interactive mode only)
    pub prompt: Option<String>,

    /// Load a glTF/GLB scene at startup
    #[arg(short = 's', long)]
    pub scene: Option<String>,
}

#[derive(Subcommand)]
pub enum GenCommands {
    /// Run as MCP server (stdio)
    McpServer,
    /// Control an external avatar (headless)
    Control {
        /// URL of the external app
        url: String,
        /// Initial prompt
        prompt: Option<String>,
    },
}

/// Spawn `localgpt-gen` with the translated arguments.
pub fn run(args: GenArgs, agent_id: &str, verbose: bool) -> Result<()> {
    let bin = which::which("localgpt-gen").map_err(|_| {
        anyhow::anyhow!(
            "localgpt-gen binary not found in PATH. \
             Install it with: cargo install --path crates/gen"
        )
    })?;

    let mut cmd = std::process::Command::new(bin);

    // Global flags
    cmd.arg("--agent").arg(agent_id);
    if verbose {
        cmd.arg("--verbose");
    }
    if let Some(ref scene) = args.scene {
        cmd.arg("--scene").arg(scene);
    }

    // Subcommand
    match args.command {
        Some(GenCommands::McpServer) => {
            cmd.arg("mcp-server");
        }
        Some(GenCommands::Control { url, prompt }) => {
            cmd.arg("control").arg(&url);
            if let Some(p) = prompt {
                cmd.arg(&p);
            }
        }
        None => {
            if let Some(ref prompt) = args.prompt {
                cmd.arg(prompt);
            }
        }
    }

    // Inherit stdio and exit with child's exit code
    let status = cmd
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}
