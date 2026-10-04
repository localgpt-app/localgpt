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
/// `$LOCALGPT_STARTER_MUSIC` — an override for the starter-track directory.
pub const LOCALGPT_STARTER_MUSIC: &str = "LOCALGPT_STARTER_MUSIC";
/// The pack is the directory that holds this file.
const PACK_MARKER: &str = "models/manifest.json";
/// The starter music is the directory that holds this file.
const MUSIC_MARKER: &str = "music/music.json";

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

/// The `.gguf` files in `dir`, sorted so "the first one" is stable.
///
/// Gen's richer rule, shared: MD and Verse each walked `read_dir` and took
/// whatever came back first, which is filesystem order and so not reproducible.
pub fn gguf_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            name.ends_with(".gguf").then_some(name)
        })
        .collect();
    files.sort();
    files
}

/// The tokenizer to load beside `gguf`: its own `<name>.tokenizer.json` first,
/// then a shared `tokenizer.json`. `None` when neither is present.
pub fn tokenizer_for(dir: &Path, gguf: &str) -> Option<String> {
    let own = format!("{}.tokenizer.json", gguf.trim_end_matches(".gguf"));
    [own, "tokenizer.json".to_string()]
        .into_iter()
        .find(|name| dir.join(name).is_file())
}

/// The first usable `(dir, gguf, tokenizer)` trio in `dir`, the form
/// mistral.rs's `GgufModelBuilder` wants. A missing tokenizer still yields
/// `tokenizer.json`, since that is the name the fetch script writes.
pub fn locate_model_in(dir: &Path) -> Option<(PathBuf, String, String)> {
    let gguf = gguf_files(dir).into_iter().next()?;
    let tokenizer = tokenizer_for(dir, &gguf).unwrap_or_else(|| "tokenizer.json".to_string());
    Some((dir.to_path_buf(), gguf, tokenizer))
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
    fn gguf_listing_is_sorted_and_tokenizers_prefer_their_own() {
        let dir = std::env::temp_dir().join(format!("lga-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "b-model.gguf",
            "a-model.gguf",
            "a-model.tokenizer.json",
            "tokenizer.json",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(gguf_files(&dir), ["a-model.gguf", "b-model.gguf"]);
        assert_eq!(
            tokenizer_for(&dir, "a-model.gguf").as_deref(),
            Some("a-model.tokenizer.json")
        );
        assert_eq!(
            tokenizer_for(&dir, "b-model.gguf").as_deref(),
            Some("tokenizer.json"),
            "no own tokenizer -> the shared one"
        );
        assert_eq!(
            locate_model_in(&dir),
            Some((
                dir.clone(),
                "a-model.gguf".into(),
                "a-model.tokenizer.json".into()
            )),
            "the first gguf is the sorted-first one, not whatever read_dir returns"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn locate_model_in_is_none_without_a_gguf() {
        assert_eq!(locate_model_in(Path::new("/definitely/not/here")), None);
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
