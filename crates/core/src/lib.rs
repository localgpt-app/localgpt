//! LocalGPT Core — what is left of the assistant's library: the agent loop,
//! its memory, config and providers, which Gen's own chat still runs on, and
//! the stdio MCP server and `Tool` trait under `localgpt world mcp`.
//!
//! It publishes no further versions: the harness goes in 0.5.0
//! (docs/rfcs/authoring/the-harness-goes.md), and what the world needs moves
//! to `localgpt-world-agent` first.

pub mod agent;
pub mod commands;
pub mod config;
pub mod env;
pub mod herdr;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod paths;
pub mod security;
pub mod text;

pub use config::Config;
