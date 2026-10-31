//! Live editing — a proof of concept (branch `poc/live-editing`).
//!
//! A `.world` folder an app keeps open while agents change it from
//! outside — the pattern of an editor whose documents are folders and
//! whose agents work in their own terminal, never inside its window. The
//! split this module enforces:
//!
//! - **The world changes only through ops.** An agent sends a batch of
//!   edit ops; [`LiveWorld::submit`] binds names, checks every field,
//!   applies the batch to a trial copy, validates the result, and only
//!   then commits — the whole batch, or nothing and the reason.
//! - **Assets are files, and content-addressed.** Agents write meshes,
//!   textures and sounds into `assets/` under any name they like; an op
//!   that references one stores an immutable copy at
//!   `assets/<sha256>.<ext>` and points the world there. Changing an asset
//!   is writing new bytes and referencing them again — a new version, while
//!   the old one stays for the history that used it.
//! - **The package is head-first.** `manifest.json` is the world now,
//!   written only by the committer; the log is how it got here, and
//!   `fold(base, log) == manifest.json` is checkable at any time.
//!
//! ```text
//! castle.world/
//!   manifest.json        the world now (read it; the committer writes it)
//!   ops.jsonl            every committed batch: author, message, ops
//!   package.json         head_revision; world_sha256 names manifest.json's bytes
//!   snapshots/base.json  the oldest state
//!   assets/              files agents write; <sha256>.<ext> copies the world uses
//!   AGENTS.md            how to change this world, for agents that read it
//!   .live/               the open app's: endpoint.json, preview.png, shots
//! ```
//!
//! No Bevy and no transport here: Gen's `--live` canvas puts an HTTP API in
//! front of it, and the `live` example runs it headless.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use localgpt_world_sync as sync;
use localgpt_world_types as wt;
use serde_json::{Value, json};
use sync::{
    ApplyError, Author, OpLogEntry, SessionMeta, SessionOp, ToolRecord, WorldDoc, fold_log,
};
use wt::{EditOp, WorldEntity, WorldManifest};

use crate::session::sha256_hex;

/// The world now.
pub const MANIFEST: &str = "manifest.json";
/// Every committed batch, one JSON entry per line.
pub const LOG: &str = "ops.jsonl";
/// Revisions and integrity.
pub const PACKAGE: &str = "package.json";
/// The oldest state the log folds from.
pub const BASE: &str = "snapshots/base.json";
/// Files agents write, and the content-addressed copies the world uses.
pub const ASSETS: &str = "assets";
/// The guide an agent reads before changing the world.
pub const AGENTS: &str = "AGENTS.md";
/// The open app's files: its endpoint, previews, screenshots.
pub const LIVE: &str = ".live";
/// Under `.live/`: what was tried outside the API and why it did not take.
pub const REFUSALS: &str = "refusals.jsonl";

/// True when `dir` is a `.world` package — a folder holding its
/// `manifest.json`, whether or not a log has grown yet.
///
/// The format's own marker, so an app asked to open a folder can tell a
/// package from a Gen `world.ron` folder or a loose manifest file: those
/// describe a world, while this *is* one, log and all. A folder that only
/// holds `manifest.json` still counts — `LiveWorld::open` completes the
/// package on first write.
pub fn is_package(dir: &Path) -> bool {
    dir.is_dir() && dir.join(MANIFEST).is_file()
}

/// Largest asset one op may bring in.
const MAX_ASSET_BYTES: u64 = 256 << 20;

/// The op kinds the format defines, for error messages.
const OP_KINDS: &[&str] = &[
    "SpawnEntity",
    "ModifyEntity",
    "DeleteEntity",
    "SetEnvironment",
    "SetCamera",
    "SetAmbience",
    "SpawnAudioEmitter",
    "RemoveAudioEmitter",
    "ModifyWorld",
    "Batch",
];

/// Why a live package couldn't be opened or written.
#[derive(Debug)]
pub struct LiveError(pub String);

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LiveError {}

impl From<std::io::Error> for LiveError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

/// A batch the world took.
#[derive(Debug, Clone)]
pub struct Committed {
    /// The revision it made: the authority's order across every branch, and
    /// the head's revision unless `forked`.
    pub revision: u64,
    /// The ops as committed — names bound to ids, assets to their copies —
    /// which is also what a canvas applies to its scene.
    pub ops: Vec<EditOp>,
    /// Entities the batch spawned, name → id.
    pub spawned: BTreeMap<String, u64>,
    /// Assets the batch stored, as referenced → content-addressed path.
    pub stored: BTreeMap<String, String>,
    /// Things worth knowing that didn't stop the batch.
    pub warnings: Vec<String>,
    /// The log entry as appended.
    ///
    /// For a view that follows the authority rather than re-reading the log:
    /// `localgpt-world-editor`'s `Editor::committed` takes this and returns
    /// the ops that move a renderer to the new tip. Handing it over is what
    /// keeps the view from having to assume a commit appended exactly one
    /// entry.
    pub entry: OpLogEntry,
    /// True when the batch started or extended a branch rather than main —
    /// the head did not move, and `manifest.json` is unchanged.
    pub forked: bool,
}

/// A batch the world refused; nothing changed.
#[derive(Debug, Clone)]
pub struct Rejected {
    pub errors: Vec<String>,
}

impl Rejected {
    fn one(message: impl Into<String>) -> Self {
        Self {
            errors: vec![message.into()],
        }
    }
}

/// A live `.world` package and the authority over it.
pub struct LiveWorld {
    dir: PathBuf,
    base: WorldDoc,
    entries: Vec<OpLogEntry>,
    head: WorldDoc,
    /// The head as a manifest: the fold's entities and scene-wide fields,
    /// and anything else the manifest carries that the fold doesn't.
    head_manifest: WorldManifest,
    meta: SessionMeta,
    /// The git commit the committer last made or read, when the package is
    /// a repository: a different `HEAD` on disk means a checkout happened
    /// under it (`git_moved`), which is not a direct write to put back.
    git_head: Option<String>,
}

