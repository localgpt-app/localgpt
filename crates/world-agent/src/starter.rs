//! The CC0 starter tracks: what an app opens when it has nothing.
//!
//! A consumer will not download five gigabytes before seeing anything, so the
//! first frame has to come from nothing at all. Of the inputs a world app
//! takes, a song is the only one whose model-free output is a *finished*
//! world rather than a draft — Verse derives the world from the music itself,
//! while a document's rule-derived draft is a placeholder a model is meant to
//! author over. So the desktop cold start is a song
//! (`docs/world-strategy.md` §13.3), and these four tracks are it.
//!
//! "Starter Worlds" by Reverie: four original algorithmic compositions
//! dedicated to the public domain under CC0-1.0, synthesized by the asset
//! repository's `generate_music.py` from additive pads, sine sub-bass,
//! plucked bells, soft percussion and a Schroeder reverb — no sampled or
//! third-party material, which is why they can ship in an Apache-2.0 app.
//!
//! The *directory* is found by [`crate::paths::starter_music_dir`]; this
//! module reads what is in it and says which track leads.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The file inside a starter-music directory that lists its tracks.
pub const MANIFEST: &str = "music.json";
/// The subdirectory of a resolved starter directory that holds the tracks.
pub const MUSIC_SUBDIR: &str = "music";

/// The track a cold start opens when the manifest offers a choice.
///
/// Warm and slow (68 BPM) — the welcoming one of the four, and the point of
/// a cold start is to be welcoming. A manifest without it falls back to the
/// first track listed, so the pick degrades rather than failing.
pub const LEAD_TRACK: &str = "Amber Drift";

/// One track in the starter manifest.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StarterTrack {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    /// The file name, relative to the manifest's own directory.
    pub file: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub bpm: f32,
}

/// The starter manifest: a version, a licence and the tracks.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StarterManifest {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub tracks: Vec<StarterTrack>,
}

impl StarterManifest {
    /// Read the manifest from a resolved starter directory — the one
    /// [`crate::paths::starter_music_dir`] returned, so the manifest is at
    /// `<dir>/music/music.json`.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MUSIC_SUBDIR).join(MANIFEST);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The track a cold start should open: [`LEAD_TRACK`] when the manifest
    /// has it, else the first one listed, else `None` for an empty manifest.
    pub fn lead(&self) -> Option<&StarterTrack> {
        self.tracks
            .iter()
            .find(|t| t.title == LEAD_TRACK)
            .or_else(|| self.tracks.first())
    }

    /// A track by title, for an app that offers the four as a choice.
    pub fn track(&self, title: &str) -> Option<&StarterTrack> {
        self.tracks.iter().find(|t| t.title == title)
    }
}

/// The audio file for a track, inside a resolved starter directory.
pub fn track_path(dir: &Path, track: &StarterTrack) -> PathBuf {
    dir.join(MUSIC_SUBDIR).join(&track.file)
}

/// The track a cold start opens, and its file, from the first starter
/// directory that exists — the whole cold-start lookup in one call.
///
/// `app_candidates` are the caller's own asset roots, exactly as
/// [`crate::paths::world_pack_dir`] takes them: only the app knows whether it
/// is running from a dev tree, beside its executable or inside a bundle.
///
/// `None` means no starter music is installed. That is not an error — an app
/// without it opens nothing and says so — so the reason is returned for a
/// log rather than thrown.
pub fn lead_track(app_candidates: &[PathBuf]) -> Result<(StarterTrack, PathBuf), String> {
    let dir = crate::paths::starter_music_dir(app_candidates)
        .ok_or_else(|| "no starter music found".to_string())?;
    let manifest = StarterManifest::read(&dir)?;
    let track = manifest
        .lead()
        .ok_or_else(|| format!("{}: the manifest lists no tracks", dir.display()))?
        .clone();
    let path = track_path(&dir, &track);
    if !path.is_file() {
        return Err(format!(
            "{}: the manifest's track is missing",
            path.display()
        ));
    }
    Ok((track, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUR: &str = r#"{
      "version": 1,
      "license": "CC0-1.0",
      "tracks": [
        {"title": "Amber Drift", "artist": "Reverie", "file": "amber-drift.mp3", "bpm": 68},
        {"title": "Tidewater", "artist": "Reverie", "file": "tidewater.mp3", "bpm": 60},
        {"title": "Nightglass", "artist": "Reverie", "file": "nightglass.mp3", "bpm": 92},
        {"title": "Emberfall", "artist": "Reverie", "file": "emberfall.mp3", "bpm": 112}
      ]
    }"#;

    fn parse(json: &str) -> StarterManifest {
        serde_json::from_str(json).expect("the manifest parses")
    }

    #[test]
    fn the_warm_slow_track_leads() {
        let manifest = parse(FOUR);
        assert_eq!(manifest.tracks.len(), 4);
        let lead = manifest.lead().expect("a lead track");
        assert_eq!(lead.title, LEAD_TRACK);
        assert_eq!(lead.bpm, 68.0);
        assert_eq!(lead.file, "amber-drift.mp3");
    }

    #[test]
    fn a_manifest_without_the_lead_takes_the_first_listed() {
        let manifest = parse(
            r#"{"tracks": [{"title": "Emberfall", "file": "emberfall.mp3"},
                            {"title": "Tidewater", "file": "tidewater.mp3"}]}"#,
        );
        assert_eq!(manifest.lead().map(|t| t.title.as_str()), Some("Emberfall"));
    }

    #[test]
    fn an_empty_manifest_has_no_lead_rather_than_panicking() {
        assert!(parse(r#"{"tracks": []}"#).lead().is_none());
        assert!(parse("{}").lead().is_none());
    }

    #[test]
    fn a_track_can_be_asked_for_by_title() {
        let manifest = parse(FOUR);
        assert_eq!(manifest.track("Nightglass").map(|t| t.bpm), Some(92.0));
        assert!(manifest.track("Not A Track").is_none());
    }

    #[test]
    fn a_tracks_file_sits_under_the_music_subdirectory() {
        let manifest = parse(FOUR);
        let lead = manifest.lead().unwrap();
        let path = track_path(Path::new("/assets"), lead);
        assert_eq!(path, Path::new("/assets/music/amber-drift.mp3"));
    }

    #[test]
    fn unknown_manifest_fields_are_ignored_rather_than_refused() {
        // The format's must-ignore discipline: a newer manifest with extra
        // keys must still open on an older build.
        let manifest = parse(
            r#"{"version": 2, "mood": "warm", "tracks":
                 [{"title": "Amber Drift", "file": "amber-drift.mp3", "stems": 4}]}"#,
        );
        assert_eq!(manifest.version, 2);
        assert_eq!(manifest.lead().map(|t| t.title.as_str()), Some(LEAD_TRACK));
    }
}
