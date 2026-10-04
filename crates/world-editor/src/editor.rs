//! The editor core: a tip, a document, a selection, and the four things that
//! change them — submit, undo, goto, fork.
//!
//! No engine and no I/O. An `Editor` is opened from a base document and a log
//! its caller read; every entry it appends is handed back for the caller to
//! persist. That is what makes the whole non-linear model testable without a
//! window, and reusable by every renderer over this format — Bevy here,
//! RealityKit and Compose in `worldwalk`, egui in `worldprobe`, three.js on
//! the web.
//!
//! ## Submitting is ingestion, not application
//!
//! A batch arrives as JSON, the way an agent sends it, and goes through
//! [`openworldformat::authoring::ingest`] — the Authoring profile: names bind
//! to ids against the fold so far, spawns get ids, partial `transform` and
//! `material` changes merge into whole values, and an unknown field is
//! *refused with a pointer* rather than ignored. The op committed to the log
//! is the ingested one, so the fold never merges and the log's patch
//! semantics stay exactly what the format says they are.
//!
//! A refusal is per-op and whole-batch: nothing is applied, the log is
//! untouched, and the reasons go back to the author so it can correct itself.

use std::collections::BTreeSet;

use openworldformat::author::Author;
use openworldformat::doc::WorldDoc;
use openworldformat::history::EditOp;
use openworldformat::oplog::OpLogEntry;
use openworldformat::session::SessionOp;
use serde_json::Value;

use crate::delta::{Delta, delta_between};
use crate::fold::{FoldCache, FoldError};
use crate::history::{EntryId, History, HistoryError};

/// What a submitted batch produced.
#[derive(Debug, Clone)]
pub struct Committed {
    /// The entry's id — the new tip.
    pub id: EntryId,
    /// The entry as it was appended, for the caller to persist.
    pub entry: OpLogEntry,
    /// The ops that move a renderer from the previous document to this one.
    pub delta: Delta,
    /// Ids allocated to entities the batch spawned by name, so the author can
    /// refer to them next time.
    pub spawned: std::collections::BTreeMap<String, u64>,
    /// Anything the ingestion wanted to say that was not fatal.
    pub warnings: Vec<String>,
}

/// Why a submission was refused, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// One reason per problem, each naming where it is.
    pub errors: Vec<String>,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the batch was refused: {}", self.errors.join("; "))
    }
}

impl std::error::Error for Refusal {}

/// What went wrong doing something other than submitting.
#[derive(Debug)]
pub enum EditorError {
    /// The log's shape, or a tip that is not in it.
    History(HistoryError),
    /// A fold that could not reach its tip.
    Fold(FoldError),
    /// There is nothing on this branch to undo — by this author, when one
    /// was named.
    NothingToUndo,
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::History(e) => write!(f, "{e}"),
            Self::Fold(e) => write!(f, "{e}"),
            Self::NothingToUndo => write!(f, "nothing on this branch to undo"),
        }
    }
}

impl std::error::Error for EditorError {}

impl From<HistoryError> for EditorError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

impl From<FoldError> for EditorError {
    fn from(e: FoldError) -> Self {
        Self::Fold(e)
    }
}

/// One world, open for editing.
#[derive(Debug)]
pub struct Editor {
    history: History,
    cache: FoldCache,
    /// The entry whose fold is on screen; `None` means the base, before any
    /// entry — a real state, and the one a brand-new world is in.
    tip: Option<EntryId>,
    doc: WorldDoc,
    /// The document's numeric ids. A `BTreeSet` and not a hash set because
    /// an outliner wants a stable order, and `EntityId` because the
    /// format's wrapper has no `Ord` — worth proposing upstream, not worth
    /// a newtype here.
    selection: BTreeSet<u64>,
    revision: u64,
}

impl Editor {
    /// Open a world: a base document and the log over it, folded to the last
    /// entry in file order.
    pub fn open(base: WorldDoc, entries: Vec<OpLogEntry>) -> Result<Self, EditorError> {
        let history = History::open(entries)?;
        let mut cache = FoldCache::new(base);
        let tip = history.last_id().map(str::to_string);
        let doc = cache.doc_at(&history, tip.as_deref())?;
        let revision = tip
            .as_deref()
            .and_then(|id| history.get(id))
            .map(|e| e.entry.revision)
            .unwrap_or(0);
        Ok(Self {
            history,
            cache,
            tip,
            doc,
            selection: BTreeSet::new(),
            revision,
        })
    }

