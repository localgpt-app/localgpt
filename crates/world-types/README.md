# localgpt-world-types

LocalGPT's world data model. The format's document types come from the
[`openworldformat`](https://crates.io/crates/openworldformat) crate, the
Open World Format's Rust reference, and are re-exported here unchanged
(`WorldManifest`, `WorldEntity`, `Shape` and the rest). What this crate
adds is LocalGPT's own surface: NPCs, the generation log, behavior
libraries, regions and the sync drift machinery.

Serde only, with no Bevy and no SpacetimeDB. The same types save as RON
and travel as JSON, and the `schema` feature writes `world.schema.json`,
byte for byte the published schema.

Part of [LocalGPT](https://github.com/localgpt-app/localgpt). License: Apache-2.0.
