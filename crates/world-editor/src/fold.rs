//! Folds that are fast enough to scrub.
//!
//! `openworldformat::session::fold_path` folds from the base every call,
//! which is right for a one-shot read and wrong for a timeline: dragging a
//! playhead across a thousand-entry session would re-apply the whole path
//! sixty times a second. The format's branching RFC names the fix — *"seek
//! within a branch uses that branch's nearest snapshot"* — and leaves it to
//! the app, which is this.
//!
//! ## What bounds this, and it is not the cache
//!
//! `WorldDoc::apply_entry` is O(document size) per entry: it clones the
//! document for atomicity, calls `apply_all` which clones it *again*, then
//! `resolve_refs` clones the whole name map to dodge a borrow. Three O(n)
//! clones per entry make any fold O(n²) — a 10,000-entry session folds whole
//! in about 11 seconds on an M-series laptop, around 1.1 ms per entry, and
//! that figure is flat against this cache's cadence because the cost is in
//! the applies, not the snapshots.
//!
//! So the cache is not a workaround for a slow fold; it is what keeps a
//! *seek* off that curve. It cannot help the first open of a long session,
//! which is upstream work: collapsing the nested clone in `apply_entry` and
//! borrowing `names` instead of cloning it would take the fold from O(n²)
//! toward O(n), and is worth raising against the format crate.
//!
//! The cache keeps a folded document for some entries and, asked for a tip,
//! walks back along the path until it finds one, then folds forward only the
//! remainder. Moving one entry along a branch costs one entry. Jumping to
//! another branch costs the distance back to a cached ancestor, which the
//! fork point usually provides because every snapshot on the trunk is shared
//! by every branch off it.
//!
//! Snapshots are kept in three places, bounded by `capacity`:
//!
//! - **On a cadence** — every `cadence` entries along a path. These bound the
//!   worst-case cold seek.
//! - **At fork points** — any entry with more than one child. These are
//!   exactly the entries two branches share, so they are what makes jumping
//!   between siblings cost the branches rather than the trunk. Without them
//!   a seek from one branch to its sibling re-folds the whole trunk, because
//!   the sibling's own tip snapshot is not on the path.
//! - **At the tip asked for** — so a repeat fold is free.
//!
//! The first two are *pinned*: an ad-hoc tip snapshot only helps where the
//! user has already been, while a pinned one bounds a seek that has not
//! happened yet, so ad-hoc snapshots are evicted first.

use std::collections::BTreeMap;

use openworldformat::doc::{ApplyError, WorldDoc};

use crate::history::{EntryId, History, HistoryError};

/// What went wrong folding.
#[derive(Debug)]
pub enum FoldError {
    /// The log's shape is wrong, or the tip is not in it.
    History(HistoryError),
    /// An op no longer applies — a fold that cannot reach the tip. The
    /// document is left on the last entry that did apply.
    Apply(ApplyError),
}

