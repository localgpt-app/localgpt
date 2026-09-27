//! Persistent app settings — the one file (besides the analysis sidecars) that
//! survives a restart. Stored as JSON next to the analysis cache under the
//! user's local app-data dir (`<local>/localgpt-verse/settings.json`).
//!
//! Holds the user-adjustable resources that reset to defaults every launch
//! today: Comfort toggles, volume, world intensity, camera mode, the onboarding
//! step, and the remembered import folders. Missing/corrupt file → defaults
//! (same graceful contract as the sidecars; the app runs fully without it).
//!
//! # Write timing
//! `save` is cheap (one small JSON write) but is debounced by the caller
//! (see `save_if_changed` in `main.rs`) to one write per second at most, so
//! dragging a slider doesn't churn the disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{CameraMode, Comfort};

/// The persisted application state. Mirrors the user-adjustable Bevy resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub comfort: Comfort,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_world_intensity")]
    pub world_intensity: f32,
    #[serde(default)]
    pub camera_mode: CameraMode,
    /// First-run onboarding step (0 = photosensitivity, 1 = controls, 2 = import).
    /// Set to a sentinel ≥ the done-step once onboarding completes so a returning
    /// user skips it.
    #[serde(default)]
    pub onboarding_done: bool,
    /// Import folders the user has opened, most-recent last. Today only the
    /// last one is re-imported at startup; accumulating them into one library
    /// is the planned multi-folder follow-up.
    #[serde(default)]
    pub last_folders: Vec<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            comfort: Comfort::default(),
            volume: default_volume(),
            world_intensity: default_world_intensity(),
            camera_mode: CameraMode::default(),
            onboarding_done: false,
            last_folders: Vec::new(),
        }
    }
}

fn default_volume() -> f32 {
    0.85
}
fn default_world_intensity() -> f32 {
    0.5
}

/// Where the settings file lives: `<local app data>/localgpt-verse/settings.json`,
/// the same `localgpt-verse/` root as the analysis cache.
pub fn settings_path() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("localgpt-verse")
            .join("settings.json"),
    )
}

/// Load settings from disk. `None` (→ caller uses defaults) when the file is
/// missing or unreadable — never panics, never blocks startup.
pub fn load() -> Option<AppSettings> {
    let path = settings_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<AppSettings>(&text) {
        Ok(s) => Some(s),
        Err(e) => {
            bevy::log::warn!("settings: can't parse {}: {e}", path.display());
            None
        }
    }
}

/// Save settings to disk. Best-effort: logs on failure, never panics. Creates
/// the parent dir if missing.
pub fn save(settings: &AppSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                bevy::log::warn!("settings: can't write {}: {e}", path.display());
            }
        }
        Err(e) => bevy::log::warn!("settings: can't serialize: {e}"),
    }
}

// --- folder recording (cross-module hand-off) ------------------------------
//
// `record_folder` is called from the UI action handlers (onboarding + library)
// which don't hold a `SettingsSnapshot` resource. They push the path here; the
// debounced saver (`main.rs::save_settings_debounced`) drains the queue each
// frame and folds new folders into the snapshot before writing. A static
// Mutex<Vec> is the simplest hand-off that avoids threading the resource
// through the action systems.

static PENDING_FOLDERS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Record a folder the user just imported, for later persistence. Called from
/// the UI handlers; drained by the debounced saver.
pub fn record_folder(folder: &PathBuf) {
    if let Ok(mut q) = PENDING_FOLDERS.lock() {
        // Avoid dupes; most-recent last.
        q.retain(|p| p != folder);
        q.push(folder.clone());
    }
}

/// Drain any folders queued by [`record_folder`] since the last call. The
/// saver merges these into `last_folders` before writing.
pub fn drain_pending_folders() -> Vec<PathBuf> {
    PENDING_FOLDERS
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.volume, s.volume);
        assert_eq!(back.world_intensity, s.world_intensity);
        assert_eq!(back.camera_mode, s.camera_mode);
        assert!(back.last_folders.is_empty());
    }

    #[test]
    fn partial_json_uses_defaults() {
        // An old/partial settings file missing fields must still load, with
        // missing values falling back to defaults (serde(default) on each).
        let json = r#"{"volume": 0.5}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(s.volume, 0.5);
        assert_eq!(s.world_intensity, default_world_intensity());
        assert_eq!(s.camera_mode, CameraMode::default());
    }

    #[test]
    fn comfort_round_trips() {
        let s = AppSettings {
            comfort: Comfort {
                reduce_flashing: true,
                gentler_motion: true,
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(back.comfort.reduce_flashing);
        assert!(back.comfort.gentler_motion);
    }
}
