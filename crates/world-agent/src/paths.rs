//! Where the shared download lives — the CC0 world asset pack — and where an
//! app keeps its own files.
//!
//! The pack is large and identical for every LocalGPT app, and it was
//! resolved by hand-copied twins before this module existed (MD's
//! `assets.rs` and Verse's `world_assets.rs`, each carrying a "keep them in
//! step" comment). This crate is the one every world app already depends
//! on, so the rule lives here once.
//!
//! An app's own asset *root* stays in the app: only it knows whether it is
//! running from a dev tree, beside its executable, or inside a macOS bundle.
//! What it hands to [`world_pack_dir`] is that root as a candidate.

use std::path::{Path, PathBuf};

/// `$LOCALGPT_WORLD_ASSETS` — an override for the asset pack directory.
pub const LOCALGPT_WORLD_ASSETS: &str = "LOCALGPT_WORLD_ASSETS";
/// `$LOCALGPT_STARTER_MUSIC` — an override for the starter-track directory.
pub const LOCALGPT_STARTER_MUSIC: &str = "LOCALGPT_STARTER_MUSIC";
/// The pack is the directory that holds this file.
const PACK_MARKER: &str = "models/manifest.json";
/// The starter music is the directory that holds this file.
const MUSIC_MARKER: &str = "music/music.json";

/// The CC0 asset pack directory — the one whose `models/manifest.json` exists.
///
/// Probed in order: `$LOCALGPT_WORLD_ASSETS`, each `app_candidates` entry (the
/// caller's own asset root, dev tree or bundle), the shared download directory
/// `<XDG data home>/localgpt/models/pack`, then a sibling checkout of the asset
/// repository under its current and former names. `None` means no pack is
/// present, which every app treats as "primitives only".
pub fn world_pack_dir(app_candidates: &[PathBuf]) -> Option<PathBuf> {
    world_pack_dir_with(app_candidates, |p| p.is_file())
}

/// [`world_pack_dir`] with an injected existence check, for tests.
pub fn world_pack_dir_with<F>(app_candidates: &[PathBuf], exists: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    shared_asset_dir(
        LOCALGPT_WORLD_ASSETS,
        PACK_MARKER,
        "pack",
        app_candidates,
        exists,
    )
}

/// The CC0 starter-track directory — the one whose `music/music.json` exists.
///
/// These four tracks ("Starter Worlds", CC0-1.0) are what the desktop app
/// opens when it has nothing else: a song is the one input whose model-free
/// world is a *finished* world rather than a draft, so it is what a person
/// with no key, no model and no download sees in the first frame
/// (`docs/world-strategy.md` §13.3).
///
/// Probed in the same order as [`world_pack_dir`], and separately from it:
/// the tracks are two megabytes against the pack's 522, so a build may carry
/// the music and fetch the pack later. `None` means no starter track is
/// present, which an app treats as "open nothing" rather than as an error.
pub fn starter_music_dir(app_candidates: &[PathBuf]) -> Option<PathBuf> {
    starter_music_dir_with(app_candidates, |p| p.is_file())
}

/// [`starter_music_dir`] with an injected existence check, for tests.
pub fn starter_music_dir_with<F>(app_candidates: &[PathBuf], exists: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    shared_asset_dir(
        LOCALGPT_STARTER_MUSIC,
        MUSIC_MARKER,
        "pack",
        app_candidates,
        exists,
    )
}

/// The probe every shared asset directory uses: an env override, the
/// caller's own roots, the shared download directory, then a sibling
/// checkout of the asset repository under its current and former names.
fn shared_asset_dir<F>(
    env_key: &str,
    marker: &str,
    shared_leaf: &str,
    app_candidates: &[PathBuf],
    exists: F,
) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(over) = std::env::var_os(env_key) {
        candidates.push(PathBuf::from(over));
    }
    candidates.extend(app_candidates.iter().cloned());
    if let Some(shared) = shared_data_dir() {
        candidates.push(shared.join("models").join(shared_leaf));
    }
    // A sibling checkout of the asset repository. `localgpt-verse-assets` is
    // the former name, kept because three apps read that pack and a rename
    // must not break a working tree mid-flight.
    for sibling in ["../localgpt-world-assets", "../localgpt-verse-assets"] {
        candidates.push(PathBuf::from(sibling));
    }
    candidates.into_iter().find(|dir| exists(&dir.join(marker)))
}

/// `$LOCALGPT_DATA_DIR` — an absolute override for the data directory.
pub const LOCALGPT_DATA_DIR: &str = "LOCALGPT_DATA_DIR";
/// `$LOCALGPT_STATE_DIR` — an absolute override for the state directory.
pub const LOCALGPT_STATE_DIR: &str = "LOCALGPT_STATE_DIR";
/// `$LOCALGPT_PROFILE` — a name that moves every app directory to
/// `localgpt-<profile>`, so two profiles never share settings or state.
pub const LOCALGPT_PROFILE: &str = "LOCALGPT_PROFILE";

/// The directories an app keeps its own files in: `data` for what it makes
/// (Gen's workspace of worlds), `state` for what it remembers (settings,
/// logs, the paired devices).
///
/// `<XDG data home>/localgpt` and `<XDG state home>/localgpt`
/// (`~/.local/share/localgpt`, `~/.local/state/localgpt`), each movable by
/// its own absolute override, and all of them by [`LOCALGPT_PROFILE`]. The
/// shared pack ([`world_pack_dir`]) deliberately ignores the profile: 522 MB
/// of models are the same files for every profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub data: PathBuf,
    pub state: PathBuf,
}

