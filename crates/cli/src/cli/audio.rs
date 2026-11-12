//! `localgpt audio` — the optional audio-analysis models.
//!
//! Nothing is installed by default. `install` downloads a model's pinned
//! files into the shared models directory (the desktop app installs into
//! the same one), and from then on `world mcp` and `audio mcp` offer its tool
//! to an agent.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::Subcommand;
use localgpt_audio_models::tools::ToolContext;
use localgpt_audio_models::{self as models, MODELS, Model};
use localgpt_world_agent::mcp::run_mcp_stdio_server;

#[derive(Subcommand)]
pub enum AudioCommands {
    /// Every model: what it does, its download and licence, and whether it is installed
    List,

    /// Download models into the shared models directory, each file checked against its pinned SHA-256
    Install {
        /// Model ids, from `localgpt audio list`
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        /// Every model
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },

    /// Delete installed models
    Remove {
        /// Model ids, from `localgpt audio list`
        #[arg(required = true)]
        ids: Vec<String>,
    },

    /// Serve the installed models' tools over stdio MCP; what they make is written into the working directory
    Mcp,
}

pub async fn run(command: AudioCommands) -> Result<()> {
    let dir = models::models_dir().ok_or_else(|| {
        anyhow!(
            "no data directory here: set {}",
            models::install::LOCALGPT_AUDIO_MODELS
        )
    })?;
    match command {
        AudioCommands::List => {
            list(&dir);
            Ok(())
        }
        AudioCommands::Install { ids, all } => {
            for model in chosen(&ids, all)? {
                install(&dir, model).await?;
            }
            Ok(())
        }
        AudioCommands::Remove { ids } => {
            for model in chosen(&ids, false)? {
                models::remove(&dir, model)?;
                println!("removed {}", model.id);
            }
            Ok(())
        }
        AudioCommands::Mcp => {
            let here = std::env::current_dir()?;
            let tools = models::tools::installed(&ToolContext {
                models: dir,
                base: here.clone(),
                out: here,
            });
            if tools.is_empty() {
                bail!("no audio model is installed (localgpt audio list)");
            }
            run_mcp_stdio_server(tools, "localgpt-audio").await
        }
    }
}

/// The models `ids` name, or all of them.
fn chosen(ids: &[String], all: bool) -> Result<Vec<&'static Model>> {
    if all {
        return Ok(MODELS.to_vec());
    }
    ids.iter()
        .map(|id| {
            models::find(id).ok_or_else(|| {
                let known: Vec<_> = MODELS.iter().map(|m| m.id).collect();
                anyhow!("no model `{id}` (models: {})", known.join(", "))
            })
        })
        .collect()
}

fn list(dir: &Path) {
    if MODELS.is_empty() {
        println!("No audio models in this build.");
        return;
    }
    for model in MODELS {
        let state = if models::installed(dir, model) {
            "installed"
        } else {
            "not installed"
        };
        println!(
            "{:<7} {} — {} ({}, {})",
            model.id,
            model.name,
            model.does,
            megabytes(model.bytes()),
            state
        );
        println!("        {}", model.licence);
    }
    println!("\nModels live in {}", dir.display());
}

async fn install(dir: &Path, model: &Model) -> Result<()> {
    if models::installed(dir, model) {
        println!("{} is installed", model.id);
        return Ok(());
    }
    eprintln!(
        "installing {} ({}, {}) — {}",
        model.id,
        model.name,
        megabytes(model.bytes()),
        model.licence
    );
    let mut shown = u64::MAX;
    models::install(dir, model, |p| {
        let percent = p.done * 100 / p.total.max(1);
        if percent != shown {
            shown = percent;
            eprint!("\r  {percent:>3}%  {}", p.file);
            let _ = std::io::stderr().flush();
        }
    })
    .await?;
    eprintln!();
    println!("installed {} in {}", model.id, home(dir, model).display());
    Ok(())
}

fn home(dir: &Path, model: &Model) -> PathBuf {
    dir.join(model.id)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
