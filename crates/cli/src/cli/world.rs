//! CLI subcommand: `localgpt world` — the `.world` package's command line.
//!
//! A world is a folder: a manifest, an append-only log of the batches that
//! built it, hashed assets, and a guide an agent reads before changing it.
//! These commands change it and read it, whoever holds it:
//!
//! - **An app has it open** — something answers at the folder's endpoint —
//!   and every command is a call to that app's API ([`super::world_api`]):
//!   the app is the authority, so it commits, and what only a window knows
//!   (what is on screen, the selection, a screenshot, a batch waiting for
//!   review) comes from it. The CLI never opens the package then: opening
//!   one is what an authority does, and one authority at a time is the rule
//!   the live-editing design rests on.
//! - **Nothing has it open**, and the commands are the authority:
//!   [`localgpt_world_agent::headless`], shared with the `live` example,
//!   because `LiveWorld` is the one authority and a second implementation of
//!   commit is the drift this workspace has paid for before.
//!
//! Both hand back the same receipt for a commit, so a command prints the
//! same lines whoever made it. This is what an agent runs instead of the
//! API: no token in a command line, no JSON quoted for a shell, short lines
//! unless `--json`, and one prefix to allow.
//!
//! The folder is the first argument when it's given — a first argument that
//! isn't a world is the command's own; else `$LOCALGPT_WORLD_DIR`, which an
//! agent's tab in the app has; else the folder the shell is in, or the
//! nearest one above it that is a world.

use std::collections::{BTreeSet, HashMap};
use std::io::{IsTerminal as _, Read as _};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use localgpt_world_agent::agent_cli::{AUTHOR_ENV, WORLD_DIR_ENV};
use localgpt_world_agent::headless::{self, Command as WorldCommand, HeadlessError};
use localgpt_world_agent::live::{self, LiveWorld};
use localgpt_world_editor::{Editor, History};
use serde_json::{Value, json};

use super::world_api::{Answer, Api};

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
    ///
    /// The batch is JSON, `[op, …]` or `{"ops": [op, …]}`: given inline, as a
    /// file, or on stdin (`-`, the default), where a heredoc spares it the
    /// shell's quoting. The world's AGENTS.md describes the ops.
    Submit {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        /// The batch: JSON, a file holding it, or `-` for stdin
        batch: Option<String>,
        /// Who the batch is by
        #[arg(long, env = AUTHOR_ENV, default_value = "cli")]
        author: String,
        /// What the batch is for — a commit message
        #[arg(long, short)]
        message: Option<String>,
        /// The entry to build on (an id from `log` or `tips`) instead of the
        /// head. An earlier entry starts a branch; the head stays where it is
        #[arg(long)]
        at: Option<String>,
        /// The revision you read the world at: if it has moved since, the
        /// batch is refused, naming both
        #[arg(long, value_name = "REVISION")]
        expected_revision: Option<u64>,
        #[command(flatten)]
        out: Out,
    },
    /// Take back the newest edit, by appending its inverse
    Undo {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        take_back: TakeBack,
        #[command(flatten)]
        out: Out,
    },
    /// Take the newest undo back, by appending its inverse
    Redo {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        take_back: TakeBack,
        #[command(flatten)]
        out: Out,
    },
    /// Every committed batch, oldest first, with ids — the head marked
    Log {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        /// Only the newest N entries
        #[arg(long, value_name = "N")]
        last: Option<usize>,
        #[command(flatten)]
        out: Out,
    },
    /// Every branch end in the history — what a branch rail would draw
    Tips {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        out: Out,
    },
    /// The world at an entry, a line per entity (`get` for its fields)
    Show {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        /// The entry: an id, `head` (the default), `current` (what the
        /// window shows) or `base`
        #[arg(long, value_name = "ENTRY")]
        at: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// Entities by name or id, as the world holds them: what a patch changes
    ///
    /// One JSON object a line, without the fields an entity hasn't (null).
    /// The world folder may come first.
    Get {
        /// Names, or ids as `#12`
        #[arg(required = true, value_name = "ENTITY")]
        names: Vec<String>,
        /// The entry: an id, `head` (the default), `current` or `base`
        #[arg(long, value_name = "ENTRY")]
        at: Option<String>,
    },
    /// Check that the fold still equals the head, and the assets their hashes
    Verify {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        out: Out,
    },
    /// Render the open app's view, and print the picture's path
    Screenshot {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        out: Out,
    },
    /// What the person has selected in the open app — what "this" means
    Selection {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        out: Out,
    },
    /// Show an entry in the open app's window; nothing is written
    Goto {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        /// The entry: an id from `log` or `tips`, `head`, or `base`
        tip: Option<String>,
        #[command(flatten)]
        out: Out,
    },
    /// The batches sent while review is on in the open app, and what became of them
    Proposals {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        #[command(flatten)]
        out: Out,
    },
    /// Serve the open world's tools to any MCP agent (stdio)
    Mcp {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
    },
    /// The git history, when the world is in git
    History {
        /// The world folder; else $LOCALGPT_WORLD_DIR, else the one you're in
        dir: Option<PathBuf>,
        /// A branch or revision to walk instead of the current one
        rev: Option<String>,
    },
}

/// What `undo` and `redo` take back.
#[derive(Args)]
pub struct TakeBack {
    /// Who is taking it back
    #[arg(long, env = AUTHOR_ENV, default_value = "cli")]
    author: String,
    /// On the path to this entry instead of the head's; off main it is a
    /// branch commit, and the head stays
    #[arg(long)]
    at: Option<String>,
    /// The author's own newest, skipping others'
    #[arg(long)]
    own: bool,
}

#[derive(Args, Clone, Copy, Default)]
pub struct Out {
    /// Print the reply as JSON, the shape the open app's API gives
    #[arg(long)]
    json: bool,
}

/// Run a `localgpt world` subcommand.
pub async fn run(command: WorldCommands) -> Result<()> {
    // The shim serves the open app's tools for as long as the agent runs.
    if let WorldCommands::Mcp { dir } = command {
        return crate::cli::world_mcp::run(dir).await;
    }
    let said = say(command).await?;
    for line in &said.out {
        println!("{line}");
    }
    for line in &said.err {
        eprintln!("{line}");
    }
    // A refusal is the agent-facing contract — a reason per problem, so
    // whatever sent the batch can correct itself — and an exit code, so a
    // script notices. Not an anyhow chain.
    if said.failed {
        std::process::exit(1);
    }
    Ok(())
}

/// What a command prints, and whether it failed: a refusal or a problem
/// goes to stderr and the exit is non-zero.
#[derive(Debug, Default, PartialEq)]
struct Said {
    out: Vec<String>,
    err: Vec<String>,
    failed: bool,
}

impl Said {
    fn out(out: Vec<String>) -> Self {
        Self {
            out,
            ..Self::default()
        }
    }

