//! The scrub budget, proven on a synthetic session before any UI exists.
//!
//! The claim the editor stack rests on is that a timeline can be dragged
//! across a long history at interactive rates, because a seek costs the
//! entries between where you were and where you are going rather than the
//! whole path from the base. `fold_path` does the latter — correctly, and at
//! O(path) every call — so this measures the difference instead of asserting
//! that it exists.
//!
//! The assertions are on **work done** (entries applied), not on wall clock:
//! a timing test would be flaky on shared CI and would not say why it got
//! slow. Entries applied is the thing that actually scales, and it is exact.
//!
//! The session here is 2,000 entries rather than 10,000 because the fold
//! itself is quadratic upstream — `WorldDoc::apply_entry` makes three O(n)
//! clones per entry — so a 10,000-entry fold takes about 11 seconds and four
//! of them do not belong in a workspace test run. The shape of every claim
//! below is independent of the length; `a_ten_thousand_entry_session` keeps
//! the original gate behind `--ignored`, with the measured numbers.

use localgpt_world_editor::{Editor, FoldCache, History};
use openworldformat::author::Author;
use openworldformat::doc::WorldDoc;
use openworldformat::entity::WorldEntity;
use openworldformat::history::EditOp;
use openworldformat::oplog::OpLogEntry;
use openworldformat::session::SessionOp;

const SESSION: u64 = 2_000;
const LONG_SESSION: u64 = 10_000;

fn base() -> WorldDoc {
    WorldDoc::new("scrub")
}

/// A spawn per entry, each the child of the one before: the longest path a
/// linear session can have for a given entry count, which is the worst case
/// for a fold.
fn long_session(n: u64) -> Vec<OpLogEntry> {
    (1..=n)
        .map(|i| OpLogEntry {
            revision: i,
            author: Author {
                peer: None,
                name: "bench".into(),
            },
            ops: vec![SessionOp::Edit(Box::new(EditOp::SpawnEntity {
                entity: WorldEntity::new(i, format!("entity-{i}")),
            }))],
            timestamp_ms: 0,
            id: Some(format!("e{i}")),
            parent: (i > 1).then(|| format!("e{}", i - 1)),
            message: None,
        })
        .collect()
}

#[test]
fn a_long_session_opens_and_folds_whole() {
    let editor = Editor::open(base(), long_session(SESSION)).expect("opens");
    assert_eq!(editor.history().len(), SESSION as usize);
    assert_eq!(editor.doc().len(), SESSION as usize);
    assert_eq!(editor.tip(), Some(format!("e{SESSION}").as_str()));
    assert_eq!(editor.tips(), vec![format!("e{SESSION}")]);
}

/// The original gate: ten thousand entries, folded whole.
///
/// Ignored by default because it takes about eleven seconds, and the reason
/// is the finding: `WorldDoc::apply_entry` clones the document twice and the
/// name map once *per entry*, so the fold is O(n²) and opening a long
/// session — not scrubbing it — is what needs upstream work. Run it with
/// `cargo test -p localgpt-world-editor -- --ignored`.
#[test]
#[ignore = "about 11s: the fold is O(n^2) upstream, see the module docs"]
fn a_ten_thousand_entry_session_opens_and_folds_whole() {
    let editor = Editor::open(base(), long_session(LONG_SESSION)).expect("opens");
    assert_eq!(editor.history().len(), LONG_SESSION as usize);
    assert_eq!(editor.doc().len(), LONG_SESSION as usize);
}

#[test]
fn dragging_a_playhead_costs_one_entry_per_step() {
    let history = History::open(long_session(SESSION)).unwrap();
    let mut cache = FoldCache::new(base());

    // Land once in the middle — this one pays for the path, as any cold
    // seek must.
    cache.doc_at(&history, Some("e1000")).unwrap();
    let warm = cache.entries_applied();
    assert_eq!(warm, 1_000);

    // Then drag forward 200 entries, the way a playhead moves.
    for i in 1_001..=1_200 {
        cache.doc_at(&history, Some(&format!("e{i}"))).unwrap();
    }

    let dragged = cache.entries_applied() - warm;
    assert_eq!(
        dragged, 200,
        "200 steps should apply 200 entries; folding from the base would be \
         over a hundred thousand"
    );
}

