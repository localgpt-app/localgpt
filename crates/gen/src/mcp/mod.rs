//! Gen's tools beyond the scene basics in `gen3d::tools`: characters,
//! interactions, terrain, UI, physics, the WorldGen pipeline, multi-file
//! worlds, AI asset generation and image input. Each is a `Tool` an MCP
//! client calls, and each sends a `GenCommand` through the `GenBridge`.

pub mod avatar_tools;
pub mod interaction_tools;
pub mod multifile_tools;
pub mod multimodal_tools;
pub mod physics_tools;
pub mod terrain_tools;
pub mod ui_tools;
pub mod worldgen_tools;
