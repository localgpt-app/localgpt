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
