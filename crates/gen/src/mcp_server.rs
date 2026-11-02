//! Gen's tool server: every gen tool over MCP, for the agent outside the
//! window — a coding agent (Claude Code, Codex, Gemini CLI), an editor (VS
//! Code, Zed, Cursor), a script. Over stdio as `localgpt-gen mcp-server`,
//! over TCP through the relay (`gen3d::mcp_relay`), and over HTTP with
//! `--mcp-http`.
//!
//! No memory, web, shell or file tools: an MCP client that is a coding agent
//! has its own, and Gen is not an agent.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use localgpt_world_agent::mcp::{Tool, ToolHandler, ToolSchema, run_mcp_stdio_server};

use crate::gen3d::GenBridge;

/// Serve the gen tools over stdio until the client goes away.
pub async fn run_mcp_server(bridge: Arc<GenBridge>) -> Result<()> {
    run_mcp_stdio_server(create_mcp_tools(bridge), "localgpt-gen").await
}

/// Serve the gen tools over the MCP streamable HTTP transport at
/// `http://127.0.0.1:{port}/mcp`.
pub async fn run_mcp_http_server(bridge: Arc<GenBridge>, port: u16) -> Result<()> {
    let handler = Arc::new(ToolHandler::new("localgpt-gen", create_mcp_tools(bridge)));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    localgpt_world_agent::mcp::http::run_mcp_http_server(handler, addr).await
}

/// Every gen tool, annotated for MCP: the scene basics, characters,
/// interactions, terrain, UI, physics, the WorldGen pipeline, multi-file
/// worlds and image input.
pub fn create_mcp_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = crate::gen3d::tools::create_gen_tools(bridge.clone());
    tools.extend(crate::mcp::avatar_tools::create_character_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::interaction_tools::create_interaction_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::terrain_tools::create_terrain_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::ui_tools::create_ui_tools(bridge.clone()));
    tools.extend(crate::mcp::physics_tools::create_physics_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::worldgen_tools::create_worldgen_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::multifile_tools::create_multifile_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::multimodal_tools::create_multimodal_tools(
        bridge,
    ));
    tools.into_iter().map(annotate_tool).collect()
}

// ---------------------------------------------------------------------------
// MCP tool annotations
// ---------------------------------------------------------------------------

/// A tool with the annotations an MCP client reads (`readOnlyHint`,
/// `destructiveHint`, `idempotentHint`).
struct AnnotatedTool {
    inner: Box<dyn Tool>,
    tool_annotations: Value,
}

#[async_trait]
impl Tool for AnnotatedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn schema(&self) -> ToolSchema {
        self.inner.schema()
    }

    fn annotations(&self) -> Option<Value> {
        Some(self.tool_annotations.clone())
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        self.inner.execute(arguments).await
    }
}

fn annotate_tool(tool: Box<dyn Tool>) -> Box<dyn Tool> {
    let annotations = tool_annotations(tool.name());
    Box::new(AnnotatedTool {
        inner: tool,
        tool_annotations: annotations,
    })
}

/// The annotations for a tool, by its name.
///
/// - **read-only**: query/export/status tools that don't change the scene
/// - **destructive**: delete/clear tools that remove entities irreversibly
/// - **idempotent**: setters that give the same result when repeated
/// - **default**: mutating but neither destructive nor idempotent (spawns, adds)
fn tool_annotations(name: &str) -> Value {
    const READ_ONLY: &[&str] = &[
        "gen_scene_info",
        "gen_entity_info",
        "gen_screenshot",
        "gen_audio_info",
        "gen_list_behaviors",
        "gen_undo_info",
        "gen_export_screenshot",
        "gen_export_gltf",
        "gen_export_html",
        "gen_export_world",
        "gen_validate_navigability",
        "gen_render_depth",
        "gen_check_drift",
        "gen_query_terrain_height",
        "get_avatar_state",
    ];

    const DESTRUCTIVE: &[&str] = &[
        "gen_delete_entity",
        "gen_delete_batch",
        "gen_clear_scene",
        "gen_unload_region",
    ];

    const IDEMPOTENT: &[&str] = &[
        "gen_modify_entity",
        "gen_modify_batch",
        "gen_set_camera",
        "gen_set_camera_mode",
        "gen_set_light",
        "gen_set_environment",
        "gen_set_sky",
        "gen_set_ambience",
        "gen_modify_audio",
        "gen_set_physics",
        "gen_set_gravity",
        "gen_set_spawn_point",
        "gen_set_npc_dialogue",
        "gen_set_tier",
        "gen_set_role",
        "gen_modify_blockout",
        "gen_bulk_modify",
        "gen_edit_navmesh",
        "gen_match_style",
        "gen_pause_behaviors",
        "move_avatar",
        "look_avatar",
        "teleport_avatar",
    ];

    if READ_ONLY.contains(&name) {
        json!({
            "readOnlyHint": true,
            "destructiveHint": false,
        })
    } else if DESTRUCTIVE.contains(&name) {
        json!({
            "readOnlyHint": false,
            "destructiveHint": true,
        })
    } else if IDEMPOTENT.contains(&name) {
        json!({
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": true,
        })
    } else {
        json!({
            "readOnlyHint": false,
            "destructiveHint": false,
        })
    }
}