impl Dirs {
    /// Resolve from the real environment.
    pub fn resolve() -> Option<Self> {
        Self::resolve_with_env(|key| std::env::var(key))
    }

    /// [`Dirs::resolve`] with a custom env lookup, for tests.
    pub fn resolve_with_env<F>(env_fn: F) -> Option<Self>
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        use etcetera::BaseStrategy;
        let strategy = etcetera::choose_base_strategy().ok()?;
        let profile = env_fn(LOCALGPT_PROFILE)
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let leaf = match &profile {
            Some(profile) => format!("localgpt-{profile}"),
            None => "localgpt".to_string(),
        };
        let absolute = |key: &str| {
            env_fn(key)
                .ok()
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let data = absolute(LOCALGPT_DATA_DIR).unwrap_or_else(|| strategy.data_dir().join(&leaf));
        let state = absolute(LOCALGPT_STATE_DIR).unwrap_or_else(|| {
            strategy
                .state_dir()
                .unwrap_or_else(|| strategy.data_dir())
                .join(&leaf)
        });
        Some(Self { data, state })
    }

    /// `<state>/logs`: where an app without a terminal writes its log.
    pub fn logs(&self) -> PathBuf {
        self.state.join("logs")
    }
}

/// `<XDG data home>/localgpt` — the base the shared pack sits under.
fn shared_data_dir() -> Option<PathBuf> {
    use etcetera::BaseStrategy;
    Some(
        etcetera::choose_base_strategy()
            .ok()?
            .data_dir()
            .join("localgpt"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(
        map: HashMap<&'static str, &'static str>,
    ) -> impl Fn(&str) -> Result<String, std::env::VarError> {
        move |key| {
            map.get(key)
                .map(|v| v.to_string())
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn pack_prefers_an_app_candidate_over_the_shared_download() {
        let app = PathBuf::from("/app/assets");
        let found = world_pack_dir_with(std::slice::from_ref(&app), |p| p.starts_with("/app/"));
        assert_eq!(found, Some(app));
    }

    #[test]
    fn pack_falls_back_to_the_former_asset_repo_name() {
        let found = world_pack_dir_with(&[], |p| p.starts_with("../localgpt-verse-assets"));
        assert_eq!(found, Some(PathBuf::from("../localgpt-verse-assets")));
    }

    #[test]
    fn app_dirs_default_under_localgpt_and_move_with_the_profile() {
        let plain = Dirs::resolve_with_env(env(HashMap::new())).unwrap();
        assert!(plain.data.ends_with("localgpt"), "{}", plain.data.display());
        assert!(
            plain.state.ends_with("localgpt"),
            "{}",
            plain.state.display()
        );
        assert_eq!(plain.logs(), plain.state.join("logs"));

        let work =
            Dirs::resolve_with_env(env(HashMap::from([(LOCALGPT_PROFILE, "work")]))).unwrap();
        assert!(
            work.data.ends_with("localgpt-work"),
            "{}",
            work.data.display()
        );
        assert!(
            work.state.ends_with("localgpt-work"),
            "{}",
            work.state.display()
        );
    }

    #[test]
    fn app_dirs_take_absolute_overrides_and_ignore_relative_ones() {
        let plain = Dirs::resolve_with_env(env(HashMap::new())).unwrap();
        let moved = Dirs::resolve_with_env(env(HashMap::from([
            (LOCALGPT_DATA_DIR, "/data/here"),
            (LOCALGPT_STATE_DIR, "relative"),
        ])))
        .unwrap();
        assert_eq!(moved.data, PathBuf::from("/data/here"));
        assert_eq!(moved.state, plain.state, "a relative override is ignored");
    }

    #[test]
    fn pack_is_none_when_no_candidate_has_a_manifest() {
        assert_eq!(
            world_pack_dir_with(&[PathBuf::from("/nope")], |_| false),
            None
        );
    }

    #[test]
    fn starter_music_prefers_an_app_candidate_over_the_shared_download() {
        let app = PathBuf::from("/app/assets");
        let found = starter_music_dir_with(std::slice::from_ref(&app), |p| p.starts_with("/app/"));
        assert_eq!(found, Some(app));
    }

    #[test]
    fn starter_music_falls_back_to_a_sibling_asset_checkout() {
        let found = starter_music_dir_with(&[], |p| p.starts_with("../localgpt-world-assets"));
        assert_eq!(found, Some(PathBuf::from("../localgpt-world-assets")));
    }

    #[test]
    fn starter_music_looks_for_its_own_marker_not_the_packs() {
        // Two megabytes of music and 522 MB of models are separate
        // downloads, so a tree with one must not be mistaken for the other.
        let music_only = |p: &Path| p.ends_with(MUSIC_MARKER);
        let pack_only = |p: &Path| p.ends_with(PACK_MARKER);
        let candidates = [PathBuf::from("/only/music")];

        assert_eq!(
            starter_music_dir_with(&candidates, music_only),
            Some(PathBuf::from("/only/music"))
        );
        assert_eq!(world_pack_dir_with(&candidates, music_only), None);
        assert_eq!(starter_music_dir_with(&candidates, pack_only), None);
    }

    #[test]
    fn starter_music_is_none_when_nothing_has_the_manifest() {
        assert_eq!(
            starter_music_dir_with(&[PathBuf::from("/nope")], |_| false),
            None
        );
    }
}
