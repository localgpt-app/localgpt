//! LocalGPT Gen — AI-driven 3D scene generation via Bevy
//!
//! This crate provides in-process 3D rendering and scene composition
//! controlled by the LLM agent through intent-level tools.

#![allow(clippy::items_after_test_module)]

pub mod character;
pub mod character_tools;
pub mod experiment;
pub mod gen3d;
pub mod gpu_lock;
pub mod heartbeat_gen;
pub mod inspector;
pub mod interaction;
pub mod mcp;
pub mod mcp_server;
pub mod physics;
pub mod terrain;
pub mod tracing_printer;
pub mod ui;
pub mod worldgen;
