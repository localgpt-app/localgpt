//! The CC0 asset pack: where it lives. The search order is shared with
//! every LocalGPT app through `localgpt-world-agent`'s `paths` module; this
//! module holds the one app-local helper the crate deliberately does not
//! own, resolving *which* root this app runs from ([`assets_dir`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// This app's own asset root, when it is not the `assets/` beside it.
pub const ASSETS_ENV: &str = "LOCALGPT_DOC_ASSETS";

/// What [`ASSETS_ENV`] was called while the crate was `localgpt-md`: still
/// read, after it.
const ASSETS_ENV_OLD: &str = "LOCALGPT_MD_ASSETS";

/// The asset-pack root: the directory whose `models/` holds `manifest.json`
/// and the GLBs. The search order lives in
/// [`localgpt_world_agent::paths::world_pack_dir`] — shared with Verse and Gen,
/// and the place the asset repository's rename is absorbed — with this app's
/// own candidates (`$LOCALGPT_DOC_ASSETS`, `assets/`, the dev tree) tried
/// first. Absent pack → `assets/` (harmless default: primitives only).
pub fn assets_dir() -> PathBuf {
    let mine = candidates(
        std::env::var_os(ASSETS_ENV),
        std::env::var_os(ASSETS_ENV_OLD),
    );
    localgpt_world_agent::world_pack_dir(&mine).unwrap_or_else(|| PathBuf::from("assets"))
}

/// This app's own candidates, in the order they are tried: the override
/// (under its name, then under its old one), `assets/` here, and the dev
/// tree's.
fn candidates(over: Option<OsString>, old: Option<OsString>) -> Vec<PathBuf> {
    let mut mine: Vec<PathBuf> = over.into_iter().chain(old).map(PathBuf::from).collect();
    mine.push(PathBuf::from("assets"));
    mine.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"));
    mine
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_comes_first_under_either_name() {
        let both = candidates(Some("new".into()), Some("old".into()));
        assert_eq!(
            both[..3],
            [
                PathBuf::from("new"),
                PathBuf::from("old"),
                PathBuf::from("assets")
            ]
        );
        assert_eq!(
            candidates(None, Some("old".into()))[0],
            PathBuf::from("old")
        );
        assert_eq!(candidates(None, None)[0], PathBuf::from("assets"));
    }
}
