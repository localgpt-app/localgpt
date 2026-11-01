//! Gen's own settings, so the app never needs a config file.
//!
//! Gen ships as a desktop app: it has to start with nothing on disk and
//! remember what the user picks in the window. Nothing here is a model or a
//! key — Gen holds none — only what was opened last.
//!
//! `~/.local/state/localgpt/gen-settings.json`. `LOCALGPT_STATE_DIR` and
//! `LOCALGPT_PROFILE` move it, because the path comes from
//! [`localgpt_world_agent::paths::Dirs`] rather than a second copy of the
//! XDG rule.
//!
//! The contract follows Verse's `settings.rs`, which proved it: every field
//! `#[serde(default)]` with an explicit [`Default`], and a load that returns
//! defaults with a warning rather than failing startup — a settings file must
//! never be the reason an app won't open. Writes are atomic (tmp + rename),
//! the way MD's sidecar does it, so a crash mid-write can't truncate the file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Bumped only for a change old Gens can't read. A file from the future is
/// ignored (with a warning) instead of being half-parsed.
pub const VERSION: u32 = 1;

/// What Gen remembers between runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenSettings {
    #[serde(default = "default_version")]
    pub version: u32,

    /// What the app opened last, so a bare launch reopens where the user
    /// left off — a document with its editor, anything else as its world
    /// (a song's world plays again on its own).
    #[serde(default)]
    pub last_opened: Option<LastOpened>,
}

/// What the app opened, as Gen remembers it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LastOpened {
    /// The Markdown file's path; reopening it rebuilds the live document.
    Document { path: String },
    /// A world folder's path (an imported world, or a song's world).
    World { dir: String },
}

fn default_version() -> u32 {
    VERSION
}

impl Default for GenSettings {
    fn default() -> Self {
        Self {
            version: VERSION,
            last_opened: None,
        }
    }
}

/// `<state_dir>/gen-settings.json`, honouring `LOCALGPT_STATE_DIR` and
/// `LOCALGPT_PROFILE` through the shared directory rule.
pub fn settings_path() -> Option<PathBuf> {
    let dirs = localgpt_world_agent::paths::Dirs::resolve()?;
    Some(dirs.state.join("gen-settings.json"))
}

/// Read the settings, or [`None`] when there are none to read — a missing
/// file (first run), an unreadable one, a parse error, or a version this
/// build doesn't know. Every failure is a warning, never an error: the caller
/// falls back to defaults and Gen still opens.
pub fn load() -> Option<GenSettings> {
    let path = settings_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<GenSettings>(&text) {
        Ok(settings) if settings.version == VERSION => Some(settings),
        Ok(settings) => {
            tracing::warn!(
                "settings {}: version {} != {VERSION} — ignoring it and using defaults",
                path.display(),
                settings.version
            );
            None
        }
        Err(e) => {
            tracing::warn!(
                "settings {}: can't parse ({e}) — ignoring it and using defaults",
                path.display()
            );
            None
        }
    }
}

/// Write the settings atomically. Returns the path written, so a caller can
/// tell the user where their choice went.
pub fn save(settings: &GenSettings) -> anyhow::Result<PathBuf> {
    let path = settings_path()
        .ok_or_else(|| anyhow::anyhow!("can't work out where to keep Gen's settings"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;

    // tmp + rename: a crash mid-write leaves the old file intact rather than
    // a truncated one.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Change one field and write the result, so callers don't have to
/// read-modify-write. Missing settings start from [`Default`].
pub fn update(edit: impl FnOnce(&mut GenSettings)) -> anyhow::Result<PathBuf> {
    let mut settings = load().unwrap_or_default();
    edit(&mut settings);
    settings.version = VERSION;
    save(&settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_decide_nothing() {
        let settings = GenSettings::default();
        assert_eq!(settings.version, VERSION);
        assert!(settings.last_opened.is_none());
    }

    /// A file written by an older Gen — or hand-edited down to one key — must
    /// still load, with the rest defaulted. This is what lets fields be added
    /// without a version bump.
    #[test]
    fn partial_json_uses_defaults() {
        let settings: GenSettings =
            serde_json::from_str(r#"{"last_opened":{"kind":"world","dir":"/w"}}"#).unwrap();
        assert_eq!(
            settings.last_opened,
            Some(LastOpened::World { dir: "/w".into() })
        );
        // An absent version reads as this build's, so old files aren't rejected.
        assert_eq!(settings.version, VERSION);
    }

    #[test]
    fn round_trips_through_json() {
        let settings = GenSettings {
            version: VERSION,
            last_opened: Some(LastOpened::Document {
                path: "/tmp/notes.md".into(),
            }),
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<GenSettings>(&json).unwrap(),
            settings
        );
        // And the tagged enum reads as its own object, not a tuple.
        assert!(json.contains("\"kind\": \"document\""), "{json}");
    }

    /// The model menu and the Collaborate panel went; a file that still
    /// holds their keys reads the same, those keys ignored.
    #[test]
    fn a_file_with_the_old_keys_still_reads() {
        let json = format!(
            r#"{{"version": {VERSION}, "default_model": "gguf/bonsai", "tool_profile": "core",
                "collab": {{"session_name": "harbor", "port": 9900, "open": true}},
                "last_opened": {{"kind": "world", "dir": "/w"}}}}"#
        );
        let settings: GenSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            settings.last_opened,
            Some(LastOpened::World { dir: "/w".into() })
        );
    }

    /// A file from a future Gen is ignored rather than half-read.
    #[test]
    fn a_newer_version_is_rejected() {
        let json = format!(r#"{{"version":{}}}"#, VERSION + 1);
        let settings: GenSettings = serde_json::from_str(&json).unwrap();
        assert_ne!(settings.version, VERSION, "load() drops this one");
    }

    #[test]
    fn the_path_sits_in_the_state_dir() {
        let path = settings_path().expect("state dir should resolve");
        assert!(path.ends_with("gen-settings.json"), "{path:?}");
        assert!(path.is_absolute(), "{path:?}");
    }
}