#[test]
fn a_cold_seek_is_bounded_by_the_snapshot_cadence() {
    let history = History::open(long_session(SESSION)).unwrap();
    let cadence: usize = 64;
    let mut cache = FoldCache::new(base()).with_cadence(cadence, 256);

    // One pass to the end lays down the cadence snapshots.
    cache
        .doc_at(&history, Some(&format!("e{SESSION}")))
        .unwrap();
    let after_pass = cache.entries_applied();
    assert_eq!(after_pass, SESSION);

    // Now seek to twenty scattered positions. Each starts from the nearest
    // cadence snapshot, so none of them can cost more than the cadence.
    let mut worst = 0;
    let mut previous = after_pass;
    for k in 1..=20u64 {
        let target = (k * 491) % SESSION + 1;
        cache.doc_at(&history, Some(&format!("e{target}"))).unwrap();
        let cost = cache.entries_applied() - previous;
        previous = cache.entries_applied();
        worst = worst.max(cost);
    }

    assert!(
        worst < cadence as u64,
        "a cold seek cost {worst} applies; the cadence is {cadence}, so it \
         should never exceed it"
    );
}

#[test]
fn seeking_between_two_long_branches_costs_the_branches_not_the_trunk() {
    // A 5,000-entry trunk, then two 50-entry branches off its end.
    let mut log = long_session(1_000);
    for i in 1..=50u64 {
        for (side, offset) in [("left", 10_000u64), ("right", 20_000u64)] {
            let parent = if i == 1 {
                "e1000".to_string()
            } else {
                format!("{side}-{}", i - 1)
            };
            log.push(OpLogEntry {
                revision: 1_000 + i,
                author: Author {
                    peer: None,
                    name: side.into(),
                },
                ops: vec![SessionOp::Edit(Box::new(EditOp::SpawnEntity {
                    entity: WorldEntity::new(offset + i, format!("{side}-{i}")),
                }))],
                timestamp_ms: 0,
                id: Some(format!("{side}-{i}")),
                parent: Some(parent),
                message: None,
            });
        }
    }

    let history = History::open(log).unwrap();
    assert!(history.is_fork_point("e1000"));

    let mut cache = FoldCache::new(base());
    cache.doc_at(&history, Some("left-50")).unwrap();
    let after_left = cache.entries_applied();
    assert_eq!(after_left, 1_050);

    // The fork point is pinned on the way past it, so the other branch
    // starts there rather than at the base.
    cache.doc_at(&history, Some("right-50")).unwrap();
    let crossing = cache.entries_applied() - after_left;
    assert_eq!(
        crossing, 50,
        "crossing to the sibling should cost its 50 entries, not the \
         trunk's 1,000"
    );

    // And the two branches really do hold different worlds.
    let left = cache.doc_at(&history, Some("left-50")).unwrap();
    let right = cache.doc_at(&history, Some("right-50")).unwrap();
    assert!(left.get_by_name("left-50").is_some());
    assert!(left.get_by_name("right-50").is_none());
    assert!(right.get_by_name("right-50").is_some());
    assert!(right.get_by_name("left-50").is_none());
}

#[test]
fn an_editor_seek_reports_only_what_changed() {
    let mut editor = Editor::open(base(), long_session(SESSION)).unwrap();

    // Back one entry: one entity has to go.
    let delta = editor.goto(Some(&format!("e{}", SESSION - 1))).unwrap();
    assert_eq!(delta.len(), 1, "one step back is one delete");

    // Back a hundred: a hundred deletes, and not a rebuild of the session.
    let delta = editor.goto(Some(&format!("e{}", SESSION - 101))).unwrap();
    assert_eq!(delta.len(), 100);
}