impl std::fmt::Display for FoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::History(e) => write!(f, "{e}"),
            Self::Apply(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FoldError {}

impl From<HistoryError> for FoldError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

impl From<ApplyError> for FoldError {
    fn from(e: ApplyError) -> Self {
        Self::Apply(e)
    }
}

/// How often to keep a snapshot along a path, and how many to hold.
///
/// Measured, not guessed. Over a 10,000-entry session (see
/// `tests/scrub_budget.rs`), the cold-seek cost against cadence is:
///
/// | cadence | snapshots | cold seek |
/// |---|---|---|
/// | 32 | 312 | 17.6 ms |
/// | 128 | 78 | 17.2 ms |
/// | 512 | 19 | 404 ms |
/// | 2048 | 4 | 880 ms |
/// | none | 1 | 2.6 s |
///
/// 128 is the knee: a quarter of the snapshots of cadence 32 for the same
/// seek. The capacity then has to exceed `path / cadence` for the cadence to
/// bound a seek *anywhere* rather than only near where the user has been,
/// and 256 carries a 32,000-entry session. Past that the cache degrades to a
/// working set, which is the right failure.
const DEFAULT_CADENCE: usize = 128;
const DEFAULT_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
struct Snapshot {
    doc: WorldDoc,
    /// Kept for the log's shape rather than for where the user has been: a
    /// cadence position, or a fork point. Evicted last.
    pinned: bool,
    /// Monotonic counter, for evicting the least recently used ad-hoc one.
    touched: u64,
}

/// Folded documents, keyed by the entry they fold *through*.
#[derive(Debug, Clone)]
pub struct FoldCache {
    base: WorldDoc,
    snapshots: BTreeMap<EntryId, Snapshot>,
    cadence: usize,
    capacity: usize,
    clock: u64,
    /// Counts entries actually applied, so a test can prove a seek is
    /// incremental rather than trusting that it felt fast.
    applied: u64,
}

impl FoldCache {
    /// A cache over a base document, with the default cadence and capacity.
    pub fn new(base: WorldDoc) -> Self {
        Self {
            base,
            snapshots: BTreeMap::new(),
            cadence: DEFAULT_CADENCE,
            capacity: DEFAULT_CAPACITY,
            clock: 0,
            applied: 0,
        }
    }

    /// Keep a snapshot every `cadence` entries along a path, holding at most
    /// `capacity` of them. A cadence of 0 is treated as 1.
    pub fn with_cadence(mut self, cadence: usize, capacity: usize) -> Self {
        self.cadence = cadence.max(1);
        self.capacity = capacity.max(1);
        self
    }

    /// The base document — the state before any entry.
    pub fn base(&self) -> &WorldDoc {
        &self.base
    }

    /// How many entries have been applied across every fold so far. A
    /// measure of work done, not of cache size.
    pub fn entries_applied(&self) -> u64 {
        self.applied
    }

    /// Forget every snapshot. The next fold pays full price.
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }

    /// Drop snapshots for entries the log no longer contains — after a
    /// branch is pruned, say. Keeps the cache honest without clearing it.
    pub fn retain_known(&mut self, history: &History) {
        self.snapshots.retain(|id, _| history.get(id).is_some());
    }

    /// The document at `tip`, or at the base when `tip` is `None`.
    ///
    /// Folds forward from the nearest cached ancestor. Snapshots the result
    /// and every cadence position it passed through on the way.
    pub fn doc_at(&mut self, history: &History, tip: Option<&str>) -> Result<WorldDoc, FoldError> {
        let Some(tip) = tip else {
            return Ok(self.base.clone());
        };
        let path = history.path_to(tip)?;

        // The deepest position on the path that we already hold.
        let start = path.iter().rposition(|id| self.snapshots.contains_key(*id));

        let (mut doc, from) = match start {
            Some(i) => {
                let id = path[i];
                let snap = self
                    .snapshots
                    .get_mut(id)
                    .expect("rposition found it a line ago");
                self.clock += 1;
                snap.touched = self.clock;
                (snap.doc.clone(), i + 1)
            }
            None => (self.base.clone(), 0),
        };

        for (offset, id) in path.iter().enumerate().skip(from) {
            let entry = history
                .get(id)
                .ok_or_else(|| HistoryError::NoSuchEntry((*id).to_string()))?;
            doc.apply_entry(&entry.entry.edit_ops())?;
            self.applied += 1;
            // `offset` is 0-based along the path, so position 1 is the
            // first entry. Pin a cadence position, and pin every fork point
            // — the entries every branch off them shares.
            let position = offset + 1;
            if position % self.cadence == 0 || history.is_fork_point(id) {
                self.insert(id, doc.clone(), true);
            }
        }

        self.insert(tip, doc.clone(), false);
        Ok(doc)
    }

    fn insert(&mut self, id: &str, doc: WorldDoc, pinned: bool) {
        self.clock += 1;
        let touched = self.clock;
        match self.snapshots.get_mut(id) {
            Some(existing) => {
                existing.touched = touched;
                // Pinning is one-way: it is a statement about the log's
                // shape, not about recency, and the shape does not un-fork.
                existing.pinned |= pinned;
            }
            None => {
                self.snapshots.insert(
                    id.to_string(),
                    Snapshot {
                        doc,
                        pinned,
                        touched,
                    },
                );
                self.evict_if_needed();
            }
        }
    }

    fn evict_if_needed(&mut self) {
        // The most recently touched snapshot is never a victim. Without this
        // a drag along a branch evicts the very snapshot the next step needs:
        // at capacity, inserting the new tip pushes the cache over, the two
        // youngest entries are the only ad-hoc ones, and the second eviction
        // takes the one just made — so every step folds back to the nearest
        // pinned position instead of costing one entry.
        let newest = self
            .snapshots
            .iter()
            .max_by_key(|(_, s)| s.touched)
            .map(|(id, _)| id.clone());

        while self.snapshots.len() > self.capacity {
            // Prefer the least recently used ad-hoc snapshot; only start on
            // the pinned ones when nothing else is left, because they are
            // what bounds a seek that has not happened yet.
            let pick = |pinned_ok: bool, exclude: Option<&String>| {
                self.snapshots
                    .iter()
                    .filter(|(id, s)| (pinned_ok || !s.pinned) && Some(*id) != exclude)
                    .min_by_key(|(_, s)| s.touched)
                    .map(|(id, _)| id.clone())
            };
            let victim = pick(false, newest.as_ref()).or_else(|| pick(true, newest.as_ref()));
            match victim {
                Some(id) => {
                    self.snapshots.remove(&id);
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{linear_log, move_entry, spawn_entry, world_with_cube};

    #[test]
    fn a_fold_reaches_the_tip_and_the_base_is_untouched() {
        let base = world_with_cube();
        let history = History::open(linear_log(3)).unwrap();
        let mut cache = FoldCache::new(base.clone());
        let doc = cache
            .doc_at(&history, history.last_id())
            .expect("folds to the tip");
        assert_eq!(doc.len(), base.len() + 3);
        assert_eq!(cache.base().len(), base.len());
    }

    #[test]
    fn no_tip_is_the_base() {
        let mut cache = FoldCache::new(world_with_cube());
        let history = History::open(linear_log(2)).unwrap();
        let doc = cache.doc_at(&history, None).unwrap();
        assert_eq!(doc.len(), 1);
        assert_eq!(cache.entries_applied(), 0);
    }

    #[test]
    fn stepping_one_entry_applies_one_entry() {
        let history = History::open(linear_log(40)).unwrap();
        let mut cache = FoldCache::new(world_with_cube());
        cache.doc_at(&history, Some("e20")).unwrap();
        let after_first = cache.entries_applied();
        assert_eq!(after_first, 20, "the first fold pays the whole path");

        cache.doc_at(&history, Some("e21")).unwrap();
        assert_eq!(
            cache.entries_applied() - after_first,
            1,
            "the next entry costs one apply, not twenty-one"
        );
    }

    #[test]
    fn a_repeat_fold_applies_nothing() {
        let history = History::open(linear_log(10)).unwrap();
        let mut cache = FoldCache::new(world_with_cube());
        cache.doc_at(&history, Some("e10")).unwrap();
        let after = cache.entries_applied();
        cache.doc_at(&history, Some("e10")).unwrap();
        assert_eq!(cache.entries_applied(), after);
    }

    #[test]
    fn jumping_to_a_sibling_branch_folds_from_the_fork_point() {
        // A trunk of 10, then two branches of one entry each off e10.
        let mut log = linear_log(10);
        log.push(spawn_entry("left", Some("e10"), 11));
        log.push(spawn_entry("right", Some("e10"), 12));
        let history = History::open(log).unwrap();

        let mut cache = FoldCache::new(world_with_cube());
        cache.doc_at(&history, Some("left")).unwrap();
        let after_left = cache.entries_applied();
        assert_eq!(after_left, 11);

        cache.doc_at(&history, Some("right")).unwrap();
        assert_eq!(
            cache.entries_applied() - after_left,
            1,
            "the trunk is shared, so only the other branch's entry applies"
        );
    }

    #[test]
    fn cadence_snapshots_bound_a_cold_seek() {
        let history = History::open(linear_log(100)).unwrap();
        let mut cache = FoldCache::new(world_with_cube()).with_cadence(10, 64);
        cache.doc_at(&history, Some("e100")).unwrap();
        let after_full = cache.entries_applied();
        assert_eq!(after_full, 100);

        // Seeking backwards to an uncached position starts at the nearest
        // cadence snapshot (e90 for a target of e95), not at the base.
        cache.doc_at(&history, Some("e95")).unwrap();
        assert_eq!(cache.entries_applied() - after_full, 5);
    }

    #[test]
    fn eviction_keeps_the_cadence_snapshots() {
        let history = History::open(linear_log(60)).unwrap();
        // Cadence 10 over 60 entries is 6 cadence snapshots; capacity 8
        // leaves room for 2 ad-hoc ones.
        let mut cache = FoldCache::new(world_with_cube()).with_cadence(10, 8);
        for i in 1..=60 {
            cache.doc_at(&history, Some(&format!("e{i}"))).unwrap();
        }
        for i in (10..=60).step_by(10) {
            assert!(
                cache.snapshots.contains_key(&format!("e{i}")),
                "cadence snapshot e{i} was evicted"
            );
        }
        assert!(cache.snapshots.len() <= 8);
    }

    #[test]
    fn a_fold_that_cannot_apply_reports_it() {
        // Move an entity that the base does not have and no entry spawns.
        let history = History::open(vec![move_entry(9_999, Some("e1"), 1)]).unwrap();
        let mut cache = FoldCache::new(world_with_cube());
        let err = cache.doc_at(&history, Some("e1")).unwrap_err();
        assert!(matches!(err, FoldError::Apply(_)), "got {err:?}");
    }

    #[test]
    fn retain_known_drops_snapshots_for_pruned_entries() {
        let history = History::open(linear_log(5)).unwrap();
        let mut cache = FoldCache::new(world_with_cube());
        cache.doc_at(&history, Some("e5")).unwrap();
        assert!(!cache.snapshots.is_empty());

        let shorter = History::open(linear_log(2)).unwrap();
        cache.retain_known(&shorter);
        assert!(cache.snapshots.keys().all(|k| k == "e1" || k == "e2"));
    }
}