impl LiveWorld {
    /// Open a folder holding `manifest.json`, making it a live package
    /// first if it is only that.
    pub fn open(dir: &Path) -> Result<Self, LiveError> {
        if !dir.join(PACKAGE).is_file() {
            init(dir)?;
        }
        // A package made elsewhere — a previs draft, a download — has the
        // format's four files and nothing an agent needs: the guide, and
        // the folder its asset files go in.
        furnish(dir)?;
        let meta: SessionMeta = read_json(dir, PACKAGE)?;
        let base_manifest: WorldManifest = read_json(dir, BASE)?;
        let base = WorldDoc::from_manifest(&base_manifest)
            .map_err(|e| LiveError(format!("{BASE} doesn't load: {e}")))?;
        let entries = read_log(dir);
        let (head, _) = fold_head(&base, &entries, &meta)
            .map_err(|e| LiveError(format!("{LOG} no longer folds: {e}")))?;
        let mut world = Self {
            dir: dir.to_path_buf(),
            head_manifest: head_as_written(dir, &meta, &base_manifest, &head),
            base,
            entries,
            head,
            meta,
            git_head: read_git_head(dir),
        };
        // manifest.json is the committer's to write; whatever is there now
        // is replaced by the head the log says (a no-op when it already is).
        world.guard();
        Ok(world)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The head revision.
    pub fn revision(&self) -> u64 {
        self.meta.head_revision
    }

    /// The state the log folds from.
    ///
    /// With [`Self::entries`], this is everything a non-linear view needs:
    /// `Editor::open(live.base().clone(), live.entries().to_vec())` gives a
    /// view that can seek to any tip, fork, and say what changed — while this
    /// stays the only thing that commits.
    pub fn base(&self) -> &WorldDoc {
        &self.base
    }

    /// The world now.
    pub fn head(&self) -> &WorldDoc {
        &self.head
    }

    /// The world now, as `manifest.json` holds it.
    pub fn head_manifest(&self) -> &WorldManifest {
        &self.head_manifest
    }

    /// The history, oldest first.
    pub fn entries(&self) -> &[OpLogEntry] {
        &self.entries
    }

    /// Where a manifest asset path lives on disk.
    pub fn asset_path(&self, path: &str) -> PathBuf {
        self.dir.join(ASSETS).join(path)
    }

    /// Take a batch of ops: `[op, …]`, or `{"ops": [op, …], "author", "message"}`
    /// (the object's author and message win over the arguments).
    ///
    /// Each op is bound before it is read: an entity may be named wherever
    /// an id goes, a spawn without an id gets the next one, and a patch's
    /// `transform`, `material` or `light` merges into the entity's current
    /// value instead of replacing it. Then it must parse with no field the
    /// format would drop, its assets must exist (and are stored by hash),
    /// and it must apply. The batch commits only if every op got through
    /// and the world it makes validates.
    pub fn submit(
        &mut self,
        input: &Value,
        author: &str,
        message: Option<&str>,
    ) -> Result<Committed, Rejected> {
        self.submit_at(None, input, author, message)
    }

    /// Commit a batch on top of entry `at` — or of main's tip when `at` is
    /// `None` or names it. Anywhere else, the batch starts a **branch** (or
    /// extends one, when `at` is a branch's tip). An object batch's own
    /// `"at"` wins over the argument, as its author and message do.
    ///
    /// A branch commit is ingested against the world at `at` (the fold of
    /// its path), appended with `at` as its parent, and leaves the head
    /// alone: `manifest.json` is main's fold, and main has not moved. Before
    /// the first branch entry lands, `refs.main` is written to name main's
    /// tip — the log is append-only, so a branch entry is always the last
    /// line, and under the head rule's "else the last entry" fallback it
    /// would otherwise become main.
    ///
    /// A ref and a parent may only name a real id, never a line number (a
    /// compaction renumbers lines), so a fork from an entry written before
    /// entries carried ids — or off a main whose tip is one — is refused
    /// rather than written wrong.
    pub fn submit_at(
        &mut self,
        at: Option<&str>,
        input: &Value,
        author: &str,
        message: Option<&str>,
    ) -> Result<Committed, Rejected> {
        let (ops, author, message, at, expected) = match input {
            Value::Array(ops) => (
                ops.clone(),
                author.to_string(),
                message.map(str::to_string),
                at.map(str::to_string),
                None,
            ),
            Value::Object(body) => {
                let Some(Value::Array(ops)) = body.get("ops") else {
                    return Err(Rejected::one(
                        "send a JSON array of ops, or {\"ops\": [...], \"author\": ..., \"message\": ..., \"at\": ..., \"expected_revision\": ...}",
                    ));
                };
                // The revision the author read, when they say so
                // (spec/session.md, "Authoring"): a world that moved
                // since refuses the batch instead of landing a stale edit.
                let expected = match body.get("expected_revision") {
                    None | Some(Value::Null) => None,
                    Some(Value::Number(n)) if n.as_u64().is_some() => n.as_u64(),
                    Some(_) => {
                        return Err(Rejected::one(
                            "\"expected_revision\" is the revision you read (a number)",
                        ));
                    }
                };
                let author = body
                    .get("author")
                    .and_then(Value::as_str)
                    .unwrap_or(author)
                    .to_string();
                let message = body
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(message.map(str::to_string));
                let at = match body.get("at") {
                    None => at.map(str::to_string),
                    // Null is the base, as it is for `goto`. On an empty log
                    // that is the head, said out loud; once there is history
                    // the base can't be built on — an entry always builds on
                    // an entry, the format has no second root — and a batch
                    // that asked for it must not land on main instead.
                    Some(Value::Null) if self.entries.is_empty() => None,
                    Some(Value::Null) => {
                        return Err(Rejected::one(
                            "\"at\": null is the base, which can't be built on once there is \
                             history: build \"at\" the first entry and undo its ops in the same \
                             batch, or name an entry id (leave \"at\" out for the head)",
                        ));
                    }
                    Some(Value::String(id)) => Some(id.clone()),
                    Some(_) => {
                        return Err(Rejected::one(
                            "\"at\" is the id of the entry to build on (null: the head)",
                        ));
                    }
                };
                (ops.clone(), author, message, at, expected)
            }
            _ => return Err(Rejected::one("send a JSON array of ops")),
        };
        // Naming main's tip is committing on main.
        let branch_from = at.filter(|at| Some(at.as_str()) != self.main_tip().as_deref());
        // The world the batch is read against: the head, or the fork point.
        let start = match &branch_from {
            None => self.head.clone(),
            Some(at) => self.fork_point(at)?.0,
        };
        if let Some(expected) = expected {
            // The head's revision, or the fork point's: the document the
            // batch is read against, which is the one the author read.
            let now = match &branch_from {
                None => self.revision(),
                Some(at) => self
                    .entries
                    .iter()
                    .find(|e| e.id.as_deref() == Some(at.as_str()))
                    .map(|e| e.revision)
                    .ok_or_else(|| Rejected::one(format!("no entry '{at}' in this log")))?,
            };
            if now != expected {
                return Err(Rejected::one(format!(
                    "the world is at revision {now}, not {expected}: read it again, then decide again"
                )));
            }
        }
        if ops.is_empty() {
            return Err(Rejected::one("the batch holds no ops"));
        }

        // Kept to prove, below, that the entry as logged folds back to the
        // world it makes.
        let before = start.clone();
        let mut ingest = Ingest {
            dir: self.dir.clone(),
            next_id: self.next_entity_id().max(start.next_id()),
            trial: start,
            spawned: BTreeMap::new(),
            stored: BTreeMap::new(),
        };
        let mut committed = Vec::with_capacity(ops.len());
        let mut errors = Vec::new();
        for (i, raw) in ops.into_iter().enumerate() {
            match ingest.op(raw) {
                Ok(op) => committed.push(op),
                Err(e) => errors.push(format!("op {i}: {e}")),
            }
        }
        if !errors.is_empty() {
            return Err(Rejected { errors });
        }

        let next = compose(&self.head_manifest, &ingest.trial);
        let mut warnings = Vec::new();
        for issue in wt::validate_manifest(&next, &wt::WorldLimits::default()) {
            match issue.severity {
                wt::Severity::Error => {
                    errors.push(format!("the world after this batch: {}", issue.message))
                }
                wt::Severity::Warning => warnings.push(issue.message),
            }
        }
        // A behavior whose target this batch deleted is among those
        // warnings (openworldformat 0.3.6 validates it), not a refusal: the
        // delete was what the batch asked for, and the behavior looks at
        // nothing until retargeted or dropped — the receipt says so.
        if !errors.is_empty() {
            return Err(Rejected { errors });
        }

        let revision = self.next_revision();
        let mut entry = entry(
            revision,
            &author,
            json!({"via": "ops", "message": message}),
            &committed,
        );
        // The entry as it will be logged must fold, by the strict fold every
        // reader uses, to the world it made in memory. A difference means
        // the package would disagree with its own history — as it did when
        // a reference bound in memory but not in the log — so refuse rather
        // than write that, and say it is the committer's fault.
        folds_back(&before, &entry, &ingest.trial).map_err(Rejected::one)?;
        let forked = branch_from.is_some();
        let entry = match branch_from {
            None => self.commit(entry, ingest.trial, next),
            Some(at) => {
                entry.parent = Some(at);
                self.commit_branch(entry)
            }
        }
        .map_err(|e| Rejected::one(format!("couldn't write the package: {e}")))?;
        Ok(Committed {
            revision,
            ops: committed,
            spawned: ingest.spawned,
            stored: ingest.stored,
            warnings,
            entry,
            forked,
        })
    }

    /// The tip whose fold is the head — spec/package.md's rule, stated once
    /// in the format crate: `refs.main`, else the log's last entry, else
    /// `None` for the base.
    pub fn main_tip(&self) -> Option<String> {
        openworldformat::main_tip(&self.meta, &self.entries)
    }

    /// The next revision. Revisions are the authority's total order across
    /// every branch, so this is one past the highest anywhere in the log —
    /// not past the head's, which a branch entry may already have taken.
    /// `undo` names what it undid by revision, so a collision would make it
    /// take back the wrong entry.
    fn next_revision(&self) -> u64 {
        self.entries
            .iter()
            .map(|e| e.revision)
            .max()
            .unwrap_or(self.meta.base_revision)
            .max(self.meta.head_revision)
            + 1
    }

    /// The next entity id nobody has used on any branch. One authority
    /// never mints the same id twice, so two branches' spawns never collide
    /// — the collision a merge would otherwise have to reallocate. Every
    /// spawn in the log counts, not only those on the head's path.
    fn next_entity_id(&self) -> u64 {
        let spawned = self
            .entries
            .iter()
            .flat_map(|e| e.ops.iter().filter_map(SessionOp::as_edit))
            .flat_map(spawned_ids)
            .max()
            .map_or(0, |id| id + 1);
        self.head_manifest
            .next_entity_id
            .max(self.head.next_id())
            .max(spawned)
    }

    /// The world at entry `at`, for a batch that branches from there — or
    /// why there can be no branch there.
    fn fork_point(&self, at: &str) -> Result<(WorldDoc, Vec<usize>), Rejected> {
        if !self.entries.iter().any(|e| e.id.as_deref() == Some(at)) {
            let synthesized = self
                .entries
                .iter()
                .enumerate()
                .any(|(n, e)| entry_id(n, e) == at);
            return Err(Rejected::one(if synthesized {
                format!(
                    "entry '{at}' has no id (it predates ids), and a branch's parent must \
                     name one — branch from a newer entry"
                )
            } else {
                format!("no entry '{at}' in this log to branch from")
            }));
        }
        if let Some(tip) = self.main_tip()
            && tip.starts_with("line-")
        {
            return Err(Rejected::one(format!(
                "main's tip ('{tip}') has no id (it predates ids), and refs.main may only \
                 name one — commit once on main, then branch"
            )));
        }
        let (doc, ids) = openworldformat::fold_path(&self.base, &self.entries, Some(at))
            .map_err(|e| Rejected::one(format!("the history doesn't fold to '{at}': {e}")))?;
        Ok((doc, path_indices(&self.entries, &ids)))
    }

    /// Undo the newest batch nobody has undone, by appending its inverse —
    /// the log never rewinds. Undo and redo entries are markers: an undo
    /// skips them, and `redo_at` takes an undo back.
    ///
    /// Only the head's own path counts: on a branched log the newest line in
    /// the file may belong to another branch, and taking it back would apply
    /// an inverse to a world that never held what it undoes.
    pub fn undo(&mut self, author: &str) -> Result<Committed, Rejected> {
        self.undo_at(None, author)
    }

    /// Undo on the path to entry `at` — main's when `at` is `None` or names
    /// main's tip. The newest batch on that path nobody has undone is taken
    /// back by appending its inverse as `at`'s child, so off main the undo is
    /// a branch commit like any other: the head stays where it was. This is
    /// what a person looking at a branch means by undo.
    pub fn undo_at(&mut self, at: Option<&str>, author: &str) -> Result<Committed, Rejected> {
        self.undo_scoped(at, author, UndoScope::Anyone)
    }

    /// Undo within a scope: anyone's newest, or the author's own newest —
    /// what a person's Cmd+Z means in a room with agents. An own undo of a
    /// batch that a later batch built on (the same entities) is refused,
    /// naming the later one: undo that first, or take the newer route.
    pub fn undo_scoped(
        &mut self,
        at: Option<&str>,
        author: &str,
        scope: UndoScope,
    ) -> Result<Committed, Rejected> {
        self.take_back(at, author, scope, false)
    }

    /// Redo: take back the newest undo on the path that nobody has redone,
    /// by appending the undo's own inverse. A batch committed after the
    /// undo ends the redo, as in any editor; within `UndoScope::Own` only the
    /// author's entries count either way.
    pub fn redo_at(
        &mut self,
        at: Option<&str>,
        author: &str,
        scope: UndoScope,
    ) -> Result<Committed, Rejected> {
        self.take_back(at, author, scope, true)
    }

    fn take_back(
        &mut self,
        at: Option<&str>,
        author: &str,
        scope: UndoScope,
        redo: bool,
    ) -> Result<Committed, Rejected> {
        let verb = if redo { "redo" } else { "undo" };
        let branch_at = at
            .filter(|at| Some(*at) != self.main_tip().as_deref())
            .map(str::to_string);
        let (start, path) = match &branch_at {
            None => {
                let (_, path) = fold_head(&self.base, &self.entries, &self.meta)
                    .map_err(|e| Rejected::one(format!("the history no longer folds: {e}")))?;
                (self.head.clone(), path)
            }
            Some(at) => self.fork_point(at)?,
        };
        let on_path: Vec<&OpLogEntry> = path.iter().map(|&i| &self.entries[i]).collect();
        let (undone, redone) = marks(&on_path);
        let mine = |e: &OpLogEntry| scope == UndoScope::Anyone || e.author.name == author;
        let k = if redo {
            // Newest undo not yet redone, walking back past redo markers
            // and redone undos; a batch met first means nothing to redo.
            let mut found = None;
            for (i, e) in on_path.iter().enumerate().rev() {
                if !mine(e) || redo_of(e).is_some() {
                    continue;
                }
                if undo_of(e).is_some() {
                    if !redone.contains(&e.revision) {
                        found = Some(i);
                        break;
                    }
                    continue;
                }
                if !e.edit_ops().is_empty() {
                    break;
                }
            }
            found.ok_or_else(|| Rejected::one("nothing to redo"))?
        } else {
            on_path
                .iter()
                .rposition(|e| mine(e) && is_batch(e) && !undone.contains(&e.revision))
                .ok_or_else(|| Rejected::one("nothing to undo"))?
        };
        let target = on_path[k].revision;
        // Within a scope the target may not be the newest: a later batch
        // that touched the same entities built on it, and taking it back
        // from under that batch would be a silent clobber. Markers and
        // batches that stand undone built nothing that is still there.
        let mine_touched = touched(&on_path[k].edit_ops());
        for later in on_path[k + 1..]
            .iter()
            .filter(|e| is_batch(e) && !undone.contains(&e.revision))
        {
            let shared: Vec<Touch> = touched(&later.edit_ops())
                .intersection(&mine_touched)
                .cloned()
                .collect();
            if !shared.is_empty() {
                let names: Vec<String> = shared.iter().map(|t| self.describe_touch(t)).collect();
                return Err(Rejected::one(format!(
                    "revision {target} can't be {verb}ne: revision {} by {} built on it ({}); \
                     take that back first",
                    later.revision,
                    later.author.name,
                    names.join(", ")
                )));
            }
        }
        // The path is a chain, so folding its prefix is the state just
        // before the target — which is what an inverse is computed against.
        let prefix: Vec<OpLogEntry> = on_path[..k].iter().map(|e| (*e).clone()).collect();
        let before = fold_log(&self.base, &prefix)
            .map_err(|e| Rejected::one(format!("the history no longer folds: {e}")))?;
        let inverse = sync::compute_inverse(&before, &on_path[k].edit_ops());
        let before_ids: HashSet<u64> = start.entities().map(|e| e.id.0).collect();
        let mut trial = start;
        trial.apply_all(&inverse).map_err(|e| {
            Rejected::one(format!(
                "revision {target} can't be {verb}ne: later changes depend on it ({e})"
            ))
        })?;
        // What the inverse takes with it beyond the target's own entities —
        // a child another author hung under an entity this batch spawned,
        // which a delete's cascade would sweep — is theirs: refuse, naming
        // it, rather than silently take it too.
        let own: HashSet<u64> = on_path[k].edit_ops().iter().flat_map(spawned_ids).collect();
        let swept: Vec<String> = before_ids
            .iter()
            .filter(|id| !trial.contains(**id) && !own.contains(id))
            .map(|id| self.describe_touch(&Touch::Entity(*id)))
            .collect();
        if !swept.is_empty() {
            return Err(Rejected::one(format!(
                "revision {target} can't be {verb}ne: taking it back would also remove {}, \
                 which others built under it; take those back first",
                swept.join(", ")
            )));
        }
        let said = if redo {
            format!("redid revision {}", undo_of(on_path[k]).unwrap_or(target))
        } else {
            format!("undid revision {target}")
        };
        let revision = self.next_revision();
        let mut entry = entry(
            revision,
            author,
            json!({"via": verb, "of": target}),
            &inverse,
        );
        let forked = branch_at.is_some();
        let entry = match branch_at {
            None => {
                let next = compose(&self.head_manifest, &trial);
                self.commit(entry, trial, next)
            }
            Some(at) => {
                entry.parent = Some(at);
                self.commit_branch(entry)
            }
        }
        .map_err(|e| Rejected::one(format!("couldn't write the package: {e}")))?;
        Ok(Committed {
            revision,
            ops: inverse,
            spawned: BTreeMap::new(),
            stored: BTreeMap::new(),
            warnings: vec![said],
            entry,
            forked,
        })
    }

    /// A touched thing, for a refusal: an entity by name, or the scene-wide
    /// thing by what it is.
    fn describe_touch(&self, touch: &Touch) -> String {
        match touch {
            Touch::Entity(id) => self
                .head
                .get(*id)
                .map(|e| e.name.0.clone())
                .unwrap_or_else(|| format!("entity {id}")),
            Touch::Environment => "the environment".into(),
            Touch::Camera => "the camera".into(),
            Touch::Ambience => "the ambience".into(),
            Touch::World(field) => format!("the world's {field}"),
            Touch::Emitter(name) => format!("emitter {name}"),
        }
    }

    /// Put `manifest.json` back if something other than the committer wrote
    /// it. Returns what happened, for the person or agent to hear — and
    /// notes it in `.live/refusals.jsonl`, since whoever wrote the file
    /// directly is not on the API to be told.
    pub fn guard(&mut self) -> Option<String> {
        let on_disk = std::fs::read(self.dir.join(MANIFEST)).ok();
        let sha = on_disk.as_deref().map(sha256_hex);
        if sha.is_some() && sha == self.meta.world_sha256 {
            return None;
        }
        // Git's files, mid-checkout or just checked out, are not a direct
        // write: the canvas follows the checkout, and nothing is put back
        // over it.
        if self.git_moved() {
            return None;
        }
        // A hash on record means the committer wrote the file before and
        // someone changed it since; none means a fresh package whose
        // manifest is being normalized for the first time — not a refusal.
        let was_written = self.meta.world_sha256.is_some();
        let said = match self.restore_manifest() {
            Ok(()) => format!(
                "{MANIFEST} was changed outside the committer; the world only changes \
                 through ops, so it is back at revision {}",
                self.meta.head_revision
            ),
            Err(e) => format!("{MANIFEST} couldn't be restored: {e}"),
        };
        if was_written {
            self.note_refusal(&format!("a direct write to {MANIFEST}"), &said);
        }
        Some(said)
    }

    /// Put `manifest.json` back as the head: the file alone once its hash
    /// is on record — the same bytes the last commit wrote, so `package.json`
    /// is left as that commit wrote it and a git tree stays clean for a
    /// checkout — and the whole head the first time, when the hash is not.
    fn restore_manifest(&mut self) -> Result<(), LiveError> {
        if self.meta.world_sha256.is_none() {
            return self.write_head();
        }
        let text = to_pretty(&self.head_manifest)?;
        write_atomic(&self.dir.join(MANIFEST), text.as_bytes())?;
        // The record follows what was written, so the next tick settles
        // instead of restoring again; `package.json` keeps the committed
        // record until the next commit, which keeps a git tree clean.
        self.meta.world_sha256 = Some(sha256_hex(text.as_bytes()));
        Ok(())
    }

    /// Append a refusal to `.live/refusals.jsonl`: what was tried, why it
    /// did not take, and the revision the world stands at.
    fn note_refusal(&self, what: &str, why: &str) {
        let line = json!({
            "at_ms": now_ms(),
            "what": what,
            "why": why,
            "revision": self.meta.head_revision,
        });
        let dir = self.dir.join(LIVE);
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(REFUSALS))
        {
            use std::io::Write;
            let _ = writeln!(file, "{line}");
        }
    }

    /// The commit the package's git `HEAD` names now, when it is a
    /// repository.
    pub fn git_head(&self) -> Option<String> {
        read_git_head(&self.dir)
    }

    /// Whether git's `HEAD` is not the commit this committer last made or
    /// read — a checkout, a reset or a pull happened under the open
    /// package. Cheap enough to ask every frame: `.git/HEAD` and its ref
    /// are read directly; git itself only for a packed ref.
    pub fn git_moved(&self) -> bool {
        self.is_git() && read_git_head(&self.dir) != self.git_head
    }

    /// Take the current `HEAD` as the known one without reopening — when a
    /// checkout led somewhere that does not open as a package, so the
    /// question is not asked again until it moves once more.
    pub fn acknowledge_git(&mut self) {
        self.git_head = read_git_head(&self.dir);
    }

