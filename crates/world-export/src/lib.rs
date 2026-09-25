//! # localgpt-world-export
//!
//! The distribution side of the LocalGPT world format: everything that turns
//! a [`WorldManifest`](localgpt_world_types::WorldManifest) into something a
//! browser or another tool can open, with no Bevy dependency so every app
//! (Gen, MD, Verse, the localgpt.world site) ships the same bytes.
//!
//! - [`html::WORLD_VIEWER_JS`] — the one web renderer (three.js), a module
//!   the site serves as-is and the HTML export embeds verbatim.
//! - [`html::generate_html`] — a self-contained page: the viewer plus the
//!   manifest as JSON. Gen's `gen_export_html` and MD's `--export x.html`
//!   both call it.
//! - [`json::to_json`] / [`json::to_json_pretty`] — the manifest as the JSON
//!   the viewer consumes (`crates/world-types/world.schema.json`).

pub mod html;
pub mod json;

pub use html::{ExportOptions, generate_html};
pub use json::{to_json, to_json_pretty};
