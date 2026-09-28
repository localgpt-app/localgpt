//! `localgpt_md` — the pure half of LocalGPT MD: Markdown in, a
//! [`localgpt_world_types::WorldManifest`] out, no renderer and no model.
//!
//! The binary (`src/main.rs`) is the Bevy app that walks the world; this
//! library is everything else — parsing (`doc`), the rule-derived draft and
//! its authored-content tiers (`draft`, `recipe`), the sidecar cache
//! (`sidecar`), the asset-pack probe (`assets`) and the shared agent entry
//! points (`agent`). Keeping that split is what lets the same code compile to
//! WASM for localgpt.md, where a dropped Markdown file becomes a world in the
//! browser with no install, no key and no model — the file never leaves the
//! tab.
//!
//! The `app` feature (default) adds the Bevy `Resource` derive on
//! [`sidecar::RecipeStore`], so the binary's systems can hold it; a WASM
//! build takes `--no-default-features` and stays renderer-free.

pub mod agent;
pub mod assets;
pub mod doc;
pub mod draft;
pub mod recipe;
pub mod sidecar;

pub use doc::{Doc, Section};
pub use draft::{compile, compile_with, validate};
pub use sidecar::RecipeStore;