    /// The branch `HEAD` is on, or `HEAD` itself when detached.
    pub fn git_branch(&self) -> Option<String> {
        if !self.is_git() {
            return None;
        }
        git(&self.dir, &["rev-parse", "--abbrev-ref", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Read the package again from disk — after a checkout moved the files
    /// under this process, which is not a direct write to put back.
    pub fn reopen(&mut self) -> Result<(), LiveError> {
        if !self.dir.join(PACKAGE).is_file() {
            return Err(LiveError(format!(
                "{PACKAGE} is not there: this checkout holds a world, not a live package, \
                 and opening it would write one into the tree"
            )));
        }
        *self = Self::open(&self.dir)?;
        Ok(())
    }

    /// Where the package disagrees with itself: the fold against
    /// `manifest.json`, and every asset the world uses against its hash.
    /// Empty when the history tells the whole story.
    pub fn verify(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let manifest: WorldManifest = match read_json(&self.dir, MANIFEST) {
            Ok(m) => m,
            Err(e) => return vec![e.0],
        };
        let folded = match fold_head(&self.base, &read_log(&self.dir), &self.meta) {
            Ok((doc, _)) => doc,
            Err(e) => return vec![format!("{LOG} no longer folds: {e}")],
        };
        let in_file: HashSet<u64> = manifest.entities.iter().map(|e| e.id.0).collect();
        for entity in folded.entities() {
            if !in_file.contains(&entity.id.0) {
                problems.push(format!(
                    "entity {} is in the history, not in {MANIFEST}",
                    entity.id.0
                ));
            }
        }
        for entity in &manifest.entities {
            match folded.get(entity.id.0) {
                None => problems.push(format!(
                    "entity {} is in {MANIFEST}, not in the history",
                    entity.name.0
                )),
                Some(f) if !wt::values_close(entity, f) => {
                    problems.push(format!("entity {} differs from the history", entity.name.0))
                }
                Some(_) => {}
            }
        }
        if !wt::values_close(&manifest.environment, &folded.environment) {
            problems.push("environment differs from the history".into());
        }
        if !wt::values_close(&manifest.camera, &folded.camera) {
            problems.push("camera differs from the history".into());
        }
        // The rest of the scene-wide fields are the history's too:
        // ModifyWorld reaches every one of them, SetAmbience the ambience.
        let differs = [
            ("meta", !wt::values_close(&manifest.meta, &folded.meta())),
            (
                "avatar",
                !wt::values_close(&manifest.avatar, &folded.avatar),
            ),
            ("tours", !wt::values_close(&manifest.tours, &folded.tours)),
            (
                "soundtrack",
                !wt::values_close(&manifest.soundtrack, &folded.soundtrack),
            ),
            (
                "ambience",
                !wt::values_close(&manifest.ambience, &folded.ambience),
            ),
            (
                "creations",
                !wt::values_close(&manifest.creations, &folded.creations),
            ),
        ];
        for (key, _) in differs.into_iter().filter(|(_, differs)| *differs) {
            problems.push(format!("{key} differs from the history"));
        }
        for path in referenced_assets(&manifest) {
            match content_address(&path) {
                Some(expected) => match std::fs::read(self.asset_path(&path)) {
                    Ok(bytes) if sha256_hex(&bytes) == expected => {}
                    Ok(_) => problems.push(format!(
                        "{ASSETS}/{path} no longer holds the bytes its name promises"
                    )),
                    Err(_) => problems.push(format!("{ASSETS}/{path} is missing")),
                },
                None => problems.push(format!("{ASSETS}/{path} isn't content-addressed")),
            }
        }
        problems
    }

    /// Append, then name the new bytes in `package.json`, then write them:
    /// a reader in another process that sees the new manifest also sees
    /// the entry and the metadata that explain it. In a git repository the
    /// batch is then a commit too, authored by whoever sent it.
    ///
    /// Every entry gets its identity here, before it is appended: an
    /// explicit `parent` when the entry it builds on has a real id, and an
    /// `id` that is the format's content hash (`compute_entry_id`: canonical
    /// JSON of every field but `id`, `message` included). Branching needs
    /// both — a ref MUST name an entry id, never a line number — and they are
    /// what make an entry the same entry across forks and copies. A previous
    /// entry with no id (a log from before this) keeps the implicit chain,
    /// which for an append is exactly right.
    ///
    /// Returns the entry as written, because that — not what the caller
    /// built — is what a view following the authority must append.
    fn commit(
        &mut self,
        mut entry: OpLogEntry,
        head: WorldDoc,
        manifest: WorldManifest,
    ) -> Result<OpLogEntry, LiveError> {
        if entry.parent.is_none() {
            // Main's tip, not the last line: once branches exist they are
            // not the same entry. A synthesized `line-<n>` stays implicit —
            // for an append onto the last line, file order already says it.
            entry.parent = self.main_tip().filter(|t| !t.starts_with("line-"));
        }
        self.stamp(&mut entry)?;
        append(&self.dir, &entry)?;
        let (author, subject, body) = describe_entry(&entry);
        self.entries.push(entry.clone());
        self.head = head;
        self.head_manifest = manifest;
        self.meta.head_revision = entry.revision;
        // Once main is named, it moves with every commit to it.
        if self.meta.refs.contains_key("main")
            && let Some(id) = &entry.id
        {
            self.meta.refs.insert("main".into(), id.clone());
        }
        self.write_head()?;
        if self.is_git() {
            git_commit(&self.dir, &author, &subject, &body)?;
            self.git_head = read_git_head(&self.dir);
        }
        Ok(entry)
    }

    /// Append an entry that starts or extends a branch. The head does not
    /// move; `refs.main` is written first if it isn't yet, so the new last
    /// line cannot be mistaken for main.
    fn commit_branch(&mut self, mut entry: OpLogEntry) -> Result<OpLogEntry, LiveError> {
        if !self.meta.refs.contains_key("main")
            && let Some(tip) = self.main_tip()
        {
            // `fork_point` refused a main tip without an id, which a ref
            // cannot name. (An empty log has no fork point to branch from.)
            self.meta.refs.insert("main".into(), tip);
        }
        self.stamp(&mut entry)?;
        append(&self.dir, &entry)?;
        let (author, subject, body) = describe_entry(&entry);
        let body = match &entry.parent {
            Some(parent) => format!("{body}\n\noff main — builds on {parent}"),
            None => body,
        };
        self.entries.push(entry.clone());
        // package.json names the refs and the log's new hash; manifest.json
        // is main's and is rewritten with the same bytes.
        self.write_head()?;
        if self.is_git() {
            git_commit(&self.dir, &author, &subject, &body)?;
            self.git_head = read_git_head(&self.dir);
        }
        Ok(entry)
    }

    /// Give an entry its content-hash id (the format's `compute_entry_id`:
    /// canonical JSON of every field but `id`, `message` included).
    fn stamp(&self, entry: &mut OpLogEntry) -> Result<(), LiveError> {
        if entry.id.is_none() {
            entry.id = Some(
                openworldformat::oplog::compute_entry_id(entry)
                    .map_err(|e| LiveError(format!("couldn't hash the entry: {e}")))?,
            );
        }
        Ok(())
    }

    /// Whether the package is a git repository (each batch a commit).
    pub fn is_git(&self) -> bool {
        self.dir.join(".git").exists()
    }

    fn write_head(&mut self) -> Result<(), LiveError> {
        let text = to_pretty(&self.head_manifest)?;
        self.meta.world_sha256 = Some(sha256_hex(text.as_bytes()));
        self.meta.log_sha256 = std::fs::read(self.dir.join(LOG))
            .ok()
            .map(|b| sha256_hex(&b));
        self.meta.updated_ms = now_ms();
        write_atomic(&self.dir.join(PACKAGE), to_pretty(&self.meta)?.as_bytes())?;
        write_atomic(&self.dir.join(MANIFEST), text.as_bytes())?;
        Ok(())
    }
}

/// The state one batch builds up as its ops come in.
struct Ingest {
    dir: PathBuf,
    trial: WorldDoc,
    next_id: u64,
    spawned: BTreeMap<String, u64>,
    stored: BTreeMap<String, String>,
}

impl Ingest {
    /// Bind, read strictly, store assets, apply to the trial.
    fn op(&mut self, mut raw: Value) -> Result<EditOp, String> {
        self.bind(&mut raw)?;
        let mut op: EditOp = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
        let mut unknown = Vec::new();
        dropped_keys(&raw, &to_value(&op), String::new(), &mut unknown);
        unknown_entity_keys(&op, &mut unknown);
        if !unknown.is_empty() {
            return Err(unknown
                .iter()
                .map(|p| {
                    format!("{p} is not a field of the format (readers would drop it — a typo?)")
                })
                .collect::<Vec<_>>()
                .join("; "));
        }
        // The deeper references — a behavior's target or center, a camera's
        // aim and focus — bind here as the top-level ones did above, so the
        // log carries ids in every marked field (spec/session.md: committed
        // ops always carry ids). The format's one list says which fields;
        // a name nothing has refuses the op. Without this, a target sent by
        // name was committed as the name, the fold bound it and the
        // package no longer verified against its own history.
        self.trial
            .bind_committed_refs(&mut op)
            .map_err(|e| e.to_string())?;
        self.store_assets(&mut op)?;
        self.trial
            .apply(&op)
            .map_err(|e| format!("doesn't apply: {e}"))?;
        Ok(op)
    }

    /// Names to ids, new ids for spawns that left theirs out, and partial
    /// struct patches merged into the current values.
    fn bind(&mut self, raw: &mut Value) -> Result<(), String> {
        let Some(object) = raw.as_object_mut() else {
            return Err("an op is an object like {\"SpawnEntity\": {...}}".into());
        };
        if object.len() != 1 {
            return Err(format!(
                "an op holds exactly one kind, one of: {}",
                OP_KINDS.join(", ")
            ));
        }
        let (kind, body) = object.iter_mut().next().expect("one key");
        match kind.as_str() {
            "SpawnEntity" => {
                let entity = body
                    .get_mut("entity")
                    .and_then(Value::as_object_mut)
                    .ok_or("SpawnEntity needs an \"entity\" object")?;
                match entity.get("id") {
                    None | Some(Value::Null) => {
                        entity.insert("id".into(), json!(self.next_id));
                    }
                    Some(Value::Number(_)) => {}
                    Some(_) => {
                        return Err(
                            "a new entity's id is a number — or leave it out to get one".into()
                        );
                    }
                }
                let id = entity
                    .get("id")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.next_id);
                self.next_id = self.next_id.max(id + 1);
                if let Some(parent) = entity.get_mut("parent") {
                    self.resolve(parent)?;
                }
                if let Some(name) = entity.get("name").and_then(Value::as_str) {
                    self.spawned.insert(name.to_string(), id);
                }
            }
            "ModifyEntity" => {
                let id = body
                    .get_mut("id")
                    .ok_or("ModifyEntity needs an \"id\" (or a name)")?;
                self.resolve(id)?;
                let id = id.as_u64().unwrap_or_default();
                let current = self.trial.get(id).map(to_value);
                if let Some(patch) = body.get_mut("patch").and_then(Value::as_object_mut) {
                    if let Some(parent) = patch.get_mut("parent") {
                        self.resolve(parent)?;
                    }
                    if let Some(current) = current {
                        for field in ["transform", "material", "light"] {
                            if let (Some(change @ Value::Object(_)), Some(now @ Value::Object(_))) =
                                (patch.get_mut(field), current.get(field))
                            {
                                *change = merged(now, change);
                            }
                        }
                    }
                }
            }
            "DeleteEntity" => {
                let id = body
                    .get_mut("id")
                    .ok_or("DeleteEntity needs an \"id\" (or a name)")?;
                self.resolve(id)?;
            }
            "SetEnvironment" => {
                if let (Some(change @ Value::Object(_)), Some(now)) =
                    (body.get_mut("env"), self.trial.environment.as_ref())
                {
                    *change = merged(&to_value(now), change);
                }
            }
            "Batch" => {
                let ops = body
                    .get_mut("ops")
                    .and_then(Value::as_array_mut)
                    .ok_or("Batch needs an \"ops\" array")?;
                for op in ops {
                    self.bind(op)?;
                }
            }
            "ModifyWorld" => {
                let patch = body
                    .get_mut("patch")
                    .and_then(Value::as_object_mut)
                    .ok_or("ModifyWorld needs a \"patch\" object")?;
                // A field that is one value merges into the world's, as
                // SetEnvironment's env does — send just a soundtrack's
                // `sections` to cut it again. A list (tours, ambience,
                // creations) is replaced whole, `meta` too, as the format
                // has it; `null` clears.
                let now = [
                    ("environment", self.trial.environment.as_ref().map(to_value)),
                    ("camera", self.trial.camera.as_ref().map(to_value)),
                    ("avatar", self.trial.avatar.as_ref().map(to_value)),
                    ("soundtrack", self.trial.soundtrack.as_ref().map(to_value)),
                ];
                for (field, current) in now {
                    if let (Some(change @ Value::Object(_)), Some(current)) =
                        (patch.get_mut(field), current)
                    {
                        *change = merged(&current, change);
                    }
                }
            }
            "SetCamera" | "SetAmbience" | "SpawnAudioEmitter" | "RemoveAudioEmitter" => {}
            other => {
                return Err(format!(
                    "\"{other}\" isn't an op kind; the format has: {}",
                    OP_KINDS.join(", ")
                ));
            }
        }
        Ok(())
    }

    /// A name becomes the id it names now (in the trial, which holds this
    /// batch's earlier ops); ids pass through.
    fn resolve(&self, reference: &mut Value) -> Result<(), String> {
        if let Value::String(name) = reference {
            let id = self
                .spawned
                .get(name.as_str())
                .copied()
                .or_else(|| self.trial.get_by_name(name).map(|e| e.id.0))
                .ok_or_else(|| format!("no entity is named \"{name}\""))?;
            *reference = json!(id);
        }
        Ok(())
    }

    /// Every asset an op references gets stored by its hash; the op then
    /// points at the copy.
    fn store_assets(&mut self, op: &mut EditOp) -> Result<(), String> {
        match op {
            EditOp::SpawnEntity { entity } => self.store_entity_assets(entity),
            EditOp::ModifyEntity { patch, .. } => {
                if let Some(Some(mesh)) = patch.mesh_asset.as_mut() {
                    self.store_mesh(mesh)?;
                }
                if let Some(Some(material)) = patch.material.as_mut() {
                    self.store_material(material)?;
                }
                Ok(())
            }
            // A song plays from `assets/`, like any file the world uses.
            EditOp::ModifyWorld { patch } => {
                if let Some(Some(song)) = patch.soundtrack.as_mut()
                    && let Some(path) = song.path.as_mut()
                {
                    *path = self.store(path)?.0;
                }
                Ok(())
            }
            EditOp::Batch { ops } => ops.iter_mut().try_for_each(|op| self.store_assets(op)),
            _ => Ok(()),
        }
    }

    fn store_entity_assets(&mut self, entity: &mut WorldEntity) -> Result<(), String> {
        if let Some(mesh) = entity.mesh_asset.as_mut() {
            self.store_mesh(mesh)?;
        }
        if let Some(material) = entity.material.as_mut() {
            self.store_material(material)?;
        }
        Ok(())
    }

    fn store_mesh(&mut self, mesh: &mut wt::MeshAssetRef) -> Result<(), String> {
        let (path, sha) = self.store(&mesh.path)?;
        if let Some(claimed) = &mesh.sha256
            && claimed.trim_start_matches("sha256:") != sha
        {
            return Err(format!(
                "{ASSETS}/{} has sha256 {sha}, not the {claimed} the op claims",
                mesh.path
            ));
        }
        mesh.path = path;
        mesh.sha256 = Some(sha);
        Ok(())
    }

    fn store_material(&mut self, material: &mut wt::MaterialDef) -> Result<(), String> {
        for slot in [
            &mut material.base_color_texture,
            &mut material.metallic_roughness_texture,
            &mut material.normal_map_texture,
            &mut material.emissive_texture,
        ] {
            if let Some(path) = slot.as_mut() {
                *path = self.store(path)?.0;
            }
        }
        Ok(())
    }

    /// Store one referenced file by its hash: `(content-addressed path, sha256)`.
    /// A path already in content-addressed form must still hold the bytes
    /// its name promises — those files never change.
    fn store(&mut self, path: &str) -> Result<(String, String), String> {
        let relative = safe_relative(path)?;
        let assets = self.dir.join(ASSETS);
        let source = assets.join(&relative);
        let size = std::fs::metadata(&source)
            .map_err(|_| format!("{ASSETS}/{path} doesn't exist — write the file first"))?
            .len();
        if size > MAX_ASSET_BYTES {
            return Err(format!(
                "{ASSETS}/{path} is {size} bytes, over the {MAX_ASSET_BYTES} limit"
            ));
        }
        let bytes = std::fs::read(&source).map_err(|e| format!("{ASSETS}/{path}: {e}"))?;
        let sha = sha256_hex(&bytes);
        if let Some(named) = content_address(path) {
            return if named == sha {
                Ok((path.to_string(), sha))
            } else {
                Err(format!(
                    "{ASSETS}/{path} was changed in place — stored assets never change; \
                     write the new version as a new file and reference that"
                ))
            };
        }
        let extension = relative
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        let stored = format!("{sha}{extension}");
        let target = assets.join(&stored);
        if !target.exists() {
            write_atomic(&target, &bytes)
                .map_err(|e| format!("can't store {ASSETS}/{path}: {e}"))?;
        }
        self.stored.insert(path.to_string(), stored.clone());
        Ok((stored, sha))
    }
}

/// A path inside `assets/`, or why not.
fn safe_relative(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if path.is_empty() || p.is_absolute() {
        return Err(format!("\"{path}\": asset paths are relative to {ASSETS}/"));
    }
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("\"{path}\" leaves {ASSETS}/"));
    }
    Ok(p.to_path_buf())
}

