//! The editor's view of a world: a tip, the document it folds to, a
//! selection, and the ability to seek.
//!
//! **This is a view, not an authority.** `world-agent`'s `LiveWorld` owns the
//! package: it ingests a batch through the Authoring profile, commits it whole
//! or refuses it with a reason per op, appends an inverse for undo, writes
//! `manifest.json`, guards it, makes the git commit and can `verify` that the
//! fold still equals the head. All of that existed before this crate, and a
//! second implementation of commit would be the drift this workspace has paid
//! for before (`docs/rfcs/authoring/world-editor-stack.md`).
//!
//! What the authority does not have is the non-linear half, which is this:
//! a branch index over the log, a fold cache fast enough to drag a playhead,
//! the delta between where the view is and where it is going, and a
//! selection that survives a seek only where its entities do.
//!
//! So the flow is one-directional on both axes. The authority commits and
//! hands the entry to [`Editor::observe`] (or [`Editor::committed`]); the
//! view follows and says what changed. The view never appends on its own.
//!
//! No I/O and no engine: an `Editor` is opened from a base document and a log
//! its caller read, and what it emits is ops for a renderer to apply.

use std::collections::BTreeSet;

use openworldformat::doc::WorldDoc;
use openworldformat::oplog::OpLogEntry;

use crate::delta::{Delta, delta_between};
use crate::fold::{FoldCache, FoldError};
use crate::history::{EntryId, History, HistoryError};

