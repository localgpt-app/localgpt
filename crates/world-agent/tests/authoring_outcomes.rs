//! The format's authoring outcome suite through this crate's live
//! authority: each batch over a fresh package holding the base world, and
//! what must come back — the same predicates the reference crate checks
//! over its own ingest, so the two authorities cannot drift apart over
//! what a batch commits (a behavior target sent by name once committed as
//! the name here, and the package stopped verifying against its history).

use std::path::PathBuf;

use localgpt_world_agent::live::{LiveWorld, Rejected};
use openworldformat::WorldDoc;
use serde_json::Value;

fn suite() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/authoring.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the vendored suite"))
        .expect("suite json")
}

/// Numbers compare by value — the suite writes `2`, an f32 field reads back `2.0`.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0),
            _ => false,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

fn check_world(name: &str, doc: &WorldDoc, predicates: &[Value]) {
    for p in predicates {
        let entity = p["entity"].as_str().expect("entity name");
        let found = doc.get_by_name(entity);
        if p["absent"].as_bool() == Some(true) {
            assert!(
                found.is_none(),
                "{name}: entity '{entity}' should be absent"
            );
            continue;
        }
        let found = found.unwrap_or_else(|| panic!("{name}: no entity '{entity}'"));
        let json = serde_json::to_value(found).unwrap();
        let path = p["path"].as_str().expect("path");
        let got = json
            .pointer(path)
            .unwrap_or_else(|| panic!("{name}: '{entity}' has nothing at {path}: {json}"));
        assert!(
            same(got, &p["equals"]),
            "{name}: '{entity}' at {path} is {got}, expected {}",
            p["equals"]
        );
    }
}

#[test]
fn the_live_authority_passes_the_authoring_outcomes() {
    let suite = suite();
    let cases = suite["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for (n, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().unwrap_or("?");
        let dir = std::env::temp_dir().join(format!("lga-authoring-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&suite["base"]).unwrap(),
        )
        .unwrap();
        let mut live = LiveWorld::open(&dir).expect("open");
        let expect = &case["expect"];
        let world_predicates: Vec<Value> = expect["world"].as_array().cloned().unwrap_or_default();
        match (
            live.submit(&case["batch"], "suite", None),
            expect["result"].as_str(),
        ) {
            (Ok(done), Some("committed")) => {
                for want in expect["warnings_contain"].as_array().into_iter().flatten() {
                    let want = want.as_str().unwrap();
                    assert!(
                        done.warnings.iter().any(|w| w.contains(want)),
                        "{name}: no warning contains {want:?}: {:?}",
                        done.warnings
                    );
                }
                if let Some(spawned) = expect["spawned"].as_object() {
                    for (sn, id) in spawned {
                        assert_eq!(
                            done.spawned.get(sn).copied(),
                            id.as_u64(),
                            "{name}: spawned {sn} -> {:?}",
                            done.spawned
                        );
                    }
                }
                for p in expect["ops"].as_array().into_iter().flatten() {
                    let i = p["op"].as_u64().expect("op index") as usize;
                    let op = serde_json::to_value(&done.ops[i]).unwrap();
                    let path = p["path"].as_str().expect("path");
                    let got = op
                        .pointer(path)
                        .unwrap_or_else(|| panic!("{name}: op {i} has nothing at {path}: {op}"));
                    assert!(
                        same(got, &p["equals"]),
                        "{name}: op {i} at {path} is {got}, expected {}",
                        p["equals"]
                    );
                }
                check_world(name, live.head(), &world_predicates);
                assert!(live.verify().is_empty(), "{name}: {:?}", live.verify());
            }
            (Err(Rejected { errors }), Some("refused")) => {
                for want in expect["errors_contain"].as_array().into_iter().flatten() {
                    let want = want.as_str().unwrap();
                    assert!(
                        errors.iter().any(|e| e.contains(want)),
                        "{name}: no error contains {want:?}: {errors:?}"
                    );
                }
                check_world(name, live.head(), &world_predicates);
                assert_eq!(live.revision(), 0, "{name}: a refusal wrote nothing");
            }
            (Ok(_), other) => panic!("{name}: committed, expected {other:?}"),
            (Err(refused), other) => {
                panic!("{name}: refused ({:?}), expected {other:?}", refused.errors)
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
