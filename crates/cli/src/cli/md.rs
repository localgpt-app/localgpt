//! CLI subcommand: `localgpt md`
//!
//! Reports on the workspace security policy (LocalGPT.md): whether it
//! exists, whether it passes sanitization, and device key presence.

use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_core::security;

#[derive(Args)]
pub struct MdArgs {
    #[command(subcommand)]
    pub command: MdCommands,
}

#[derive(Subcommand)]
pub enum MdCommands {
    /// Show current security posture
    Status,
}

pub async fn run(args: MdArgs) -> Result<()> {
    match args.command {
        MdCommands::Status => show_status().await,
    }
}

async fn show_status() -> Result<()> {
    let config = Config::load()?;
    let workspace = config.workspace_path();

    println!("Security Status:");

    // Policy file
    let policy_path = workspace.join(security::POLICY_FILENAME);
    if policy_path.exists() {
        match security::load_policy(&workspace) {
            Some(content) => {
                println!(
                    "  Policy:     {} (active, {} chars after sanitization)",
                    policy_path.display(),
                    content.len()
                );
            }
            None => {
                println!(
                    "  Policy:     {} (rejected by sanitization — see logs)",
                    policy_path.display()
                );
            }
        }
    } else {
        println!("  Policy:     Not created ({})", policy_path.display());
    }

    // Device key (used to encrypt bridge credentials)
    let key_path = config.paths.device_key();
    if key_path.exists() {
        println!("  Device Key: Present");
    } else {
        println!("  Device Key: Missing (run `localgpt init`)");
    }

    // Protected files
    println!(
        "  Protected:  {} workspace files, {} external paths",
        security::PROTECTED_FILES.len(),
        security::PROTECTED_EXTERNAL_PATHS.len()
    );

    Ok(())
}