    fn failed(err: Vec<String>) -> Self {
        Self {
            err,
            failed: true,
            ..Self::default()
        }
    }

    /// A reply as JSON on stdout — a refusal's too, so a reader parses one
    /// stream — failing when it is one.
    fn json(value: &Value, failed: bool) -> Self {
        Self {
            out: vec![value.to_string()],
            err: Vec::new(),
            failed,
        }
    }
}

async fn say(command: WorldCommands) -> Result<Said> {
    use WorldCommands as C;
    Ok(match command {
        C::Mcp { .. } => unreachable!("served by run"),
        C::Init { dir, git } => match Api::holding(&dir) {
            // Opening the package is the authority's; it has one.
            Some(api) => Said::out(vec![format!(
                "{} is live already, open in an app at {}",
                dir.display(),
                api.url
            )]),
            None => headless_said(&dir, WorldCommand::Init { git })?,
        },
        C::History { dir, rev } => {
            // Git's history, read from git: no authority is opened.
            let (dir, rev) = folder_first(dir, rev);
            headless_said(&world_dir(dir)?, WorldCommand::History { rev })?
        }
        C::Submit {
            dir,
            batch,
            author,
            message,
            at,
            expected_revision,
            out,
        } => {
            let (dir, batch) = folder_first(dir, batch);
            let dir = world_dir(dir)?;
            let batch = read_batch(batch.as_deref().unwrap_or("-"))?;
            let batch = envelope(batch, &author, message, at, expected_revision)?;
            match Api::holding(&dir) {
                Some(api) => answered(api.post("/ops", batch).await?, out, receipt_lines),
                None => committed(
                    headless::run(
                        &dir,
                        WorldCommand::Submit {
                            batch,
                            author,
                            message: None,
                            at: None,
                        },
                    ),
                    out,
                )?,
            }
        }
        C::Undo {
            dir,
            take_back,
            out,
        } => take_back_said(&world_dir(dir)?, take_back, false, out).await?,
        C::Redo {
            dir,
            take_back,
            out,
        } => take_back_said(&world_dir(dir)?, take_back, true, out).await?,
        C::Log { dir, last, out } => {
            let dir = world_dir(dir)?;
            let log = match Api::holding(&dir) {
                Some(api) => match api.get("/log", &[]).await? {
                    answer if answer.ok() => answer.body,
                    answer => return Ok(answered(answer, out, |_| Vec::new())),
                },
                None => offline_log(&dir)?,
            };
            if out.json {
                Said::json(&log, false)
            } else {
                Said::out(log_lines(&log, last))
            }
        }
        C::Tips { dir, out } => {
            let dir = world_dir(dir)?;
            let (tips, log) = match Api::holding(&dir) {
                Some(api) => {
                    let tips = api.get("/tips", &[]).await?;
                    if !tips.ok() {
                        return Ok(answered(tips, out, |_| Vec::new()));
                    }
                    // The revisions are the log's: a tip is named by its id.
                    (tips.body, api.get("/log", &[]).await?.body)
                }
                None => (offline_tips(&dir)?, offline_log(&dir)?),
            };
            if out.json {
                Said::json(&tips, false)
            } else {
                Said::out(tips_lines(&tips, &log))
            }
        }
        C::Show { dir, at, out } => {
            let (world, which) = world_at(&world_dir(dir)?, at.as_deref()).await?;
            if out.json {
                Said::json(&world, false)
            } else {
                Said::out(outline(&world, &which))
            }
        }
        C::Get { names, at } => {
            // The world folder first, when it's given: a first name that is
            // a world, with names after it.
            let mut names = names;
            let dir = (names.len() > 1 && live::is_package(Path::new(&names[0])))
                .then(|| PathBuf::from(names.remove(0)));
            let (world, which) = world_at(&world_dir(dir)?, at.as_deref()).await?;
            picked(&world, &names, &which)
        }
        C::Verify { dir, out } => {
            let dir = world_dir(dir)?;
            match Api::holding(&dir) {
                Some(api) => {
                    let answer = api.get("/verify", &[]).await?;
                    let ok = answer.body["ok"] == true;
                    if out.json {
                        Said::json(&answer.body, !ok)
                    } else if ok {
                        Said::out(vec![
                            "ok: the open app's fold equals manifest.json, and every asset matches its hash"
                                .into(),
                        ])
                    } else {
                        Said::failed(refusal_lines(&answer.body))
                    }
                }
                None => match headless::run(&dir, WorldCommand::Verify) {
                    Ok(_) if out.json => Said::json(&json!({"ok": true, "problems": []}), false),
                    Ok(report) => Said::out(report.lines),
                    Err(e) => refused(e, out)?,
                },
            }
        }
        C::Screenshot { dir, out } => {
            let dir = world_dir(dir)?;
            match Api::holding(&dir) {
                Some(api) => answered(api.get("/screenshot", &[]).await?, out, screenshot_lines),
                None => needs_app("a screenshot", &dir),
            }
        }
        C::Selection { dir, out } => {
            let dir = world_dir(dir)?;
            let Some(api) = Api::holding(&dir) else {
                return Ok(needs_app("the selection", &dir));
            };
            let answer = api.get("/selection", &[]).await?;
            if out.json || !answer.ok() {
                answered(answer, out, |_| Vec::new())
            } else {
                // The rest of a selection arrives as ids; the world on
                // screen names them.
                let shown = api.get("/world", &[("tip", "current")]).await?;
                Said::out(selection_lines(&answer.body, &shown.body))
            }
        }
        C::Goto { dir, tip, out } => {
            let (dir, tip) = folder_first(dir, tip);
            let Some(tip) = tip else {
                bail!("name the entry to show: an id from `log` or `tips`, `head`, or `base`");
            };
            let dir = world_dir(dir)?;
            let Some(api) = Api::holding(&dir) else {
                return Ok(needs_app("goto", &dir));
            };
            let tip = match tip.as_str() {
                "base" | "null" => Value::Null,
                tip => json!(tip),
            };
            answered(
                api.post("/goto", json!({"tip": tip})).await?,
                out,
                goto_lines,
            )
        }
        C::Proposals { dir, out } => {
            let dir = world_dir(dir)?;
            match Api::holding(&dir) {
                Some(api) => answered(api.get("/proposals", &[]).await?, out, proposal_lines),
                None => needs_app("review", &dir),
            }
        }
    })
}

/// The world a command acts on: the folder given; else
/// `$LOCALGPT_WORLD_DIR`, which an agent's tab in the app has; else the
/// folder the shell is in, or the nearest one above it that is a world.
pub fn world_dir(given: Option<PathBuf>) -> Result<PathBuf> {
    let here = std::env::current_dir()?;
    resolve(
        given,
        std::env::var_os(WORLD_DIR_ENV).map(PathBuf::from),
        &here,
    )
}

