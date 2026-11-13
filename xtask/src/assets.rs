//! `cargo xtask fetch-assets`: the CC0 pack Document, Verse and Gen share, and
//! the four starter tracks, copied once per machine into the one directory
//! every app reads (it was `scripts/fetch-assets.sh`).
//!
//! The pack's source is the `localgpt-world-assets` repository (formerly
//! `localgpt-verse-assets`): `models/` holds `manifest.json` and the packed
//! `.glb` files through Git LFS, all Poly Haven CC0; `music/` the CC0
//! "Starter Worlds" tracks a cold start opens. Everything lands under
//! `<data dir>/localgpt/models/pack` (`~/.local/share/...` on macOS and
//! Linux), the base `localgpt-world-agent`'s `paths` resolves, so a 522 MB
//! pack is not one copy per app. Idempotent: run it again after pulling new
//! assets upstream.
//!
//! `LOCALGPT_WORLD_ASSETS_SRC` names the source checkout (else a sibling
//! of this repository, or of the main checkout when this is a worktree);
//! `LOCALGPT_WORLD_ASSETS` moves where the pack lands.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository's names, current and former: a rename must not break a
/// working tree mid-flight.
const SOURCES: [&str; 2] = ["localgpt-world-assets", "localgpt-verse-assets"];

pub fn run(root: &Path) -> Result<(), String> {
    let source = source(root)?;
    let pack = pack_root()?;
    let report = fetch(&source, &pack)?;
    for missing in &report.missing {
        println!("  missing {missing} (run 'git lfs pull' in the assets repo)");
    }
    println!(
        "  {} assets referenced, {} copied",
        report.referenced, report.copied
    );
    let models = pack.join("models");
    for krate in ["crates/verse", "crates/doc"] {
        link_models(&root.join(krate).join("assets"), &models);
    }
    match report.music {
        Some(copied) => println!(
            "starter music -> {} ({copied} copied)",
            pack.join("music").display()
        ),
        None => println!("  no music/ in the assets repo: the app will have no cold-start song"),
    }
    println!("done -> {}", models.display());
    println!("license: the pack is Poly Haven CC0 (see the assets repo's LICENSE);");
    println!("         the starter tracks are CC0 originals (see music/NOTICE)");
    Ok(())
}

/// The manifest, its models and the starter tracks from `source` into
/// `into` (`models/`, `music/`), the layout the shared pack has and a
/// portable bundle's `assets/` too; the starter files copied, when the
/// source has music.
pub(crate) fn fetch_into(source: &Path, into: &Path) -> Result<Option<usize>, String> {
    let report = fetch(source, into)?;
    for missing in &report.missing {
        println!("    missing {missing} (run 'git lfs pull' in the assets repo)");
    }
    Ok(report.music)
}

/// What a fetch did.
#[derive(Debug, Default, PartialEq)]
struct Report {
    referenced: usize,
    copied: usize,
    /// Files the manifest names that the source does not hold: LFS objects
    /// not pulled.
    missing: Vec<String>,
    /// The starter tracks copied, when the source has any.
    music: Option<usize>,
}

