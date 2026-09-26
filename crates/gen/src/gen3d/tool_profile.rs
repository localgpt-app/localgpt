//! Tool profiles: how much of the toolbelt a generation agent sees.
//!
//! The full host agent carries 77 `gen_*` tools — measured at ~58KB of JSON
//! schema, ~14k tokens on every request — plus the safe and CLI tools. That's
//! a rounding error on a 128k cloud context but a third of a 32k local
//! model's window before it sees a word of the conversation. Profiles trade
//! capability for context:
//!
//! - **core** — 22 scene-editing essentials, ~15KB / ~3.8k tokens: spawn,
//!   modify, delete, query, camera/environment/light, behaviors, undo, world
//!   save/load/clear. No CLI tools (shell/files), no
//!   avatar/terrain/ui/physics/multifile/WorldGen modules. For small local
//!   models (e.g. a 27B at 32k context) that mostly drive spawn/modify.
//! - **standard** — 49 tools, ~37KB / ~9.4k tokens: core + the rest of
//!   `gen3d::tools` (mesh loading, exports, audio, physics) + terrain + the
//!   WorldGen pipeline (WG1–WG7).
//! - **full** — everything (the default; today's behavior).
//!
//! Selected with `--tools core|standard|full` or `[gen] tool_profile` in
//! config.toml (the flag wins). Applies to the host, headless, and remote
//! guest agents — guests still get the remote scope on top (the profile can
//! only shrink that set, never widen it). The external MCP relay always
//! serves the full set: MCP clients like Claude Code handle big schemas fine.

use localgpt_core::agent::tools::Tool;

/// A tool-profile selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolProfile {
    /// Scene-editing essentials only.
    Core,
    /// Core + meshes/exports/audio + terrain + WorldGen.
    Standard,
    /// The whole toolbelt.
    #[default]
    Full,
}

/// The `gen3d::tools` subset that stays enabled at [`ToolProfile::Core`].
const CORE_GEN_TOOLS: &[&str] = &[
    // spawn / modify / delete
    "gen_spawn_primitive",
    "gen_spawn_batch",
    "gen_modify_entity",
    "gen_modify_batch",
    "gen_delete_entity",
    "gen_delete_batch",
    // query — screenshot included so small models keep a visual feedback loop
    "gen_scene_info",
    "gen_entity_info",
    "gen_screenshot",
    // worlds
    "gen_save_world",
    "gen_load_world",
    "gen_clear_scene",
    // camera / environment / light
    "gen_set_camera",
    "gen_set_environment",
    "gen_set_light",
    // behaviors
    "gen_add_behavior",
    "gen_remove_behavior",
    "gen_list_behaviors",
    "gen_pause_behaviors",
    // undo
    "gen_undo",
    "gen_redo",
    "gen_undo_info",
];

/// Extra `gen_*` tools enabled at [`ToolProfile::Standard`]: the rest of
/// `gen3d::tools` (mesh loading, exports, audio, physics), the terrain tools,
/// and the WorldGen pipeline.
const STANDARD_GEN_TOOLS: &[&str] = &[
    // rest of gen3d::tools
    "gen_spawn_mesh",
    "gen_load_gltf",
    "gen_fork_world",
    "gen_export_gltf",
    "gen_export_html",
    "gen_export_world",
    "gen_export_screenshot",
    "gen_set_ambience",
    "gen_audio_emitter",
    "gen_modify_audio",
    "gen_audio_info",
    "gen_add_collider",
    "gen_add_force",
    "gen_add_joint",
    "gen_set_gravity",
    "gen_set_physics",
    // terrain
    "gen_add_terrain",
    "gen_add_foliage",
    "gen_add_water",
    "gen_add_path",
    "gen_set_sky",
    "gen_query_terrain_height",
    // WorldGen (WG1–WG7)
    "gen_plan_layout",
    "gen_apply_blockout",
    "gen_populate_region",
    "gen_modify_blockout",
    "gen_build_navmesh",
    "gen_edit_navmesh",
    "gen_validate_navigability",
    "gen_evaluate_scene",
    "gen_auto_refine",
    "gen_regenerate",
    "gen_bulk_modify",
    "gen_set_role",
    "gen_set_tier",
    "gen_preview_world",
    "gen_render_depth",
];

/// Host-side CLI tools (shell / filesystem / browser) — dropped below
/// [`ToolProfile::Full`] so "small context" really means small.
const CLI_TOOLS: &[&str] = &["bash", "read_file", "write_file", "edit_file", "browser"];

/// Tools the profile filter is allowed to drop: the `gen_*` scene toolbelt
/// and the host CLI tools. Anything else (memory_search, web_search,
/// spawn_agent, …) passes at every profile — safe default for tools added
/// in the future.
fn is_profile_scoped(name: &str) -> bool {
    name.starts_with("gen_") || CLI_TOOLS.contains(&name)
}

