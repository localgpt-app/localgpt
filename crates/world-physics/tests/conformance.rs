//! The `ext-physics` conformance, run the way any engine runs it: the
//! same worlds and the same outcome assertions the JS reference tests
//! (mirrored from `openworldformat/conformance/`), folded and simulated
//! here. One assertion file, two engines, the same predicates.

use std::path::PathBuf;

use localgpt_world_physics::{
    Assertion, BOUNCE_SPEED, EXTENSION_NAME, OutcomesDoc, SimOptions, fold_trajectories,
    run_outcomes, simulate, trajectory_op,
};
use localgpt_world_sync::oplog::OpLogEntry;
use localgpt_world_sync::protocol::Author;
use localgpt_world_sync::session::SessionOp;
use localgpt_world_types as wt;
use localgpt_world_types::EditOp;

fn fixture(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../world-types/conformance")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn physics_world() -> wt::WorldManifest {
    serde_json::from_str(&fixture("physics.json")).unwrap()
}

fn outcomes_doc() -> OutcomesDoc {
    serde_json::from_str(&fixture("outcomes/physics.json")).unwrap()
}

#[test]
fn the_physics_conformance_outcomes_pass_under_this_solver() {
    let result = run_outcomes(&physics_world(), &outcomes_doc());
    assert!(result.ok, "failures: {:?}", result.failures);
}

#[test]
fn bodies_are_collected_from_the_declaration_never_inferred() {
    let world = localgpt_world_physics::collect_physics(&physics_world());
    assert_eq!(world.gravity, [0.0, -9.81, 0.0]);
    let mut names: Vec<&str> = world.dynamic.iter().map(|b| b.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["ball", "bouncy_ball"]);
    let mut statics: Vec<&str> = world.statics.iter().map(|s| s.name.as_str()).collect();
    statics.sort();
    assert_eq!(statics, vec!["ground", "pedestal"]);

    // hello-world declares no bodies: nothing participates.
    let plain: wt::WorldManifest = serde_json::from_str(&fixture("shapes.json")).unwrap();
    let plain = localgpt_world_physics::collect_physics(&plain);
    assert!(plain.dynamic.is_empty() && plain.statics.is_empty());
}

#[test]
fn the_simulation_is_deterministic() {
    let opts = SimOptions {
        until_s: 6.0,
        ..SimOptions::default()
    };
    assert_eq!(
        simulate(&physics_world(), &opts),
        simulate(&physics_world(), &opts)
    );
}

#[test]
fn a_dropped_ball_finds_the_pedestal_and_rests_on_it() {
    let opts = SimOptions {
        until_s: 6.0,
        ..SimOptions::default()
    };
    let sim = simulate(&physics_world(), &opts);
    let first = sim
        .contacts
        .iter()
        .find(|c| c.body == "ball" && c.other == "pedestal")
        .expect("ball never touched the pedestal");
    assert!(
        first.t_s < 2.0,
        "contact at {}s, later than expected",
        first.t_s
    );
    // Pedestal top is y = 1, ball radius 0.3: it rests at [0, 1.3, 0].
    let rest = sim.resting["ball"];
    assert!((rest[1] - 1.3).abs() < 0.05, "ball rests at {rest:?}");
    assert_eq!(rest[0], 0.0, "no horizontal drift");
    let bounces = sim
        .bounces
        .iter()
        .filter(|b| b.body == "bouncy_ball")
        .count();
    assert!(bounces >= 4, "{bounces} bounces");
    assert!(sim.settled_s < 6.0);
}