/// Copies the manifest, every model it names that is not already there,
/// and the starter tracks, from `source` into `pack`.
fn fetch(source: &Path, pack: &Path) -> Result<Report, String> {
    let manifest_path = source.join("models/manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let files: BTreeSet<String> = manifest["assets"]
        .as_array()
        .ok_or_else(|| format!("{}: no \"assets\" list", manifest_path.display()))?
        .iter()
        .filter_map(|asset| asset["file"].as_str().map(str::to_string))
        .collect();
    let models = pack.join("models");
    std::fs::create_dir_all(&models).map_err(|e| format!("{}: {e}", models.display()))?;
    std::fs::write(models.join("manifest.json"), &text)
        .map_err(|e| format!("{}: {e}", models.display()))?;

    let mut report = Report {
        referenced: files.len(),
        ..Report::default()
    };
    for file in &files {
        let (from, to) = (source.join("models").join(file), models.join(file));
        if !from.is_file() {
            report.missing.push(file.clone());
            continue;
        }
        if to.is_file() {
            continue;
        }
        copy(&from, &to)?;
        report.copied += 1;
    }

    let music = source.join("music");
    if music.is_dir() {
        let out = pack.join("music");
        let mut copied = 0;
        let entries = std::fs::read_dir(&music).map_err(|e| format!("{}: {e}", music.display()))?;
        for entry in entries.flatten() {
            let from = entry.path();
            let to = out.join(entry.file_name());
            if from.is_file() && !to.is_file() {
                copy(&from, &to)?;
                copied += 1;
            }
        }
        report.music = Some(copied);
    }
    Ok(report)
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

/// The assets checkout: `LOCALGPT_WORLD_ASSETS_SRC`, else the first sibling
/// that has the pack, beside this repository or, from a worktree, beside
/// the main checkout.
fn source(root: &Path) -> Result<PathBuf, String> {
    if let Some(named) = std::env::var_os("LOCALGPT_WORLD_ASSETS_SRC") {
        let named = PathBuf::from(named);
        return if named.join("models/manifest.json").is_file() {
            Ok(named)
        } else {
            Err(format!("{}: no models/manifest.json", named.display()))
        };
    }
    checkouts(root)
        .into_iter()
        .find(|dir| dir.join("models/manifest.json").is_file())
        .ok_or_else(|| {
            "no pack found: clone localgpt-world-assets next to this repository and \
             run 'git lfs pull' there first (or point LOCALGPT_WORLD_ASSETS_SRC at it)"
                .to_string()
        })
}

/// Where an assets checkout may be, in order: beside this repository, then,
/// from a worktree, beside the main checkout; under each name it has had.
pub(crate) fn checkouts(root: &Path) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = root.parent().map(Path::to_path_buf).into_iter().collect();
    if let Some(main) = main_checkout(root)
        .and_then(|main| main.parent().map(Path::to_path_buf))
        .filter(|parent| !parents.contains(parent))
    {
        parents.push(main);
    }
    parents
        .iter()
        .flat_map(|parent| SOURCES.iter().map(move |name| parent.join(name)))
        .collect()
}

/// The main checkout's folder when `root` is a worktree of it.
fn main_checkout(root: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .ok()?;
    let common = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    common.parent().map(Path::to_path_buf)
}

/// Where the pack lands: `LOCALGPT_WORLD_ASSETS`, else `<data dir>/localgpt/
/// models/pack`, the base `localgpt-world-agent`'s `paths` resolves.
pub(crate) fn pack_root() -> Result<PathBuf, String> {
    if let Some(over) = std::env::var_os("LOCALGPT_WORLD_ASSETS") {
        return Ok(PathBuf::from(over));
    }
    use etcetera::BaseStrategy;
    let strategy =
        etcetera::choose_base_strategy().map_err(|e| format!("no home directory: {e}"))?;
    Ok(strategy
        .data_dir()
        .join("localgpt")
        .join("models")
        .join("pack"))
}

/// A Bevy app's asset root takes the pack as its `models/`: Bevy reads one
/// asset base, so `models/` sits beside `fonts/`, `ml/` and `music/`, and a
/// link keeps one copy on disk. A real folder there is left alone.
fn link_models(assets: &Path, models: &Path) {
    let link = assets.join("models");
    let is_link = std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
    if link.exists() && !is_link {
        println!("  {} exists and is not a link: left alone", link.display());
        return;
    }
    let linked = std::fs::create_dir_all(assets).and_then(|()| {
        if is_link {
            std::fs::remove_file(&link)?;
        }
        symlink_dir(models, &link)
    });
    match linked {
        Ok(()) => println!("linked {} -> {}", link.display(), models.display()),
        Err(e) => println!("  {}: {e}", link.display()),
    }
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_is_copied_once_and_what_is_missing_is_named() {
        let dir = std::env::temp_dir().join(format!("xtask-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (source, pack) = (dir.join("assets"), dir.join("pack"));
        std::fs::create_dir_all(source.join("models/rocks")).unwrap();
        std::fs::create_dir_all(source.join("music")).unwrap();
        std::fs::write(
            source.join("models/manifest.json"),
            r#"{"assets": [{"file": "rocks/a.glb"}, {"file": "rocks/a.glb"}, {"file": "tree.glb"}]}"#,
        )
        .unwrap();
        std::fs::write(source.join("models/rocks/a.glb"), b"glb").unwrap();
        std::fs::write(source.join("music/music.json"), b"{}").unwrap();

        let first = fetch(&source, &pack).unwrap();
        assert_eq!(
            first,
            Report {
                referenced: 2,
                copied: 1,
                missing: vec!["tree.glb".into()],
                music: Some(1),
            }
        );
        assert!(pack.join("models/manifest.json").is_file());
        assert_eq!(
            std::fs::read(pack.join("models/rocks/a.glb")).unwrap(),
            b"glb"
        );
        assert!(pack.join("music/music.json").is_file());

        // Again: nothing is copied twice.
        let again = fetch(&source, &pack).unwrap();
        assert_eq!((again.copied, again.music), (0, Some(0)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