fn resolve(given: Option<PathBuf>, env: Option<PathBuf>, here: &Path) -> Result<PathBuf> {
    if let Some(dir) = given.or(env.filter(|dir| !dir.as_os_str().is_empty())) {
        return Ok(dir);
    }
    // Looking up from `assets/` finds the world. It takes the package's own
    // snapshot to count — a `manifest.json` alone is also a web project's,
    // and opening a folder as a world writes to it.
    here.ancestors()
        .find(|dir| live::is_package(dir) && dir.join(live::BASE).is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow!(
                "no world here: name its folder, or run this inside one \
                 (neither {} nor a folder above it is a world package)",
                here.display()
            )
        })
}

/// The world folder comes first when it's given, so a first argument that
/// isn't a world, with nothing after it, is the command's own: a batch, a
/// revision, an entry.
fn folder_first(dir: Option<PathBuf>, next: Option<String>) -> (Option<PathBuf>, Option<String>) {
    match (dir, next) {
        (Some(dir), None) if !live::is_package(&dir) => {
            (None, Some(dir.to_string_lossy().into_owned()))
        }
        other => other,
    }
}

/// `-` reads stdin, a file is read, and anything else is the JSON itself,
/// so `localgpt world submit '[{...}]'` works without a temporary file.
fn read_batch(arg: &str) -> Result<Value> {
    const GIVE: &str =
        "give the batch: JSON, a file holding it, or on stdin (a heredoc: <<'EOF' … EOF)";
    let path = Path::new(arg);
    let text = if arg == "-" {
        if std::io::stdin().is_terminal() {
            bail!("{GIVE}");
        }
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        if s.trim().is_empty() {
            bail!("no batch came on stdin: {GIVE}");
        }
        s
    } else if path.is_file() {
        std::fs::read_to_string(path)?
    } else if path.is_dir() {
        bail!("{arg} is a folder, but not a world: it has no manifest.json");
    } else {
        arg.to_string()
    };
    serde_json::from_str(&text).map_err(|e| anyhow!("the batch isn't JSON: {e}"))
}

/// The batch as the API takes it, `{"ops": […], …}`, with what the flags
/// say wherever the batch doesn't say it itself: a batch's own author,
/// message, `at` and `expected_revision` win, as they do at the authority.
fn envelope(
    batch: Value,
    author: &str,
    message: Option<String>,
    at: Option<String>,
    expected_revision: Option<u64>,
) -> Result<Value> {
    let mut body = match batch {
        Value::Array(ops) => json!({ "ops": ops }),
        object @ Value::Object(_) => object,
        _ => bail!("a batch is [op, …] or {{\"ops\": [op, …]}}"),
    };
    let flags = [
        ("author", Some(json!(author))),
        ("message", message.map(Value::from)),
        ("at", at.map(Value::from)),
        ("expected_revision", expected_revision.map(Value::from)),
    ];
    for (key, value) in flags {
        if let Some(value) = value
            && body.get(key).is_none()
        {
            body[key] = value;
        }
    }
    Ok(body)
}

async fn take_back_said(dir: &Path, take_back: TakeBack, redo: bool, out: Out) -> Result<Said> {
    let TakeBack { author, at, own } = take_back;
    match Api::holding(dir) {
        Some(api) => {
            let mut body = json!({ "author": author });
            if let Some(at) = at {
                body["at"] = json!(at);
            }
            if own {
                body["own"] = json!(true);
            }
            let path = if redo { "/redo" } else { "/undo" };
            Ok(answered(api.post(path, body).await?, out, receipt_lines))
        }
        None => {
            let command = if redo {
                WorldCommand::Redo { author, at, own }
            } else {
                WorldCommand::Undo { author, at, own }
            };
            committed(headless::run(dir, command), out)
        }
    }
}

/// The open app's answer: its lines when it did it, its reasons when not.
fn answered(answer: Answer, out: Out, lines: fn(&Value) -> Vec<String>) -> Said {
    match (out.json, answer.ok()) {
        (true, ok) => Said::json(&answer.body, !ok),
        (false, true) => Said::out(lines(&answer.body)),
        (false, false) => Said::failed(refusal_lines(&answer.body)),
    }
}

/// A headless commit, said as the open app's would be.
fn committed(result: Result<headless::Report, HeadlessError>, out: Out) -> Result<Said> {
    match result {
        Ok(report) => {
            let receipt = report.receipt.unwrap_or_default();
            Ok(if out.json {
                Said::json(&receipt, false)
            } else {
                Said::out(receipt_lines(&receipt))
            })
        }
        Err(e) => refused(e, out),
    }
}

/// A headless refusal in the open app's shape, so it reads the same.
fn refused(e: HeadlessError, out: Out) -> Result<Said> {
    let body = match e {
        HeadlessError::Refused(errors) => json!({"result": "rejected", "errors": errors}),
        HeadlessError::Invalid(problems) => json!({"ok": false, "problems": problems}),
        // An app took the folder between the look and the write.
        e @ HeadlessError::Open { .. } => json!({"result": "rejected", "errors": [e.to_string()]}),
        HeadlessError::Failed(e) => bail!("{e}"),
    };
    Ok(if out.json {
        Said::json(&body, true)
    } else {
        Said::failed(refusal_lines(&body))
    })
}

fn headless_said(dir: &Path, command: WorldCommand) -> Result<Said> {
    match headless::run(dir, command) {
        Ok(report) => Ok(Said::out(report.lines)),
        Err(e) => refused(e, Out::default()),
    }
}

/// What only a window has, asked of a world no window holds.
fn needs_app(what: &str, dir: &Path) -> Said {
    Said::failed(vec![format!(
        "{what} needs the app, and nothing has {} open — open it with: localgpt-app --world {}",
        dir.display(),
        dir.display()
    )])
}

/// The world at an entry and what to call that entry: from the app when it
/// holds the world, else folded here.
async fn world_at(dir: &Path, at: Option<&str>) -> Result<(Value, String)> {
    let which = match at {
        None | Some("head") => "the head".to_string(),
        Some("current") => "the entry on screen".to_string(),
        Some("base" | "null") => "the base".to_string(),
        Some(id) => format!("entry {}", short(id)),
    };
    if let Some(api) = Api::holding(dir) {
        let tip = match at {
            None | Some("head") => None,
            Some("base") => Some("null"),
            Some(tip) => Some(tip),
        };
        let query: Vec<(&str, &str)> = tip.map(|tip| ("tip", tip)).into_iter().collect();
        let answer = api.get("/world", &query).await?;
        if !answer.ok() {
            bail!("{}", refusal_lines(&answer.body).join("\n"));
        }
        return Ok((answer.body, which));
    }
    let live = LiveWorld::open(dir).map_err(|e| anyhow!("{e}"))?;
    let manifest = match at {
        // No window, so nothing is on screen but the head.
        None | Some("head" | "current") => live.head_manifest().clone(),
        Some("base" | "null") => live.base().to_manifest(),
        Some(id) => live.manifest_at(id).map_err(|e| anyhow!("{e}"))?,
    };
    // Through text, as the API does, so an f32 reads as manifest.json has it.
    let world = serde_json::from_str(&serde_json::to_string(&manifest)?)?;
    let which = if at == Some("current") {
        "the head".to_string()
    } else {
        which
    };
    Ok((world, which))
}

