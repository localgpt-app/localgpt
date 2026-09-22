//! Tool scoping for prompts from collaborative clients.
//!
//! Remote users steer an agent running on the host machine, so by default
//! (`--remote-tools safe`) their prompts run on a separate agent that can
//! only edit the scene:
//!
//! - **Scene tools only** — gen/world, character, interaction, terrain, UI,
//!   physics, and worldgen tools. No shell or file tools, no memory
//!   read/write (the host's memory is private), no web access, no
//!   multimodal inputs (they read host files), no experiment queue.
//! - **No writes to the host's disk** — saving/forking worlds and exporting
//!   files stay host-only, so a remote user can't overwrite the host's
//!   saved worlds or fill its disk. Scene edits themselves are allowed
//!   (that's the point of the session) and are undoable.
//! - **No caller-chosen paths** — every tool is wrapped so arguments that
//!   name filesystem locations (`path`, `output_path`, …) are refused and
//!   world names must be plain identifiers; tools fall back to their
//!   workspace defaults.
//!
//! The same scoped tool set is served to CLI backends through a dedicated
//! MCP relay, with the backend's own built-in tools disabled.

use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use localgpt_core::agent::providers::ToolSchema;
use localgpt_core::agent::tools::{PermissionLevel, Tool};
use serde_json::Value;

use crate::gen3d::GenBridge;

/// Argument names that let a caller pick a filesystem location.
const PATH_ARGS: &[&str] = &[
    "path",
    "output_path",
    "output_dir",
    "dir",
    "directory",
    "file",
    "file_path",
    "image_path",
    "depth_map_path",
    "workspace",
];

/// Tools that write to the host's disk — never offered to remote prompts.
pub const HOST_ONLY_TOOLS: &[&str] = &[
    "gen_save_world",
    "gen_fork_world",
    "gen_export_screenshot",
    "gen_export_gltf",
    "gen_render_depth",
    "gen_preview_world",
];

/// `(tool, argument)` pairs whose value becomes a filesystem location or an
/// asset reference, so it must be a plain name.
const PLAIN_NAME_ARGS: &[(&str, &str)] = &[
    ("gen_save_world", "name"),
    ("gen_load_world", "name"),
    ("gen_fork_world", "source"),
    ("gen_fork_world", "new_name"),
    ("gen_add_npc", "model"),
];

/// Why a remote tool call was refused (pure check, unit-tested).
pub fn check_remote_args(tool: &str, args: &Value) -> Result<(), String> {
    let Some(obj) = args.as_object() else {
        return Ok(());
    };
    for key in PATH_ARGS {
        if obj.get(*key).is_some_and(|v| !v.is_null()) {
            return Err(format!(
                "`{key}` is not allowed in collaborative prompts — remote users can't choose \
                 file locations; omit it to use the default"
            ));
        }
    }
    for (t, key) in PLAIN_NAME_ARGS {
        if *t == tool
            && let Some(value) = obj.get(*key).and_then(Value::as_str)
            && !is_plain_name(value)
        {
            return Err(format!(
                "`{key}` may only contain letters, digits, '-', '_' and '.' in collaborative prompts"
            ));
        }
    }
    Ok(())
}

/// A name that is safe to join onto a directory: non-empty, no separators,
/// no `..`, not absolute, not hidden.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.contains("..")
}

/// Wraps a scene tool with [`check_remote_args`].
struct RemoteScoped {
    inner: Box<dyn Tool>,
}

#[async_trait]
impl Tool for RemoteScoped {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn schema(&self) -> ToolSchema {
        let mut schema = self.inner.schema();
        // Hide path parameters from the model so it doesn't try them.
        if let Some(props) = schema
            .parameters
            .get_mut("properties")
            .and_then(Value::as_object_mut)
        {
            for key in PATH_ARGS {
                props.remove(*key);
            }
        }
        if let Some(required) = schema
            .parameters
            .get_mut("required")
            .and_then(Value::as_array_mut)
        {
            required.retain(|v| v.as_str().is_none_or(|k| !PATH_ARGS.contains(&k)));
        }
        schema
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
        if let Err(reason) = check_remote_args(self.inner.name(), &args) {
            bail!(reason);
        }
        self.inner.execute(arguments).await
    }

    fn permission_level(&self) -> PermissionLevel {
        self.inner.permission_level()
    }

    fn annotations(&self) -> Option<Value> {
        self.inner.annotations()
    }
}

/// Scene-editing tools for remote prompts, each path-scoped.
pub fn create_remote_scene_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    tools.extend(crate::gen3d::tools::create_gen_tools(bridge.clone()));
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
    tools.extend(crate::mcp::worldgen_tools::create_worldgen_tools(bridge));
    tools
        .into_iter()
        .filter(|t| !HOST_ONLY_TOOLS.contains(&t.name()))
        .map(|inner| Box::new(RemoteScoped { inner }) as Box<dyn Tool>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_args_refused() {
        assert!(check_remote_args("gen_export_gltf", &json!({"path": "/tmp/x.glb"})).is_err());
        assert!(check_remote_args("gen_render_depth", &json!({"output_path": "a.png"})).is_err());
        // Null / absent is fine (tool uses its default).
        assert!(check_remote_args("gen_export_gltf", &json!({"path": null})).is_ok());
        assert!(check_remote_args("gen_spawn_primitive", &json!({"name": "a/b"})).is_ok());
    }

    #[test]
    fn remote_tool_set_is_scene_only() {
        let (bridge, _channels) = crate::gen3d::create_gen_channels();
        let tools = create_remote_scene_tools(bridge);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"gen_spawn_primitive"));
        for forbidden in HOST_ONLY_TOOLS {
            assert!(!names.contains(forbidden), "{forbidden} must be host-only");
        }
        for name in &names {
            assert!(
                !name.starts_with("memory_")
                    && !name.starts_with("web_")
                    && !matches!(*name, "bash" | "read_file" | "write_file" | "edit_file"),
                "{name} must not be offered to remote prompts"
            );
        }
        // Path parameters are hidden from every schema.
        for tool in &tools {
            let schema = tool.schema();
            if let Some(props) = schema
                .parameters
                .get("properties")
                .and_then(Value::as_object)
            {
                for key in PATH_ARGS {
                    assert!(!props.contains_key(*key), "{} exposes {key}", tool.name());
                }
            }
        }
    }

    #[test]
    fn world_names_must_be_plain() {
        for bad in ["../../x", "/etc", "a/b", ".hidden", "", "a..b", "x\\y"] {
            assert!(
                check_remote_args("gen_save_world", &json!({"name": bad})).is_err(),
                "{bad:?} should be refused"
            );
        }
        assert!(check_remote_args("gen_save_world", &json!({"name": "castle-v2_final.1"})).is_ok());
        assert!(check_remote_args("gen_fork_world", &json!({"source": "~/.ssh"})).is_err());
        assert!(check_remote_args("gen_add_npc", &json!({"model": "http://x/y.glb"})).is_err());
        assert!(check_remote_args("gen_add_npc", &json!({"model": "default_humanoid"})).is_ok());
    }
}
