//! The authority and the view agree.
//!
//! `LiveWorld` owns the package and the commit; `localgpt-world-editor`'s
//! `Editor` is the non-linear view over the same log — tips, forks, seeks and
//! deltas. The split only works if the two never disagree about what the world
//! is, which is what this asserts: after every commit the view's document
//! matches the authority's head, and after a seek it matches what the log says
//! that point was.
//!
//! The view is built from the authority's own parts — `base()` and
//! `entries()` — so there is one fold, not two.

use std::path::PathBuf;

use localgpt_world_agent::headless::{self, Command};
use localgpt_world_agent::live::LiveWorld;
use localgpt_world_editor::Editor;
use serde_json::json;

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("lga-authority-view-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    headless::run(&dir, Command::Init { git: false }).expect("init");
    dir
}

fn spawn(name: &str) -> serde_json::Value {
    json!({"ops": [{"SpawnEntity": {"entity": {"name": name}}}]})
}

/// A view over the authority's current fold.
fn view_of(live: &LiveWorld) -> Editor {
    Editor::open(live.base().clone(), live.entries().to_vec()).expect("the view opens")
}

fn names(doc: &openworldformat::doc::WorldDoc) -> Vec<String> {
    let mut out: Vec<String> = doc.entities().map(|e| e.name.0.clone()).collect();
    out.sort();
    out
}

