//! Where the shared downloads live: the local-LLM model directory and the CC0
//! world asset pack.
//!
//! Both are large, both are identical for every LocalGPT app, and both were
//! resolved by hand-copied twins before this module existed — `shared_llm_dir`
//! in `localgpt-core`, in MD's `llm.rs` and in Verse's `llm.rs`, each carrying
//! a "keep them in step" comment; the pack in MD's `assets.rs` and Verse's
//! `world_assets.rs`. This crate is the one every world app already depends
//! on, so the rule lives here once.
//!
//! An app's own asset *root* stays in the app: only it knows whether it is
//! running from a dev tree, beside its executable, or inside a macOS bundle.
//! What it hands to [`world_pack_dir`] is that root as a candidate.

use std::path::{Path, PathBuf};

/// `$LOCALGPT_LLM_DIR` — an absolute override for the model directory.
pub const LOCALGPT_LLM_DIR: &str = "LOCALGPT_LLM_DIR";
/// `$LOCALGPT_WORLD_ASSETS` — an override for the asset pack directory.
pub const LOCALGPT_WORLD_ASSETS: &str = "LOCALGPT_WORLD_ASSETS";
/// The pack is the directory that holds this file.
const PACK_MARKER: &str = "models/manifest.json";

/// The local-LLM model directory every LocalGPT app shares, so one GGUF
/// download serves Gen, MD and Verse: `$LOCALGPT_LLM_DIR`, else
/// `<XDG data home>/localgpt/models/llm` (`~/.local/share/localgpt/models/llm`).
///
/// Deliberately ignores `LOCALGPT_PROFILE` and `LOCALGPT_DATA_DIR`: a 5 GB
/// model is the same file for every profile.
pub fn shared_llm_dir() -> Option<PathBuf> {
    shared_llm_dir_with_env(|key| std::env::var(key))
}

/// [`shared_llm_dir`] with a custom env lookup, for tests.
pub fn shared_llm_dir_with_env<F>(env_fn: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    if let Some(dir) = env_fn(LOCALGPT_LLM_DIR)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(dir);
    }
    Some(shared_data_dir()?.join("models").join("llm"))
}

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
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(over) = std::env::var_os(LOCALGPT_WORLD_ASSETS) {
        candidates.push(PathBuf::from(over));
    }
    candidates.extend(app_candidates.iter().cloned());
    if let Some(shared) = shared_data_dir() {
        candidates.push(shared.join("models").join("pack"));
    }
    // A sibling checkout of the asset repository. `localgpt-verse-assets` is
    // the former name, kept because three apps read that pack and a rename
    // must not break a working tree mid-flight.
    for sibling in ["../localgpt-world-assets", "../localgpt-verse-assets"] {
        candidates.push(PathBuf::from(sibling));
    }
    candidates
        .into_iter()
        .find(|dir| exists(&dir.join(PACK_MARKER)))
}

/// `<XDG data home>/localgpt` — the base both shared downloads sit under.
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
    fn llm_dir_defaults_under_the_shared_data_dir() {
        let dir = shared_llm_dir_with_env(env(HashMap::new())).unwrap();
        assert!(dir.ends_with("localgpt/models/llm"), "{}", dir.display());
    }

    #[test]
    fn llm_dir_ignores_the_profile_and_data_dir() {
        // A 5 GB model is the same file for every profile, so unlike every
        // other LocalGPT directory this one must not move with them.
        let default = shared_llm_dir_with_env(env(HashMap::new())).unwrap();
        for var in ["LOCALGPT_PROFILE", "LOCALGPT_DATA_DIR"] {
            assert_eq!(
                shared_llm_dir_with_env(env(HashMap::from([(var, "/elsewhere")]))),
                Some(default.clone()),
                "{var} moved the shared model directory"
            );
        }
    }

    #[test]
    fn llm_dir_takes_absolute_overrides_and_ignores_relative_ones() {
        let default = shared_llm_dir_with_env(env(HashMap::new())).unwrap();
        assert_eq!(
            shared_llm_dir_with_env(env(HashMap::from([(LOCALGPT_LLM_DIR, "/models/here")]))),
            Some(PathBuf::from("/models/here"))
        );
        assert_eq!(
            shared_llm_dir_with_env(env(HashMap::from([(LOCALGPT_LLM_DIR, "relative")]))),
            Some(default.clone())
        );
        assert_eq!(
            shared_llm_dir_with_env(env(HashMap::from([(LOCALGPT_LLM_DIR, "")]))),
            Some(default)
        );
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
    fn pack_is_none_when_no_candidate_has_a_manifest() {
        assert_eq!(
            world_pack_dir_with(&[PathBuf::from("/nope")], |_| false),
            None
        );
    }
}
