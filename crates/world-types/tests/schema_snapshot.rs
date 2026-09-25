//! `world.schema.json` is the published JSON Schema of the format. It must
//! match the types; regenerate it with `examples/schema.rs` when they change.

#![cfg(feature = "schema")]

#[test]
fn committed_schema_is_current() {
    let schema = schemars::schema_for!(localgpt_world_types::WorldManifest);
    let generated = serde_json::to_string_pretty(&schema).unwrap();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("world.schema.json");
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        committed.trim_end(),
        generated.trim_end(),
        "world.schema.json is stale; run: cargo run -p localgpt-world-types --example schema \
         --features schema > crates/world-types/world.schema.json"
    );
}
