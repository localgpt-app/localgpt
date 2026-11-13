//! Gen's tools beyond the scene basics and physics in `gen3d::tools`:
//! characters, interactions, terrain, UI, the WorldGen pipeline, multi-file
//! worlds and image input. Each is a `Tool` an MCP client calls, and each
//! sends a `GenCommand` through the `GenBridge`.

pub mod avatar_tools;
pub mod interaction_tools;
pub mod multifile_tools;
pub mod multimodal_tools;
pub mod terrain_tools;
pub mod ui_tools;
pub mod worldgen_tools;