    /// Open a world with no history at all.
    pub fn new(base: WorldDoc) -> Self {
        Self::open(base, Vec::new()).expect("an empty log always opens")
    }

    /// The document on screen: the fold at the current tip.
    pub fn doc(&self) -> &WorldDoc {
        &self.doc
    }

    /// The entry on screen, or `None` at the base.
    pub fn tip(&self) -> Option<&str> {
        self.tip.as_deref()
    }

    /// The log, as a tree.
    pub fn history(&self) -> &History {
        &self.history
    }

    /// The revision the current tip carries.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// What is selected. Selection is the editor's, not the document's: it
    /// does not fold, it is not in the log, and it survives a seek only where
    /// the entities do.
    pub fn selection(&self) -> &BTreeSet<u64> {
        &self.selection
    }

    /// Select exactly these entities, dropping any that the document on
    /// screen does not have.
    pub fn select<I: IntoIterator<Item = u64>>(&mut self, ids: I) {
        self.selection = ids
            .into_iter()
            .filter(|id| self.doc.contains(*id))
            .collect();
    }

    /// Clear the selection.
    pub fn deselect_all(&mut self) {
        self.selection.clear();
    }

    /// Submit a batch, the way an agent does: JSON, read strictly, ingested
    /// against the document on screen, committed whole or refused whole.
    ///
    /// The new entry's parent is the current tip, so submitting while
    /// scrubbed back is a fork rather than a rewrite — history is never
    /// destroyed by editing in the middle of it.
    pub fn submit(&mut self, batch: &Value) -> Result<Committed, Refusal> {
        let ingested =
            openworldformat::authoring::ingest(&self.doc, batch).map_err(|refused| Refusal {
                errors: refused.errors,
            })?;

        let author = batch
            .get("author")
            .and_then(Value::as_str)
            .map(|name| Author {
                peer: None,
                name: name.to_string(),
            })
            .unwrap_or_default();
        let message = batch
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string);

