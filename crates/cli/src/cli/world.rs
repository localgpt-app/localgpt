//! CLI subcommand: `localgpt world` — the `.world` package's command line.
//!
//! A world is a folder: a manifest, an append-only log of the batches that
//! built it, hashed assets, and a guide an agent reads before changing it.
//! This is the authority for that folder when no app has it open — scriptable,
//! CI-able, and the way an agent or a shell commits a batch without a window.
//!
//! The commands are [`localgpt_world_agent::headless`], shared with the
//! `live` example, because `LiveWorld` is the one authority and a second
//! implementation of commit is the drift this workspace has paid for before.
//! Nothing but parsing and printing happens here.
//!
//! While an app answers at the folder's endpoint the world is that app's to
//! write, so a write refuses and says where to send it instead.

use std::io::Read as _;
use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use localgpt_world_agent::headless::{self, Command as WorldCommand};

#[derive(Subcommand)]
pub enum WorldCommands {
    /// Open a folder as a world package, creating one if it isn't yet
    Init {
        /// The world folder
        dir: PathBuf,
        /// Also make it a git repository, so every batch is a commit
        #[arg(long)]
        git: bool,
    },
    /// Commit a batch of edit ops — whole, or not at all
    Submit {
        /// The world folder
        dir: PathBuf,
        /// A JSON batch: `[op, …]` or `{"ops": [op, …]}`. `-` reads stdin
        #[arg(default_value = "-")]
        batch: String,
        /// Who to attribute the batch to
        #[arg(long, default_value = "cli")]
        author: String,
        /// What the batch is for — a commit message
        #[arg(long, short)]
        message: Option<String>,
    },
    /// Take back the newest edit, by appending its inverse
    Undo {
        /// The world folder
        dir: PathBuf,
        /// Whose edit to take back
        #[arg(long, default_value = "cli")]
        author: String,
    },
    /// Every committed batch, oldest first
    Log {
        /// The world folder
        dir: PathBuf,
    },
    /// Check that the fold still equals the head, and the assets their hashes
    Verify {
        /// The world folder
        dir: PathBuf,
    },
    /// Serve the open world's tools to any MCP agent (stdio)
    Mcp {
        /// The world folder. Defaults to the working directory
        dir: Option<PathBuf>,
    },
    /// The git history, when the world is in git
    History {
        /// The world folder
        dir: PathBuf,
        /// A branch or revision to walk instead of the current one
        rev: Option<String>,
    },
}

/// Run a `localgpt world` subcommand.
pub async fn run(command: WorldCommands) -> Result<()> {
    // The shim is not a command against a closed package: it serves the open
    // app's API, so it never reaches the headless authority.
    if let WorldCommands::Mcp { dir } = command {
        return crate::cli::world_mcp::run(dir).await;
    }
    let (dir, command) = translate(command)?;
    match headless::run(&dir, command) {
        Ok(report) => {
            for line in report.lines {
                println!("{line}");
            }
            Ok(())
        }
        // A refusal is the agent-facing contract — a reason per problem, so
        // whatever sent the batch can correct itself — and an exit code, so a
        // script notices. Not an anyhow chain.
        Err(e @ headless::HeadlessError::Refused(_))
        | Err(e @ headless::HeadlessError::Invalid(_)) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

/// Clap's shape into the authority's.
fn translate(command: WorldCommands) -> Result<(PathBuf, WorldCommand)> {
    Ok(match command {
        WorldCommands::Init { dir, git } => (dir, WorldCommand::Init { git }),
        WorldCommands::Submit {
            dir,
            batch,
            author,
            message,
        } => {
            let text = if batch == "-" {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else if std::path::Path::new(&batch).is_file() {
                std::fs::read_to_string(&batch)?
            } else {
                // A batch given inline, so `localgpt world submit w '[{...}]'`
                // works without a temporary file.
                batch.clone()
            };
            (
                dir,
                WorldCommand::Submit {
                    batch: serde_json::from_str(&text)
                        .map_err(|e| anyhow::anyhow!("the batch isn't JSON: {e}"))?,
                    author,
                    message,
                },
            )
        }
        WorldCommands::Undo { dir, author } => (dir, WorldCommand::Undo { author }),
        WorldCommands::Log { dir } => (dir, WorldCommand::Log),
        WorldCommands::Verify { dir } => (dir, WorldCommand::Verify),
        WorldCommands::History { dir, rev } => (dir, WorldCommand::History { rev }),
        WorldCommands::Mcp { .. } => unreachable!("handled before translate"),
    })
}
