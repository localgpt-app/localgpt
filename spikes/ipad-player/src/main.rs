//! The native player spike: one song world on an iPad, drawn by
//! `localgpt-world-bevy` and played by `localgpt-world-audio`, the crates
//! the desktop app draws and plays with, so what it measures is what a
//! tablet or phone app built on them would cost. README.md has what it asks
//! and the results.
//!
//! The player is iOS. Built for the Mac this binary only says how to run it;
//! `src/bin/bundle.rs` builds it for the simulator or the iPad and starts it.

#[cfg(target_os = "ios")]
mod camera;
#[cfg(target_os = "ios")]
mod hud;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
mod player;

fn main() {
    #[cfg(target_os = "ios")]
    player::run();
    #[cfg(not(target_os = "ios"))]
    eprintln!(
        "The player runs on iOS. From spikes/ipad-player, `cargo run -- sim` starts it on \
         the simulator and `cargo run -- device` on a paired iPad (README.md)."
    );
}
