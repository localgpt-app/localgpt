//! LocalGPT Gen — the world builder, in a Bevy window.
//!
//! In-process 3D rendering and scene composition, driven through
//! intent-level tools by whatever agent reaches them: an MCP client over the
//! relay or the stdio server (`mcp_server`), the world's live API
//! (`gen3d::live`), or a collaborative room (`net`). Gen runs no model of
//! its own and holds no model's configuration.

#![allow(clippy::items_after_test_module)]

pub mod character;
pub mod config;
pub mod desktop;
pub mod gen3d;
pub mod inspector;
pub mod interaction;
pub mod mcp;
pub mod mcp_server;
#[cfg(feature = "multiplayer")]
pub mod net;
pub mod physics;
pub mod settings;
pub mod terrain;
pub mod ui;
pub mod worldgen;
