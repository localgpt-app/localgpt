//! MCP server for LocalGPT Gen — exposes gen tools + core tools over stdio.
//!
//! This allows external CLI backends (gemini-cli, claude cli, codex) and
//! MCP-capable editors (VS Code, Zed, Cursor) to drive the Bevy scene.
//!
//! Exposed tools:
//! - All gen tools (spawn, modify, camera, audio, behaviors, world, etc.)
//! - memory_search, memory_get (read), memory_save, memory_log (write)
//! - web_fetch, web_search
//! - Experiment queue tools (gen_queue_experiment, gen_list_experiments, gen_experiment_status)

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use localgpt_core::agent::providers::ToolSchema;
use localgpt_core::agent::tools::Tool;
use localgpt_core::config::Config;
use localgpt_core::mcp::server::ToolHandler;

use crate::experiment::ExperimentTracker;
use crate::gen3d::GenBridge;

/// Run the MCP stdio server with gen tools + core tools.
pub async fn run_mcp_server(bridge: Arc<GenBridge>, config: Config) -> Result<()> {
    let tools = create_mcp_tools(bridge, &config)?;
    localgpt_core::mcp::server::run_mcp_stdio_server(tools, "localgpt-gen").await
}

/// Run the MCP HTTP server with gen tools + core tools on the given port.
///
/// This serves the MCP streamable HTTP transport at `http://127.0.0.1:{port}/mcp`.
pub async fn run_mcp_http_server(bridge: Arc<GenBridge>, config: Config, port: u16) -> Result<()> {
    let tools = create_mcp_tools(bridge, &config)?;
    let handler = Arc::new(ToolHandler::new("localgpt-gen", tools));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    localgpt_core::mcp::http_transport::run_mcp_http_server(handler, addr).await
}

/// Create the combined tool set for the MCP server:
/// gen tools + safe core tools + memory write tools + experiment tools.
///
/// CLI tools (bash, read_file, write_file, edit_file) are excluded because
/// external CLI backends already have their own file/shell tools.
pub fn create_mcp_tools(bridge: Arc<GenBridge>, config: &Config) -> Result<Vec<Box<dyn Tool>>> {
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::mcp::memory_tools::create_memory_write_tools;
    use localgpt_core::memory::MemoryManager;

    let workspace = config.workspace_path();

    // Core read tools: memory_search, memory_get, web_fetch, web_search
    let memory = MemoryManager::new_with_agent(&config.memory, "gen-mcp")?;
    let memory = Arc::new(memory);
    let mut tools = create_safe_tools(config, Some(memory))?;

    // Core write tools: memory_save, memory_log
    tools.extend(create_memory_write_tools(workspace));

    // Gen tools: all scene manipulation tools
    tools.extend(crate::gen3d::tools::create_gen_tools(bridge.clone()));

    // P1/P2/P3/P4/P5 tools: character + interaction + terrain + UI + physics
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

    // WG1 tools: worldgen blockout pipeline
    tools.extend(crate::mcp::worldgen_tools::create_worldgen_tools(
        bridge.clone(),
    ));

    // Multi-file worldgen + sync/drift tools
    tools.extend(crate::mcp::multifile_tools::create_multifile_tools(
        bridge.clone(),
    ));

    // AI1 tools: AI asset generation
    tools.extend(crate::mcp::asset_gen_tools::create_asset_gen_tools(
        bridge.clone(),
    ));

    // AI3 tools: Multimodal input for image-guided world generation
    tools.extend(crate::mcp::multimodal_tools::create_multimodal_tools(
        bridge,
    ));

    // Experiment queue tools: queue, list, status
    let tracker = Arc::new(ExperimentTracker::new(&config.paths.state_dir));
    tools.extend(crate::mcp::experiment_tools::create_experiment_tools(
        tracker,
    ));

    // Wrap all tools with MCP annotations (readOnlyHint, destructiveHint, etc.)
    let tools = tools.into_iter().map(|t| annotate_tool(t)).collect();

    Ok(tools)
}

// ---------------------------------------------------------------------------
// MCP tool annotations
// ---------------------------------------------------------------------------

/// Wraps an existing tool to add MCP annotations.
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

/// Wrap a tool with MCP annotations based on its name.
fn annotate_tool(tool: Box<dyn Tool>) -> Box<dyn Tool> {
    let annotations = tool_annotations(tool.name());
    Box::new(AnnotatedTool {
        inner: tool,
        tool_annotations: annotations,
    })
}

/// Return MCP annotations for a tool based on its name.
///
/// Categories:
/// - **read-only**: query/export/status tools that don't mutate scene state
/// - **destructive**: delete/clear tools that remove entities irreversibly
/// - **idempotent**: setters that produce the same result when called repeatedly
/// - **default**: mutating but non-destructive, non-idempotent (spawns, adds)
fn tool_annotations(name: &str) -> Value {
    // Read-only tools — inspect scene state, export files, query status
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
        "gen_list_experiments",
        "gen_experiment_status",
        "gen_validate_navigability",
        "gen_render_depth",
        "gen_preview_world",
        "gen_check_drift",
        "gen_query_terrain_height",
        "gen_generation_status",
        "gen_npc_observe",
        "gen_list_assets",
        "gen_asset_status",
        "get_avatar_state",
        "memory_search",
        "memory_get",
        "web_fetch",
        "web_search",
    ];

    // Destructive tools — irreversibly remove entities or clear scene
    const DESTRUCTIVE: &[&str] = &[
        "gen_delete_entity",
        "gen_delete_batch",
        "gen_clear_scene",
        "gen_unload_region",
    ];

    // Idempotent tools — setters that produce the same result on repeat calls
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
        "gen_set_npc_brain",
        "gen_set_npc_memory",
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
        // Default: mutating, non-destructive, non-idempotent (spawns, adds, etc.)
        json!({
            "readOnlyHint": false,
            "destructiveHint": false,
        })
    }
}
