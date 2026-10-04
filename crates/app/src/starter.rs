//! The cold start: what the app opens when nothing was asked for and nothing
//! was remembered.
//!
//! A consumer will not download five gigabytes before seeing anything
//! (`docs/world-strategy.md` §13.3), so the first frame has to come from
//! nothing: no key, no model, no download. Of the three inputs this app
//! takes, a **song** is the only one that qualifies — Verse derives the world
//! from the music itself, so a song's model-free world is a *finished* world,
//! while a document's rule-derived draft is a placeholder a model is meant to
//! author over and a prompt needs a model at all.
//!
//! So a cold start opens one of the four CC0 "Starter Worlds" tracks through
//! the same [`crate::song`] path a user's own song takes. Nothing here is a
//! special case in the renderer: it is an ordinary song world that the app
//! happens to pick.
//!
//! The tracks are two megabytes, against the 522 MB asset pack and the ~5 GB
//! model, which is why they resolve separately
//! ([`localgpt_world_agent::paths::starter_music_dir`]) and can ship with a
//! build that fetches the other two later.

use std::path::{Path, PathBuf};

use localgpt_world_agent::starter;

/// The app's own places to look for starter music, most specific first.
///
/// The order is Verse's `asset_root` discipline: an explicit override (handled
/// inside [`starter::lead_track`]), then beside the executable, then inside a
/// macOS bundle's `Resources`, and only then the dev tree — so a packaged
/// build never prefers a stale source checkout that happens to be on the
/// machine that built it. Nothing is relative to the working directory: an
/// app launched from Finder has one unrelated to where it was installed.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join("assets"));
        out.push(dir.join("../Resources/assets"));
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"));
    out
}

/// Build the starter world, if starter music is installed.
///
/// `Ok(None)` means there is none — a build without the tracks, which is a
/// state to report rather than a failure. `Err` is a track that is there and
/// would not open.
pub fn open(workspace: &Path) -> anyhow::Result<Option<(String, String)>> {
    let mine = candidates();
    if localgpt_world_agent::paths::starter_music_dir(&mine).is_none() {
        return Ok(None);
    }
    let (track, path) = starter::lead_track(&mine).map_err(|e| anyhow::anyhow!(e))?;
    let (dir, world) = crate::song::prepare(&path, workspace)?;

    // Prefer what the world says about itself: Verse reads the file's own
    // tags, so a track re-encoded with better metadata shows that rather
    // than the manifest's copy.
    let title = world
        .soundtrack
        .as_ref()
        .and_then(|s| s.title.clone())
        .unwrap_or_else(|| track.title.clone());
    let artist = world
        .soundtrack
        .as_ref()
        .and_then(|s| s.artist.clone())
        .filter(|a| !a.is_empty())
        .or_else(|| (!track.artist.is_empty()).then(|| track.artist.clone()));

    let label = match artist {
        Some(artist) => format!("{title} — {artist}"),
        None => title,
    };
    Ok(Some((label, dir.to_string_lossy().into_owned())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_looks_beside_its_executable_before_the_dev_tree() {
        // The packaging bug this ordering exists to prevent: a shipped build
        // on a developer's machine must not load the checkout's assets.
        let found = candidates();
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let dev_at = found.iter().position(|p| *p == dev).expect("the dev tree");
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let beside = found
                .iter()
                .position(|p| *p == dir.join("assets"))
                .expect("beside the executable");
            assert!(
                beside < dev_at,
                "the dev tree is tried before the executable's own assets"
            );
        }
    }

    #[test]
    fn nothing_in_the_candidate_list_is_relative_to_the_working_directory() {
        // An app launched from Finder has a working directory unrelated to
        // where it was installed, so a bare `assets` would resolve to
        // nothing — or to someone else's.
        for path in candidates() {
            assert!(
                path.is_absolute(),
                "{} is relative, so it depends on the working directory",
                path.display()
            );
        }
    }

    #[test]
    fn a_build_without_starter_music_reports_absence_rather_than_failing() {
        // The override points at a directory with no manifest, which is what
        // a build that ships no tracks looks like.
        let empty = std::env::temp_dir().join(format!("lga-starter-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        // SAFETY: single-threaded test, and the variable is restored below.
        unsafe { std::env::set_var(localgpt_world_agent::paths::LOCALGPT_STARTER_MUSIC, &empty) };
        let out = open(&empty);
        unsafe { std::env::remove_var(localgpt_world_agent::paths::LOCALGPT_STARTER_MUSIC) };
        std::fs::remove_dir_all(&empty).ok();

        // Absent music is `Ok(None)`. A sibling asset checkout may still
        // satisfy the probe in a dev tree, in which case opening is fine too
        // — what must not happen is an error.
        assert!(out.is_ok(), "absent starter music became an error: {out:?}");
    }
}
