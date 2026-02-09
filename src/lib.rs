//! LocalGPT - A lightweight, local-only AI assistant with persistent memory
//!
//! This crate provides the core functionality for LocalGPT, including:
//! - Agent core with LLM provider abstraction
//! - Memory system with markdown files and SQLite index
//! - Heartbeat runner for continuous operation
//! - HTTP server for UI integration
//! - Desktop GUI (egui-based)

pub mod agent;
pub mod commands;
pub mod concurrency;
pub mod config;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod heartbeat;
pub mod memory;
pub mod server;

pub use config::Config;
