//! Print the JSON Schema of `WorldManifest`.
//!
//! ```bash
//! cargo run -p localgpt-world-types --example schema --features schema \
//!     > crates/world-types/world.schema.json
//! ```
//!
//! `tests/schema_snapshot.rs` fails when the committed file is stale.

fn main() {
    let schema = schemars::schema_for!(localgpt_world_types::WorldManifest);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
