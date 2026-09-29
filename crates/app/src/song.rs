//! Song mode: a song file opened as a world that performs it.
//!
//! Verse's library builds the world — the track's analysis (decoded once and
//! cached), its mood, the props, the soundtrack's curves and the modulations
//! that perform them — and this writes it where Gen's viewport can load it,
//! with the song beside it so Gen plays it and the modulations follow what
//! is heard.
//!
//! The world folder holds a real `assets/` with two links: `models` to the
//! shared CC0 pack, and `music/<file>` to the song. The song never leaves the
//! machine; a world exported from here carries its audio only when the song
//! is a starter-pack track whose licence allows it, as Verse's own export.

use std::path::{Path, PathBuf};

use localgpt_world_types as wt;

/// Where a song's world lives in the app's workspace.
pub fn world_dir(workspace: &Path, song: &Path) -> PathBuf {
    workspace
        .join("songs")
        .join(crate::document::document_key(song))
}

/// Build `song`'s world and write it, with its audio and the pack linked, to
/// its folder. Returns the folder and the world.
pub fn prepare(song: &Path, workspace: &Path) -> anyhow::Result<(PathBuf, wt::WorldManifest)> {
    let mut world = localgpt_verse::song_world(song).ok_or_else(|| {
        anyhow::anyhow!(
            "{} is not a song Verse can read (mp3, flac, ogg, wav, m4a, aiff)",
            song.display()
        )
    })?;
    let dir = world_dir(workspace, song);
    let assets = dir.join("assets");
    // A folder, not a link: a document world may have left `assets` as a
    // link to the whole pack, and the song must never be written into it.
    if std::fs::symlink_metadata(&assets).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(&assets)?;
    }
    std::fs::create_dir_all(assets.join("music"))?;

    let file_name = song
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("{} has no file name", song.display()))?
        .to_string_lossy()
        .into_owned();
    let relative = format!("music/{file_name}");
    link(song, &assets.join(&relative))?;
    if let Some(soundtrack) = &mut world.soundtrack {
        soundtrack.path = Some(relative);
    }
    if world.all_entities().any(|e| e.mesh_asset.is_some()) {
        let models = localgpt_md::assets::assets_dir().join("models");
        if models.join("manifest.json").is_file() {
            link(&models, &assets.join("models"))?;
        } else {
            tracing::warn!("the world uses pack models but no pack is installed");
        }
    }
    let ron = ron::ser::to_string_pretty(&world, ron::ser::PrettyConfig::default())?;
    std::fs::write(dir.join("world.ron"), ron)?;
    Ok((dir, world))
}

/// Point `link` at `target`, replacing whatever link was there. Copies where
/// there are no symlinks.
fn link(target: &Path, link: &Path) -> std::io::Result<()> {
    let target = target.canonicalize()?;
    if std::fs::read_link(link).ok().as_deref() == Some(target.as_path()) {
        return Ok(());
    }
    if std::fs::symlink_metadata(link).is_ok() {
        if link.is_dir() && !std::fs::symlink_metadata(link)?.file_type().is_symlink() {
            std::fs::remove_dir_all(link)?;
        } else {
            std::fs::remove_file(link)?;
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, link)
    }
    #[cfg(not(unix))]
    {
        if target.is_dir() {
            copy_dir(&target, link)
        } else {
            std::fs::copy(&target, link).map(|_| ())
        }
    }
}

#[cfg(not(unix))]
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