#[test]
fn the_view_follows_every_commit_and_agrees_with_the_head() {
    let dir = temp("follows");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);

    for name in ["lighthouse", "jetty", "buoy"] {
        let done = live.submit(&spawn(name), "test", None).expect(name);
        // The authority hands over the entry; the view follows it.
        let delta = view.committed(done.entry).expect("the view follows");
        assert_eq!(delta.len(), 1, "one spawn is one op");
        assert_eq!(
            names(view.doc()),
            names(live.head()),
            "the view disagreed with the head after committing {name}"
        );
    }
    assert_eq!(view.revision(), live.revision());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_view_can_seek_back_while_the_authority_stays_at_the_head() {
    let dir = temp("seek");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);

    let first = live.submit(&spawn("lighthouse"), "test", None).unwrap();
    view.committed(first.entry).unwrap();
    let first_id = view.tip().map(str::to_string);
    let second = live.submit(&spawn("jetty"), "test", None).unwrap();
    view.committed(second.entry).unwrap();

    assert_eq!(names(view.doc()), ["jetty", "lighthouse"]);

    // Seeking is a view change only: the authority is untouched, and nothing
    // was appended.
    let entries_before = live.entries().len();
    let delta = view.goto(first_id.as_deref()).expect("seek back");
    assert_eq!(delta.len(), 1, "one entity has to go");
    assert_eq!(names(view.doc()), ["lighthouse"]);
    assert_eq!(
        names(live.head()),
        ["jetty", "lighthouse"],
        "the head moved"
    );
    assert_eq!(live.entries().len(), entries_before, "a seek wrote nothing");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_refused_batch_leaves_the_view_and_the_authority_where_they_were() {
    let dir = temp("refused");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);
    let done = live.submit(&spawn("lighthouse"), "test", None).unwrap();
    view.committed(done.entry).unwrap();

    let before_view = names(view.doc());
    let before_head = names(live.head());
    let refused = live.submit(
        &json!({"ops": [{"ModifyEntity": {"id": 1, "patch": {"colour": "red"}}}]}),
        "test",
        None,
    );
    assert!(refused.is_err(), "a typo should be refused");

    // Nothing to follow, so nothing moved on either side.
    assert_eq!(names(view.doc()), before_view);
    assert_eq!(names(live.head()), before_head);
    assert_eq!(view.revision(), live.revision());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_undo_is_a_commit_the_view_follows_like_any_other() {
    let dir = temp("undo");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);

    let done = live.submit(&spawn("lighthouse"), "test", None).unwrap();
    view.committed(done.entry).unwrap();
    assert_eq!(names(view.doc()), ["lighthouse"]);

    let undone = live.undo("test").expect("undo");
    let delta = view
        .committed(undone.entry)
        .expect("the view follows an undo");
    assert_eq!(delta.len(), 1, "the inverse deletes what was spawned");
    assert!(names(view.doc()).is_empty());
    assert_eq!(names(view.doc()), names(live.head()));
    // The log grew; it did not rewind.
    assert_eq!(live.entries().len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_view_reopened_from_the_package_sees_what_the_authority_wrote() {
    // The restart case: a view built from the log on disk must reach the same
    // world as the one that followed the commits live.
    let dir = temp("reopen");
    {
        let mut live = LiveWorld::open(&dir).expect("open");
        for name in ["lighthouse", "jetty"] {
            live.submit(&spawn(name), "test", None).unwrap();
        }
    }
    let live = LiveWorld::open(&dir).expect("reopen");
    let view = view_of(&live);
    assert_eq!(names(view.doc()), ["jetty", "lighthouse"]);
    assert_eq!(names(view.doc()), names(live.head()));
    assert_eq!(view.history().len(), 2);
    assert_eq!(view.tips().len(), 1, "a linear log has one tip");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn every_new_entry_carries_its_content_hash_parent_and_message() {
    let dir = temp("identity");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);

    let first = live
        .submit(&spawn("lighthouse"), "test", Some("light the coast"))
        .unwrap();
    let second = live.submit(&spawn("jetty"), "test", None).unwrap();

    let (a, b) = (&first.entry, &second.entry);
    // The id is the format's content hash of the entry as written — so
    // recomputing it from the entry gives the same answer.
    for e in [a, b] {
        let id = e.id.as_deref().expect("an id on every new entry");
        assert!(id.starts_with("sha256:"), "{id}");
        assert_eq!(
            Some(id.to_string()),
            openworldformat::oplog::compute_entry_id(e).ok(),
            "the id is not the content hash of the entry"
        );
    }
    // The chain is explicit: the second names the first.
    assert_eq!(
        b.parent, a.id,
        "the parent should be the previous entry's id"
    );
    assert_eq!(a.parent, None, "the first builds on the base");
    // Draft 0.3: the message is the entry's own field.
    assert_eq!(a.message.as_deref(), Some("light the coast"));
    assert_eq!(b.message, None, "no message given, none invented");

    // The bug class this guards: what the authority hands over must be what
    // it wrote, or the view would name entries differently from the file.
    view.committed(first.entry.clone()).unwrap();
    view.committed(second.entry.clone()).unwrap();
    let on_disk: Vec<_> = LiveWorld::open(&dir)
        .unwrap()
        .entries()
        .iter()
        .map(|e| e.id.clone())
        .collect();
    let in_view: Vec<_> = (0..view.history().len())
        .map(|n| view.history().id_at(n).map(str::to_string))
        .collect();
    assert_eq!(on_disk, in_view, "the view and the file disagree on ids");
    std::fs::remove_dir_all(&dir).ok();
}

/// A package whose log branches: e1 spawns A, then e2 (spawning B) and e3
/// (spawning C) both build on e1. The head is the last entry, e3, and the
/// world there is A and C — never B, which lives only on the other branch.
fn branched(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lga-branched-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("snapshots")).unwrap();
    let base = r#"{"version": 3, "meta": {"name": "branched"}, "entities": []}"#;
    std::fs::write(dir.join("manifest.json"), base).unwrap();
    std::fs::write(dir.join("snapshots/base.json"), base).unwrap();
    let entry = |id: &str, parent: Option<&str>, rev: u64, name: &str, eid: u64| {
        let mut e = json!({"revision": rev, "author": {"name": "test"}, "timestamp_ms": rev,
            "id": id, "ops": [{"SpawnEntity": {"entity": {"id": eid, "name": name}}}]});
        if let Some(p) = parent {
            e["parent"] = json!(p);
        }
        e.to_string()
    };
    let log = [
        entry("e1", None, 1, "A", 1),
        entry("e2", Some("e1"), 2, "B", 2),
        entry("e3", Some("e1"), 3, "C", 3),
    ]
    .join("\n");
    std::fs::write(dir.join("ops.jsonl"), log + "\n").unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"format_version": 2, "name": "branched", "base_revision": 0, "head_revision": 3}"#,
    )
    .unwrap();
    dir
}

