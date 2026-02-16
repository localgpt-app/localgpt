//! Egui-based web UI for LocalGPT
//!
//! This module provides a WASM-compiled egui UI that runs in the browser.

#[cfg(feature = "egui-web")]
mod app;

#[cfg(feature = "egui-web")]
pub use app::WebApp;