/// The log in the API's shape, read from the folder.
fn offline_log(dir: &Path) -> Result<Value> {
    let live = LiveWorld::open(dir).map_err(|e| anyhow!("{e}"))?;
    // A log that will not open as a tree — a dangling parent, say — still
    // prints, without ids: reading history should degrade, not fail.
    let history = History::open(live.entries().to_vec()).ok();
    let entries: Vec<Value> = live
        .entries()
        .iter()
        .enumerate()
        .map(|(n, entry)| {
            let id = history.as_ref().and_then(|h| h.id_at(n));
            json!({
                "id": id,
                "parent": id.and_then(|id| history.as_ref()?.parent_of(id)),
                "revision": entry.revision,
                "author": entry.author.name,
                "message": live::message_of(entry),
                "changes": live::describe_ops(&entry.edit_ops()),
            })
        })
        .collect();
    Ok(json!({
        "revision": live.revision(),
        "head": live.main_tip(),
        "entries": entries,
    }))
}

/// The tips in the API's shape, read from the folder. Tips and ids are the
/// *view's* vocabulary, so they are composed here: the authority
/// (`world-agent`) deliberately does not depend on its viewers.
fn offline_tips(dir: &Path) -> Result<Value> {
    let live = LiveWorld::open(dir).map_err(|e| anyhow!("{e}"))?;
    let view = Editor::open(live.base().clone(), live.entries().to_vec())
        .map_err(|e| anyhow!("the history doesn't open as a tree: {e}"))?;
    Ok(json!({
        "tips": view.tips(),
        "head": live.main_tip(),
        "revision": live.revision(),
    }))
}

/// One line an entry, oldest first, each prefixed with the entry's id — the
/// name a tip, `--at` and `goto` all use for a point in the history — and
/// the head marked, and what the window shows when one does.
fn log_lines(log: &Value, last: Option<usize>) -> Vec<String> {
    let entries = log["entries"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if entries.is_empty() {
        return vec!["no history yet — the base is the whole world".into()];
    }
    let head = log["head"].as_str();
    let current = log["current"].as_str();
    let mut lines: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(n, entry)| {
            let id = entry["id"].as_str();
            let what = match entry["message"].as_str().filter(|m| !m.is_empty()) {
                Some(message) => format!("\"{message}\" {}", joined(&entry["changes"])),
                None => joined(&entry["changes"]),
            };
            let mut line = format!(
                "r{:<3} {:<10} {what}",
                entry["revision"].as_u64().unwrap_or_default(),
                entry["author"].as_str().unwrap_or("?")
            );
            if let Some(id) = id {
                line = format!("{id} {line}");
            }
            // File order is not the chain once a log branches: say what an
            // entry builds on wherever it is not the line above.
            if let Some(parent) = entry["parent"].as_str()
                && n > 0
                && entries[n - 1]["id"].as_str() != Some(parent)
            {
                line += &format!("  (builds on {})", short(parent));
            }
            if id.is_some() && id == head {
                line += "  ← head";
            }
            if id.is_some() && id == current {
                line += "  ← on screen";
            }
            line
        })
        .collect();
    if let Some(n) = last {
        lines.drain(..lines.len().saturating_sub(n));
    }
    lines
}

/// Every branch end, with its revision, the head marked — and, when a window
/// shows the world, the entry it shows.
///
/// "Head" is the format's: `refs.main`, else the last entry — the
/// authority's `main_tip`, since once a branch is committed the last line is
/// a branch's.
fn tips_lines(tips: &Value, log: &Value) -> Vec<String> {
    let ids: Vec<&str> = tips["tips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if ids.is_empty() {
        return vec!["no history yet — the base is the whole world".into()];
    }
    let revision = |id: &str| {
        log["entries"]
            .as_array()
            .and_then(|entries| entries.iter().find(|e| e["id"] == id))
            .map_or(0, |e| e["revision"].as_u64().unwrap_or(0))
    };
    let head = tips["head"].as_str();
    // Only an app's answer has `current`: a CLI has no canvas.
    let current = tips.get("current");
    let on_screen = current.and_then(Value::as_str);
    let mut lines: Vec<String> = ids
        .iter()
        .map(|&tip| {
            let mut line = format!("{tip}  r{}", revision(tip));
            if Some(tip) == head {
                line += "  ← head";
            }
            if Some(tip) == on_screen {
                line += "  ← on screen";
            }
            line
        })
        .collect();
    match (current, on_screen) {
        (Some(Value::Null), _) => lines.push("on screen: the base".into()),
        // The person has stepped back to an entry that is no branch's end.
        (_, Some(shown)) if !ids.contains(&shown) => {
            lines.push(format!("on screen: {shown}  r{}", revision(shown)))
        }
        _ => {}
    }
    lines
}

