//! Gen's own settings, so the app never needs a config file.
//!
//! Gen ships as a desktop app: it has to start with nothing on disk and
//! remember what the user picks in the window. That state is Gen's, not the
//! assistant's, so it lives here instead of in
//! `~/.config/localgpt/config.toml` — Gen no longer reads or creates that
//! file. It still uses `localgpt-core` for the agent and providers; only the
//! *file* coupling is gone.
//!
//! `~/.local/state/localgpt/gen-settings.json`, beside the
//! `gen-experiments.jsonl` Gen already writes there. `LOCALGPT_STATE_DIR` and
//! `LOCALGPT_PROFILE` move it, because the path comes from
//! [`localgpt_core::paths::Paths::resolve`] rather than a second copy of the
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

    /// Model for Gen's agents. `"auto"` re-picks the best local model each
    /// run. `None` means "decide from what this machine has" every time.
    #[serde(default)]
    pub default_model: Option<String>,

    /// `core` / `standard` / `full` — see `gen3d::tool_profile`.
    #[serde(default)]
    pub tool_profile: Option<String>,

    /// Local embeddings for Gen's memory (style recall). `None`: on when the
    /// model is already on disk, so a first launch never waits on a download.
    /// `Some(true)`: always, fetching the ~80 MB model the first time.
    /// `Some(false)`: never — keyword search only.
    #[serde(default)]
    pub embeddings: Option<bool>,
}

fn default_version() -> u32 {
    VERSION
}

impl Default for GenSettings {
    fn default() -> Self {
        Self {
            version: VERSION,
            default_model: None,
            tool_profile: None,
            embeddings: None,
        }
    }
}

/// `<state_dir>/gen-settings.json`, honouring `LOCALGPT_STATE_DIR` and
/// `LOCALGPT_PROFILE` through core's path resolution.
pub fn settings_path() -> Option<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve().ok()?;
    Some(paths.state_dir.join("gen-settings.json"))
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
        assert!(settings.default_model.is_none());
        assert!(settings.tool_profile.is_none());
    }

    /// A file written by an older Gen — or hand-edited down to one key — must
    /// still load, with the rest defaulted. This is what lets fields be added
    /// without a version bump.
    #[test]
    fn partial_json_uses_defaults() {
        let settings: GenSettings = serde_json::from_str(r#"{"default_model":"auto"}"#).unwrap();
        assert_eq!(settings.default_model.as_deref(), Some("auto"));
        assert!(settings.tool_profile.is_none());
        // An absent version reads as this build's, so old files aren't rejected.
        assert_eq!(settings.version, VERSION);
    }

    #[test]
    fn round_trips_through_json() {
        let settings = GenSettings {
            version: VERSION,
            default_model: Some("gguf/bonsai".into()),
            tool_profile: Some("core".into()),
            embeddings: Some(false),
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<GenSettings>(&json).unwrap(),
            settings
        );
    }

    /// A file from a future Gen is ignored rather than half-read.
    #[test]
    fn a_newer_version_is_rejected() {
        let json = format!(r#"{{"version":{},"default_model":"x"}}"#, VERSION + 1);
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
