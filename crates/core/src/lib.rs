//! LocalGPT Core — shared library for agent, memory, config, and security.
//!
//! This crate contains the platform-independent logic consumed by the CLI,
//! server, mobile, and gen crates. It has zero platform-specific dependencies
//! and compiles cleanly for `aarch64-apple-ios` and `aarch64-linux-android`.

pub mod agent;
pub mod commands;
pub mod concurrency;
pub mod config;
pub mod cron;
pub mod env;
pub mod heartbeat;
pub mod hooks;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod outbox;
pub mod paths;
pub mod security;
pub mod text;

pub use config::Config;
