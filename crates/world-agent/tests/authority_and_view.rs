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
