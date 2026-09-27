//! Full-screen overlays: onboarding, pause, queue, settings, credits, library.
//!
//! Overlays are fully opaque while shown (unlike the HUD, which fades): each
//! spawns on a state/flag change and despawns when it clears. The world keeps
//! playing behind them, dimmed — "quieter, not hidden". Modal visibility
//! lives in [`crate::OverlayStack`]; button presses are broadcast as
//! [`UiAction`] messages consumed by focused handlers (ARCHITECTURE R3/R4).

mod actions;
mod credits;
mod library;
mod onboarding;
mod pause;
mod queue;
mod settings;
mod widgets;

pub use actions::{
    FolderPickRx, UiAction, app_actions, comfort_actions, dispatch_buttons, onboarding_actions,
    overlay_actions, overlay_scroll, poll_folder_pick, queue_actions, world_actions,
};
pub use credits::sync_credits_overlay;
pub use library::{LibraryFilter, library_actions, sync_library_overlay};
pub use onboarding::{despawn_first_run, refresh_onboarding, spawn_first_run};
pub use pause::{sync_pause_overlay, update_intensity_knob};
pub use queue::sync_queue_overlay;
pub use settings::{sync_settings_overlay, update_comfort_toggles};
