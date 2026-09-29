//! The conformance worlds (`conformance/*.json`) are the reference every
//! renderer of the format loads. This test keeps them valid, round-trippable
//! and complete.

use std::collections::BTreeSet;
use std::path::PathBuf;

use localgpt_world_types as wt;

fn fixtures() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("conformance dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {}", dir.display());
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

fn parse(name: &str, text: &str) -> wt::WorldManifest {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn fixtures_parse_validate_and_roundtrip() {
    for (name, text) in fixtures() {
        let manifest = parse(&name, &text);
        manifest
            .check_version()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!manifest.entities.is_empty(), "{name}: no entities");

        // Ids and names are unique; parents exist.
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for e in &manifest.entities {
            assert!(ids.insert(e.id.0), "{name}: duplicate id {}", e.id);
            assert!(
                names.insert(e.name.as_str()),
                "{name}: duplicate name {}",
                e.name
            );
        }
        for e in &manifest.entities {
            if let Some(parent) = e.parent {
                assert!(
                    ids.contains(&parent.0),
                    "{name}: {} has no parent {parent}",
                    e.name
                );
            }
        }
        assert!(
            manifest.next_entity_id > *ids.iter().max().unwrap(),
            "{name}: next_entity_id must exceed every id"
        );

        // No validation errors (warnings are allowed and printed).
        let issues = wt::validate_manifest(&manifest, &wt::WorldLimits::default());
        for issue in &issues {
            println!("{name}: {:?}: {}", issue.severity, issue.message);
        }
        assert!(
            issues.iter().all(|i| i.severity != wt::Severity::Error),
            "{name}: validation errors"
        );

        // RON (Gen's world.ron) and JSON (the web viewer) carry the same world.
        let ron_text = ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| panic!("{name}: to ron: {e}"));
        let from_ron: wt::WorldManifest =
            ron::from_str(&ron_text).unwrap_or_else(|e| panic!("{name}: from ron: {e}"));
        assert_eq!(from_ron, manifest, "{name}: RON round trip");
        let json_value = serde_json::to_value(&manifest).unwrap();
        let from_json: wt::WorldManifest = serde_json::from_value(json_value).unwrap();
        assert_eq!(from_json, manifest, "{name}: JSON round trip");
    }
}

#[test]
fn fixture_textures_exist_and_every_slot_is_covered() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance")
        .join("assets");
    let mut slots = BTreeSet::new();
    for (name, text) in fixtures() {
        let manifest = parse(&name, &text);
        for e in &manifest.entities {
            for (slot, path) in e.material.iter().flat_map(|m| m.textures()) {
                assert!(
                    assets.join(path).is_file(),
                    "{name}: {} uses missing texture {path}",
                    e.name
                );
                slots.insert(format!("{slot:?}"));
            }
        }
    }
    assert_eq!(
        slots,
        set(&["BaseColor", "Emissive", "MetallicRoughness", "Normal"])
    );
}

/// Externally tagged variant names of every value under `key` in every
/// entity of every fixture (`"beat"` → `beat`, `{"stem": ..}` → `stem`).
fn variant_names(key: &str, nested: Option<&str>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (name, text) in fixtures() {
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        for e in v["entities"].as_array().unwrap_or_else(|| panic!("{name}")) {
            let items: Vec<&serde_json::Value> = match e.get(key) {
                Some(serde_json::Value::Array(a)) => a.iter().collect(),
                Some(other) => vec![other],
                None => continue,
            };
            for item in items {
                let item = match nested {
                    Some(field) => &item[field],
                    None => item,
                };
                match item {
                    serde_json::Value::String(s) => {
                        out.insert(s.clone());
                    }
                    serde_json::Value::Object(o) => {
                        out.extend(o.keys().cloned());
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn fixtures_cover_every_variant() {
    assert_eq!(
        variant_names("shape", None),
        set(&[
            "Capsule",
            "Cone",
            "Cuboid",
            "Cylinder",
            "Icosahedron",
            "Plane",
            "Pyramid",
            "Sphere",
            "Tetrahedron",
            "Torus",
            "Wedge",
        ])
    );
    assert_eq!(
        variant_names("behaviors", None),
        set(&[
            "Bob",
            "Bounce",
            "LookAt",
            "Orbit",
            "PathFollow",
            "Pulse",
            "Spin",
        ])
    );
    assert_eq!(
        variant_names("modulations", Some("target")),
        set(&[
            "emissive",
            "light_intensity",
            "offset_y",
            "opacity",
            "scale"
        ])
    );
    assert_eq!(
        variant_names("modulations", Some("signal")),
        set(&[
            "bass",
            "beat",
            "constant",
            "energy",
            "highs",
            "oscillator",
            "stem",
        ])
    );
    assert_eq!(
        variant_names("light", Some("light_type")),
        set(&["directional", "point", "spot"])
    );
}

#[test]
fn instances_expand_the_same_way_everywhere() {
    let (_, text) = fixtures()
        .into_iter()
        .find(|(name, _)| name == "instances.json")
        .expect("instances.json");
    let mut manifest = parse("instances.json", &text);
    let instances = manifest
        .entities
        .iter()
        .filter(|e| e.instance_of.is_some())
        .count();
    let parts = manifest.creations[0].parts.len();
    // `tree_stump` removes its crown, and with it the top above the crown.
    let removed = 2;
    assert_eq!(manifest.expand_instances(), instances * parts - removed);
    let stump: Vec<&str> = manifest
        .entities
        .iter()
        .filter(|e| e.name.as_str().starts_with("tree_stump"))
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(stump, ["tree_stump", "tree_stump/trunk"]);

    // The web viewer's `expandInstances` numbers parts the same way: from
    // `first_expansion_id`, instance by instance, part by part.
    let tall_crown = manifest
        .entities
        .iter()
        .find(|e| e.name.as_str() == "tree_tall/crown")
        .expect("tree_tall/crown");
    // The first free id is 15; three instances come before this one.
    assert_eq!(tall_crown.id.0, 15 + 3 * parts as u64 + 1);
    assert_eq!(tall_crown.transform.position, [0.0, 2.6, 0.0]);
    assert_eq!(tall_crown.behaviors.len(), 1);
    let autumn_top = manifest
        .entities
        .iter()
        .find(|e| e.name.as_str() == "tree_autumn/top")
        .expect("tree_autumn/top");
    assert_eq!(
        autumn_top.material.as_ref().unwrap().color,
        [0.85, 0.55, 0.1, 1.0]
    );
    let issues = wt::validate_manifest(&manifest, &wt::WorldLimits::default());
    assert!(issues.iter().all(|i| i.severity != wt::Severity::Error));
}

#[test]
fn triggers_cover_every_event_and_action() {
    let mut events = BTreeSet::new();
    let mut actions = BTreeSet::new();
    for (name, text) in fixtures() {
        for e in &parse(&name, &text).entities {
            for t in &e.triggers {
                let json = serde_json::to_value(t).unwrap();
                events.insert(json["on"]["event"].as_str().unwrap().to_string());
                actions.insert(json["action"]["action"].as_str().unwrap().to_string());
            }
        }
    }
    assert_eq!(
        events,
        set(&[
            "area_enter",
            "area_exit",
            "click",
            "collision",
            "proximity",
            "start",
            "timer",
        ])
    );
    assert_eq!(
        actions,
        set(&[
            "animate",
            "hide",
            "host",
            "remove",
            "show",
            "show_text",
            "teleport",
            "toggle",
        ])
    );
}
