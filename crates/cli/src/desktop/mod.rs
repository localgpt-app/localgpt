//! Desktop GUI module using egui/eframe
//!
//! This module provides a native desktop application that embeds the LocalGPT agent
//! directly - no HTTP, no daemon needed. The agent runs in a background thread
//! and communicates with the UI via channels.

mod app;
mod state;
mod views;
mod worker;

pub use app::DesktopApp;
