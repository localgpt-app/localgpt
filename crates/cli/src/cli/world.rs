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
use localgpt_world_agent::live::LiveWorld;
use localgpt_world_editor::{Editor, History};

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
    /// Every branch end in the history — what a branch rail would draw
    Tips {
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
    // Tips and ids are the *view's* vocabulary, so they are composed here —
    // the authority (`world-agent`) deliberately does not depend on its
    // viewers, and `world-editor::History` is the workspace's one
    // implementation of the format's id rule.
    match &command {
        WorldCommands::Tips { dir } => {
            for line in tips_lines(dir)? {
                println!("{line}");
            }
            return Ok(());
        }
        WorldCommands::Log { dir } => {
            for line in log_lines(dir)? {
                println!("{line}");
            }
            return Ok(());
        }
        _ => {}
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

/// Every branch end, with its revision, the head marked.
///
/// "Head" is the format's: the last entry in file order, which is what a
/// fold names when no tip is given. A CLI has no canvas, so there is no
/// "current" beyond that.
fn tips_lines(dir: &std::path::Path) -> Result<Vec<String>> {
    let live = LiveWorld::open(dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let view = Editor::open(live.base().clone(), live.entries().to_vec())
        .map_err(|e| anyhow::anyhow!("the history doesn't open as a tree: {e}"))?;
    let tips = view.tips();
    if tips.is_empty() {
        return Ok(vec![
            "no history yet — the base is the whole world".to_string(),
        ]);
    }
    let head = view.tip();
    Ok(tips
        .iter()
        .map(|tip| {
            let revision = view
                .history()
                .get(tip)
                .map(|e| e.entry.revision)
                .unwrap_or(0);
            let marker = if Some(*tip) == head { "  ← head" } else { "" };
            format!("{tip}  r{revision}{marker}")
        })
        .collect())
}

/// The authority's log, each line prefixed with the entry's id — the name a
/// tip, a `POST /goto` and a person all use for a point in the history.
///
/// The lines are `headless`'s own (one per entry, in file order) and the ids
/// come from `History` by the same index, so there is one formatter and one
/// copy of the id rule. A log that will not open as a tree — a dangling
/// parent, say — still prints, without ids: reading history should degrade,
/// not fail.
fn log_lines(dir: &std::path::Path) -> Result<Vec<String>> {
    let report = headless::run(dir, WorldCommand::Log).map_err(|e| anyhow::anyhow!("{e}"))?;
    let live = LiveWorld::open(dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let Ok(history) = History::open(live.entries().to_vec()) else {
        return Ok(report.lines);
    };
    Ok(report
        .lines
        .into_iter()
        .enumerate()
        .map(|(n, line)| match history.id_at(n) {
            Some(id) => format!("{id} {line}"),
            None => line,
        })
        .collect())
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
        WorldCommands::Mcp { .. } | WorldCommands::Tips { .. } => {
            unreachable!("handled before translate")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lg-world-cli-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        headless::run(&dir, WorldCommand::Init { git: false }).expect("init");
        dir
    }

    fn submit(dir: &std::path::Path, name: &str) {
        headless::run(
            dir,
            WorldCommand::Submit {
                batch: json!({"ops": [{"SpawnEntity": {"entity": {"name": name}}}]}),
                author: "test".into(),
                message: None,
            },
        )
        .expect(name);
    }

    #[test]
    fn an_empty_history_says_the_base_is_the_world() {
        let dir = package("empty");
        let lines = tips_lines(&dir).unwrap();
        assert_eq!(lines, ["no history yet — the base is the whole world"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn first_word(line: &str) -> &str {
        line.split_whitespace().next().unwrap_or_default()
    }

    #[test]
    fn a_linear_history_has_one_tip_and_it_is_the_head() {
        let dir = package("linear");
        submit(&dir, "lighthouse");
        submit(&dir, "jetty");
        let lines = tips_lines(&dir).unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains(" r2"), "{lines:?}");
        assert!(lines[0].ends_with("← head"), "{lines:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_log_names_each_entry_by_the_id_a_tip_would_use() {
        let dir = package("log");
        submit(&dir, "lighthouse");
        submit(&dir, "jetty");
        let lines = log_lines(&dir).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        // The authority writes the format's content hash as each new
        // entry's id — `sha256:` and 64 hex characters — different per entry.
        for line in &lines {
            let id = first_word(line);
            let hex = id
                .strip_prefix("sha256:")
                .unwrap_or_else(|| panic!("{line}"));
            assert_eq!(hex.len(), 64, "{line}");
            assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{line}");
        }
        assert_ne!(first_word(&lines[0]), first_word(&lines[1]));
        // The id a tip reports is the id the log shows for the same entry.
        let tip = tips_lines(&dir).unwrap();
        assert_eq!(
            first_word(&tip[0]),
            first_word(&lines[1]),
            "{tip:?} vs {lines:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