/// What went wrong seeking, or following a commit.
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
    /// entry in file order — the head while no ref names another.
    pub fn open(base: WorldDoc, entries: Vec<OpLogEntry>) -> Result<Self, EditorError> {
        let last = entries
            .len()
            .checked_sub(1)
            .map(|n| entries[n].id.clone().unwrap_or_else(|| format!("line-{n}")));
        Self::open_at(base, entries, last.as_deref())
    }

    /// Open a world showing `tip` (`None`: the base).
    ///
    /// Which tip is the head is `package.json`'s to say (`refs.main`), and a
    /// view reads no files, so a caller holding the package passes it —
    /// `LiveWorld::main_tip()`. Once a branch has been committed the last
    /// line is a branch's, and [`Self::open`] would show that instead.
    pub fn open_at(
        base: WorldDoc,
        entries: Vec<OpLogEntry>,
        tip: Option<&str>,
    ) -> Result<Self, EditorError> {
        let history = History::open(entries)?;
        if let Some(id) = tip
            && history.get(id).is_none()
        {
            return Err(HistoryError::NoSuchEntry(id.to_string()).into());
        }
        let mut cache = FoldCache::new(base);
        let tip = tip.map(str::to_string);
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

    /// Follow a commit the authority just made: take its entry as the new
    /// tip and return the ops that move a renderer there.
    ///
    /// The entry is appended exactly as given, so its `parent` is what the
    /// authority decided — which is how a commit made while the view was
    /// scrubbed back becomes a fork rather than a rewrite.
    ///
    /// This is the only way the log grows here. The view has no `submit`
    /// and no `undo`: both are `LiveWorld`'s, because both have to touch the
    /// package.
    pub fn committed(&mut self, entry: OpLogEntry) -> Result<Delta, EditorError> {
        let revision = entry.revision;
        let id = self.history.push(entry)?;
        let doc = self.cache.doc_at(&self.history, Some(&id))?;
        let delta = delta_between(&self.doc, &doc);
        self.doc = doc;
        self.tip = Some(id);
        self.revision = revision;
        self.selection.retain(|id| self.doc.contains(*id));
        Ok(delta)
    }

    /// Take a commit the authority made, and follow it only if it continues
    /// the entry on screen.
    ///
    /// Someone looking at the entry a commit builds on is watching that line
    /// of history, so the view moves with it and returns the delta. A commit
    /// anywhere else — on main while the view is scrubbed back, or on a
    /// branch it is not on — grows the history (a new row, a new tip) and
    /// leaves the document on screen alone: `None`. A terminal's rule for
    /// output: keep up only if you were already at the end.
    ///
    /// [`Self::committed`] is the unconditional form, for a caller that
    /// always wants to see the newest commit.
    pub fn observe(&mut self, entry: OpLogEntry) -> Result<Option<Delta>, EditorError> {
        let id = self.history.push(entry)?;
        if self.history.parent_of(&id) != self.tip.as_deref() {
            return Ok(None);
        }
        self.goto(Some(&id)).map(Some)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{linear_log, spawn_entry, world_with_cube};
    use openworldformat::history::EditOp;

    /// An entry as the authority would hand it over: ops already ingested,
    /// a parent the authority chose.
    fn entry(id: &str, parent: Option<&str>, revision: u64) -> OpLogEntry {
        spawn_entry(id, parent, revision)
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
    fn following_a_commit_moves_the_tip_and_says_what_changed() {
        let mut editor = Editor::new(world_with_cube());
        let delta = editor.committed(entry("e1", None, 1)).unwrap();

        assert_eq!(editor.tip(), Some("e1"));
        assert_eq!(editor.revision(), 1);
        assert_eq!(editor.doc().len(), 2);
        assert_eq!(delta.len(), 1);
        assert!(matches!(delta.ops[0], EditOp::SpawnEntity { .. }));
    }

    #[test]
    fn the_view_never_appends_on_its_own() {
        // The whole point of the split: commits are the authority's, so the
        // log only grows when it hands one over.
        let mut editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        let before = editor.history().len();
        editor.goto(Some("e1")).unwrap();
        editor.goto(None).unwrap();
        editor.fork_from("e2").unwrap();
        editor.select([1]);
        assert_eq!(editor.history().len(), before);
    }

    #[test]
    fn a_commit_made_while_scrubbed_back_forks_instead_of_rewriting() {
        let mut editor = Editor::open(world_with_cube(), linear_log(3)).unwrap();
        editor.fork_from("e1").unwrap();

        // The authority decided this entry's parent; the view appends it as
        // given, which is what makes the branch appear.
        editor.committed(entry("variant", Some("e1"), 4)).unwrap();

        assert!(editor.history().is_fork_point("e1"));
        let mut tips = editor.tips();
        tips.sort_unstable();
        assert_eq!(tips, vec!["e3", "variant"], "the trunk's tip survives");

        editor.goto(Some("e3")).unwrap();
        assert_eq!(editor.doc().len(), 4);
    }

    #[test]
    fn observing_a_commit_follows_it_only_from_the_entry_it_builds_on() {
        let mut editor = Editor::open(world_with_cube(), linear_log(2)).unwrap();

        // At the end of main: a commit on main is followed.
        let delta = editor.observe(entry("e3", Some("e2"), 3)).unwrap();
        assert!(delta.is_some_and(|d| d.len() == 1));
        assert_eq!(editor.tip(), Some("e3"));

        // Scrubbed back: a commit on main grows the history, and the
        // document on screen stays the one the person chose.
        editor.goto(Some("e1")).unwrap();
        let on_screen = editor.doc().len();
        assert!(
            editor
                .observe(entry("e4", Some("e3"), 4))
                .unwrap()
                .is_none()
        );
        assert_eq!(editor.tip(), Some("e1"));
        assert_eq!(editor.revision(), 1);
        assert_eq!(editor.doc().len(), on_screen);
        assert_eq!(editor.tips(), vec!["e4"]);

        // A branch from the entry on screen is what the person is looking
        // at: followed.
        assert!(
            editor
                .observe(entry("b1", Some("e1"), 5))
                .unwrap()
                .is_some()
        );
        assert_eq!(editor.tip(), Some("b1"));
        let mut tips = editor.tips();
        tips.sort_unstable();
        assert_eq!(tips, vec!["b1", "e4"]);
    }

    #[test]
    fn opening_at_a_tip_shows_that_tip_not_the_last_line() {
        // The last line is a branch's once one has been committed after main
        // moved; the head is whatever the package's ref says.
        let mut log = linear_log(2);
        log.push(entry("b1", Some("e1"), 3));
        let editor = Editor::open_at(world_with_cube(), log.clone(), Some("e2")).unwrap();
        assert_eq!(editor.tip(), Some("e2"));
        assert_eq!(editor.revision(), 2);
        assert_eq!(
            Editor::open(world_with_cube(), log.clone()).unwrap().tip(),
            Some("b1")
        );
        assert!(Editor::open_at(world_with_cube(), log, Some("nowhere")).is_err());
    }

    #[test]
    fn two_branches_hold_different_worlds_at_the_same_time() {
        let mut editor = Editor::new(world_with_cube());
        editor.committed(entry("root", None, 1)).unwrap();
        editor.committed(entry("left", Some("root"), 2)).unwrap();
        editor.fork_from("root").unwrap();
        editor.committed(entry("right", Some("root"), 3)).unwrap();

        editor.goto(Some("left")).unwrap();
        let left = editor.doc().len();
        editor.goto(Some("right")).unwrap();
        assert_eq!(editor.doc().len(), left, "each branch holds one spawn");
        assert_eq!(editor.history().children_of("root").len(), 2);
    }

    #[test]
    fn a_commit_whose_parent_is_not_in_the_log_is_refused() {
        let mut editor = Editor::new(world_with_cube());
        let err = editor
            .committed(entry("orphan", Some("nowhere"), 1))
            .unwrap_err();
        assert!(matches!(err, EditorError::History(_)), "{err:?}");
        assert!(editor.history().is_empty(), "nothing was appended");
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
    fn selection_drops_entities_a_seek_removed() {
        let mut editor = Editor::open(world_with_cube(), linear_log(1)).unwrap();
        let spawned: Vec<u64> = editor.doc().entities().map(|e| e.id.0).collect();
        editor.select(spawned.clone());
        assert_eq!(editor.selection().len(), spawned.len());

        editor.goto(None).unwrap();
        assert_eq!(
            editor.selection().iter().copied().collect::<Vec<_>>(),
            vec![1],
            "only the base's entity exists at the base"
        );
    }

    #[test]
    fn selecting_an_entity_that_is_not_there_selects_nothing() {
        let mut editor = Editor::new(world_with_cube());
        editor.select([9_999]);
        assert!(editor.selection().is_empty());
        editor.select([1]);
        editor.deselect_all();
        assert!(editor.selection().is_empty());
    }
}
