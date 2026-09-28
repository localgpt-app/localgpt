//! A dev hook for looking at the window without a person at it:
//! `LOCALGPT_APP_SCREENSHOT=<png>` saves one frame of the primary window, a
//! few seconds after launch, then exits. `LOCALGPT_APP_SCREENSHOT_AFTER`
//! sets the delay in seconds (default 12 — long enough for a world's assets
//! and a model's first builds to land).
//!
//! It reads the window back through Bevy, so it needs no screen-recording
//! permission; like MD's and Verse's hooks, the frame comes back black when
//! the Mac is locked.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

pub const LOCALGPT_APP_SCREENSHOT: &str = "LOCALGPT_APP_SCREENSHOT";
pub const LOCALGPT_APP_SCREENSHOT_AFTER: &str = "LOCALGPT_APP_SCREENSHOT_AFTER";

/// Added only when the variable is set.
pub struct ScreenshotPlugin {
    pub path: String,
    pub after_secs: f32,
}

impl ScreenshotPlugin {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var(LOCALGPT_APP_SCREENSHOT).ok()?;
        let after_secs = std::env::var(LOCALGPT_APP_SCREENSHOT_AFTER)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12.0);
        Some(Self { path, after_secs })
    }
}

#[derive(Resource)]
struct Pending {
    path: String,
    at: f32,
    taken: bool,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Pending {
            path: self.path.clone(),
            at: self.after_secs,
            taken: false,
        })
        .add_systems(Update, take_then_exit);
    }
}

fn take_then_exit(
    time: Res<Time>,
    mut pending: ResMut<Pending>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    if !pending.taken && t >= pending.at {
        pending.taken = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(pending.path.clone()));
    }
    // The readback lands a frame or two later; leave it time to be written.
    if pending.taken && t >= pending.at + 2.0 {
        exit.write(AppExit::Success);
    }
}
