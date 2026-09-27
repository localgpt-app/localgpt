//! Hot reload: poll the document's modified time and recompile on change.
//! Polling, rather than OS file events, also catches editors that save by
//! writing a temp file and renaming it over the original.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;

use crate::draft;
use crate::scene::CurrentWorld;

pub struct WatchPlugin;

impl Plugin for WatchPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, poll);
    }
}

/// The file on screen and the modification time last loaded from it.
#[derive(Resource)]
pub struct DocSource {
    path: PathBuf,
    modified: Option<SystemTime>,
    timer: Timer,
}

impl DocSource {
    pub fn new(path: PathBuf) -> Self {
        let modified = modified_time(&path);
        Self {
            path,
            modified,
            timer: Timer::new(Duration::from_millis(400), TimerMode::Repeating),
        }
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn poll(
    time: Res<Time>,
    mut source: ResMut<DocSource>,
    store: Res<crate::sidecar::RecipeStore>,
    mut world: ResMut<CurrentWorld>,
) {
    if !source.timer.tick(time.delta()).just_finished() {
        return;
    }
    let modified = modified_time(&source.path);
    // Unchanged, or missing mid-save: keep the world we have.
    if modified.is_none() || modified == source.modified {
        return;
    }
    source.modified = modified;

    match crate::load(&source.path, &store) {
        Ok(next) => {
            for issue in draft::validate(&next.manifest) {
                warn!("{:?}: {}", issue.severity, issue.message);
            }
            let changed = next
                .doc
                .sections
                .iter()
                .enumerate()
                .filter(|(i, s)| world.doc.sections.get(*i).map(|old| old.hash) != Some(s.hash))
                .count();
            info!(
                "reloaded {}: {} sections, {changed} changed",
                source.path.display(),
                next.doc.sections.len()
            );
            *world = next;
        }
        Err(err) => warn!("reloading {} failed: {err}", source.path.display()),
    }
}
