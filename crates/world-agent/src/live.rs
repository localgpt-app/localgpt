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
    base_manifest: WorldManifest,
    base: WorldDoc,
    entries: Vec<OpLogEntry>,
    head: WorldDoc,
    /// The head as a manifest: the fold's entities and scene settings, plus
    /// what no op reaches (meta, avatar, tours, soundtrack, creations).
    head_manifest: WorldManifest,
    meta: SessionMeta,
}

impl LiveWorld {
    /// Open a folder holding `manifest.json`, making it a live package
    /// first if it is only that.
    pub fn open(dir: &Path) -> Result<Self, LiveError> {
        if !dir.join(PACKAGE).is_file() {
            init(dir)?;
        }
        let meta: SessionMeta = read_json(dir, PACKAGE)?;
        let base_manifest: WorldManifest = read_json(dir, BASE)?;
        let base = WorldDoc::from_manifest(&base_manifest)
            .map_err(|e| LiveError(format!("{BASE} doesn't load: {e}")))?;
        let entries = read_log(dir);
        let (head, _) = fold_head(&base, &entries, &meta)
            .map_err(|e| LiveError(format!("{LOG} no longer folds: {e}")))?;
        let mut world = Self {
            dir: dir.to_path_buf(),
            head_manifest: compose(&base_manifest, &head),
            base_manifest,
            base,
            entries,
            head,
            meta,
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
        let (ops, author, message, at) = match input {
            Value::Array(ops) => (
                ops.clone(),
                author.to_string(),
                message.map(str::to_string),
                at.map(str::to_string),
            ),
            Value::Object(body) => {
                let Some(Value::Array(ops)) = body.get("ops") else {
                    return Err(Rejected::one(
                        "send a JSON array of ops, or {\"ops\": [...], \"author\": ..., \"message\": ..., \"at\": ...}",
                    ));
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
                    // Null is main's tip, said out loud.
                    Some(Value::Null) => None,
                    Some(Value::String(id)) => Some(id.clone()),
                    Some(_) => {
                        return Err(Rejected::one(
                            "\"at\" is the id of the entry to build on (null: the head)",
                        ));
                    }
                };
                (ops.clone(), author, message, at)
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
        if ops.is_empty() {
            return Err(Rejected::one("the batch holds no ops"));
        }

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
    /// the log never rewinds. Undo entries themselves aren't undone (no
    /// redo in this proof of concept).
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
        let undone: HashSet<u64> = on_path.iter().copied().filter_map(undo_of).collect();
        let Some(k) = on_path.iter().rposition(|e| {
            !e.edit_ops().is_empty() && undo_of(e).is_none() && !undone.contains(&e.revision)
        }) else {
            return Err(Rejected::one("nothing to undo"));
        };
        let target = on_path[k].revision;
        // The path is a chain, so folding its prefix is the state just
        // before the target — which is what an inverse is computed against.
        let prefix: Vec<OpLogEntry> = on_path[..k].iter().map(|e| (*e).clone()).collect();
        let before = fold_log(&self.base, &prefix)
            .map_err(|e| Rejected::one(format!("the history no longer folds: {e}")))?;
        let inverse = sync::compute_inverse(&before, &on_path[k].edit_ops());
        let mut trial = start;
        trial.apply_all(&inverse).map_err(|e| {
            Rejected::one(format!(
                "revision {target} can't be undone: later changes depend on it ({e})"
            ))
        })?;
        let revision = self.next_revision();
        let mut entry = entry(
            revision,
            author,
            json!({"via": "undo", "of": target}),
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
            warnings: vec![format!("undid revision {target}")],
            entry,
            forked,
        })
    }

    /// Put `manifest.json` back if something other than the committer wrote
    /// it. Returns what happened, for the person or agent to hear.
    pub fn guard(&mut self) -> Option<String> {
        let on_disk = std::fs::read(self.dir.join(MANIFEST)).ok();
        let sha = on_disk.as_deref().map(sha256_hex);
        if sha.is_some() && sha == self.meta.world_sha256 {
            return None;
        }
        match self.write_head() {
            Ok(()) => Some(format!(
                "{MANIFEST} was changed outside the committer; the world only changes \
                 through ops, so it is back at revision {}",
                self.meta.head_revision
            )),
            Err(e) => Some(format!("{MANIFEST} couldn't be restored: {e}")),
        }
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
        // No op reaches these, so they can only be the base's.
        let (file, base) = (to_value(&manifest), to_value(&self.base_manifest));
        for key in ["meta", "avatar", "tours", "soundtrack", "creations"] {
            if file.get(key) != base.get(key) {
                problems.push(format!(
                    "{key} differs from the base, and no op could have changed it"
                ));
            }
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
    manifest.environment = doc.environment.clone();
    manifest.camera = doc.camera.clone();
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
            EditOp::Batch { ops } => out.extend(describe_ops(ops)),
            other => out.push(format!("{other:?}").chars().take(60).collect()),
        }
    }
    out
}

/// Run git in the package; its error output becomes the error.
fn git(dir: &Path, args: &[&str]) -> Result<String, LiveError> {
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
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// The revision an undo entry undid.
fn undo_of(entry: &OpLogEntry) -> Option<u64> {
    entry.ops.iter().find_map(|op| match op {
        SessionOp::Tool(t) if t.tool == "live" && t.args["via"] == "undo" => t.args["of"].as_u64(),
        _ => None,
    })
}

/// Make `dir` a live package around the manifest it holds.
fn init(dir: &Path) -> Result<(), LiveError> {
    let bytes = std::fs::read(dir.join(MANIFEST))
        .map_err(|e| LiveError(format!("{}: {e}", dir.join(MANIFEST).display())))?;
    let manifest: WorldManifest = serde_json::from_slice(&bytes)
        .map_err(|e| LiveError(format!("{MANIFEST} isn't a world: {e}")))?;
    std::fs::create_dir_all(dir.join("snapshots"))?;
    std::fs::create_dir_all(dir.join(ASSETS))?;
    std::fs::create_dir_all(dir.join(LIVE))?;
    write_atomic(&dir.join(BASE), &bytes)?;
    if !dir.join(LOG).exists() {
        std::fs::write(dir.join(LOG), b"")?;
    }
    if !dir.join(AGENTS).exists() {
        std::fs::write(dir.join(AGENTS), AGENTS_GUIDE)?;
    }
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
URL=$(jq -r .url .live/endpoint.json); TOKEN=$(jq -r .token .live/endpoint.json)
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
- A `patch` holds only what changes. `transform`, `material` and `light` merge
  into the current value (send just `position` to move something); `null`
  removes a field.
- Op kinds: SpawnEntity, ModifyEntity, DeleteEntity (takes children with it),
  SetEnvironment (merges too), SetCamera, Batch.
- Meters, Y up. Rotations: `rotation_degrees`, X then Y then Z. Colors: RGBA
  0–1, sRGB; `emissive` is linear and glows above 1.
- Shapes: copy the forms in `manifest.json` — {"Cuboid": {"x": 1, "y": 1, "z": 1}},
  {"Sphere": {"radius": 0.5}}, {"Cylinder": {"radius": 0.2, "height": 3}},
  {"Plane": {"x": 10, "z": 10}}, Cone, Capsule, Torus, Pyramid, Wedge …

## Assets are files you write

Write meshes (.glb), textures (.png) and sounds into `assets/` under any name,
then reference the file by its path inside `assets/` in an op:
`"material": {"base_color_texture": "brick.png"}`. The app stores a copy named
by its SHA-256 and points the world at that. Stored copies never change: to
change a texture, write new bytes (to the same working name is fine) and send
the op again — that makes a new version, and the old one stays for history.

## See and ask

- `GET $URL/screenshot` — renders the view now and replies with the PNG's path.
  Look at it after a change.
- `GET $URL/selection` — what the person has selected; "this" means it.
- `POST $URL/undo` — undoes the head's newest change by appending its inverse.
- `.live/preview.png` refreshes by itself after every change.

## History is a tree

Every entry in `ops.jsonl` has an `id` and builds on a `parent`. The head —
what `manifest.json` holds — is main's tip, and a batch lands there unless it
says otherwise.

- `GET $URL/log` — the history: each entry's id, parent, revision, author,
  message, and which entry is the `head`.
- `GET $URL/tips` — every branch end, which one is the `head`, and which entry
  the window is showing (`current`).
- `POST $URL/goto -d '{"tip": "<id>"}'` — show any entry in the window (`null`
  for before it began). Nothing is written; this is how you look at an
  earlier state.
- Add `"at": "<id>"` to a batch to build on that entry instead of the head.
  From an earlier entry this starts a branch: main and `manifest.json` stay as
  they were, and the reply's `id` is the branch's new tip — send the next
  batch `"at"` it to keep going. To try an alternative from what the person
  is looking at, build `"at"` the `current` from `/tips`.
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