/// The hash a content-addressed name promises (`<sha256>.<ext>`).
fn content_address(path: &str) -> Option<String> {
    let stem = path.split('.').next()?;
    (stem.len() == 64 && !path.contains('/') && stem.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| stem.to_ascii_lowercase())
}

/// Every asset a manifest's entities reference, relative to `assets/`.
fn referenced_assets(manifest: &WorldManifest) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for entity in &manifest.entities {
        if let Some(mesh) = &entity.mesh_asset {
            paths.push(mesh.path.clone());
        }
        if let Some(material) = &entity.material {
            paths.extend(material.textures().map(|(_, p)| p.to_string()));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

/// JSON merge patch (RFC 7396) of `change` over `current`: objects merge
/// key by key, `null` removes, anything else replaces.
fn merged(current: &Value, change: &Value) -> Value {
    match (current, change) {
        (Value::Object(now), Value::Object(delta)) => {
            let mut out = now.clone();
            for (key, value) in delta {
                if value.is_null() {
                    out.remove(key);
                } else {
                    let next = out
                        .get(key)
                        .map_or_else(|| value.clone(), |old| merged(old, value));
                    out.insert(key.clone(), next);
                }
            }
            Value::Object(out)
        }
        _ => change.clone(),
    }
}

/// Keys in `raw` the typed value didn't keep: fields the format doesn't
/// have, which every reader would silently drop. Explicit nulls are fine.
fn dropped_keys(raw: &Value, back: &Value, path: String, out: &mut Vec<String>) {
    match (raw, back) {
        (Value::Object(raw), Value::Object(back)) => {
            for (key, value) in raw {
                let here = format!("{path}/{key}");
                match back.get(key) {
                    Some(kept) => dropped_keys(value, kept, here, out),
                    None if !value.is_null() => out.push(here),
                    None => {}
                }
            }
        }
        (Value::Array(raw), Value::Array(back)) => {
            for (i, (value, kept)) in raw.iter().zip(back).enumerate() {
                dropped_keys(value, kept, format!("{path}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// An entity keeps keys it doesn't know in `extra`, the room for
/// extensions; only `ext-*` keys belong there.
fn unknown_entity_keys(op: &EditOp, out: &mut Vec<String>) {
    match op {
        EditOp::SpawnEntity { entity } => out.extend(
            entity
                .extra
                .keys()
                .filter(|k| !k.starts_with("ext-"))
                .map(|k| format!("/SpawnEntity/entity/{k}")),
        ),
        EditOp::Batch { ops } => ops.iter().for_each(|op| unknown_entity_keys(op, out)),
        _ => {}
    }
}

/// The head as a manifest: `prev`'s fields no op reaches, the document's
/// entities in `prev`'s order (new ones after, parents before children).
fn compose(prev: &WorldManifest, doc: &WorldDoc) -> WorldManifest {
    let mut manifest = prev.clone();
    let mut order: Vec<&WorldEntity> = Vec::new();
    let mut seen = HashSet::new();
    for entity in &prev.entities {
        if let Some(now) = doc.get(entity.id.0) {
            order.push(now);
            seen.insert(entity.id.0);
        }
    }
    for entity in doc.entities_parent_first() {
        if seen.insert(entity.id.0) {
            order.push(entity);
        }
    }
    // Stable: an entity waits until its parent has been written.
    let mut written: HashSet<u64> = HashSet::new();
    let mut waiting: HashMap<u64, Vec<&WorldEntity>> = HashMap::new();
    let mut out = Vec::with_capacity(order.len());
    for entity in order {
        match entity.parent {
            Some(parent) if doc.contains(parent.0) && !written.contains(&parent.0) => {
                waiting.entry(parent.0).or_default().push(entity);
            }
            _ => release(entity, &mut written, &mut waiting, &mut out),
        }
    }
    // Cycles would leave entities waiting; validation reports those.
    out.extend(waiting.into_values().flatten().cloned());
    manifest.entities = out;
    // Every scene-wide field is the document's: ModifyWorld changes any of
    // them, and SetAmbience the ambience.
    manifest.meta = doc.meta();
    manifest.environment = doc.environment.clone();
    manifest.camera = doc.camera.clone();
    manifest.avatar = doc.avatar.clone();
    manifest.tours = doc.tours.clone();
    manifest.soundtrack = doc.soundtrack.clone();
    manifest.ambience = doc.ambience.clone();
    manifest.creations = doc.creations.clone();
    manifest.next_entity_id = prev.next_entity_id.max(doc.next_id());
    manifest
}

fn release(
    entity: &WorldEntity,
    written: &mut HashSet<u64>,
    waiting: &mut HashMap<u64, Vec<&WorldEntity>>,
    out: &mut Vec<WorldEntity>,
) {
    out.push(entity.clone());
    written.insert(entity.id.0);
    for child in waiting.remove(&entity.id.0).unwrap_or_default() {
        release(child, written, waiting, out);
    }
}

/// The id the format gives entry `n`: its own, or `line-<n>` when it has
/// none (spec/session.md, "Entry identity").
fn entry_id(n: usize, e: &OpLogEntry) -> String {
    e.id.clone().unwrap_or_else(|| format!("line-{n}"))
}

/// The head, and the indices of the entries on the path to it, base-first.
///
/// spec/package.md's head rule: `manifest.json` is the fold of the *path* to
/// main's tip — `refs.main`, else the log's last entry — never a fold of
/// every line in file order. On a linear log the two are the same, which is
/// why folding every line went unnoticed; on a branched one it produces a
/// world that never existed on any branch (both variants at once), and that
/// is what the canvas drew and what `manifest.json` was written as.
///
/// Which tip is main's is the format crate's `main_tip` — the rule stated
/// once, so this and every other reader agree once branches exist and the
/// last line is no longer main's.
fn fold_head(
    base: &WorldDoc,
    entries: &[OpLogEntry],
    meta: &SessionMeta,
) -> Result<(WorldDoc, Vec<usize>), ApplyError> {
    if entries.is_empty() {
        return Ok((base.clone(), Vec::new()));
    }
    let tip = openworldformat::main_tip(meta, entries);
    let (doc, ids) = openworldformat::fold_path(base, entries, tip.as_deref())?;
    Ok((doc, path_indices(entries, &ids)))
}

/// The log indices of a path's entry ids, base-first.
fn path_indices(entries: &[OpLogEntry], ids: &[String]) -> Vec<usize> {
    let index: HashMap<String, usize> = entries
        .iter()
        .enumerate()
        .map(|(n, e)| (entry_id(n, e), n))
        .collect();
    ids.iter().filter_map(|id| index.get(id).copied()).collect()
}

fn entry(revision: u64, author: &str, intent: Value, ops: &[EditOp]) -> OpLogEntry {
    let message = intent
        .get("message")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .map(str::to_string);
    let mut session_ops = vec![SessionOp::Tool(ToolRecord {
        tool: "live".into(),
        args: intent,
        result_hash: None,
        phase: None,
        timestamp_ms: None,
    })];
    session_ops.extend(ops.iter().cloned().map(|op| SessionOp::Edit(Box::new(op))));
    OpLogEntry {
        revision,
        author: Author {
            peer: None,
            name: author.to_string(),
            // The batch envelope names its author; whether a person or a
            // model sent it is not something this intake can vouch for.
            source: None,
        },
        ops: session_ops,
        timestamp_ms: now_ms(),
        id: None,
        parent: None,
        // Draft 0.3: an entry carries its commit message, and the message is
        // part of the entry's identity. The tool record keeps the intent as
        // history; this is the field readers and the content hash use.
        message,
    }
}

/// An entry as a commit: author, subject (the sender's message, or what the
/// ops did), and a body listing the ops.
fn describe_entry(entry: &OpLogEntry) -> (String, String, String) {
    let ops = entry.edit_ops();
    let lines = describe_ops(&ops);
    let intent = entry.ops.iter().find_map(|op| match op {
        SessionOp::Tool(t) if t.tool == "live" => Some(&t.args),
        _ => None,
    });
    let subject = match intent {
        Some(args) if args["via"] == "undo" => format!("undo revision {}", args["of"]),
        Some(args) if args["via"] == "redo" => format!("redo of revision {}", args["of"]),
        Some(args) => args["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| lines.first().cloned().unwrap_or_else(|| "edit".into())),
        None => "edit".into(),
    };
    let body = format!("revision {}\n\n{}", entry.revision, lines.join("\n"));
    (entry.author.name.clone(), subject, body)
}

/// One line per op, for people: `spawn #21 lantern`, `modify #10 (transform)`.
pub fn describe_ops(ops: &[EditOp]) -> Vec<String> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            EditOp::SpawnEntity { entity } => {
                out.push(format!("spawn #{} {}", entity.id.0, entity.name.0))
            }
            EditOp::DeleteEntity { id } => out.push(format!("delete #{}", id.0)),
            EditOp::ModifyEntity { id, patch } => {
                let value = to_value(patch);
                let fields: Vec<&str> = value
                    .as_object()
                    .map(|o| o.keys().map(String::as_str).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|k| match k {
                        "name" | "transform" | "parent" | "shape" | "material" | "light"
                        | "behaviors" | "audio" | "mesh_asset" | "modulations" | "instance_of"
                        | "triggers" => k,
                        _ => "extension",
                    })
                    .collect();
                out.push(format!("modify #{} ({})", id.0, fields.join(", ")));
            }
            EditOp::SetEnvironment { .. } => out.push("set environment".into()),
            EditOp::SetCamera { .. } => out.push("set camera".into()),
            EditOp::SetAmbience { .. } => out.push("set ambience".into()),
            EditOp::SpawnAudioEmitter { name, .. } => out.push(format!("add emitter {name}")),
            EditOp::RemoveAudioEmitter { name, .. } => out.push(format!("remove emitter {name}")),
            EditOp::ModifyWorld { patch } => {
                let value = to_value(patch);
                let fields: Vec<&str> = value
                    .as_object()
                    .map(|o| o.keys().map(String::as_str).collect())
                    .unwrap_or_default();
                out.push(format!("modify world ({})", fields.join(", ")));
            }
            EditOp::Batch { ops } => out.extend(describe_ops(ops)),
        }
    }
    out
}

/// The commit `HEAD` names: `.git/HEAD` read directly, through its ref when
/// it is symbolic; git asked only when the ref is packed. `None` when the
/// folder is not a repository or the branch is unborn.
fn read_git_head(dir: &Path) -> Option<String> {
    let git_dir = dir.join(".git");
    if !git_dir.exists() {
        return None;
    }
    if git_dir.is_file() {
        // A worktree or a submodule: `.git` names the real git dir. Ask
        // git rather than follow the pointer and its common dir by hand.
        return git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let direct = match head.strip_prefix("ref: ") {
        Some(reference) => std::fs::read_to_string(git_dir.join(reference.trim()))
            .ok()
            .map(|s| s.trim().to_string()),
        None => Some(head.to_string()),
    };
    direct.filter(|s| !s.is_empty()).or_else(|| {
        git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// Run git in the package; its error output becomes the error.
fn git(dir: &Path, args: &[&str]) -> Result<String, LiveError> {
    git_bytes(dir, args).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Run git in the package for its raw output — a blob's bytes.
fn git_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, LiveError> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=LocalGPT Live",
            "-c",
            "user.email=live@localgpt.invalid",
        ])
        .args(args)
        .output()
        .map_err(|e| LiveError(format!("git: {e}")))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(LiveError(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_commit(dir: &Path, author: &str, subject: &str, body: &str) -> Result<(), LiveError> {
    git(dir, &["add", "--all", "--", MANIFEST, LOG, PACKAGE, ASSETS])?;
    let author = format!("{author} <{}@agents.localgpt.invalid>", slug(author));
    git(
        dir,
        &[
            "commit", "--quiet", "--author", &author, "-m", subject, "-m", body,
        ],
    )?;
    Ok(())
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "someone".into() } else { s }
}

/// Make a live package a git repository: the open app's files stay out
/// of it, and concurrent appends to the log merge as a union.
pub fn init_git(dir: &Path) -> Result<(), LiveError> {
    if dir.join(".git").exists() {
        return Ok(());
    }
    git(dir, &["init", "--quiet"])?;
    std::fs::write(dir.join(".gitignore"), format!("{LIVE}/\n"))?;
    std::fs::write(dir.join(".gitattributes"), format!("{LOG} merge=union\n"))?;
    git(dir, &["add", "--all"])?;
    git(
        dir,
        &[
            "commit",
            "--quiet",
            "-m",
            "base",
            "-m",
            "the world before any ops",
        ],
    )?;
    Ok(())
}

/// One commit of the world's history, as a keyframe.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub commit: String,
    pub author: String,
    pub subject: String,
    pub manifest: WorldManifest,
}

/// The world at every commit that changed it, oldest first — on the
/// current branch, or on `rev` (a branch, a tag, a commit). Each commit
/// holds a whole `manifest.json`, so a replay walks keyframes and needs no
/// fold; a snapshot that doesn't parse is skipped.
pub fn git_history(dir: &Path, rev: Option<&str>) -> Result<Vec<Snapshot>, LiveError> {
    let rev = rev.unwrap_or("HEAD");
    let log = git(
        dir,
        &[
            "log",
            "--reverse",
            "--format=%H%x1f%an%x1f%s",
            rev,
            "--",
            MANIFEST,
        ],
    )?;
    let mut snapshots = Vec::new();
    for line in log.lines() {
        let mut parts = line.split('\x1f');
        let (Some(commit), Some(author), Some(subject)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Ok(text) = git(dir, &["show", &format!("{commit}:{MANIFEST}")]) else {
            continue;
        };
        if let Ok(manifest) = serde_json::from_str(&text) {
            snapshots.push(Snapshot {
                commit: commit.to_string(),
                author: author.to_string(),
                subject: subject.to_string(),
                manifest,
            });
        }
    }
    Ok(snapshots)
}

/// The ops that turn one keyframe into the next — what a canvas applies
/// to step a replay forward (or back).
pub fn diff_manifests(from: &WorldManifest, to: &WorldManifest) -> Vec<EditOp> {
    let Ok(doc) = WorldDoc::from_manifest(from) else {
        return Vec::new();
    };
    let mut ops = sync::diff_scene(&doc, &to.entities, to.environment.as_ref());
    // diff_scene leaves triggers and extensions out; a replay shows them too.
    for entity in &to.entities {
        if let Some(old) = doc.get(entity.id.0) {
            let mut patch = wt::EntityPatch::between(old, entity);
            patch.transform = None;
            patch.shape = None;
            patch.material = None;
            patch.light = None;
            patch.behaviors = None;
            patch.audio = None;
            patch.mesh_asset = None;
            patch.modulations = None;
            if !patch.is_empty() {
                ops.push(EditOp::modify(entity.id, patch));
            }
        }
    }
    ops
}

/// The entity ids an op spawns, nested batches included.
fn spawned_ids(op: &EditOp) -> Vec<u64> {
    match op {
        EditOp::SpawnEntity { entity } => vec![entity.id.0],
        EditOp::Batch { ops } => ops.iter().flat_map(spawned_ids).collect(),
        _ => Vec::new(),
    }
}

/// Whose entries an undo or a redo may take back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoScope {
    /// The newest on the path, whoever made it.
    Anyone,
    /// The author's own newest, skipping others' — a person's Cmd+Z in a
    /// room with agents.
    Own,
}

/// The head as a manifest: the file itself when it is the record's — the
/// same bytes again, so the guard never mistakes its own writing for a
/// direct write — else composed from the base and the fold. Composing is
/// not byte-stable across opens: a fresh composition can order entities
/// differently from one grown commit by commit, and a guard comparing
/// hashes would then restore the file every tick, note a refusal each
/// time and leave a git tree dirty.
fn head_as_written(
    dir: &Path,
    meta: &SessionMeta,
    base_manifest: &WorldManifest,
    head: &WorldDoc,
) -> WorldManifest {
    let composed = compose(base_manifest, head);
    let Some(recorded) = meta.world_sha256.as_deref() else {
        return composed;
    };
    let Ok(bytes) = std::fs::read(dir.join(MANIFEST)) else {
        return composed;
    };
    if sha256_hex(&bytes) != recorded {
        return composed;
    }
    serde_json::from_slice::<WorldManifest>(&bytes).unwrap_or(composed)
}

/// The revision an undo entry undid.
fn undo_of(entry: &OpLogEntry) -> Option<u64> {
    marker_of(entry, "undo")
}

/// The undo entry a redo entry took back.
fn redo_of(entry: &OpLogEntry) -> Option<u64> {
    marker_of(entry, "redo")
}

fn marker_of(entry: &OpLogEntry, via: &str) -> Option<u64> {
    entry.ops.iter().find_map(|op| match op {
        SessionOp::Tool(t) if t.tool == "live" && t.args["via"] == via => t.args["of"].as_u64(),
        _ => None,
    })
}

/// A batch someone made, as opposed to an undo or redo marker.
fn is_batch(entry: &OpLogEntry) -> bool {
    !entry.edit_ops().is_empty() && undo_of(entry).is_none() && redo_of(entry).is_none()
}

/// Which revisions on a path stand undone right now, and which undo entries
/// have been redone — the markers replayed in order, so an undo, a redo and
/// an undo again come out as they went.
fn marks(on_path: &[&OpLogEntry]) -> (HashSet<u64>, HashSet<u64>) {
    let mut undone = HashSet::new();
    let mut redone = HashSet::new();
    for e in on_path {
        if let Some(n) = undo_of(e) {
            undone.insert(n);
        } else if let Some(u) = redo_of(e) {
            redone.insert(u);
            if let Some(n) = on_path
                .iter()
                .find(|x| x.revision == u)
                .and_then(|x| undo_of(x))
            {
                undone.remove(&n);
            }
        }
    }
    (undone, redone)
}

/// What a batch's ops touch: entities by id, and the scene-wide things an
/// op can set — the environment, the camera, the ambience, a world field,
/// an audio emitter by name — so an undo that would silently take back
/// another author's later change to the same thing is refused, whatever
/// the thing is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Touch {
    Entity(u64),
    Environment,
    Camera,
    Ambience,
    World(String),
    Emitter(String),
}

fn touched(ops: &[EditOp]) -> HashSet<Touch> {
    let mut set = HashSet::new();
    fn walk(op: &EditOp, set: &mut HashSet<Touch>) {
        match op {
            EditOp::SpawnEntity { entity } => {
                set.insert(Touch::Entity(entity.id.0));
            }
            EditOp::DeleteEntity { id } | EditOp::ModifyEntity { id, .. } => {
                set.insert(Touch::Entity(id.0));
            }
            EditOp::SetEnvironment { .. } => {
                set.insert(Touch::Environment);
            }
            EditOp::SetCamera { .. } => {
                set.insert(Touch::Camera);
            }
            EditOp::SetAmbience { .. } => {
                set.insert(Touch::Ambience);
            }
            EditOp::SpawnAudioEmitter { name, .. } | EditOp::RemoveAudioEmitter { name, .. } => {
                set.insert(Touch::Emitter(name.clone()));
            }
            EditOp::ModifyWorld { patch } => {
                // Every field the patch names, cleared or set.
                let world = |field: &str| Touch::World(field.to_string());
                let named: [(bool, Touch); 8] = [
                    (patch.meta.is_some(), world("meta")),
                    (patch.environment.is_some(), Touch::Environment),
                    (patch.camera.is_some(), Touch::Camera),
                    (patch.avatar.is_some(), world("avatar")),
                    (patch.tours.is_some(), world("tours")),
                    (patch.soundtrack.is_some(), world("soundtrack")),
                    (patch.ambience.is_some(), Touch::Ambience),
                    (patch.creations.is_some(), world("creations")),
                ];
                set.extend(named.into_iter().filter(|(set, _)| *set).map(|(_, t)| t));
            }
            EditOp::Batch { ops } => ops.iter().for_each(|op| walk(op, set)),
        }
    }
    ops.iter().for_each(|op| walk(op, &mut set));
    set
}

/// Whether `entry`, folded strictly onto `before` the way every reader
/// folds the log, gives `after` — the world the committer made in memory.
/// Per entity, with the tolerance `verify` uses, plus the environment and
/// the camera. The error names what differs; it is a bug in the committer,
/// never in the batch.
fn folds_back(before: &WorldDoc, entry: &OpLogEntry, after: &WorldDoc) -> Result<(), String> {
    let refolded = fold_log(before, std::slice::from_ref(entry))
        .map_err(|e| format!("the entry as logged doesn't fold: {e}"))?;
    let mut differ: Vec<String> = Vec::new();
    for entity in after.entities() {
        match refolded.get(entity.id.0) {
            Some(folded) if wt::values_close(entity, folded) => {}
            _ => differ.push(entity.name.0.clone()),
        }
    }
    for entity in refolded.entities() {
        if after.get(entity.id.0).is_none() {
            differ.push(entity.name.0.clone());
        }
    }
    if !wt::values_close(&after.environment, &refolded.environment) {
        differ.push("environment".into());
    }
    if !wt::values_close(&after.camera, &refolded.camera) {
        differ.push("camera".into());
    }
    if differ.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the entry as logged would not fold to the world it makes ({}): the committer \
             would write a package that disagrees with its own history. Nothing was written; \
             this is a bug in the committer, please report it",
            differ.join(", ")
        ))
    }
}

/// The refusals noted in `.live/refusals.jsonl`, oldest first: what was
/// tried outside the API and why it did not take.
pub fn refusals(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join(LIVE).join(REFUSALS))
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The assets as they stood at `commit`, under `<into>/<commit>/assets/…`:
/// each blob cached once by hash in `<into>/blobs/` and linked into place,
/// so a replay loads what that commit held rather than today's bytes. The
/// root returned has the package's layout (`assets/` inside it) and the
/// commit's own files; a commit with no `assets/` gives an empty root.
pub fn assets_at(dir: &Path, commit: &str, into: &Path) -> Result<PathBuf, LiveError> {
    let root = into.join(commit);
    let blobs = into.join("blobs");
    std::fs::create_dir_all(&blobs)?;
    let listing = git(dir, &["ls-tree", "-r", "-z", commit, "--", ASSETS])?;
    for record in listing.split('\0').filter(|r| !r.is_empty()) {
        // "<mode> <type> <sha>\t<path>"
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split_whitespace();
        let (Some(_mode), Some(kind), Some(sha)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if kind != "blob" {
            continue;
        }
        let blob = blobs.join(sha);
        if !blob.exists() {
            let bytes = git_bytes(dir, &["cat-file", "blob", sha])?;
            write_atomic(&blob, &bytes)?;
        }
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !target.exists() {
            std::fs::hard_link(&blob, &target)
                .or_else(|_| std::fs::copy(&blob, &target).map(|_| ()))?;
        }
    }
    Ok(root)
}

/// What a live package has beyond the format's four files: the guide an
/// agent reads, and `assets/` for the files it writes. Written when
/// missing, never over — so a package drafted by `localgpt-previs` or
/// unzipped from a download gets them the first time it opens.
fn furnish(dir: &Path) -> Result<(), LiveError> {
    std::fs::create_dir_all(dir.join(ASSETS))?;
    if !dir.join(AGENTS).exists() {
        std::fs::write(dir.join(AGENTS), AGENTS_GUIDE)?;
    }
    Ok(())
}

/// Make `dir` a live package around the manifest it holds.
fn init(dir: &Path) -> Result<(), LiveError> {
    let bytes = std::fs::read(dir.join(MANIFEST))
        .map_err(|e| LiveError(format!("{}: {e}", dir.join(MANIFEST).display())))?;
    let manifest: WorldManifest = serde_json::from_slice(&bytes)
        .map_err(|e| LiveError(format!("{MANIFEST} isn't a world: {e}")))?;
    std::fs::create_dir_all(dir.join("snapshots"))?;
    std::fs::create_dir_all(dir.join(LIVE))?;
    write_atomic(&dir.join(BASE), &bytes)?;
    if !dir.join(LOG).exists() {
        std::fs::write(dir.join(LOG), b"")?;
    }
    furnish(dir)?;
    let mut meta = SessionMeta::new(manifest.meta.name.clone());
    meta.app = Some("localgpt-live".into());
    write_atomic(&dir.join(PACKAGE), to_pretty(&meta)?.as_bytes())?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(dir: &Path, file: &str) -> Result<T, LiveError> {
    let bytes = std::fs::read(dir.join(file)).map_err(|e| LiveError(format!("{file}: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| LiveError(format!("{file}: {e}")))
}

/// The log's entries; unreadable lines are skipped (a torn tail loses itself).
fn read_log(dir: &Path) -> Vec<OpLogEntry> {
    std::fs::read_to_string(dir.join(LOG))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| sync::decode_line(l).ok())
        .collect()
}

fn append(dir: &Path, entry: &OpLogEntry) -> Result<(), LiveError> {
    use std::io::Write as _;
    let line = sync::encode_line(entry).map_err(|e| LiveError(e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG))?;
    file.write_all(format!("{line}\n").as_bytes())?;
    file.sync_data()?;
    Ok(())
}

/// Write a temp file beside the target, then rename it over: a reader sees
/// the old file or the new one, never half of one.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Pretty JSON with arrays of plain values on one line —
/// `"position": [0.0, 1.5, 0.0]` — so a world reads like one and diffs
/// small. Numbers keep their shortest form (f32s print as written).
pub fn to_pretty<T: serde::Serialize>(value: &T) -> Result<String, LiveError> {
    let pretty = serde_json::to_string_pretty(value).map_err(|e| LiveError(e.to_string()))?;
    let lines: Vec<&str> = pretty.lines().collect();
    let mut out = String::with_capacity(pretty.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with('[') {
            let mut items = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let item = lines[j].trim();
                if item.starts_with(']') {
                    break;
                }
                items.push(item.trim_end_matches(','));
                j += 1;
            }
            let plain = items
                .iter()
                .all(|item| !item.ends_with('{') && !item.ends_with('[') && !item.starts_with('{'));
            if j < lines.len() && plain {
                let close = lines[j].trim();
                out.push_str(line);
                out.push_str(&items.join(", "));
                out.push_str(close);
                out.push('\n');
                i = j + 1;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    Ok(out)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What `AGENTS.md` says in a new live package.
pub const AGENTS_GUIDE: &str = r#"# This folder is a live world

`manifest.json` is the world as it is now, in the Open World Format
(https://openworldformat.org). LocalGPT shows it live while it's open
(`localgpt-app --world <this folder>`). Read `manifest.json` freely, but don't
write it: the app does, and puts it back if anything else does.

## Change the world by sending ops

Send a batch of edit ops to the open app. A batch applies whole or not at all:
if any op is wrong, nothing changes and the reply says which op and why.

```sh
W=/path/to/this/folder   # the folder this file is in; your shell may not be in it
URL=$(jq -r .url "$W/.live/endpoint.json"); TOKEN=$(jq -r .token "$W/.live/endpoint.json")
# no jq: URL=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["url"])' "$W/.live/endpoint.json")
curl -s -X POST "$URL/ops" -H "Authorization: Bearer $TOKEN" -d '{
  "author": "your-name", "message": "what this change is for",
  "ops": [
    {"SpawnEntity": {"entity": {"name": "lantern",
      "transform": {"position": [2.0, 0.5, 1.0]},
      "shape": {"Cylinder": {"radius": 0.1, "height": 1.0}},
      "material": {"color": [0.2, 0.2, 0.2, 1.0], "emissive": [4.0, 2.8, 1.2, 1.0]}}}},
    {"ModifyEntity": {"id": "cuboid", "patch": {"transform": {"position": [0.0, 2.0, 0.0]}}}},
    {"DeleteEntity": {"id": "torus"}}
  ]}'
```

- Use an entity's name wherever an id goes. Leave `id` out of a new entity and
  it gets one; the reply lists them.
- The reply is your receipt: `result` (`committed`, or `proposed` under
  review), `revision` and `id` of the entry, `parent`, `head`, `branch` (true
  when it landed off main), `changes` (one line per op, like `spawn #101
  bench`), `spawned` (name → id), `stored` (asset → its copy), `warnings`
  and `on_screen`; or `errors`, one per failing op, and nothing changed. A
  refusal is HTTP 422 with `{"result": "rejected", "errors": [...]}`; a
  stale revision reads `"the world is at revision 1, not 0: read it again,
  then decide again"`.
- Add `"expected_revision": N` — the `revision` you read from `GET /log` (or
  the entry's, when building on one) — when the batch must land on the world
  you read. If anyone moved it since, the batch is refused naming both
  revisions; read again, then decide again. Leave it out to bind by name
  against the world as it is now. Revisions number every entry across the
  whole tree in the order written, so a branch tip can carry a higher
  revision than the head; `expected_revision` is always compared with the
  revision of the entry you build on — the head's, or the `at` entry's.
- With review on in the canvas, the reply is `proposed` (HTTP 202) and your
  batch waits on the canvas for the person to accept or reject it: poll
  `GET /proposals` for its status, and don't resend meanwhile.
- A `patch` holds only what changes. `transform`, `material` and `light` merge
  into the current value (send just `position` to move something); every
  other field — `behaviors`, `shape`, `parent`, `audio`, `mesh_asset` — is
  replaced whole; `null` removes a field.
- Op kinds: SpawnEntity, ModifyEntity, DeleteEntity (takes children with it),
  SetEnvironment (merges too), SetCamera, ModifyWorld, Batch.
- ModifyWorld changes the world's scene-wide fields: `{"ModifyWorld":
  {"patch": {"soundtrack": {...}}}}`. `soundtrack`, `environment`, `camera`
  and `avatar` merge into the current value; `tours`, `ambience`,
  `creations` and `meta` are replaced whole; `null` clears one.
- Names bind wherever the format marks a reference — an op's `id`, `parent`,
  a behavior's `target` or `center`, a camera's `aim_entity` and
  `focus_entity`, the avatar's `model_entity` — and commit as ids. A name
  nothing has refuses the batch, naming it.
- Behaviors animate an entity without code: `"behaviors": [...]` on an entity
  or in a patch, the new list replacing the old (`[]` or `null` clears it).
  `{"LookAt": {"target": "bench"}}` turns the entity to face another;
  `{"Orbit": {"center": "sun", "radius": 3, "speed": 30}}` circles one
  (`center_point: [x, y, z]` circles a point; `axis`, `tilt`, `phase`);
  `{"Spin": {"axis": [0, 1, 0], "speed": 90}}` (degrees a second);
  `{"Bob": {"axis": [0, 1, 0], "amplitude": 0.2, "frequency": 0.5}}`;
  `{"Pulse": {"min_scale": 0.9, "max_scale": 1.1, "frequency": 1}}`;
  `{"PathFollow": {"waypoints": [[0, 1, 0], [4, 1, 0]], "speed": 1}}`;
  Bounce. Delete a target later and the receipt warns; the behavior stays
  and looks at nothing until retargeted or dropped. Read back, a behavior
  carries the id it bound to and every field, defaults filled (`axis`,
  `phase`, `tilt`), so expect a bigger block than you sent.
- Meters, Y up. Rotations: `rotation_degrees`, X then Y then Z. Colors: RGBA
  0–1, sRGB; `emissive` is linear and glows above 1.
- Shapes: copy the forms in `manifest.json` — {"Cuboid": {"x": 1, "y": 1, "z": 1}},
  {"Sphere": {"radius": 0.5}}, {"Cylinder": {"radius": 0.2, "height": 3}},
  {"Plane": {"x": 10, "z": 10}}, Cone, Capsule, Torus, Pyramid, Wedge …
  A position is the shape's center, for every shape (a pyramid's base sits
  at −height/2): a 1.5 m cuboid rests on the ground at y 0.75.

## Assets are files you write

Write meshes (.glb), textures (.png) and sounds into `assets/` under any name,
then reference the file by its path inside `assets/` in an op:
`"material": {"base_color_texture": "brick.png"}`. The app stores a copy named
by its SHA-256 and points the world at that. Stored copies never change: to
change a texture, write new bytes (to the same working name is fine) and send
the op again — that makes a new version, and the old one stays for history.

## A song

A world can perform a song. Write the audio file into `assets/`, then set the
world's `soundtrack` with ModifyWorld: `path` (inside `assets/`, stored like
any asset), `title`, `artist`, `license`, `duration` in seconds, `bpm` and
`beat_offset` (the first beat, in seconds) for the beat grid, `sections` (where
each section starts, as fractions of the duration, from 0) and `energy` (the
loudness, one value a second, 0–1). The open app plays the file and draws its
timeline from these numbers — the clock is `duration` long — and modulations
read `energy`, `beat` and the stems from them. Send just `sections` later to
cut the song again.

## Cameras and shots (ext-cinematography)

An entity carrying `ext-cinematography.camera` is a camera; one also carrying
`ext-cinematography.shot` is a setup in the shot list. The entity's transform
places it; the entity's name is the shot's name ("12A"). Everything is an
ordinary entity op — revising a shot is a `ModifyEntity` patch, no special op
kind.

`camera` fields: `sensor_mm` `[w, h]` (default `[24.89, 18.66]`, Super 35),
`focal_length_mm` (default 35), `aspect_ratio` (the frame crop; default the
sensor's), `squeeze` (default 1), `aim` `[x, y, z]` (look at this world
point, +Y up; absent: the entity's local −Z), `aim_entity` (an entity the
camera looks at — write its name, it commits as the id — over `aim` while
the entity exists, so the shot follows the actor when the blocking moves),
`focus_distance_m`, `focus_entity` (an entity the camera focuses on, over
`focus_distance_m`), `f_stop`. A drafted single tracks its actor's head this
way (`aim_entity: "MAYA.head"`): keep those fields when you revise it, unless
the ask is to aim somewhere else. `shot` fields: `scene`, `order` (integer
sort key; ties break by entity id), `in_s`/`out_s` (seconds on the clock),
`size` (EWS, WS, MS, MCU, CU, ECU, INSERT, OTS, POV, TWO), `description`.

An `ext-*` patch replaces the whole extension value, so a revision sends the
full block back with its change. Read the entity from `manifest.json` first
(the fold at the tip you're building on), change the fields, send it:

- **"stay on her"** / **"track him"** — aim and focus by entity, so the
  camera keeps the actor when they move; `aim` stays as the fallback point:

  ```json
  {"ModifyEntity": {"id": "1B", "patch": {"ext-cinematography": {
    "camera": {"focal_length_mm": 50, "aspect_ratio": 2.39, "aim": [0.7, 1.6, 0.0],
               "aim_entity": "MAYA.head", "focus_entity": "MAYA.head"},
    "shot": {"scene": "1", "order": 2, "in_s": 0.5, "out_s": 1.5,
             "size": "MCU", "description": "her"}}}}}
  ```

- **"a 50"** — the lens is `focal_length_mm`:

  ```json
  {"ModifyEntity": {"id": "1B", "patch": {"ext-cinematography": {
    "camera": {"focal_length_mm": 50, "aspect_ratio": 2.39, "aim": [0.7, 1.6, 0.0]},
    "shot": {"scene": "1", "order": 2, "in_s": 0.5, "out_s": 1.5,
             "size": "MCU", "description": "her"}}}}}
  ```

- **"eye level"** — the camera's height is its transform position's y; 1.6 m
  is a standing eye, ~1.2 m sitting. The transform merges, so send just the
  position:

  ```json
  {"ModifyEntity": {"id": "1B", "patch": {"transform": {"position": [0.7, 1.6, 2.0]}}}}
  ```

- **"from the doorway"** — put the camera where the doorway is and aim it at
  the subject (find both from `manifest.json`):

  ```json
  {"ModifyEntity": {"id": "1B", "patch": {
    "transform": {"position": [3.0, 1.6, 4.5]},
    "ext-cinematography": {
      "camera": {"focal_length_mm": 35, "aspect_ratio": 2.39, "aim": [0.0, 1.0, 0.0]},
      "shot": {"scene": "1", "order": 2, "in_s": 0.5, "out_s": 1.5,
               "size": "MCU", "description": "her, from the doorway"}}}}}
  ```

Reorder the shot list by patching `shot.order` the same way. To try a
treatment without touching main, send the revision batch `"at"` the entry it
builds on — the branch is the treatment.

- `GET $URL/board` — the previs board of the entry on screen as a printable
  page (`?tip=<id>` for any entry, `?tip=head` for main's tip). Add
  `&vs=<id>` to put two tips' boards side by side, changed shots marked —
  how two treatments get compared.

## See and ask

- `GET $URL/world` — the head as a manifest (what `manifest.json` holds);
  `?tip=current` for the entry the window shows, `?tip=<id>` for any entry,
  `?tip=null` for the base — an unknown tip is a 404, never the head by
  mistake. `GET $URL/verify` — whether the package agrees with its history
  (`ok`, `problems`).
- `GET $URL/screenshot` — renders the view now and replies with the PNG's
  path, `tip`, `revision` and `at_head`; shots are saved as
  `.live/shot-N.png`, each kept. It is the person's camera: look at
  `.live/preview.png` first to see whether your corner of the world is in
  frame. There is no framing it from here short of `SetCamera`, which moves
  the shared camera and is a recorded change like any other.
- `GET $URL/selection` — what the person has selected; "this" means it.
- `POST $URL/undo -d '{"author": "your-name"}'` — undoes the head's newest
  change by appending its inverse: a new entry under your name (`you` when
  the body names none), with a receipt like `/ops`'s (the same fields;
  `spawned` and `stored` empty) and `warnings: ["undid revision N"]`. `{"at": "<id>"}`: the newest on that entry's path
  instead, as a branch. `{"own": true}`: your own newest, skipping others' —
  refused, naming the entry, if someone built on it since. `POST $URL/redo`
  takes the newest undo back the same way (`author` and `own` too); a batch
  since the undo ends it.
- `.live/preview.png` refreshes by itself after every change.
- Writing `manifest.json` or `ops.jsonl` yourself does nothing lasting: the
  committer puts them back and notes it in `.live/refusals.jsonl`
  (`GET $URL/refusals`; the file appears on the first refusal). If a change
  of yours vanished, that is why — send it as a batch.

## History is a tree

Every entry in `ops.jsonl` has an `id` and builds on a `parent`. The head —
what `manifest.json` holds — is main's tip, and a batch lands there unless it
says otherwise.

- `GET $URL/log` — the history: each entry's `id`, `parent`, `revision`,
  `author`, `message` (also under `intent.message`, with `intent.via`:
  `ops`, `undo` or `redo`), `changes`, which entry is the `head`, and
  `revision` — the head's, the one `expected_revision` wants.
- `GET $URL/tips` — every branch end, which one is the `head`, and which entry
  the window is showing (`current`), with its `revision` — the entry on
  screen's, not the head's.
- `POST $URL/goto -d '{"tip": "<id>"}'` — show any entry in the window (`null`
  for before it began, `"head"` for main's tip). Nothing is written; this is
  how you look at an earlier state. The reply is `{result: "sought", tip,
  revision, changes}` — `changes` is what the window applied to get there.
  `manifest.json` and `GET /world` keep answering from the head meanwhile;
  read the shown state with `GET /world?tip=current`.
- When the folder is a git repository, every entry is also a commit, and a
  `git checkout` made under the open window is followed: within a second the
  window shows the checked-out state and `/log`, `/tips` and `/world` answer
  from it. Nothing is written back over a checkout. Entries are consecutive
  commits on the folder's one git line, branch entries included, in the
  order written; the tree — who builds on whom — lives in `ops.jsonl`, not
  in git branches.
- Add `"at": "<id>"` to a batch to build on that entry instead of the head.
  From an earlier entry this starts a branch: main and `manifest.json` stay as
  they were, and the reply's `id` is the branch's new tip — send the next
  batch `"at"` it to keep going. To try an alternative from what the person
  is looking at, build `"at"` the `current` from `/tips` — unless it is
  `null`: the base, before the first entry, can't be built on once there is
  history (`"at": null` is refused, never landed on main). To branch from
  the very beginning, build `"at"` the first entry and undo its ops in the
  same batch.
- A reply's `on_screen` says whether the window shows your change. It doesn't
  when the person is looking at another point in the history, and then a
  screenshot won't show it either.

Everything you send is recorded in `ops.jsonl` under your author name.
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A fresh package around a two-entity world.
    fn world() -> (PathBuf, LiveWorld) {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "localgpt-live-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = json!({
            "version": 3,
            "meta": {"name": "yard", "description": "a test yard"},
            "tours": [{"name": "walk", "waypoints": []}],
            "entities": [
                {"id": 1, "name": "ground", "transform": {"position": [0.0, 0.0, 0.0]},
                 "shape": {"Plane": {"x": 20.0, "z": 20.0}}},
                {"id": 2, "name": "crate", "transform": {"position": [0.0, 0.5, 0.0], "scale": [2.0, 2.0, 2.0]},
                 "shape": {"Cuboid": {"x": 1.0, "y": 1.0, "z": 1.0}},
                 "material": {"color": [0.6, 0.4, 0.2, 1.0], "roughness": 0.8}}
            ],
            "next_entity_id": 3
        });
        std::fs::write(
            dir.join(MANIFEST),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let live = LiveWorld::open(&dir).unwrap();
        (dir, live)
    }

    fn on_disk(dir: &Path) -> WorldManifest {
        read_json(dir, MANIFEST).unwrap()
    }

    #[test]
    fn a_folder_with_a_manifest_is_a_package_however_empty() {
        let dir = std::env::temp_dir().join(format!(
            "lga-is-package-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!is_package(&dir), "an empty folder is not a package yet");
        std::fs::write(dir.join(MANIFEST), b"{\"version\": 3}").unwrap();
        assert!(is_package(&dir), "a manifest makes it one, log or no log");
        assert!(
            !is_package(dir.join(MANIFEST).as_path()),
            "a file is not a package"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_batch_by_name_commits_and_the_history_tells_the_whole_story() {
        let (dir, mut live) = world();
        let done = live
            .submit(
                &json!({"author": "claude", "message": "a lamp by the crate", "ops": [
                    {"SpawnEntity": {"entity": {"name": "lamp", "parent": "crate",
                        "transform": {"position": [0.0, 1.0, 0.0]}}}},
                    {"ModifyEntity": {"id": "crate", "patch": {"transform": {"position": [3.0, 0.5, 0.0]}}}}
                ]}),
                "anyone",
                None,
            )
            .unwrap();
        assert_eq!(done.revision, 1);
        assert_eq!(done.spawned.get("lamp"), Some(&3));

        let manifest = on_disk(&dir);
        let lamp = manifest
            .entities
            .iter()
            .find(|e| e.name.0 == "lamp")
            .unwrap();
        assert_eq!(lamp.parent.map(|p| p.0), Some(2), "parent bound by name");
        let crate_ = manifest
            .entities
            .iter()
            .find(|e| e.name.0 == "crate")
            .unwrap();
        assert_eq!(crate_.transform.position, [3.0, 0.5, 0.0]);
        assert_eq!(
            crate_.transform.scale,
            [2.0, 2.0, 2.0],
            "a partial transform merges"
        );
        assert_eq!(manifest.tours.len(), 1, "what no op reaches is kept");
        assert_eq!(manifest.meta.description.as_deref(), Some("a test yard"));

        let log = read_log(&dir);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].author.name, "claude");
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn one_bad_op_leaves_the_world_untouched() {
        let (dir, mut live) = world();
        let before = std::fs::read(dir.join(MANIFEST)).unwrap();
        let refused = live
            .submit(
                &json!([
                    {"ModifyEntity": {"id": "crate", "patch": {"materail": {"color": [1.0, 0.0, 0.0, 1.0]}}}},
                    {"DeleteEntity": {"id": "nobody"}},
                    {"MoveEntity": {"id": 2}}
                ]),
                "claude",
                None,
            )
            .unwrap_err();
        assert_eq!(refused.errors.len(), 3, "{:?}", refused.errors);
        assert!(
            refused.errors[0].contains("/ModifyEntity/patch/materail"),
            "{:?}",
            refused.errors
        );
        assert!(refused.errors[1].contains("no entity is named \"nobody\""));
        assert!(refused.errors[2].contains("isn't an op kind"));
        assert_eq!(std::fs::read(dir.join(MANIFEST)).unwrap(), before);
        assert!(read_log(&dir).is_empty());
        assert_eq!(live.revision(), 0);
    }

    #[test]
    fn a_typo_on_a_new_entity_is_refused() {
        let (_dir, mut live) = world();
        let refused = live
            .submit(
                &json!([{"SpawnEntity": {"entity": {"name": "post", "postion": [1.0, 0.0, 0.0]}}}]),
                "claude",
                None,
            )
            .unwrap_err();
        assert!(
            refused.errors[0].contains("postion"),
            "{:?}",
            refused.errors
        );
    }

    #[test]
    fn undo_appends_the_inverse_and_restores_the_world() {
        let (dir, mut live) = world();
        let start = on_disk(&dir);
        live.submit(
            &json!([{"DeleteEntity": {"id": "crate"}}, {"SpawnEntity": {"entity": {"name": "post"}}}]),
            "claude",
            None,
        )
        .unwrap();
        assert!(on_disk(&dir).entities.iter().all(|e| e.name.0 != "crate"));

        let undone = live.undo("you").unwrap();
        assert_eq!(undone.revision, 2);
        let after = on_disk(&dir);
        let names = |m: &WorldManifest| {
            let mut n: Vec<String> = m.entities.iter().map(|e| e.name.0.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(names(&after), names(&start));
        assert_eq!(read_log(&dir).len(), 2, "the log grew; it never rewinds");
        assert!(live.verify().is_empty(), "{:?}", live.verify());
        assert!(live.undo("you").is_err(), "undo entries aren't undone");
    }

    #[test]
    fn a_behavior_target_sent_by_name_commits_as_the_id_and_the_package_verifies() {
        let (dir, mut live) = world();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post", "transform": {"position": [3.0, 0.5, 0.0]}}}}]),
            "claude",
            None,
        )
        .unwrap();
        let done = live
            .submit(
                &json!([{"ModifyEntity": {"id": "crate", "patch": {"behaviors": [{"LookAt": {"target": "post"}}]}}}]),
                "claude",
                None,
            )
            .unwrap();
        let logged = serde_json::to_value(&done.ops[0]).unwrap();
        assert_eq!(
            logged["ModifyEntity"]["patch"]["behaviors"][0]["LookAt"]["target"],
            json!(3),
            "the committed op carries the id: {logged}"
        );
        let crate_ = on_disk(&dir)
            .entities
            .into_iter()
            .find(|e| e.name.0 == "crate")
            .unwrap();
        assert_eq!(
            serde_json::to_value(&crate_.behaviors).unwrap(),
            json!([{"LookAt": {"target": 3}}]),
            "manifest.json carries the id"
        );
        assert!(live.verify().is_empty(), "{:?}", live.verify());
        // A spawn carrying a reference binds the same way.
        let done = live
            .submit(
                &json!([{"SpawnEntity": {"entity": {"name": "moon", "behaviors": [{"Orbit": {"center": "post", "radius": 2.0}}]}}}]),
                "claude",
                None,
            )
            .unwrap();
        let logged = serde_json::to_value(&done.ops[0]).unwrap();
        assert_eq!(
            logged["SpawnEntity"]["entity"]["behaviors"][0]["Orbit"]["center"],
            json!(3),
            "{logged}"
        );
        assert!(live.verify().is_empty(), "{:?}", live.verify());
        // A name nothing has is refused, naming it.
        let refused = live
            .submit(
                &json!([{"ModifyEntity": {"id": "crate", "patch": {"behaviors": [{"LookAt": {"target": "nobody"}}]}}}]),
                "claude",
                None,
            )
            .unwrap_err();
        assert!(refused.errors[0].contains("nobody"), "{:?}", refused.errors);
    }

    #[test]
    fn a_committed_entry_must_fold_back_to_the_world_it_made() {
        let (_dir, live) = world();
        let before = live.head().clone();
        let look_at = |target: wt::EntityRef| EditOp::ModifyEntity {
            id: wt::EntityId(2),
            patch: wt::EntityPatch {
                behaviors: Some(vec![wt::BehaviorDef::LookAt { target }]),
                ..Default::default()
            },
        };
        // The world the committer made: the reference bound to its id.
        let mut after = before.clone();
        after
            .apply_all(std::slice::from_ref(&look_at(wt::EntityRef::id(1))))
            .unwrap();
        // Logged with the id, the entry folds back to it.
        let bound = entry(1, "t", json!({}), &[look_at(wt::EntityRef::id(1))]);
        assert!(folds_back(&before, &bound, &after).is_ok());
        // What the intake used to do: apply the op with the name, keeping
        // the name in the world it made, while the fold binds a logged name
        // to the id — the two disagree, and the check names the entity.
        let by_name = look_at(wt::EntityRef::name("ground"));
        let mut kept_the_name = before.clone();
        kept_the_name
            .apply_all(std::slice::from_ref(&by_name))
            .unwrap();
        let unbound = entry(1, "t", json!({}), std::slice::from_ref(&by_name));
        let refolded = fold_log(&before, std::slice::from_ref(&unbound)).unwrap();
        let (kept, folded) = (
            kept_the_name.get(2).unwrap().behaviors.clone(),
            refolded.get(2).unwrap().behaviors.clone(),
        );
        assert_ne!(kept, folded, "the fold binds what the apply kept: {kept:?}");
        let why = folds_back(&before, &unbound, &kept_the_name).unwrap_err();
        assert!(why.contains("crate") && why.contains("disagrees"), "{why}");
    }

    #[test]
    fn undo_and_redo_stay_on_their_branch() {
        let (_dir, mut live) = world();
        let r1 = live
            .submit(
                &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
                "a",
                None,
            )
            .unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "bench"}}}]),
            "a",
            None,
        )
        .unwrap();
        // A branch from r1: a lamp that main never has.
        let r1_id = r1.entry.id.clone().expect("entries have ids");
        let r3 = live
            .submit(
                &json!({"ops": [{"SpawnEntity": {"entity": {"name": "lamp"}}}], "at": r1_id}),
                "a",
                None,
            )
            .unwrap();
        assert!(r3.forked);
        let r3_id = r3.entry.id.clone().unwrap();
        // Undo on the branch takes the lamp, as the branch's own child.
        let r4 = live.undo_at(Some(&r3_id), "a").unwrap();
        assert!(r4.forked);
        assert_eq!(r4.entry.parent.as_deref(), Some(r3_id.as_str()));
        assert_eq!(r4.warnings, vec!["undid revision 3".to_string()]);
        // Main has no undo to redo; the branch does, at its tip.
        assert!(live.redo_at(None, "a", UndoScope::Anyone).is_err());
        let r4_id = r4.entry.id.clone().unwrap();
        let r5 = live.redo_at(Some(&r4_id), "a", UndoScope::Anyone).unwrap();
        assert!(r5.forked);
        assert_eq!(r5.entry.parent.as_deref(), Some(r4_id.as_str()));
        assert_eq!(r5.warnings, vec!["redid revision 3".to_string()]);
        // Main's own undo and redo are untouched by the branch's.
        let r6 = live.undo("a").unwrap();
        assert_eq!(r6.warnings, vec!["undid revision 2".to_string()]);
        let r7 = live.redo_at(None, "a", UndoScope::Anyone).unwrap();
        assert_eq!(r7.warnings, vec!["redid revision 2".to_string()]);
        assert!(!r7.forked);
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn a_reopened_package_settles_after_one_direct_write_and_its_git_tree_stays_clean() {
        // Grown commit by commit, the head's entity order is commit order;
        // composed afresh on reopen it would be parent-first — and a guard
        // comparing hashes would then restore the file every tick.
        let (dir, first) = world();
        init_git(&dir).unwrap();
        drop(first);
        let mut live = LiveWorld::open(&dir).unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "a",
            None,
        )
        .unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "lamp", "parent": "ground"}}}]),
            "a",
            None,
        )
        .unwrap();
        live.reopen().unwrap();
        let recorded = std::fs::read(dir.join(MANIFEST)).unwrap();
        let mut tampered = recorded.clone();
        tampered.push(b'\n');
        std::fs::write(dir.join(MANIFEST), &tampered).unwrap();
        assert!(live.guard().is_some(), "the direct write is noticed");
        assert!(live.guard().is_none(), "and put back once, not every tick");
        assert_eq!(
            std::fs::read(dir.join(MANIFEST)).unwrap(),
            recorded,
            "the same bytes again"
        );
        assert_eq!(refusals(&dir).len(), 1);
        let status = git(&dir, &["status", "--porcelain"]).unwrap();
        assert_eq!(status.trim(), "", "the git tree is clean: {status}");
    }

    #[test]
    fn own_undo_refuses_a_scene_wide_change_another_built_on() {
        let (_dir, mut live) = world();
        live.submit(
            &json!([{"SetEnvironment": {"env": {"background_color": [1.0, 0.0, 0.0, 1.0]}}}]),
            "alice",
            None,
        )
        .unwrap();
        live.submit(
            &json!([{"SetEnvironment": {"env": {"background_color": [0.0, 0.0, 1.0, 1.0]}}}]),
            "bob",
            None,
        )
        .unwrap();
        let refused = live.undo_scoped(None, "alice", UndoScope::Own).unwrap_err();
        assert!(
            refused.errors[0].contains("bob") && refused.errors[0].contains("the environment"),
            "{:?}",
            refused.errors
        );
    }

    #[test]
    fn own_undo_refuses_to_sweep_what_another_hung_under_it() {
        let (_dir, mut live) = world();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "alice",
            None,
        )
        .unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "lamp", "parent": "post"}}}]),
            "bob",
            None,
        )
        .unwrap();
        let refused = live.undo_scoped(None, "alice", UndoScope::Own).unwrap_err();
        assert!(
            refused.errors[0].contains("lamp") && refused.errors[0].contains("also remove"),
            "{:?}",
            refused.errors
        );
        // Bob takes his lamp back; now Alice's post can go.
        live.undo_scoped(None, "bob", UndoScope::Own).unwrap();
        live.undo_scoped(None, "alice", UndoScope::Own).unwrap();
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn a_redo_is_labelled_as_one_in_git() {
        let (dir, first) = world();
        init_git(&dir).unwrap();
        drop(first);
        let mut live = LiveWorld::open(&dir).unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "a",
            None,
        )
        .unwrap();
        live.undo("a").unwrap();
        live.redo_at(None, "a", UndoScope::Anyone).unwrap();
        let subject = git(&dir, &["log", "-1", "--format=%s"]).unwrap();
        assert!(subject.starts_with("redo of revision"), "{subject}");
    }

    #[test]
    fn redo_takes_an_undo_back_and_a_batch_since_ends_it() {
        let (dir, mut live) = world();
        let has = |dir: &Path, name: &str| on_disk(dir).entities.iter().any(|e| e.name.0 == name);
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "claude",
            None,
        )
        .unwrap();
        live.undo("you").unwrap();
        assert!(!has(&dir, "post"));
        // After an undo, the redo is the undo's inverse.
        let redone = live.redo_at(None, "you", UndoScope::Anyone).unwrap();
        assert_eq!(redone.revision, 3);
        assert_eq!(redone.warnings, vec!["redid revision 1".to_string()]);
        assert!(has(&dir, "post"), "the redo put the post back");
        assert!(
            live.redo_at(None, "you", UndoScope::Anyone).is_err(),
            "redone once"
        );
        // Undo again takes the same batch back; redo after a new batch is over.
        live.undo("you").unwrap();
        assert!(!has(&dir, "post"));
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "bench"}}}]),
            "claude",
            None,
        )
        .unwrap();
        let refused = live.redo_at(None, "you", UndoScope::Anyone).unwrap_err();
        assert!(
            refused.errors[0].contains("nothing to redo"),
            "{:?}",
            refused.errors
        );
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn own_undo_skips_others_and_refuses_what_they_built_on() {
        let (dir, mut live) = world();
        let has = |dir: &Path, name: &str| on_disk(dir).entities.iter().any(|e| e.name.0 == name);
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "alice",
            None,
        )
        .unwrap();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "bench"}}}]),
            "bob",
            None,
        )
        .unwrap();
        // Alice's own undo takes her post, not Bob's newer bench.
        let undone = live.undo_scoped(None, "alice", UndoScope::Own).unwrap();
        assert_eq!(undone.warnings, vec!["undid revision 1".to_string()]);
        assert!(!has(&dir, "post") && has(&dir, "bench"));
        // Carol's own undo: she made nothing.
        assert!(live.undo_scoped(None, "carol", UndoScope::Own).is_err());
        // Bob paints the crate; Alice moves it; Bob's own undo would pull
        // the paint from under Alice's move — refused, naming her entry.
        live.submit(
            &json!([{"ModifyEntity": {"id": "crate", "patch": {"material": {"color": [1.0, 0.0, 0.0, 1.0]}}}}]),
            "bob",
            None,
        )
        .unwrap();
        live.submit(
            &json!([{"ModifyEntity": {"id": "crate", "patch": {"transform": {"position": [3.0, 0.5, 0.0]}}}}]),
            "alice",
            None,
        )
        .unwrap();
        let refused = live.undo_scoped(None, "bob", UndoScope::Own).unwrap_err();
        assert!(
            refused.errors[0].contains("alice") && refused.errors[0].contains("crate"),
            "{:?}",
            refused.errors
        );
        // Alice can take hers back; then Bob's goes.
        live.undo_scoped(None, "alice", UndoScope::Own).unwrap();
        live.undo_scoped(None, "bob", UndoScope::Own).unwrap();
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn a_direct_write_is_put_back_and_noted() {
        let (dir, mut live) = world();
        let before = std::fs::read(dir.join(MANIFEST)).unwrap();
        std::fs::write(dir.join(MANIFEST), b"{\"version\": 3, \"entities\": []}").unwrap();
        let said = live.guard().expect("the guard noticed");
        assert!(said.contains("outside the committer"), "{said}");
        assert_eq!(std::fs::read(dir.join(MANIFEST)).unwrap(), before);
        let notes = refusals(&dir);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0]["what"].as_str().unwrap().contains("direct write"));
        assert_eq!(notes[0]["revision"], 0);
        assert!(live.guard().is_none(), "nothing to put back now");
    }

    #[test]
    fn a_checkout_under_the_open_package_is_noticed_and_followed() {
        let (dir, first) = world();
        init_git(&dir).unwrap();
        // A fresh handle, as the app holds one: it learns the head from disk.
        drop(first);
        let mut live = LiveWorld::open(&dir).unwrap();
        assert!(!live.git_moved(), "nothing moved yet");
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "claude",
            None,
        )
        .unwrap();
        assert!(
            !live.git_moved(),
            "the committer's own commit is not a move"
        );
        assert_eq!(live.revision(), 1);
        let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        git(&dir, &["checkout", "--quiet", "HEAD~1"]).unwrap();
        assert!(live.git_moved(), "a checkout under the package is a move");
        live.reopen().unwrap();
        assert!(!live.git_moved());
        assert_eq!(live.revision(), 0, "the package is what the checkout left");
        assert!(live.head().get_by_name("post").is_none());
        assert!(live.verify().is_empty(), "{:?}", live.verify());

        git(&dir, &["checkout", "--quiet", &branch]).unwrap();
        assert!(live.git_moved());
        live.reopen().unwrap();
        assert_eq!(live.revision(), 1);
        assert!(live.head().get_by_name("post").is_some());
    }

    #[test]
    fn assets_at_a_commit_are_that_commits_bytes() {
        let (dir, _live) = world();
        init_git(&dir).unwrap();
        let asset = dir.join(ASSETS).join("sign.txt");
        std::fs::write(&asset, b"first").unwrap();
        git(&dir, &["add", "--all"]).unwrap();
        git(&dir, &["commit", "--quiet", "-m", "first sign"]).unwrap();
        let first = git(&dir, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        std::fs::write(&asset, b"second").unwrap();
        git(&dir, &["commit", "--quiet", "-am", "second sign"]).unwrap();
        let second = git(&dir, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let cache = dir.join(LIVE).join("replay");
        let root = assets_at(&dir, &first, &cache).unwrap();
        assert_eq!(
            std::fs::read(root.join(ASSETS).join("sign.txt")).unwrap(),
            b"first"
        );
        let root = assets_at(&dir, &second, &cache).unwrap();
        assert_eq!(
            std::fs::read(root.join(ASSETS).join("sign.txt")).unwrap(),
            b"second"
        );
        // Asked again, the same files; the blobs are cached once by hash.
        assets_at(&dir, &first, &cache).unwrap();
        assert_eq!(std::fs::read_dir(cache.join("blobs")).unwrap().count(), 2);
    }

    #[test]
    fn a_song_comes_by_modify_world_stored_like_any_asset_and_is_cut_again() {
        let (dir, mut live) = world();
        std::fs::write(dir.join(ASSETS).join("tide.mp3"), b"not really an mp3").unwrap();
        let done = live
            .submit(
                &json!([{"ModifyWorld": {"patch": {"soundtrack": {
                    "path": "tide.mp3", "title": "Tidewater", "duration": 66.0,
                    "bpm": 140.0, "sections": [0.0, 0.5]}}}}]),
                "claude",
                None,
            )
            .unwrap();
        let stored = format!("{}.mp3", sha256_hex(b"not really an mp3"));
        assert_eq!(
            done.stored["tide.mp3"], stored,
            "the song is stored by hash"
        );
        assert_eq!(describe_ops(&done.ops), ["modify world (soundtrack)"]);
        let song = on_disk(&dir).soundtrack.expect("a song");
        assert_eq!(song.path.as_deref(), Some(stored.as_str()));
        assert_eq!((song.duration, song.bpm), (66.0, 140.0));

        // Just the sections: the rest of the song stays as it was.
        live.submit(
            &json!([{"ModifyWorld": {"patch": {"soundtrack": {"sections": [0.0, 0.25, 0.75]}}}}]),
            "claude",
            None,
        )
        .unwrap();
        let song = on_disk(&dir).soundtrack.expect("still a song");
        assert_eq!(song.sections, [0.0, 0.25, 0.75]);
        assert_eq!(song.title.as_deref(), Some("Tidewater"));
        assert_eq!(
            (song.duration, song.path.as_deref()),
            (66.0, Some(stored.as_str()))
        );

        // `null` clears it, and the history still folds to the head.
        live.submit(
            &json!([{"ModifyWorld": {"patch": {"soundtrack": null}}}]),
            "claude",
            None,
        )
        .unwrap();
        assert!(on_disk(&dir).soundtrack.is_none());
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn every_scene_wide_field_the_log_changes_reaches_the_head() {
        let (dir, mut live) = world();
        live.submit(
            &json!([{"SetAmbience": {"ambience": [
                {"name": "sea", "source": {"Ocean": {"wave_size": 0.5}}, "volume": 0.4}]}}]),
            "claude",
            None,
        )
        .unwrap();
        let head = on_disk(&dir);
        assert_eq!(
            head.ambience.len(),
            1,
            "the ambience is the log's, not the base's"
        );
        assert_eq!(head.tours.len(), 1, "the base's tour");

        live.submit(
            &json!([{"ModifyWorld": {"patch": {"tours": []}}}]),
            "claude",
            None,
        )
        .unwrap();
        assert!(on_disk(&dir).tours.is_empty(), "a list is replaced whole");
        assert!(live.verify().is_empty(), "{:?}", live.verify());
    }

    #[test]
    fn assets_are_stored_by_hash_and_never_change_in_place() {
        let (dir, mut live) = world();
        std::fs::write(dir.join(ASSETS).join("brick.png"), b"first bricks").unwrap();
        let done = live
            .submit(
                &json!([{"ModifyEntity": {"id": "crate", "patch": {"material": {"base_color_texture": "brick.png"}}}}]),
                "claude",
                None,
            )
            .unwrap();
        let first = done.stored["brick.png"].clone();
        assert_eq!(first, format!("{}.png", sha256_hex(b"first bricks")));
        let crate_ = |m: &WorldManifest| {
            m.entities
                .iter()
                .find(|e| e.name.0 == "crate")
                .cloned()
                .unwrap()
        };
        let material = crate_(&on_disk(&dir)).material.unwrap();
        assert_eq!(material.base_color_texture.as_deref(), Some(first.as_str()));
        assert_eq!(material.roughness, 0.8, "a partial material merges");

        // A new version: same working name, new bytes, the op again.
        std::fs::write(dir.join(ASSETS).join("brick.png"), b"second bricks").unwrap();
        let again = live
            .submit(
                &json!([{"ModifyEntity": {"id": "crate", "patch": {"material": {"base_color_texture": "brick.png"}}}}]),
                "claude",
                None,
            )
            .unwrap();
        assert_ne!(again.stored["brick.png"], first);
        assert!(
            dir.join(ASSETS).join(&first).exists(),
            "the old version stays"
        );

        // A stored copy changed in place is refused by name.
        std::fs::write(dir.join(ASSETS).join(&first), b"tampered").unwrap();
        let refused = live
            .submit(
                &json!([{"ModifyEntity": {"id": "crate", "patch": {"material": {"base_color_texture": first}}}}]),
                "claude",
                None,
            )
            .unwrap_err();
        assert!(
            refused.errors[0].contains("changed in place"),
            "{:?}",
            refused.errors
        );

        let missing = live
            .submit(
                &json!([{"SpawnEntity": {"entity": {"name": "statue", "mesh_asset": {"path": "../statue.glb"}}}}]),
                "claude",
                None,
            )
            .unwrap_err();
        assert!(
            missing.errors[0].contains("leaves assets/"),
            "{:?}",
            missing.errors
        );
    }

    #[test]
    fn a_direct_write_to_the_manifest_is_put_back() {
        let (dir, mut live) = world();
        std::fs::write(dir.join(MANIFEST), b"{\"oops\": true}").unwrap();
        let said = live.guard().expect("noticed");
        assert!(said.contains("only changes"));
        assert_eq!(on_disk(&dir).entities.len(), 2);
        assert!(live.guard().is_none());
    }

    #[test]
    fn a_reopened_package_folds_back_to_its_head() {
        let (dir, mut live) = world();
        live.submit(
            &json!([{"SpawnEntity": {"entity": {"name": "post"}}}]),
            "claude",
            None,
        )
        .unwrap();
        let reopened = LiveWorld::open(&dir).unwrap();
        assert_eq!(reopened.revision(), 1);
        assert!(reopened.head().get_by_name("post").is_some());
        assert!(reopened.verify().is_empty(), "{:?}", reopened.verify());
    }

    #[test]
    fn in_a_git_repository_every_batch_is_a_commit_and_a_keyframe() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // no git on this machine
        }
        let (dir, mut live) = world();
        init_git(&dir).unwrap();
        live.submit(
            &json!({"author": "claude", "message": "a post by the crate",
                    "ops": [{"SpawnEntity": {"entity": {"name": "post"}}}]}),
            "x",
            None,
        )
        .unwrap();
        live.submit(&json!([{"DeleteEntity": {"id": "crate"}}]), "maya", None)
            .unwrap();
        live.undo("you").unwrap();

        let history = git_history(&dir, None).unwrap();
        let line: Vec<(String, String)> = history
            .iter()
            .map(|s| (s.author.clone(), s.subject.clone()))
            .collect();
        assert_eq!(line.len(), 4, "{line:?}");
        assert_eq!(line[1], ("claude".into(), "a post by the crate".into()));
        assert_eq!(line[2], ("maya".into(), "delete #2".into()));
        assert_eq!(line[3], ("you".into(), "undo revision 2".into()));

        // A replay steps keyframe to keyframe by diffing them.
        let step = diff_manifests(&history[1].manifest, &history[2].manifest);
        assert!(
            matches!(step.as_slice(), [EditOp::DeleteEntity { id }] if id.0 == 2),
            "{step:?}"
        );
        let back = diff_manifests(&history[2].manifest, &history[3].manifest);
        assert!(
            matches!(back.as_slice(), [EditOp::SpawnEntity { entity }] if entity.name.0 == "crate")
        );
    }

    #[test]
    fn pretty_output_keeps_plain_arrays_on_one_line() {
        let text = to_pretty(
            &json!({"position": [0.0, 1.5, -2.0], "tags": ["a", "b"], "list": [{"x": 1}]}),
        )
        .unwrap();
        assert!(text.contains("\"position\": [0.0, 1.5, -2.0]"), "{text}");
        assert!(text.contains("\"tags\": [\"a\", \"b\"]"), "{text}");
        assert!(text.contains("\"list\": [\n"), "{text}");
    }
}