#[test]
fn on_a_branched_log_the_head_is_the_fold_of_its_path_not_of_every_line() {
    // The bug this pins: folding every line in file order gives A, B and C —
    // a world that never existed on any branch — and the canvas drew it.
    let dir = branched("head");
    let live = LiveWorld::open(&dir).expect("open");
    let view = view_of(&live);

    assert_eq!(names(live.head()), ["A", "C"], "the authority's head");
    assert_eq!(names(view.doc()), ["A", "C"], "the view's head");
    assert_eq!(view.tip(), Some("e3"));

    // And what `open` wrote to manifest.json is that same world.
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut written: Vec<&str> = manifest["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    written.sort();
    assert_eq!(written, ["A", "C"], "manifest.json");
    assert!(live.verify().is_empty(), "{:?}", live.verify());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn on_a_branched_log_undo_takes_back_the_heads_newest_not_the_files() {
    // e2 is on the other branch; undoing it would apply B's inverse to a
    // world that has no B.
    let dir = branched("undo");
    let mut live = LiveWorld::open(&dir).expect("open");
    let undone = live.undo("test").expect("undo on the head's path");
    assert_eq!(undone.warnings, ["undid revision 3"], "it took back e3");
    assert_eq!(names(live.head()), ["A"]);
    std::fs::remove_dir_all(&dir).ok();
}

/// `package.json`'s `refs.main`, as written.
fn main_ref(dir: &std::path::Path) -> Option<String> {
    let package: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("package.json")).unwrap()).unwrap();
    package["refs"]["main"].as_str().map(str::to_string)
}

/// The authority's head as a view would open it: at main's tip, which once a
/// branch exists is not the last line.
fn view_at_head(live: &LiveWorld) -> Editor {
    Editor::open_at(
        live.base().clone(),
        live.entries().to_vec(),
        live.main_tip().as_deref(),
    )
    .expect("the view opens at the head")
}

