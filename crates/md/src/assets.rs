//! The CC0 asset pack: the manifest types, kind resolution, and scatter
//! math live in the `localgpt-world-agent` crate (extracted in M5). This
//! module re-exports what the app uses and holds the two app-local helpers
//! the crate deliberately does not own: resolving *where* the pack lives
//! ([`assets_dir`]) and reading the manifest from it
//! ([`read_manifest_from_disk`]).

use std::path::{Path, PathBuf};

pub use localgpt_world_agent::assets::{AssetManifest, read_manifest_at};

/// The asset-pack root: the directory whose `models/` holds `manifest.json`
/// and the GLBs. The search order lives in
/// [`localgpt_world_agent::paths::world_pack_dir`] — shared with Verse and Gen,
/// and the place the asset repository's rename is absorbed — with this app's
/// own candidates (`$LOCALGPT_MD_ASSETS`, `assets/`, the dev tree) tried
/// first. Absent pack → `assets/` (harmless default: primitives only).
pub fn assets_dir() -> PathBuf {
    let mut mine: Vec<PathBuf> = Vec::new();
    if let Some(over) = std::env::var_os("LOCALGPT_MD_ASSETS") {
        mine.push(PathBuf::from(over));
    }
    mine.push(PathBuf::from("assets"));
    mine.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"));
    localgpt_world_agent::world_pack_dir(&mine).unwrap_or_else(|| PathBuf::from("assets"))
}

/// The pack under [`assets_dir`], if one is present.
// Called by the `llm` tier's worker and `--generate`.
#[allow(dead_code)]
pub fn read_manifest_from_disk() -> Option<AssetManifest> {
    read_manifest_at(&assets_dir())
}