        let entry = self.append(ingested.ops, author, message);
        let committed = self.commit(entry, ingested.doc);
        Ok(Committed {
            spawned: ingested.spawned,
            warnings: ingested.warnings,
            ..committed
        })
    }

    /// Undo the most recent edit on this branch — appending its inverse, not
    /// removing anything. The log only ever grows, which is what makes undo
    /// survive a save, a fork and another client.
    ///
    /// With `author`, undoes that author's most recent edit instead, so one
    /// person's undo does not take back someone else's work.
    pub fn undo(&mut self, author: Option<&str>) -> Result<Committed, EditorError> {
        let path = match self.tip.as_deref() {
            Some(tip) => self.history.path_to(tip)?,
            None => return Err(EditorError::NothingToUndo),
        };

        // Walk back along this branch for the newest entry that still has an
        // edit to take back, replaying the path so each candidate's inverse
        // is computed against the document as it was *before* that entry.
        for (position, id) in path.iter().enumerate().rev() {
            let entry = self
                .history
                .get(id)
                .ok_or_else(|| HistoryError::NoSuchEntry((*id).to_string()))?;
            if let Some(name) = author
                && entry.entry.author.name != name
            {
                continue;
            }
            let ops = entry.entry.edit_ops();
            if ops.is_empty() {
                continue;
            }

            let before = if position == 0 {
                self.cache.base().clone()
            } else {
                let parent = path[position - 1];
                self.cache.doc_at(&self.history, Some(parent))?
            };

            let inverses = inverse_ops(&before, &ops);
            if inverses.is_empty() {
                continue;
            }

            let undo_author = Author {
                peer: None,
                name: author
                    .map(str::to_string)
                    .unwrap_or_else(|| entry.entry.author.name.clone()),
            };
            let message = Some(format!("undo {id}"));
            let mut doc = self.doc.clone();
            if doc.apply_entry(&inverses).is_err() {
                continue;
            }
            let appended = self.append(inverses, undo_author, message);
            return Ok(self.commit(appended, doc));
        }

        Err(EditorError::NothingToUndo)
    }

    /// Show the document at `tip` — a scrub. Returns the ops that move a
    /// renderer there from what it is showing.
    ///
    /// `None` seeks to the base. Nothing is written: a seek is a view change,
    /// and a world is only edited by submitting.
    pub fn goto(&mut self, tip: Option<&str>) -> Result<Delta, EditorError> {
        if let Some(id) = tip
            && self.history.get(id).is_none()
        {
            return Err(HistoryError::NoSuchEntry(id.to_string()).into());
        }
        let doc = self.cache.doc_at(&self.history, tip)?;
        let delta = delta_between(&self.doc, &doc);
        self.revision = tip
            .and_then(|id| self.history.get(id))
            .map(|e| e.entry.revision)
            .unwrap_or(0);
        self.tip = tip.map(str::to_string);
        self.doc = doc;
        self.selection.retain(|id| self.doc.contains(*id));
        Ok(delta)
    }

    /// Move the tip to `at` so the next submission branches from there.
    ///
    /// This is [`Self::goto`] under the name that says what it is for: in
    /// this model a fork is not an operation on the log, it is editing from
    /// an entry that already has a child. The branch appears when the next
    /// batch lands.
    pub fn fork_from(&mut self, at: &str) -> Result<Delta, EditorError> {
        self.goto(Some(at))
    }

    /// Every branch end, for a branch rail.
    pub fn tips(&self) -> Vec<&str> {
        self.history.tips()
    }

    /// Append an entry built from ops the caller has already ingested.
    fn append(&mut self, ops: Vec<EditOp>, author: Author, message: Option<String>) -> OpLogEntry {
        OpLogEntry {
            revision: self.revision + 1,
            author,
            ops: ops
                .into_iter()
                .map(|op| SessionOp::Edit(Box::new(op)))
                .collect(),
            timestamp_ms: 0,
            // Identity is left to the caller's log: an id here would have to
            // be a content hash over the entry, and `oplog::compute_entry_id`
            // is the format's own, so the caller that persists can set it
            // without this crate guessing a scheme.
            id: None,
            parent: self.tip.clone(),
            message,
        }
    }

    /// Commit an appended entry: it becomes the tip, and the document it
    /// produces becomes what is on screen.
    fn commit(&mut self, entry: OpLogEntry, doc: WorldDoc) -> Committed {
        let revision = entry.revision;
        let id = self
            .history
            .push(entry.clone())
            .expect("an appended entry's parent is the tip it was built on");
        let delta = delta_between(&self.doc, &doc);
        self.doc = doc;
        self.tip = Some(id.clone());
        self.revision = revision;
        self.selection.retain(|id| self.doc.contains(*id));
        Committed {
            id,
            entry,
            delta,
            spawned: Default::default(),
            warnings: Vec::new(),
        }
    }
}

