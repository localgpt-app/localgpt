//! The world authority without an app: the commands a script, a CI job or a
//! terminal runs against a `.world` package.
//!
//! This is the logic behind `localgpt world` and the `live` example. It lives
//! here, once, because [`crate::live::LiveWorld`] is the authority and a
//! second implementation of commit is the drift this workspace has paid for
//! before (`docs/rfcs/authoring/world-editor-stack.md`).
//!
//! Nothing here prints, exits or reads stdin: a [`Command`] goes in and a
//! [`Report`] of lines comes out, so the caller formats and the tests assert.
//!
//! ## One authority at a time
//!
//! A package open in an app is that app's to write — it holds the fold, it
//! guards `manifest.json`, and a second writer would commit behind its back.
//! So a write refuses while an app *answers* at the folder's endpoint, and
//! says where to send it instead. An app that was killed leaves its
//! `endpoint.json` behind, and a stale file must not lock a world forever, so
//! the check is whether something is listening, not whether the file exists.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use localgpt_world_types as wt;

use crate::live::{self, LiveWorld};

/// How long to wait for an app to answer before treating its endpoint as
/// stale. Local, so a listening socket answers in microseconds.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(300);

/// What to do to a package.
#[derive(Debug, Clone)]
pub enum Command {
    /// Open the folder as a package, creating one if it isn't yet.
    Init { git: bool },
    /// Commit a batch of ops, whole or not at all.
    Submit {
        batch: Value,
        author: String,
        message: Option<String>,
    },
    /// Take back the newest edit by appending its inverse.
    Undo { author: String },
    /// Every committed batch.
    Log,
    /// Check that the fold still equals the head and the assets match.
    Verify,
    /// The git history, when the package is in git.
    History { rev: Option<String> },
}

impl Command {
    /// True when this would append to the log, and so needs the folder to
    /// itself.
    pub fn writes(&self) -> bool {
        matches!(self, Self::Submit { .. } | Self::Undo { .. })
    }
}

/// Lines for the caller to print.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub lines: Vec<String>,
}

impl Report {
    fn one(line: impl Into<String>) -> Self {
        Self {
            lines: vec![line.into()],
        }
    }
}

/// Why a command could not run.
#[derive(Debug)]
pub enum HeadlessError {
    /// An app answers at this folder's endpoint; the URL to send to instead.
    Open { url: String, send_to: String },
    /// The batch was refused, with a reason per problem.
    Refused(Vec<String>),
    /// `verify` found problems.
    Invalid(Vec<String>),
    /// Anything else — unreadable package, bad JSON, git not present.
    Failed(String),
}

impl std::fmt::Display for HeadlessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { url, send_to } => write!(
                f,
                "an app has this world open — send it there: POST {url}/{send_to}"
            ),
            Self::Refused(errors) => write!(f, "rejected: {}", errors.join("; ")),
            Self::Invalid(problems) => write!(f, "{}", problems.join("; ")),
            Self::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HeadlessError {}

impl From<live::LiveError> for HeadlessError {
    fn from(e: live::LiveError) -> Self {
        Self::Failed(e.to_string())
    }
}

/// The URL of an app that answers for this folder, if one does.
///
/// `None` covers both "no endpoint file" and "a file left behind by an app
/// that died" — the second is removed, because a crash must not lock a world.
pub fn holder(dir: &Path) -> Option<String> {
    let endpoint = dir.join(live::LIVE).join("endpoint.json");
    let bytes = std::fs::read(&endpoint).ok()?;
    let open: Value = serde_json::from_slice(&bytes).ok()?;
    let url = open["url"].as_str().unwrap_or_default().to_string();
    let answers = url
        .trim_start_matches("http://")
        .parse::<std::net::SocketAddr>()
        .ok()
        .and_then(|addr| std::net::TcpStream::connect_timeout(&addr, ANSWER_TIMEOUT).ok())
        .is_some();
    if answers {
        Some(url)
    } else {
        let _ = std::fs::remove_file(&endpoint);
        None
    }
}

