//! `localgpt_md` — the pure half of LocalGPT MD: Markdown in, a
//! [`localgpt_world_types::WorldManifest`] out, no renderer and no model.
//!
//! The binary (`src/main.rs`) is the Bevy app that walks the world; this
//! library is everything else — parsing (`doc`), the rule-derived draft and
//! its authored-content tiers (`draft`, `recipe`), the sidecar cache
//! (`sidecar`) and the asset-pack probe (`assets`). Keeping that split is
//! what lets `crates/app` take the library Bevy-free
//! (`default-features = false`), and what would let it compile to WASM
//! again.
//!
//! Nothing here runs a model. What a section's place looks like beyond the
//! draft comes from the sidecar beside the document, which an agent working
//! on the file writes; the library applies it.
//!
//! The `app` feature (default) adds the Bevy `Resource` derive on
//! [`sidecar::RecipeStore`], so the binary's systems can hold it; a build
//! with `--no-default-features` stays renderer-free.

pub mod assets;
pub mod doc;
pub mod draft;
pub mod recipe;
pub mod sidecar;

pub use doc::{Doc, Section};
pub use draft::{compile, compile_with, validate};
pub use sidecar::RecipeStore;