/// The world in a line per entity: its id and name, what it is, where, and
/// what it's in.
fn outline(world: &Value, which: &str) -> Vec<String> {
    let entities = world["entities"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let names: HashMap<u64, &str> = entities
        .iter()
        .filter_map(|e| Some((e["id"].as_u64()?, e["name"].as_str()?)))
        .collect();
    let mut lines = vec![format!(
        "{}, at {which}: {} {}",
        world["meta"]["name"].as_str().unwrap_or("this world"),
        entities.len(),
        if entities.len() == 1 {
            "entity"
        } else {
            "entities"
        }
    )];
    if let Some(song) = world["soundtrack"].as_object() {
        let mut line = match song.get("title").and_then(Value::as_str) {
            Some(title) => format!("song: \"{title}\""),
            None => format!("song: {}", song.get("path").map_or("untitled".into(), text)),
        };
        if let Some(artist) = song.get("artist").and_then(Value::as_str) {
            line += &format!(" by {artist}");
        }
        if let Some(duration) = song.get("duration") {
            line += &format!(", {} s", number(duration));
        }
        lines.push(line);
    }
    for entity in entities {
        lines.push(entity_line(entity, &names));
    }
    lines
}

fn entity_line(entity: &Value, names: &HashMap<u64, &str>) -> String {
    let mut line = format!(
        "#{} {}",
        entity["id"],
        entity["name"].as_str().unwrap_or_default()
    );
    let kind = kind_of(entity);
    if !kind.is_empty() {
        line += &format!("  {kind}");
    }
    if let Some(at) = entity["transform"]["position"].as_array() {
        let at: Vec<String> = at.iter().map(number).collect();
        line += &format!("  at ({})", at.join(", "));
    }
    if let Some(parent) = entity["parent"].as_u64() {
        match names.get(&parent) {
            Some(name) => line += &format!("  in {name}"),
            None => line += &format!("  in #{parent}"),
        }
    }
    if entity["transform"]["visible"] == false {
        line += "  hidden";
    }
    let behaviors: Vec<&str> = entity["behaviors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| b.as_object()?.keys().next().map(String::as_str))
        .collect();
    if !behaviors.is_empty() {
        line += &format!("  · {}", behaviors.join(", "));
    }
    line
}

/// What an entity is: a shot or a camera, its shape or mesh, a light, a
/// sound — whichever it has.
fn kind_of(entity: &Value) -> String {
    let mut parts = Vec::new();
    let cinematography = &entity["ext-cinematography"];
    if cinematography["shot"].is_object() {
        parts.push("shot".to_string());
    } else if cinematography["camera"].is_object() {
        parts.push("camera".to_string());
    }
    if let Some(shape) = entity["shape"].as_object().and_then(|s| s.keys().next()) {
        parts.push(shape.clone());
    }
    match &entity["mesh_asset"] {
        Value::String(mesh) => parts.push(format!("mesh {mesh}")),
        Value::Object(_) => parts.push("mesh".to_string()),
        _ => {}
    }
    if let Some(light) = entity["light"].as_object() {
        let kind = light
            .get("light_type")
            .and_then(Value::as_str)
            .unwrap_or("a");
        parts.push(format!("{kind} light"));
    }
    if entity["audio"].is_object() {
        parts.push("sound".to_string());
    }
    parts.join(" + ")
}

/// The entities `names` pick — by name, or by id as `#12` or `12` — one JSON
/// object a line, each once; a name nothing has is a failure, said.
fn picked(world: &Value, names: &[String], which: &str) -> Said {
    let entities = world["entities"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut said = Said::default();
    let mut seen = BTreeSet::new();
    for name in names {
        let id = name.strip_prefix('#').unwrap_or(name).parse::<u64>().ok();
        let found: Vec<&Value> = entities
            .iter()
            .filter(|e| e["name"] == name.as_str() || (id.is_some() && e["id"].as_u64() == id))
            .collect();
        if found.is_empty() {
            said.err.push(format!("nothing called {name} at {which}"));
            said.failed = true;
        }
        for entity in found {
            if seen.insert(entity["id"].as_u64()) {
                // A field that is null is one the entity hasn't: left out,
                // it is half the line.
                let mut entity = entity.clone();
                if let Some(fields) = entity.as_object_mut() {
                    fields.retain(|_, value| !value.is_null());
                }
                said.out.push(entity.to_string());
            }
        }
    }
    said
}

/// A commit, or a proposal, from the receipt either authority hands back
/// (`live::receipt`), with what is worth knowing indented under it.
fn receipt_lines(receipt: &Value) -> Vec<String> {
    let changes = joined(&receipt["changes"]);
    let mut lines = match receipt["result"].as_str() {
        Some("proposed") => vec![format!(
            "proposed #{}: {changes} — it waits on the canvas for the person (`localgpt world proposals`)",
            receipt["proposal"]
        )],
        _ if receipt["branch"] == true => vec![
            format!(
                "committed revision {} on a branch: {changes}",
                receipt["revision"]
            ),
            // The id is what continues the branch; main is untouched.
            format!(
                "its tip is {} (pass it as --at to go on); the head stays at {}",
                receipt["id"].as_str().unwrap_or("?"),
                receipt["head"].as_str().map_or("the base", short)
            ),
        ],
        _ => vec![format!(
            "committed revision {}: {changes}",
            receipt["revision"]
        )],
    };
    for (asset, copy) in receipt["stored"].as_object().into_iter().flatten() {
        lines.push(format!("  stored {asset} as {}", text(copy)));
    }
    for warning in receipt["warnings"].as_array().into_iter().flatten() {
        lines.push(format!("  {}", text(warning)));
    }
    if receipt["on_screen"] == false {
        lines.push("  not on screen: the window shows another entry".into());
    }
    lines
}

/// Why nothing happened: a reason per op, a busy canvas, or the problems
/// `verify` found — either authority's.
fn refusal_lines(body: &Value) -> Vec<String> {
    if body["result"] == "busy" {
        return vec![format!(
            "busy: {}",
            body["why"]
                .as_str()
                .unwrap_or("the canvas is busy; try again")
        )];
    }
    if let Some(errors) = body["errors"].as_array() {
        return errors
            .iter()
            .map(|e| format!("rejected: {}", text(e)))
            .collect();
    }
    if let Some(problems) = body["problems"].as_array() {
        return problems.iter().map(text).collect();
    }
    match body["error"].as_str() {
        Some(error) => vec![error.to_string()],
        None => vec![text(body)],
    }
}

fn screenshot_lines(shot: &Value) -> Vec<String> {
    vec![
        text(&shot["path"]),
        format!(
            "of revision {}{}",
            shot["revision"],
            if shot["at_head"] == false {
                ", not the head: the window shows another entry"
            } else {
                ", the head"
            }
        ),
    ]
}

fn selection_lines(selection: &Value, shown: &Value) -> Vec<String> {
    let name_of = |id: u64| {
        shown["entities"]
            .as_array()
            .and_then(|all| all.iter().find(|e| e["id"].as_u64() == Some(id)))
            .and_then(|e| e["name"].as_str())
            .map_or_else(|| format!("#{id}"), |name| format!("#{id} {name}"))
    };
    let picked = &selection["selected"];
    if picked.is_null() {
        return vec!["nothing is selected".into()];
    }
    let mut lines = vec![format!(
        "selected: #{} {}",
        picked["id"],
        picked["name"].as_str().unwrap_or_default()
    )];
    let others: Vec<String> = selection["others"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(name_of)
        .collect();
    if !others.is_empty() {
        lines.push(format!("and: {}", others.join(", ")));
    }
    lines
}

fn goto_lines(sought: &Value) -> Vec<String> {
    let applied = sought["changes"].as_array().map_or(0, Vec::len);
    vec![format!(
        "the window shows {} (revision {}): {applied} {} applied to get there",
        sought["tip"].as_str().map_or("the base", short),
        sought["revision"],
        if applied == 1 { "change" } else { "changes" }
    )]
}

fn proposal_lines(review: &Value) -> Vec<String> {
    let mut lines = vec![
        match (review["review"] == true, review["pending"].as_u64()) {
            (_, Some(n)) => format!("#{n} is on the canvas, waiting for the person"),
            (true, None) => "review is on, and nothing is waiting".to_string(),
            (false, None) => "review is off: batches commit as they arrive".to_string(),
        },
    ];
    for proposal in review["proposals"].as_array().into_iter().flatten() {
        let status = match proposal["status"].as_str() {
            Some("accepted") => format!("accepted as revision {}", proposal["revision"]),
            Some("rejected") => match proposal["why"].as_str() {
                Some(why) => format!("rejected ({why})"),
                None => "rejected".to_string(),
            },
            other => other.unwrap_or("?").to_string(),
        };
        lines.push(format!(
            "#{} {:<10} {status}: {}",
            proposal["proposal"],
            proposal["author"].as_str().unwrap_or("?"),
            joined(&proposal["changes"])
        ));
    }
    lines
}

/// A list of changes in a line.
fn joined(changes: &Value) -> String {
    let changes: Vec<String> = changes.as_array().into_iter().flatten().map(text).collect();
    if changes.is_empty() {
        "no change".into()
    } else {
        changes.join(" · ")
    }
}

/// A string as itself, anything else as JSON.
fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_string)
}

/// A number as a person writes it: two decimals at most, none trailing.
fn number(value: &Value) -> String {
    let fixed = format!("{:.2}", value.as_f64().unwrap_or_default());
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    let shown = if trimmed == "-0" { "0" } else { trimmed };
    shown.to_string()
}

/// An id short enough to read in a line: a content hash's first 12 hex
/// digits, anything else as it is.
fn short(id: &str) -> &str {
    match id.strip_prefix("sha256:") {
        Some(hex) if hex.len() > 12 => &id[..7 + 12],
        _ => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

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

    fn submit(dir: &Path, name: &str) {
        headless::run(
            dir,
            WorldCommand::Submit {
                batch: json!({"ops": [{"SpawnEntity": {"entity": {"name": name}}}]}),
                author: "test".into(),
                message: None,
                at: None,
            },
        )
        .expect(name);
    }

    fn first_word(line: &str) -> &str {
        line.split_whitespace().next().unwrap_or_default()
    }

    async fn lines(command: WorldCommands) -> Vec<String> {
        let said = say(command).await.expect("the command runs");
        assert!(!said.failed, "{said:?}");
        said.out
    }

    fn tips(dir: &Path) -> WorldCommands {
        WorldCommands::Tips {
            dir: Some(dir.to_path_buf()),
            out: Out::default(),
        }
    }

    fn log(dir: &Path) -> WorldCommands {
        WorldCommands::Log {
            dir: Some(dir.to_path_buf()),
            last: None,
            out: Out::default(),
        }
    }

    #[tokio::test]
    async fn an_empty_history_says_the_base_is_the_world() {
        let dir = package("empty");
        assert_eq!(
            lines(tips(&dir)).await,
            ["no history yet — the base is the whole world"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_linear_history_has_one_tip_and_it_is_the_head() {
        let dir = package("linear");
        submit(&dir, "lighthouse");
        submit(&dir, "jetty");
        let lines = lines(tips(&dir)).await;
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains(" r2"), "{lines:?}");
        assert!(lines[0].ends_with("← head"), "{lines:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_branch_committed_with_at_is_a_tip_and_the_head_stays_on_main() {
        let dir = package("branch");
        submit(&dir, "lighthouse");
        submit(&dir, "jetty");
        let log_before = lines(log(&dir)).await;
        let (first, second) = (first_word(&log_before[0]), first_word(&log_before[1]));

        let said = say(WorldCommands::Submit {
            dir: Some(dir.clone()),
            batch: Some(r#"[{"SpawnEntity": {"entity": {"name": "buoy"}}}]"#.into()),
            author: "test".into(),
            message: None,
            at: Some(first.to_string()),
            expected_revision: None,
            out: Out::default(),
        })
        .await
        .expect("a branch");
        assert!(said.out[0].contains("on a branch"), "{said:?}");
        assert!(
            said.out[1].ends_with(&format!("the head stays at {}", short(second))),
            "{said:?}"
        );

        let tips = lines(tips(&dir)).await;
        assert_eq!(tips.len(), 2, "{tips:?}");
        let head: Vec<&String> = tips.iter().filter(|l| l.ends_with("← head")).collect();
        assert_eq!(head.len(), 1, "{tips:?}");
        assert_eq!(first_word(head[0]), second, "main's tip, not the last line");

        // The log says where the branch builds, since file order no longer does.
        let log = lines(log(&dir)).await;
        assert!(
            log[2].ends_with(&format!("(builds on {})", short(first))),
            "{log:?}"
        );
        assert!(!log[1].contains("builds on"), "{log:?}");
        assert!(log[1].ends_with("← head"), "{log:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn the_log_names_each_entry_by_the_id_a_tip_would_use() {
        let dir = package("log");
        submit(&dir, "lighthouse");
        submit(&dir, "jetty");
        let lines_now = lines(log(&dir)).await;
        assert_eq!(lines_now.len(), 2, "{lines_now:?}");
        // The authority writes the format's content hash as each new
        // entry's id — `sha256:` and 64 hex characters — different per entry.
        for line in &lines_now {
            let id = first_word(line);
            let hex = id
                .strip_prefix("sha256:")
                .unwrap_or_else(|| panic!("{line}"));
            assert_eq!(hex.len(), 64, "{line}");
            assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{line}");
        }
        assert_ne!(first_word(&lines_now[0]), first_word(&lines_now[1]));
        // The id a tip reports is the id the log shows for the same entry.
        let tip = lines(tips(&dir)).await;
        assert_eq!(first_word(&tip[0]), first_word(&lines_now[1]));
        // And --last keeps the newest.
        let last = lines(WorldCommands::Log {
            dir: Some(dir.clone()),
            last: Some(1),
            out: Out::default(),
        })
        .await;
        assert_eq!(last, lines_now[1..]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn with_no_app_a_batch_commits_here_and_says_what_it_did() {
        let dir = package("offline");
        let said = say(WorldCommands::Submit {
            dir: Some(dir.clone()),
            batch: Some(r#"{"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}]}"#.into()),
            author: "claude".into(),
            message: Some("hang a lamp".into()),
            at: None,
            expected_revision: Some(0),
            out: Out::default(),
        })
        .await
        .unwrap();
        assert_eq!(said.out, ["committed revision 1: spawn #1 lamp"]);
        let log = lines(log(&dir)).await;
        assert!(
            log[0].contains("claude") && log[0].contains("\"hang a lamp\""),
            "{log:?}"
        );

        // A stale read is refused, naming both revisions, and fails.
        let said = say(WorldCommands::Submit {
            dir: Some(dir.clone()),
            batch: Some(r#"[{"DeleteEntity": {"id": "lamp"}}]"#.into()),
            author: "claude".into(),
            message: None,
            at: None,
            expected_revision: Some(0),
            out: Out::default(),
        })
        .await
        .unwrap();
        assert!(said.failed, "{said:?}");
        assert!(said.err[0].starts_with("rejected: "), "{said:?}");
        assert!(said.err[0].contains("revision 1"), "{said:?}");

        // Reading one entity gives its fields, as a patch would change them.
        let said = say(WorldCommands::Get {
            names: vec![dir.to_string_lossy().into_owned(), "lamp".into()],
            at: None,
        })
        .await
        .unwrap();
        let lamp: Value = serde_json::from_str(&said.out[0]).unwrap();
        assert_eq!(lamp["name"], "lamp");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn what_only_a_window_knows_needs_the_app_and_says_so() {
        let dir = package("windowless");
        for command in [
            WorldCommands::Screenshot {
                dir: Some(dir.clone()),
                out: Out::default(),
            },
            WorldCommands::Selection {
                dir: Some(dir.clone()),
                out: Out::default(),
            },
            WorldCommands::Proposals {
                dir: Some(dir.clone()),
                out: Out::default(),
            },
        ] {
            let said = say(command).await.unwrap();
            assert!(said.failed, "{said:?}");
            assert!(said.err[0].contains("needs the app"), "{said:?}");
            assert!(said.err[0].contains("localgpt-app --world"), "{said:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_world_is_the_folder_given_else_the_env_else_the_nearest_world_up() {
        let root = tempfile::tempdir().unwrap();
        let world = root.path().join("harbour.world");
        std::fs::create_dir_all(&world).unwrap();
        headless::run(&world, WorldCommand::Init { git: false }).unwrap();
        let inside = world.join("assets");
        std::fs::create_dir_all(&inside).unwrap();
        let (given, env) = (PathBuf::from("/a"), PathBuf::from("/b"));

        assert_eq!(
            resolve(Some(given.clone()), Some(env.clone()), &inside).unwrap(),
            given
        );
        assert_eq!(resolve(None, Some(env.clone()), &inside).unwrap(), env);
        assert_eq!(resolve(None, Some(PathBuf::new()), &inside).unwrap(), world);
        assert_eq!(resolve(None, None, &world).unwrap(), world);

        // A folder holding only a manifest.json — a web project's — is not
        // taken for a world on the way up, since opening one writes to it.
        let web = root.path().join("site");
        std::fs::create_dir_all(web.join("src")).unwrap();
        std::fs::write(web.join("manifest.json"), "{}").unwrap();
        let err = resolve(None, None, &web.join("src")).unwrap_err();
        assert!(err.to_string().contains("no world here"), "{err}");
    }

    #[test]
    fn a_first_argument_that_isnt_a_world_is_the_commands_own() {
        let world = package("first");
        assert_eq!(
            folder_first(Some(world.clone()), None),
            (Some(world.clone()), None)
        );
        assert_eq!(
            folder_first(Some(PathBuf::from("[{}]")), None),
            (None, Some("[{}]".to_string()))
        );
        assert_eq!(
            folder_first(Some(world.clone()), Some("main".into())),
            (Some(world.clone()), Some("main".to_string()))
        );
        std::fs::remove_dir_all(&world).ok();
    }

    #[test]
    fn every_command_and_flag_the_worlds_guide_names_is_one_this_cli_has() {
        // The guide is written into every live world; a command it names
        // that this CLI lacks would send an agent after nothing.
        let cli = WorldCommands::augment_subcommands(clap::Command::new("world"));
        let commands: Vec<&str> = cli.get_subcommands().map(|c| c.get_name()).collect();
        let mut flags: Vec<String> = cli
            .get_subcommands()
            .flat_map(|c| c.get_arguments())
            .filter_map(|a| a.get_long().map(|long| format!("--{long}")))
            .collect();
        // Clap's own, which a command has once it is built.
        flags.push("--help".into());
        let guide = live::AGENTS_GUIDE;
        let named: Vec<&str> = guide
            .split("localgpt world ")
            .skip(1)
            .filter_map(|rest| rest.split(|c: char| !c.is_ascii_lowercase()).next())
            .collect();
        assert!(named.len() > 10, "{named:?}");
        for command in named {
            assert!(commands.contains(&command), "the guide names `{command}`");
        }
        // The app's own flag, the one way the guide names its command line.
        let ours = guide.replace("localgpt-app --world", "");
        for word in ours.split(|c: char| c.is_whitespace() || "`()".contains(c)) {
            if let Some(flag) = word.strip_prefix("--") {
                let flag = format!(
                    "--{}",
                    flag.trim_end_matches(|c: char| !c.is_ascii_lowercase())
                );
                assert!(flags.contains(&flag), "the guide names `{flag}`");
            }
        }
    }

    #[test]
    fn a_flag_fills_in_only_what_the_batch_does_not_say() {
        let body = envelope(
            json!({"ops": [], "author": "batch"}),
            "flag",
            Some("why".into()),
            Some("sha256:aa".into()),
            Some(3),
        )
        .unwrap();
        assert_eq!(
            body,
            json!({"ops": [], "author": "batch", "message": "why", "at": "sha256:aa", "expected_revision": 3})
        );
        assert!(envelope(json!("ops"), "a", None, None, None).is_err());
    }

    #[test]
    fn show_says_each_thing_in_a_line() {
        let world = json!({"meta": {"name": "harbour"}, "entities": [
            {"id": 2, "name": "jetty", "transform": {"position": [0.0, 0.5, -3.0]},
             "shape": {"Cuboid": {"x": 2, "y": 0.3, "z": 8}}},
            {"id": 3, "name": "lantern", "parent": 2,
             "transform": {"position": [2.0, 0.5, 1.25], "visible": false},
             "shape": {"Cylinder": {}}, "light": {"light_type": "point"},
             "behaviors": [{"Pulse": {}}]},
            {"id": 9, "name": "1A", "transform": {"position": [-0.004, 1.6, 4.0]},
             "ext-cinematography": {"camera": {}, "shot": {}}},
        ]});
        assert_eq!(
            outline(&world, "the head"),
            [
                "harbour, at the head: 3 entities",
                "#2 jetty  Cuboid  at (0, 0.5, -3)",
                "#3 lantern  Cylinder + point light  at (2, 0.5, 1.25)  in jetty  hidden  · Pulse",
                "#9 1A  shot  at (0, 1.6, 4)",
            ]
        );
    }

    #[test]
    fn a_receipt_reads_the_same_whoever_committed() {
        let receipt = json!({"result": "committed", "revision": 4, "id": "sha256:0123456789abcdef0123",
            "parent": "sha256:ff", "branch": true, "head": "sha256:fedcba9876543210fedc",
            "changes": ["spawn #7 buoy"], "spawned": {"buoy": 7}, "stored": {"rope.png": "ab12.png"},
            "warnings": ["the bench it looks at is gone"], "on_screen": false});
        assert_eq!(
            receipt_lines(&receipt),
            [
                "committed revision 4 on a branch: spawn #7 buoy",
                "its tip is sha256:0123456789abcdef0123 (pass it as --at to go on); the head stays at sha256:fedcba987654",
                "  stored rope.png as ab12.png",
                "  the bench it looks at is gone",
                "  not on screen: the window shows another entry",
            ]
        );
        let proposed =
            json!({"result": "proposed", "proposal": 3, "changes": ["delete #4"], "warnings": []});
        assert!(receipt_lines(&proposed)[0].starts_with("proposed #3: delete #4"));
        assert_eq!(
            refusal_lines(&json!({"result": "busy", "why": "a replay is running"})),
            ["busy: a replay is running"]
        );
    }

    /// What a stand-in for the open app was sent.
    #[derive(Debug, Clone)]
    struct Sent {
        method: String,
        target: String,
        auth: String,
        body: Value,
    }

    /// A stand-in for the open app: a world folder whose endpoint answers
    /// each `(method, path)` with a canned reply, keeping what it was sent.
    struct FakeApp {
        dir: tempfile::TempDir,
        sent: Arc<Mutex<Vec<Sent>>>,
    }

    fn fake_app(replies: Vec<(&'static str, &'static str, u16, Value)>) -> FakeApp {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".live")).unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        std::fs::write(
            dir.path().join(".live/endpoint.json"),
            json!({"url": url, "token": "secret"}).to_string(),
        )
        .unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let keep = sent.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // The holder's look connects and sends nothing.
                let Some(request) = read_request(&stream) else {
                    continue;
                };
                let path = request.target.split('?').next().unwrap_or_default();
                let (status, reply) = replies
                    .iter()
                    .find(|(m, p, ..)| *m == request.method && *p == path)
                    .map(|(.., status, reply)| (*status, reply.clone()))
                    .unwrap_or((404, json!({"error": "no such endpoint"})));
                keep.lock().unwrap().push(request);
                let text = reply.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        FakeApp { dir, sent }
    }

    fn read_request(stream: &TcpStream) -> Option<Sent> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let mut words = line.split_whitespace();
        let (method, target) = (words.next()?.to_string(), words.next()?.to_string());
        let (mut length, mut auth) = (0, String::new());
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).ok()?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let (key, value) = header.split_once(':')?;
            match key.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().ok()?,
                "authorization" => auth = value.trim().to_string(),
                _ => {}
            }
        }
        let mut body = vec![0; length];
        std::io::Read::read_exact(&mut reader, &mut body).ok()?;
        Some(Sent {
            method,
            target,
            auth,
            body: serde_json::from_slice(&body).unwrap_or_default(),
        })
    }

    #[tokio::test]
    async fn with_an_app_open_a_batch_goes_to_it_with_the_token_from_the_folder() {
        let app = fake_app(vec![(
            "POST",
            "/ops",
            200,
            json!({"result": "committed", "revision": 7, "id": "sha256:aa", "parent": null,
                   "branch": false, "head": "sha256:aa", "on_screen": true,
                   "changes": ["spawn #12 lamp"], "spawned": {"lamp": 12}, "stored": {}, "warnings": []}),
        )]);
        let said = say(WorldCommands::Submit {
            dir: Some(app.dir.path().to_path_buf()),
            batch: Some(r#"[{"SpawnEntity": {"entity": {"name": "lamp"}}}]"#.into()),
            author: "claude".into(),
            message: Some("a lamp by the crate".into()),
            at: None,
            expected_revision: Some(6),
            out: Out::default(),
        })
        .await
        .unwrap();
        assert_eq!(said.out, ["committed revision 7: spawn #12 lamp"]);
        let sent = app.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].auth, "Bearer secret");
        assert_eq!(
            sent[0].body,
            json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}], "author": "claude",
                   "message": "a lamp by the crate", "expected_revision": 6})
        );
    }

    #[tokio::test]
    async fn the_apps_refusals_and_proposals_are_said_and_a_refusal_fails() {
        let app = fake_app(vec![
            (
                "POST",
                "/ops",
                422,
                json!({"result": "rejected", "errors": ["op 1 (SpawnEntity): unknown field `colour`"]}),
            ),
            (
                "POST",
                "/undo",
                202,
                json!({"result": "proposed", "proposal": 2, "changes": ["delete #4"], "warnings": []}),
            ),
        ]);
        let dir = Some(app.dir.path().to_path_buf());
        let said = say(WorldCommands::Submit {
            dir: dir.clone(),
            batch: Some(r#"[{"SpawnEntity": {"entity": {"name": "x", "colour": 1}}}]"#.into()),
            author: "claude".into(),
            message: None,
            at: None,
            expected_revision: None,
            out: Out::default(),
        })
        .await
        .unwrap();
        assert!(said.failed);
        assert_eq!(
            said.err,
            ["rejected: op 1 (SpawnEntity): unknown field `colour`"]
        );

        let said = say(WorldCommands::Undo {
            dir,
            take_back: TakeBack {
                author: "claude".into(),
                at: None,
                own: true,
            },
            out: Out { json: true },
        })
        .await
        .unwrap();
        assert!(!said.failed, "a proposal is not a failure");
        let reply: Value = serde_json::from_str(&said.out[0]).unwrap();
        assert_eq!(reply["proposal"], 2);
        let sent = app.sent.lock().unwrap();
        assert_eq!(sent[1].body, json!({"author": "claude", "own": true}));
    }

    #[tokio::test]
    async fn with_an_app_open_reads_come_from_it_and_mark_what_it_shows() {
        let app = fake_app(vec![
            (
                "GET",
                "/log",
                200,
                json!({"revision": 2, "head": "sha256:bb", "current": "sha256:aa", "entries": [
                    {"id": "sha256:aa", "parent": null, "revision": 1, "author": "yi", "message": null, "changes": ["spawn #1 jetty"]},
                    {"id": "sha256:bb", "parent": "sha256:aa", "revision": 2, "author": "claude", "message": "a lamp", "changes": ["spawn #2 lamp"]},
                ]}),
            ),
            (
                "GET",
                "/world",
                200,
                json!({"meta": {"name": "harbour"}, "entities": [
                    {"id": 1, "name": "jetty", "transform": {"position": [0, 0, 0]}},
                    {"id": 2, "name": "lamp", "transform": {"position": [1, 2, 3]}},
                ]}),
            ),
        ]);
        let dir = app.dir.path().to_path_buf();
        let log = lines(log(&dir)).await;
        assert_eq!(
            log,
            [
                "sha256:aa r1   yi         spawn #1 jetty  ← on screen",
                "sha256:bb r2   claude     \"a lamp\" spawn #2 lamp  ← head",
            ]
        );

        let said = say(WorldCommands::Get {
            names: vec![
                dir.to_string_lossy().into_owned(),
                "#2".into(),
                "mast".into(),
            ],
            at: Some("current".into()),
        })
        .await
        .unwrap();
        assert_eq!(
            said.out,
            [r#"{"id":2,"name":"lamp","transform":{"position":[1,2,3]}}"#]
        );
        assert_eq!(said.err, ["nothing called mast at the entry on screen"]);
        assert!(said.failed);
        let sent = app.sent.lock().unwrap();
        assert_eq!(sent.last().unwrap().target, "/world?tip=current");
    }
}