impl ToolProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }

    /// Parse a profile name (`core` / `standard` / `full`, case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "core" => Some(Self::Core),
            "standard" => Some(Self::Standard),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    /// Resolve the effective profile: the `--tools` flag wins over
    /// `[gen] tool_profile`, which wins over the default (full). An invalid
    /// flag value is an error; an invalid config value warns and falls back
    /// to full so a typo doesn't block startup.
    pub fn resolve(flag: Option<&str>, config: Option<&str>) -> anyhow::Result<Self> {
        if let Some(value) = flag {
            return Self::parse(value).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid --tools profile '{value}' — expected core, standard, or full"
                )
            });
        }
        if let Some(value) = config {
            if let Some(profile) = Self::parse(value) {
                return Ok(profile);
            }
            tracing::warn!(
                "invalid [gen] tool_profile '{value}' in config — expected core, standard, or \
                 full; using full"
            );
        }
        Ok(Self::Full)
    }

    /// Whether a tool with this name is offered at this profile.
    pub fn allows(self, name: &str) -> bool {
        match self {
            Self::Full => true,
            Self::Core => !is_profile_scoped(name) || CORE_GEN_TOOLS.contains(&name),
            Self::Standard => {
                !is_profile_scoped(name)
                    || CORE_GEN_TOOLS.contains(&name)
                    || STANDARD_GEN_TOOLS.contains(&name)
            }
        }
    }
}

impl std::fmt::Display for ToolProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Filter a built tool list down to what the profile allows.
pub fn apply_tool_profile(tools: Vec<Box<dyn Tool>>, profile: ToolProfile) -> Vec<Box<dyn Tool>> {
    if profile == ToolProfile::Full {
        return tools;
    }
    tools
        .into_iter()
        .filter(|t| profile.allows(t.name()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profiles() {
        assert_eq!(ToolProfile::parse("core"), Some(ToolProfile::Core));
        assert_eq!(ToolProfile::parse("Standard"), Some(ToolProfile::Standard));
        assert_eq!(ToolProfile::parse(" FULL "), Some(ToolProfile::Full));
        assert_eq!(ToolProfile::parse("everything"), None);
    }

    #[test]
    fn resolve_prefers_flag_then_config_then_default() {
        assert_eq!(
            ToolProfile::resolve(Some("core"), Some("full")).unwrap(),
            ToolProfile::Core
        );
        assert_eq!(
            ToolProfile::resolve(None, Some("standard")).unwrap(),
            ToolProfile::Standard
        );
        assert_eq!(ToolProfile::resolve(None, None).unwrap(), ToolProfile::Full);
        assert!(ToolProfile::resolve(Some("nope"), None).is_err());
        // A bad config value warns and falls back to full.
        assert_eq!(
            ToolProfile::resolve(None, Some("nope")).unwrap(),
            ToolProfile::Full
        );
    }

    #[test]
    fn core_keeps_essentials_and_safe_tools() {
        let p = ToolProfile::Core;
        assert!(p.allows("gen_spawn_primitive"));
        assert!(p.allows("gen_screenshot"));
        assert!(p.allows("gen_undo"));
        // Non-scoped tools (safe tools, spawn_agent) pass at every profile.
        assert!(p.allows("memory_search"));
        assert!(p.allows("spawn_agent"));
        // Dropped at core: CLI tools, pipelines, modules, mesh/export/audio.
        assert!(!p.allows("bash"));
        assert!(!p.allows("read_file"));
        assert!(!p.allows("gen_plan_layout"));
        assert!(!p.allows("gen_add_npc"));
        assert!(!p.allows("gen_load_gltf"));
        assert!(!p.allows("gen_audio_emitter"));
        assert!(!p.allows("gen_add_terrain"));
    }

    #[test]
    fn standard_adds_pipelines_but_not_modules_or_cli() {
        let p = ToolProfile::Standard;
        for name in [
            "gen_plan_layout",
            "gen_apply_blockout",
            "gen_add_terrain",
            "gen_set_sky",
            "gen_load_gltf",
            "gen_export_gltf",
            "gen_audio_emitter",
            "gen_set_physics",
        ] {
            assert!(p.allows(name), "standard should allow {name}");
        }
        for name in [
            "gen_add_npc",      // avatar
            "gen_write_region", // multifile
            "gen_add_hud",      // ui
            "gen_add_door",     // interaction
            "gen_spawn_player", // interaction
            "bash",
            "edit_file",
        ] {
            assert!(!p.allows(name), "standard should drop {name}");
        }
    }

    #[test]
    fn full_keeps_everything() {
        let p = ToolProfile::Full;
        for name in [
            "gen_spawn_primitive",
            "gen_add_npc",
            "gen_plan_layout",
            "bash",
            "anything_at_all",
        ] {
            assert!(p.allows(name));
        }
    }

    #[test]
    fn profiles_are_nested() {
        for name in CORE_GEN_TOOLS {
            assert!(
                ToolProfile::Standard.allows(name),
                "{name} lost at standard"
            );
            assert!(ToolProfile::Full.allows(name), "{name} lost at full");
        }
        for name in STANDARD_GEN_TOOLS {
            assert!(ToolProfile::Full.allows(name), "{name} lost at full");
        }
    }

    /// Drift guard: every name in the profile lists must exist in the real
    /// toolbelt, so a renamed tool fails the build instead of silently
    /// vanishing from a profile.
    #[test]
    fn profile_names_exist_in_the_real_toolbelt() {
        let (bridge, _channels) = crate::gen3d::create_gen_channels();
        let mut tools = crate::gen3d::tools::create_gen_tools(bridge.clone());
        tools.extend(crate::mcp::terrain_tools::create_terrain_tools(
            bridge.clone(),
        ));
        tools.extend(crate::mcp::worldgen_tools::create_worldgen_tools(bridge));
        let real: std::collections::HashSet<&str> = tools.iter().map(|t| t.name()).collect();
        for name in CORE_GEN_TOOLS.iter().chain(STANDARD_GEN_TOOLS.iter()) {
            assert!(real.contains(name), "profile lists unknown tool '{name}'");
        }
    }
}