#[test]
fn a_batch_at_an_earlier_entry_starts_a_branch_and_leaves_the_head_alone() {
    let dir = temp("fork");
    let mut live = LiveWorld::open(&dir).expect("open");
    let first = live.submit(&spawn("lighthouse"), "test", None).unwrap();
    let second = live.submit(&spawn("jetty"), "test", None).unwrap();
    let (e1, e2) = (first.entry.id.unwrap(), second.entry.id.unwrap());
    assert_eq!(main_ref(&dir), None, "a linear log needs no ref");
    let manifest_before = std::fs::read(dir.join("manifest.json")).unwrap();

    let fork = live
        .submit_at(Some(&e1), &spawn("buoy"), "test", Some("a buoy instead"))
        .expect("a batch at an earlier entry");

    assert!(fork.forked);
    assert_eq!(fork.entry.parent.as_deref(), Some(e1.as_str()));
    assert_eq!(
        fork.revision, 3,
        "revisions stay the authority's total order"
    );
    // The head did not move: main is still e2, on disk and in memory.
    assert_eq!(names(live.head()), ["jetty", "lighthouse"]);
    assert_eq!(live.revision(), 2);
    assert_eq!(live.main_tip().as_deref(), Some(e2.as_str()));
    assert_eq!(
        std::fs::read(dir.join("manifest.json")).unwrap(),
        manifest_before,
        "manifest.json is main's, and main did not move"
    );
    // The trap: the branch entry is the last line, so without a ref the
    // format's fallback would make it main.
    assert_eq!(main_ref(&dir).as_deref(), Some(e2.as_str()));

    // Both worlds exist: the view sees two tips, each its own world.
    let mut view = view_at_head(&live);
    let mut tips: Vec<String> = view.tips().into_iter().map(str::to_string).collect();
    tips.sort();
    let mut expected = vec![e2.clone(), fork.entry.id.clone().unwrap()];
    expected.sort();
    assert_eq!(tips, expected);
    assert_eq!(names(view.doc()), ["jetty", "lighthouse"]);
    view.goto(fork.entry.id.as_deref()).unwrap();
    assert_eq!(names(view.doc()), ["buoy", "lighthouse"]);

    assert!(live.verify().is_empty(), "{:?}", live.verify());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_commit_on_main_after_a_branch_builds_on_main_and_moves_its_ref() {
    let dir = temp("main-after-fork");
    let mut live = LiveWorld::open(&dir).expect("open");
    let e1 = live
        .submit(&spawn("lighthouse"), "test", None)
        .unwrap()
        .entry
        .id
        .unwrap();
    let e2 = live
        .submit(&spawn("jetty"), "test", None)
        .unwrap()
        .entry
        .id
        .unwrap();
    let fork = live
        .submit_at(Some(&e1), &spawn("buoy"), "test", None)
        .unwrap();

    let main = live.submit(&spawn("pier"), "test", None).unwrap();

    assert!(!main.forked);
    assert_eq!(
        main.entry.parent.as_deref(),
        Some(e2.as_str()),
        "main builds on main's tip, not on the last line (the branch's)"
    );
    let e4 = main.entry.id.clone().unwrap();
    assert_eq!(
        main_ref(&dir).as_deref(),
        Some(e4.as_str()),
        "the ref moved"
    );
    assert_eq!(names(live.head()), ["jetty", "lighthouse", "pier"]);
    let revisions: Vec<u64> = live.entries().iter().map(|e| e.revision).collect();
    assert_eq!(revisions, [1, 2, 3, 4], "no revision taken twice");
    // One authority never mints an entity id twice, on any branch.
    assert_ne!(
        fork.spawned["buoy"], main.spawned["pier"],
        "two branches spawned the same id"
    );

    // Undo takes back main's newest, never the branch's.
    let undone = live.undo("test").unwrap();
    assert_eq!(undone.warnings, ["undid revision 4"]);
    assert_eq!(names(live.head()), ["jetty", "lighthouse"]);
    assert_eq!(undone.revision, 5);
    assert!(live.verify().is_empty(), "{:?}", live.verify());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reopening_a_package_whose_last_line_is_a_branch_keeps_main_as_the_head() {
    // On the fixture, main is e3 (the last line, no ref yet). Extend the
    // other branch, e2: its entry becomes the last line.
    let dir = branched("reopen-fork");
    {
        let mut live = LiveWorld::open(&dir).expect("open");
        let fork = live
            .submit_at(Some("e2"), &spawn("D"), "test", None)
            .unwrap();
        assert!(fork.forked);
        assert_eq!(main_ref(&dir).as_deref(), Some("e3"));
    }
    let live = LiveWorld::open(&dir).expect("reopen");
    assert_eq!(live.main_tip().as_deref(), Some("e3"));
    assert_eq!(
        names(live.head()),
        ["A", "C"],
        "main, not the newest branch"
    );
    let view = view_at_head(&live);
    assert_eq!(names(view.doc()), names(live.head()));
    assert!(live.verify().is_empty(), "{:?}", live.verify());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_batch_names_where_it_builds_as_it_names_its_author() {
    let dir = temp("at-in-body");
    let mut live = LiveWorld::open(&dir).expect("open");
    let e1 = live
        .submit(&spawn("lighthouse"), "test", None)
        .unwrap()
        .entry
        .id
        .unwrap();
    live.submit(&spawn("jetty"), "test", None).unwrap();

    let body = json!({"at": e1, "ops": [{"SpawnEntity": {"entity": {"name": "buoy"}}}]});
    let fork = live.submit(&body, "test", None).unwrap();
    assert!(fork.forked);
    assert_eq!(fork.entry.parent.as_deref(), Some(e1.as_str()));

    // Null is the head said out loud; naming main's tip is the same.
    let body = json!({"at": null, "ops": [{"SpawnEntity": {"entity": {"name": "pier"}}}]});
    assert!(!live.submit(&body, "test", None).unwrap().forked);
    let tip = live.main_tip().unwrap();
    let on_tip = live
        .submit_at(Some(&tip), &spawn("crane"), "test", None)
        .unwrap();
    assert!(!on_tip.forked);
    assert_eq!(names(live.head()), ["crane", "jetty", "lighthouse", "pier"]);

    let refused = live
        .submit(
            &json!({"at": 7, "ops": [{"SpawnEntity": {"entity": {"name": "x"}}}]}),
            "test",
            None,
        )
        .unwrap_err();
    assert!(refused.errors[0].contains("\"at\""), "{:?}", refused.errors);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_branch_from_nowhere_or_from_an_entry_without_an_id_is_refused() {
    let dir = temp("refuse-fork");
    let mut live = LiveWorld::open(&dir).expect("open");
    live.submit(&spawn("lighthouse"), "test", None).unwrap();
    let refused = live
        .submit_at(Some("sha256:nope"), &spawn("buoy"), "test", None)
        .unwrap_err();
    assert!(
        refused.errors[0].contains("no entry"),
        "{:?}",
        refused.errors
    );
    std::fs::remove_dir_all(&dir).ok();

    // A log from before ids: e1 carries one, the line after it does not, so
    // main's tip is the synthesized `line-1`, which a ref cannot name.
    let dir = branched("legacy");
    let log = std::fs::read_to_string(dir.join("ops.jsonl")).unwrap();
    let first = log.lines().next().unwrap();
    let legacy = r#"{"revision": 2, "author": {"name": "test"}, "timestamp_ms": 2, "ops": [{"SpawnEntity": {"entity": {"id": 2, "name": "B"}}}]}"#;
    std::fs::write(dir.join("ops.jsonl"), format!("{first}\n{legacy}\n")).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"format_version": 2, "name": "legacy", "base_revision": 0, "head_revision": 2}"#,
    )
    .unwrap();
    let mut live = LiveWorld::open(&dir).expect("open");
    assert_eq!(live.main_tip().as_deref(), Some("line-1"));

    let refused = live.submit_at(Some("line-1"), &spawn("C"), "test", None);
    // `line-1` is main's tip, so naming it is a commit on main — allowed.
    assert!(!refused.unwrap().forked);
    let fresh = live.main_tip().unwrap();
    assert!(
        fresh.starts_with("sha256:"),
        "the new entry has an id: {fresh}"
    );

    // The id-less line is now history: no branch may name it as a parent.
    let refused = live
        .submit_at(Some("line-1"), &spawn("D"), "test", None)
        .unwrap_err();
    assert!(
        refused.errors[0].contains("has no id"),
        "{:?}",
        refused.errors
    );

    // e1 has an id and main's tip now does too: the branch is fine.
    let fork = live
        .submit_at(Some("e1"), &spawn("D"), "test", None)
        .unwrap();
    assert!(fork.forked);
    assert_eq!(main_ref(&dir).as_deref(), Some(fresh.as_str()));
    assert_eq!(names(live.head()), ["A", "B", "C"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_view_at_the_head_does_not_jump_to_a_branch_it_was_not_on() {
    let dir = temp("observe");
    let mut live = LiveWorld::open(&dir).expect("open");
    let mut view = view_of(&live);
    let first = live.submit(&spawn("lighthouse"), "test", None).unwrap();
    view.observe(first.entry.clone()).unwrap();
    let second = live.submit(&spawn("jetty"), "test", None).unwrap();
    view.observe(second.entry).unwrap();

    let e1 = first.entry.id.unwrap();
    let fork = live
        .submit_at(Some(&e1), &spawn("buoy"), "test", None)
        .unwrap();
    assert!(
        view.observe(fork.entry.clone()).unwrap().is_none(),
        "stayed put"
    );
    assert_eq!(names(view.doc()), names(live.head()));

    // From the fork point, the person sees the branch they asked for.
    view.goto(Some(&e1)).unwrap();
    let more = live
        .submit_at(Some(&e1), &spawn("bell"), "test", None)
        .unwrap();
    assert!(view.observe(more.entry).unwrap().is_some(), "followed");
    assert_eq!(names(view.doc()), ["bell", "lighthouse"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn undo_at_a_branch_takes_back_the_branchs_newest_and_leaves_the_head() {
    let dir = temp("undo-at");
    let mut live = LiveWorld::open(&dir).expect("open");
    let e1 = live
        .submit(&spawn("lighthouse"), "test", None)
        .unwrap()
        .entry
        .id
        .unwrap();
    live.submit(&spawn("jetty"), "test", None).unwrap();
    let b1 = live
        .submit_at(Some(&e1), &spawn("buoy"), "test", None)
        .unwrap();
    let b1_id = b1.entry.id.clone().unwrap();
    let head_before = names(live.head());

    let undone = live
        .undo_at(Some(&b1_id), "test")
        .expect("undo on the branch");
    assert!(undone.forked, "off main, an undo is a branch commit");
    assert_eq!(undone.warnings, [format!("undid revision {}", b1.revision)]);
    assert_eq!(undone.entry.parent.as_deref(), Some(b1_id.as_str()));
    assert_eq!(names(live.head()), head_before, "the head did not move");

    // The branch's world is back to its fork point's.
    let mut view = view_at_head(&live);
    view.goto(undone.entry.id.as_deref()).unwrap();
    assert_eq!(names(view.doc()), ["lighthouse"]);

    // Undo again on the branch: the buoy's undo is not undone, and the
    // lighthouse (shared with main) is next.
    let again = live.undo_at(undone.entry.id.as_deref(), "test").unwrap();
    assert_eq!(again.warnings, ["undid revision 1"]);
    view.observe(again.entry.clone()).unwrap();
    assert!(names(view.doc()).is_empty(), "{:?}", names(view.doc()));
    assert_eq!(names(live.head()), head_before, "main still has both");

    // At main's tip, undo_at is plain undo.
    let tip = live.main_tip();
    let on_main = live.undo_at(tip.as_deref(), "test").unwrap();
    assert!(!on_main.forked);
    assert_eq!(names(live.head()), ["lighthouse"]);
    assert!(live.verify().is_empty(), "{:?}", live.verify());
    std::fs::remove_dir_all(&dir).ok();
}