/// Write a minimal `manifest.json` when the folder has none.
///
/// The name comes from the folder, with a `.world` suffix dropped, so
/// `harbour.world/` is "harbour".
fn seed_manifest(dir: &Path) -> Result<(), HeadlessError> {
    let manifest = dir.join(live::MANIFEST);
    if manifest.exists() {
        return Ok(());
    }
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "world".into());
    let name = name.strip_suffix(".world").unwrap_or(&name).to_string();
    let seed = serde_json::json!({
        "version": wt::WORLD_SCHEMA_VERSION,
        "meta": {"name": name},
        "entities": [],
    });
    std::fs::create_dir_all(dir).map_err(|e| HeadlessError::Failed(e.to_string()))?;
    std::fs::write(&manifest, openworldformat::package::manifest_text(&seed))
        .map_err(|e| HeadlessError::Failed(format!("{}: {e}", manifest.display())))
}

/// Run `command` against the package in `dir`.
pub fn run(dir: &Path, command: Command) -> Result<Report, HeadlessError> {
    if command.writes()
        && let Some(url) = holder(dir)
    {
        return Err(HeadlessError::Open {
            url,
            send_to: match command {
                Command::Undo { .. } => "undo".into(),
                _ => "ops".into(),
            },
        });
    }

    match command {
        Command::Init { git } => {
            // The authority opens a package; it does not invent one, and
            // `LiveWorld::open` needs a manifest to fold from. A CLI `init`
            // on an empty folder should still work, so seed the smallest
            // valid world — the schema version, a name from the folder, no
            // entities — and let the authority take it from there. An
            // existing manifest is never touched.
            seed_manifest(dir)?;
            let world = LiveWorld::open(dir)?;
            if git {
                live::init_git(dir).map_err(HeadlessError::from)?;
            }
            Ok(Report::one(format!(
                "{} is live at revision {} ({} entities){}",
                dir.display(),
                world.revision(),
                world.head().len(),
                if world.is_git() { ", in git" } else { "" }
            )))
        }
        Command::Submit {
            batch,
            author,
            message,
        } => {
            let mut world = LiveWorld::open(dir)?;
            match world.submit(&batch, &author, message.as_deref()) {
                Ok(done) => Ok(Report::one(format!(
                    "committed revision {}: {}",
                    done.revision,
                    live::describe_ops(&done.ops).join(" · ")
                ))),
                Err(refused) => Err(HeadlessError::Refused(refused.errors)),
            }
        }
        Command::Undo { author } => {
            let mut world = LiveWorld::open(dir)?;
            match world.undo(&author) {
                Ok(done) => Ok(Report::one(format!(
                    "committed revision {}: {}",
                    done.revision,
                    done.warnings.join(" ")
                ))),
                Err(refused) => Err(HeadlessError::Refused(refused.errors)),
            }
        }
        Command::Log => {
            let world = LiveWorld::open(dir)?;
            Ok(Report {
                lines: world
                    .entries()
                    .iter()
                    .map(|entry| {
                        format!(
                            "r{:<3} {:<10} {}",
                            entry.revision,
                            entry.author.name,
                            live::describe_ops(&entry.edit_ops()).join(" · ")
                        )
                    })
                    .collect(),
            })
        }
        Command::Verify => {
            let world = LiveWorld::open(dir)?;
            let problems = world.verify();
            if problems.is_empty() {
                Ok(Report::one(format!(
                    "ok: fold(base, {} entries) == manifest.json, and every asset matches its hash",
                    world.entries().len()
                )))
            } else {
                Err(HeadlessError::Invalid(problems))
            }
        }
        Command::History { rev } => {
            let steps = live::git_history(dir, rev.as_deref())
                .map_err(|e| HeadlessError::Failed(e.to_string()))?;
            Ok(Report {
                lines: steps
                    .iter()
                    .map(|s| {
                        format!(
                            "{} {:<10} {:<40} {} entities",
                            &s.commit[..7.min(s.commit.len())],
                            s.author,
                            s.subject,
                            s.manifest.entities.len()
                        )
                    })
                    .collect(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lga-headless-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn init_makes_a_package_out_of_an_empty_folder() {
        let dir = temp("init");
        let report = run(&dir, Command::Init { git: false }).expect("init");
        assert!(report.lines[0].contains("revision 0"), "{:?}", report.lines);
        assert!(dir.join(live::PACKAGE).is_file(), "package.json");
        assert!(dir.join(live::MANIFEST).is_file(), "manifest.json");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_batch_commits_and_shows_up_in_the_log() {
        let dir = temp("submit");
        run(&dir, Command::Init { git: false }).unwrap();

        let batch = json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}]});
        let report = run(
            &dir,
            Command::Submit {
                batch,
                author: "test".into(),
                message: Some("hang a lamp".into()),
            },
        )
        .expect("the batch commits");
        assert!(report.lines[0].contains("revision 1"), "{:?}", report.lines);

        let log = run(&dir, Command::Log).expect("log");
        assert_eq!(log.lines.len(), 1);
        assert!(log.lines[0].contains("test"), "{:?}", log.lines);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_field_is_refused_whole_with_a_reason() {
        let dir = temp("refuse");
        run(&dir, Command::Init { git: false }).unwrap();
        let batch = json!({"ops": [
            {"SpawnEntity": {"entity": {"name": "good"}}},
            {"SpawnEntity": {"entity": {"name": "bad", "colour": "red"}}}
        ]});
        let err = run(
            &dir,
            Command::Submit {
                batch,
                author: "test".into(),
                message: None,
            },
        )
        .expect_err("refused");
        match err {
            HeadlessError::Refused(errors) => {
                assert!(
                    errors.iter().any(|e| e.contains("colour")),
                    "the reason should name the field: {errors:?}"
                );
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        // Nothing was committed: the valid op in the batch must not apply.
        let log = run(&dir, Command::Log).unwrap();
        assert!(log.lines.is_empty(), "{:?}", log.lines);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_holds_after_a_commit() {
        let dir = temp("verify");
        run(&dir, Command::Init { git: false }).unwrap();
        run(
            &dir,
            Command::Submit {
                batch: json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}]}),
                author: "test".into(),
                message: None,
            },
        )
        .unwrap();
        let report = run(&dir, Command::Verify).expect("verify");
        assert!(report.lines[0].starts_with("ok:"), "{:?}", report.lines);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undo_appends_an_inverse_rather_than_removing_the_entry() {
        let dir = temp("undo");
        run(&dir, Command::Init { git: false }).unwrap();
        run(
            &dir,
            Command::Submit {
                batch: json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}]}),
                author: "test".into(),
                message: None,
            },
        )
        .unwrap();
        run(
            &dir,
            Command::Undo {
                author: "test".into(),
            },
        )
        .expect("undo");

        let log = run(&dir, Command::Log).unwrap();
        assert_eq!(log.lines.len(), 2, "the log grew: {:?}", log.lines);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_writes_need_the_folder_to_themselves() {
        assert!(
            Command::Submit {
                batch: json!([]),
                author: "a".into(),
                message: None
            }
            .writes()
        );
        assert!(Command::Undo { author: "a".into() }.writes());
        for read_only in [
            Command::Log,
            Command::Verify,
            Command::Init { git: false },
            Command::History { rev: None },
        ] {
            assert!(!read_only.writes(), "{read_only:?} should not be a write");
        }
    }

    #[test]
    fn a_stale_endpoint_does_not_lock_the_world() {
        // An app that was killed leaves endpoint.json behind. If the file
        // alone were taken as "open", the world would be unwritable forever.
        let dir = temp("stale");
        run(&dir, Command::Init { git: false }).unwrap();
        let live_dir = dir.join(live::LIVE);
        std::fs::create_dir_all(&live_dir).unwrap();
        // Port 1 on loopback: nothing listens there.
        std::fs::write(
            live_dir.join("endpoint.json"),
            br#"{"url": "http://127.0.0.1:1", "token": "x"}"#,
        )
        .unwrap();

        assert!(holder(&dir).is_none(), "a dead endpoint is not a holder");
        assert!(
            !live_dir.join("endpoint.json").exists(),
            "and it is cleaned up"
        );
        run(
            &dir,
            Command::Submit {
                batch: json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}]}),
                author: "test".into(),
                message: None,
            },
        )
        .expect("writing works once the stale endpoint is gone");
        std::fs::remove_dir_all(&dir).ok();
    }
}
