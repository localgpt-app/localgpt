//! Desktop mode: Gen as a windowed app that doesn't need a terminal.
//!
//! When Gen starts without a terminal (double-clicked, or launched from the
//! macOS app bundle) or with `--desktop`, prompts come from a panel inside
//! the window instead of the REPL. The pieces:
//!
//! - [`chat`]: channels between the agent thread and the panel, and the
//!   events the agent loop reports (streamed text, tool calls, errors).
//! - [`panel`]: the egui prompt panel, plus the guard that keeps typing from
//!   driving the camera.
//! - [`models`]: which models this machine can switch to (installed CLI
//!   backends, local Ollama models).
//! - [`collab`] (multiplayer): the panel's Collaborate section — host a
//!   session or pair with one and launch a viewer window.
//! - [`client_panel`] (multiplayer): the viewer's own prompt panel, for a
//!   `--join` window without a terminal.
//! - [`shell_env`]: recovers the login shell's `PATH`, which apps launched
//!   from Finder don't inherit, so CLI backends like `claude` are found.

pub mod chat;
#[cfg(feature = "multiplayer")]
pub mod client_panel;
#[cfg(feature = "multiplayer")]
pub(crate) mod collab;
pub mod models;
pub mod panel;
pub mod shell_env;

pub use chat::{AgentChannels, ChatEvent, ChatSink, PanelChannels, create_chat_channels};
#[cfg(feature = "multiplayer")]
pub use client_panel::ClientPanelPlugin;
pub use panel::{PanelSettings, PromptPanelPlugin};