#[test]
fn extension_ops_parse_fold_to_nothing_and_roundtrip() {
    let line = r#"{ "ext-physics": { "t_s": [0.0, 0.1], "bodies": { "ball": [[0, 5, 0], [0, 4.95, 0]] } } }"#;
    let op: SessionOp = serde_json::from_str(line).unwrap();
    match &op {
        SessionOp::Extension(record) => {
            assert_eq!(record.name, "ext-physics");
            assert_eq!(record.body["t_s"][0], 0.0);
        }
        other => panic!("classified as {other:?}"),
    }
    // Round-trips verbatim.
    let back = serde_json::to_string(&op).unwrap();
    assert!(back.contains("\"ext-physics\""));

    // A non-ext single key is not an extension op: untagged fails it,
    // the log keeps its old behavior for unknown kinds.
    assert!(serde_json::from_str::<SessionOp>(r#"{ "nope": {} }"#).is_err());

    // Folds to nothing: the document never moves.
    let entry = OpLogEntry {
        revision: 1,
        author: Author {
            peer: None,
            name: "t".into(),
        },
        ops: vec![op],
        timestamp_ms: 0,
        id: None,
        parent: None,
    };
    assert!(entry.edit_ops().is_empty());
    assert!(entry.is_history_only());
    let base = localgpt_world_sync::WorldDoc::new("base");
    let folded = localgpt_world_sync::fold_log(&base, &[entry]).unwrap();
    assert_eq!(folded.len(), 0);
}

#[test]
fn ext_fields_survive_a_modify_patch_and_null_clears() {
    let manifest = physics_world();
    let mut doc = localgpt_world_sync::WorldDoc::new("physics");
    for entity in &manifest.entities {
        doc.apply(&EditOp::spawn(entity.clone())).unwrap();
    }
    // The spawn carried the component in `extra`…
    let ball = doc.get_by_name("ball").expect("ball");
    assert!(ball.extra.contains_key(EXTENSION_NAME));

    // …a patch updates it, and `null` clears it (Some(None), not a
    // dropped key — the reason EntityPatch's serde is hand-written).
    let set: wt::EditOp = serde_json::from_str(
        r#"{ "ModifyEntity": { "id": 4, "patch": { "ext-physics": { "body": "static" } } } }"#,
    )
    .unwrap();
    let clear: wt::EditOp = serde_json::from_str(
        r#"{ "ModifyEntity": { "id": 4, "patch": { "ext-physics": null } } }"#,
    )
    .unwrap();
    doc.apply(&set).unwrap();
    let patched = doc.get_by_name("ball").unwrap();
    assert_eq!(patched.extra[EXTENSION_NAME]["body"], "static");
    doc.apply(&clear).unwrap();
    assert!(
        !doc.get_by_name("ball")
            .unwrap()
            .extra
            .contains_key(EXTENSION_NAME)
    );

    // A null on a core slot still clears (regression for the hand-
    // written patch serde).
    let clear_shape: wt::EditOp =
        serde_json::from_str(r#"{ "ModifyEntity": { "id": 4, "patch": { "shape": null } } }"#)
            .unwrap();
    doc.apply(&clear_shape).unwrap();
    assert!(doc.get_by_name("ball").unwrap().shape.is_none());
}

#[test]
fn a_trajectory_op_round_trips() {
    let opts = SimOptions {
        until_s: 2.0,
        sample_dt_s: 0.25,
        ..SimOptions::default()
    };
    let sim = simulate(&physics_world(), &opts);
    let op = trajectory_op(&sim);
    let entry = OpLogEntry {
        revision: 1,
        author: Author {
            peer: None,
            name: "t".into(),
        },
        ops: vec![op],
        timestamp_ms: 0,
        id: None,
        parent: None,
    };
    // The entry is history-only…
    assert!(entry.is_history_only());
    // …and the tracks fold back out.
    let tracks = fold_trajectories(&[entry]);
    let ball = &tracks.bodies["ball"];
    assert_eq!(ball.len(), sim.samples.len());
    assert_eq!(ball[0].1, [0.0, 5.0, 0.0]);
    assert!(tracks.span_s > 0.0);
}

#[test]
fn assertion_shapes_parse_from_the_conformance_file() {
    // The untagged assertion enum reads the file's own shapes.
    let doc = outcomes_doc();
    assert!(
        doc.expect
            .iter()
            .any(|a| matches!(a, Assertion::Contact { .. }))
    );
    assert!(
        doc.expect
            .iter()
            .any(|a| matches!(a, Assertion::Rest { .. }))
    );
    assert!(
        doc.expect
            .iter()
            .any(|a| matches!(a, Assertion::Bounces { .. }))
    );
    assert_eq!(BOUNCE_SPEED, 0.5);
}

/// The openworldformat repository holds the governed copies of these
/// fixtures; when its checkout sits beside this one, they must agree —
/// the cross-engine conformance is only meaningful over the same file.
#[test]
fn fixtures_match_the_openworldformat_copies_when_present() {
    let sibling =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../openworldformat/conformance");
    for relative in ["physics.json", "outcomes/physics.json"] {
        let governed = sibling.join(relative);
        if !governed.is_file() {
            continue; // CI checks out localgpt alone; drift is caught locally
        }
        let theirs = std::fs::read_to_string(&governed).unwrap();
        let ours = fixture(relative);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&theirs).unwrap(),
            serde_json::from_str::<serde_json::Value>(&ours).unwrap(),
            "{relative} drifted from the openworldformat copy"
        );
    }
}