/// The ops that take back `ops`, newest first, computed against the document
/// as it was before they applied.
fn inverse_ops(before: &WorldDoc, ops: &[EditOp]) -> Vec<EditOp> {
    let mut doc = before.clone();
    let mut inverses = Vec::with_capacity(ops.len());
    for op in ops {
        // Each inverse is taken against the state its op was about to
        // change, so a batch undoes as a batch rather than as a sequence of
        // inverses computed against the wrong document.
        match op.compute_inverse(&doc) {
            Ok(inverse) => inverses.push(inverse),
            Err(_) => break,
        }
        if doc.apply(op).is_err() {
            break;
        }
    }
    inverses.reverse();
    inverses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{linear_log, world_with_cube};
    use serde_json::json;

    fn spawn_batch(name: &str, author: &str) -> Value {
        json!({
            "author": author,
            "message": format!("spawn {name}"),
            "ops": [{"SpawnEntity": {"entity": {"name": name}}}]
        })
    }

    #[test]
    fn a_new_world_is_at_the_base_with_no_tip() {
        let editor = Editor::new(world_with_cube());
        assert_eq!(editor.tip(), None);
        assert_eq!(editor.revision(), 0);
        assert_eq!(editor.doc().len(), 1);
        assert!(editor.tips().is_empty());
    }

    #[test]
    fn opening_a_log_folds_to_the_last_entry() {
        let editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        assert_eq!(editor.tip(), Some("e3"));
        assert_eq!(editor.revision(), 3);
        assert_eq!(editor.doc().len(), 4);
    }

    #[test]
    fn submitting_binds_a_name_to_a_new_id_and_reports_it() {
        let mut editor = Editor::new(world_with_cube());
        let committed = editor.submit(&spawn_batch("lamp", "claude")).unwrap();

        assert_eq!(committed.spawned.len(), 1);
        let id = committed.spawned["lamp"];
        assert!(editor.doc().get(id).is_some());
        assert_eq!(editor.doc().get_by_name("lamp").map(|e| e.id.0), Some(id));
        assert_eq!(committed.entry.author.name, "claude");
        assert_eq!(committed.entry.message.as_deref(), Some("spawn lamp"));
        assert_eq!(editor.tip(), Some(committed.id.as_str()));
        assert_eq!(editor.revision(), 1);
    }

    #[test]
    fn a_submission_produces_a_delta_that_describes_the_change() {
        let mut editor = Editor::new(world_with_cube());
        let committed = editor.submit(&spawn_batch("lamp", "claude")).unwrap();
        assert_eq!(committed.delta.len(), 1);
        assert!(matches!(committed.delta.ops[0], EditOp::SpawnEntity { .. }));
    }

    #[test]
    fn an_unknown_field_is_refused_whole_and_nothing_is_written() {
        let mut editor = Editor::new(world_with_cube());
        let before = editor.doc().len();

        let batch = json!({
            "author": "claude",
            "ops": [
                {"SpawnEntity": {"entity": {"name": "good"}}},
                {"ModifyEntity": {"id": 1, "patch": {"colour": "red"}}}
            ]
        });
        let refusal = editor.submit(&batch).unwrap_err();

        assert!(!refusal.errors.is_empty());
        assert!(
            refusal.errors.iter().any(|e| e.contains("colour")),
            "the reason should name the field: {:?}",
            refusal.errors
        );
        assert_eq!(editor.doc().len(), before, "the valid op must not apply");
        assert_eq!(editor.tip(), None, "the log must be untouched");
        assert!(editor.history().is_empty());
    }

    #[test]
    fn an_empty_batch_is_refused() {
        let mut editor = Editor::new(world_with_cube());
        assert!(editor.submit(&json!({"ops": []})).is_err());
        assert!(editor.submit(&json!("not a batch")).is_err());
    }

    #[test]
    fn undo_appends_an_inverse_rather_than_removing_anything() {
        let mut editor = Editor::new(world_with_cube());
        editor.submit(&spawn_batch("lamp", "claude")).unwrap();
        assert_eq!(editor.doc().len(), 2);

        let undone = editor.undo(None).unwrap();
        assert_eq!(editor.doc().len(), 1, "the spawn is taken back");
        assert_eq!(
            editor.history().len(),
            2,
            "the log grew; undo removed no entry"
        );
        assert!(matches!(
            undone.delta.ops.first(),
            Some(EditOp::DeleteEntity { .. })
        ));
        assert_eq!(undone.entry.message.as_deref(), Some("undo line-0"));
    }

    #[test]
    fn undo_by_author_skips_someone_elses_edit() {
        let mut editor = Editor::new(world_with_cube());
        editor.submit(&spawn_batch("yi-lamp", "yi")).unwrap();
        editor
            .submit(&spawn_batch("claude-lamp", "claude"))
            .unwrap();
        assert_eq!(editor.doc().len(), 3);

        editor.undo(Some("yi")).unwrap();
        assert!(
            editor.doc().get_by_name("claude-lamp").is_some(),
            "claude's work must survive yi's undo"
        );
        assert!(
            editor.doc().get_by_name("yi-lamp").is_none(),
            "yi's own entity should be gone"
        );
    }

    #[test]
    fn undo_with_nothing_to_take_back_says_so() {
        let mut editor = Editor::new(world_with_cube());
        assert!(matches!(editor.undo(None), Err(EditorError::NothingToUndo)));

        editor.submit(&spawn_batch("lamp", "claude")).unwrap();
        assert!(matches!(
            editor.undo(Some("nobody")),
            Err(EditorError::NothingToUndo)
        ));
    }

    #[test]
    fn a_seek_moves_the_document_and_reports_the_ops_to_get_there() {
        let mut editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        assert_eq!(editor.doc().len(), 4);

        let delta = editor.goto(Some("e1")).unwrap();
        assert_eq!(editor.tip(), Some("e1"));
        assert_eq!(editor.doc().len(), 2);
        assert_eq!(delta.len(), 2, "two entities have to be deleted");

        let back = editor.goto(Some("e3")).unwrap();
        assert_eq!(editor.doc().len(), 4);
        assert_eq!(back.len(), 2, "and spawned again on the way back");
    }

    #[test]
    fn seeking_to_the_base_is_a_real_state() {
        let mut editor = Editor::open(world_with_cube(), linear_log(2)).unwrap();
        editor.goto(None).unwrap();
        assert_eq!(editor.tip(), None);
        assert_eq!(editor.doc().len(), 1);
        assert_eq!(editor.revision(), 0);
    }

    #[test]
    fn seeking_nowhere_is_refused_and_changes_nothing() {
        let mut editor = Editor::open(world_with_cube(), linear_log(2)).unwrap();
        assert!(editor.goto(Some("nope")).is_err());
        assert_eq!(editor.tip(), Some("e2"));
    }

    #[test]
    fn a_seek_writes_nothing() {
        let mut editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        let before = editor.history().len();
        editor.goto(Some("e1")).unwrap();
        editor.goto(None).unwrap();
        editor.goto(Some("e3")).unwrap();
        assert_eq!(editor.history().len(), before);
    }

    #[test]
    fn submitting_from_an_earlier_tip_forks_instead_of_rewriting() {
        let mut editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        editor.fork_from("e1").unwrap();

        let committed = editor.submit(&spawn_batch("variant", "yi")).unwrap();

        assert_eq!(committed.entry.parent.as_deref(), Some("e1"));
        assert!(editor.history().is_fork_point("e1"));

        let mut tips = editor.tips();
        tips.sort_unstable();
        assert_eq!(
            tips,
            vec!["e3", committed.id.as_str()],
            "the trunk's tip survives the fork"
        );

        // The trunk is still foldable and still has what it had.
        editor.goto(Some("e3")).unwrap();
        assert_eq!(editor.doc().len(), 4);
        assert!(editor.doc().get_by_name("variant").is_none());
    }

    #[test]
    fn two_branches_hold_different_worlds_at_the_same_time() {
        let mut editor = Editor::new(world_with_cube());
        let root = editor.submit(&spawn_batch("shared", "yi")).unwrap().id;

        let left = editor.submit(&spawn_batch("left", "yi")).unwrap().id;
        editor.fork_from(&root).unwrap();
        let right = editor.submit(&spawn_batch("right", "yi")).unwrap().id;

        editor.goto(Some(&left)).unwrap();
        assert!(editor.doc().get_by_name("left").is_some());
        assert!(editor.doc().get_by_name("right").is_none());
        assert!(editor.doc().get_by_name("shared").is_some());

        editor.goto(Some(&right)).unwrap();
        assert!(editor.doc().get_by_name("right").is_some());
        assert!(editor.doc().get_by_name("left").is_none());
        assert!(editor.doc().get_by_name("shared").is_some());
    }

    #[test]
    fn selection_drops_entities_a_seek_removed() {
        let mut editor = Editor::new(world_with_cube());
        let committed = editor.submit(&spawn_batch("lamp", "yi")).unwrap();
        let lamp = committed.spawned["lamp"];

        editor.select([1, lamp]);
        assert_eq!(editor.selection().len(), 2);

        editor.goto(None).unwrap();
        assert_eq!(
            editor.selection().iter().copied().collect::<Vec<_>>(),
            vec![1],
            "the lamp does not exist at the base, so it leaves the selection"
        );
    }

    #[test]
    fn selecting_an_entity_that_is_not_there_selects_nothing() {
        let mut editor = Editor::new(world_with_cube());
        editor.select([9_999]);
        assert!(editor.selection().is_empty());
    }

    #[test]
    fn every_appended_entry_names_the_tip_it_was_built_on() {
        let mut editor = Editor::new(world_with_cube());
        let first = editor.submit(&spawn_batch("one", "yi")).unwrap();
        assert_eq!(first.entry.parent, None, "the first builds on the base");

        let second = editor.submit(&spawn_batch("two", "yi")).unwrap();
        assert_eq!(second.entry.parent.as_deref(), Some(first.id.as_str()));
    }
}
