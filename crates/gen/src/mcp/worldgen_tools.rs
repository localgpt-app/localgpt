//! MCP tool handlers for WorldGen: procedural blockout pipeline.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use crate::worldgen;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

// ---------------------------------------------------------------------------
// gen_plan_layout
// ---------------------------------------------------------------------------

pub struct GenPlanLayoutTool {
    bridge: Arc<GenBridge>,
}

impl GenPlanLayoutTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPlanLayoutTool {
    fn name(&self) -> &str {
        "gen_plan_layout"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_plan_layout".to_string(),
            description: "Generate a structured world layout plan from a text description. Returns a BlockoutSpec JSON that can be reviewed/adjusted before applying with gen_apply_blockout.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Natural language world description (e.g., 'a medieval village with a forest and lake')"
                    },
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [50, 50],
                        "description": "World dimensions in meters [X, Z]"
                    },
                    "seed": {
                        "type": "integer",
                        "description": "Random seed for deterministic generation (optional)"
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prompt"))?;

        let size = args["size"]
            .as_array()
            .map(|a| {
                [
                    a[0].as_f64().unwrap_or(50.0) as f32,
                    a[1].as_f64().unwrap_or(50.0) as f32,
                ]
            })
            .unwrap_or([50.0, 50.0]);

        let seed = args["seed"].as_u64().map(|v| v as u32);

        let cmd = GenCommand::PlanLayout {
            prompt: prompt.to_string(),
            size,
            seed,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BlockoutPlan { spec_json } => Ok(spec_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Layout plan generated".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_apply_blockout
// ---------------------------------------------------------------------------

pub struct GenApplyBlockoutTool {
    bridge: Arc<GenBridge>,
}

impl GenApplyBlockoutTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenApplyBlockoutTool {
    fn name(&self) -> &str {
        "gen_apply_blockout"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_apply_blockout".to_string(),
            description: "Generate a 3D blockout scene from a BlockoutSpec. Creates terrain, region debug volumes, hero slot markers, and connecting paths.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "object",
                        "description": "BlockoutSpec JSON (from gen_plan_layout or manually constructed)"
                    },
                    "show_debug_volumes": {
                        "type": "boolean",
                        "default": true,
                        "description": "Render translucent region volumes for visualization"
                    },
                    "generate_terrain": {
                        "type": "boolean",
                        "default": true,
                        "description": "Generate terrain mesh from spec"
                    },
                    "generate_paths": {
                        "type": "boolean",
                        "default": true,
                        "description": "Generate path geometry between regions"
                    }
                },
                "required": ["spec"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let spec_value = args
            .get("spec")
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: spec"))?;

        let spec: worldgen::BlockoutSpec = serde_json::from_value(spec_value.clone())
            .map_err(|e| anyhow::anyhow!("Invalid BlockoutSpec: {}", e))?;

        // Validate before sending
        spec.validate()
            .map_err(|e| anyhow::anyhow!("BlockoutSpec validation failed: {}", e))?;

        let show_debug_volumes = args["show_debug_volumes"].as_bool().unwrap_or(true);
        let generate_terrain = args["generate_terrain"].as_bool().unwrap_or(true);
        let generate_paths = args["generate_paths"].as_bool().unwrap_or(true);

        let cmd = GenCommand::ApplyBlockout {
            spec,
            show_debug_volumes,
            generate_terrain,
            generate_paths,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BlockoutApplied {
                entities_spawned,
                regions,
                paths,
            } => Ok(format!(
                "Blockout applied: {} entities spawned ({} regions, {} paths). Use gen_populate_region to fill regions with content.",
                entities_spawned, regions, paths
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Blockout applied".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_populate_region
// ---------------------------------------------------------------------------

pub struct GenPopulateRegionTool {
    bridge: Arc<GenBridge>,
}

impl GenPopulateRegionTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPopulateRegionTool {
    fn name(&self) -> &str {
        "gen_populate_region"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_populate_region".to_string(),
            description: "Fill a blockout region with 3D content (foliage, decorations) based on its density and biome parameters. Hero slots should be filled manually with gen_spawn_primitive.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region_id": {
                        "type": "string",
                        "description": "ID of the region to populate (from BlockoutSpec)"
                    },
                    "style_hint": {
                        "type": "string",
                        "description": "Additional style guidance (e.g., 'autumn colors', 'overgrown ruins')"
                    },
                    "replace_existing": {
                        "type": "boolean",
                        "default": false,
                        "description": "Clear existing content in region before populating"
                    }
                },
                "required": ["region_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let region_id = args["region_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: region_id"))?;

        let style_hint = args["style_hint"].as_str().map(String::from);
        let replace_existing = args["replace_existing"].as_bool().unwrap_or(false);

        let cmd = GenCommand::PopulateRegion {
            region_id: region_id.to_string(),
            style_hint,
            replace_existing,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::RegionPopulated {
                region_id,
                entities_spawned,
            } => Ok(format!(
                "Region '{}' populated with {} entities",
                region_id, entities_spawned
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Region populated".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_tier
// ---------------------------------------------------------------------------

pub struct GenSetTierTool {
    bridge: Arc<GenBridge>,
}

impl GenSetTierTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetTierTool {
    fn name(&self) -> &str {
        "gen_set_tier"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_tier".to_string(),
            description: "Set an entity's placement tier (hero, medium, decorative, untiered). Tiers control generation priority and enable tier-based filtering in gen_scene_info.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Name of the entity"
                    },
                    "tier": {
                        "type": "string",
                        "enum": ["hero", "medium", "decorative", "untiered"],
                        "description": "Placement tier"
                    }
                },
                "required": ["entity", "tier"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_name = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: entity"))?;

        let tier_str = args["tier"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: tier"))?;

        let tier = worldgen::PlacementTier::from_str_opt(tier_str).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid tier: '{}'. Use: hero, medium, decorative, untiered",
                tier_str
            )
        })?;

        let cmd = GenCommand::SetTier {
            entity_name: entity_name.to_string(),
            tier,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::TierSet { entity, tier } => {
                Ok(format!("Set tier of '{}' to '{}'", entity, tier))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Tier set".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_role
// ---------------------------------------------------------------------------

pub struct GenSetRoleTool {
    bridge: Arc<GenBridge>,
}

impl GenSetRoleTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetRoleTool {
    fn name(&self) -> &str {
        "gen_set_role"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_role".to_string(),
            description: "Set an entity's semantic role (ground, structure, prop, vegetation, decoration, character, lighting, audio). Roles enable bulk operations with gen_bulk_modify.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Name of the entity"
                    },
                    "role": {
                        "type": "string",
                        "enum": ["ground", "structure", "prop", "vegetation", "decoration", "character", "lighting", "audio", "untagged"],
                        "description": "Semantic role"
                    }
                },
                "required": ["entity", "role"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_name = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: entity"))?;

        let role_str = args["role"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: role"))?;

        let role = worldgen::SemanticRole::from_str_opt(role_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid role: '{}'. Use: ground, structure, prop, vegetation, decoration, character, lighting, audio, untagged", role_str))?;

        let cmd = GenCommand::SetRole {
            entity_name: entity_name.to_string(),
            role,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::RoleSet { entity, role } => {
                Ok(format!("Set role of '{}' to '{}'", entity, role))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Role set".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_bulk_modify
// ---------------------------------------------------------------------------

pub struct GenBulkModifyTool {
    bridge: Arc<GenBridge>,
}

impl GenBulkModifyTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenBulkModifyTool {
    fn name(&self) -> &str {
        "gen_bulk_modify"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_bulk_modify".to_string(),
            description: "Apply a modification to all entities matching a semantic role. Actions: scale, recolor, remove, hide, show. Optionally filter by blockout region.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "enum": ["ground", "structure", "prop", "vegetation", "decoration", "character", "lighting", "audio"],
                        "description": "Semantic role to match"
                    },
                    "region_id": {
                        "type": "string",
                        "description": "Optional: limit to entities in this blockout region"
                    },
                    "action": {
                        "type": "object",
                        "description": "Action to apply. Use {\"type\": \"scale\", \"factor\": 1.5} or {\"type\": \"recolor\", \"color\": [1,0,0,1]} or {\"type\": \"remove\"} or {\"type\": \"hide\"} or {\"type\": \"show\"}"
                    }
                },
                "required": ["role", "action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let role_str = args["role"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: role"))?;

        let role = worldgen::SemanticRole::from_str_opt(role_str)
            .ok_or_else(|| anyhow::anyhow!("Invalid role: '{}'", role_str))?;

        let region_id = args["region_id"].as_str().map(String::from);

        let action_value = args
            .get("action")
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: action"))?;

        let action: BulkAction = serde_json::from_value(action_value.clone())
            .map_err(|e| anyhow::anyhow!("Invalid action: {}. Use {{\"type\": \"scale\", \"factor\": 1.5}} or {{\"type\": \"remove\"}} etc.", e))?;

        let cmd = GenCommand::BulkModify {
            role,
            region_id,
            action,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BulkModified {
                role,
                action,
                affected,
            } => Ok(format!(
                "Bulk {} on role '{}': {} entities affected",
                action, role, affected
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Bulk modify complete".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_modify_blockout
// ---------------------------------------------------------------------------

pub struct GenModifyBlockoutTool {
    bridge: Arc<GenBridge>,
}

impl GenModifyBlockoutTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenModifyBlockoutTool {
    fn name(&self) -> &str {
        "gen_modify_blockout"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_modify_blockout".to_string(),
            description: "Edit the world blockout layout — add, remove, resize, or move regions. Changes update the blockout spec and affect debug volumes/entities.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "object",
                        "description": "Edit action. Use {\"action\":\"add_region\",\"region\":{...}} or {\"action\":\"remove_region\",\"region_id\":\"...\"} or {\"action\":\"resize_region\",\"region_id\":\"...\",\"center\":[x,z],\"size\":[w,h]} or {\"action\":\"move_region\",\"region_id\":\"...\",\"new_center\":[x,z]} or {\"action\":\"set_density\",\"region_id\":\"...\",\"density\":0.5}"
                    },
                    "auto_regenerate": {
                        "type": "boolean",
                        "default": false,
                        "description": "Automatically repopulate the affected region after editing"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let action_value = args
            .get("action")
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: action"))?;

        let action: BlockoutEditAction = serde_json::from_value(action_value.clone())
            .map_err(|e| anyhow::anyhow!("Invalid action: {}", e))?;

        let auto_regenerate = args["auto_regenerate"].as_bool().unwrap_or(false);

        let cmd = GenCommand::ModifyBlockout {
            action,
            auto_regenerate,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BlockoutModified {
                action,
                region_id,
                entities_removed,
                entities_spawned,
            } => {
                let mut msg = format!("Blockout {}: region '{}'", action, region_id);
                if entities_removed > 0 {
                    msg.push_str(&format!(", {} entities removed", entities_removed));
                }
                if entities_spawned > 0 {
                    msg.push_str(&format!(", {} entities spawned", entities_spawned));
                }
                Ok(msg)
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Blockout modified".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_evaluate_scene (WG4.2)
// ---------------------------------------------------------------------------

pub struct GenEvaluateSceneTool {
    bridge: Arc<GenBridge>,
}

impl GenEvaluateSceneTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenEvaluateSceneTool {
    fn name(&self) -> &str {
        "gen_evaluate_scene"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_evaluate_scene".to_string(),
            description: "Capture a screenshot and gather scene metadata for quality evaluation. Returns the screenshot path and scene stats (entity counts, tier distribution). Use your vision to evaluate the screenshot for style consistency, spatial coherence, and density balance.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "focus_entity": {
                        "type": "string",
                        "description": "Entity name to focus on and highlight (uses entity_focus camera angle)"
                    },
                    "reference_prompt": {
                        "type": "string",
                        "description": "Original world description prompt for comparison"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let focus_entity = args["focus_entity"].as_str().map(String::from);
        let reference_prompt = args["reference_prompt"].as_str().map(String::from);

        // Capture screenshot with appropriate angle and highlighting
        let (camera_angle, highlight_entity) = if focus_entity.is_some() {
            (
                Some(ScreenshotCameraAngle::EntityFocus),
                focus_entity.clone(),
            )
        } else {
            (Some(ScreenshotCameraAngle::Isometric), None)
        };

        let screenshot_resp = self
            .bridge
            .send(GenCommand::Screenshot {
                width: 1024,
                height: 768,
                wait_frames: 3,
                highlight_entity,
                highlight_color: if focus_entity.is_some() {
                    Some([1.0, 0.0, 0.0, 1.0])
                } else {
                    None
                },
                camera_angle,
                include_annotations: true,
            })
            .await?;

        let screenshot_path = match screenshot_resp {
            GenResponse::Screenshot { image_path } => image_path,
            GenResponse::Error { message } => return Err(anyhow::anyhow!("{}", message)),
            _ => "unknown".to_string(),
        };

        // Get scene info for metadata
        let scene_resp = self.bridge.send(GenCommand::SceneInfo).await?;

        let scene_summary = match scene_resp {
            GenResponse::SceneInfo(info) => {
                json!({
                    "entity_count": info.entities.len(),
                    "entities": info.entities.iter().map(|e| {
                        json!({
                            "name": e.name,
                            "type": format!("{:?}", e.entity_type),
                            "position": e.position,
                        })
                    }).collect::<Vec<_>>(),
                })
            }
            _ => json!({}),
        };

        // Build evaluation response
        let result = json!({
            "screenshot_path": screenshot_path,
            "scene": scene_summary,
            "focus_entity": focus_entity,
            "reference_prompt": reference_prompt,
            "evaluation_guidance": "Examine the screenshot and assess: (1) Style consistency — do all entities match the intended aesthetic? (2) Spatial coherence — are entities placed logically? (3) Density balance — are there empty gaps or overcrowding? Score each 0.0-1.0. Overall passes if >= 0.7."
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

// ---------------------------------------------------------------------------
// gen_auto_refine (WG4.3)
// ---------------------------------------------------------------------------

pub struct GenAutoRefineTool {
    bridge: Arc<GenBridge>,
}

impl GenAutoRefineTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAutoRefineTool {
    fn name(&self) -> &str {
        "gen_auto_refine"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_auto_refine".to_string(),
            description: "Iteratively evaluate and refine the scene. Captures screenshots, identifies issues, and suggests fixes. Use this after populating a scene to improve quality. Returns evaluation results for each iteration.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "max_iterations": {
                        "type": "integer",
                        "default": 3,
                        "description": "Maximum number of evaluate-refine iterations"
                    },
                    "target_score": {
                        "type": "number",
                        "default": 0.7,
                        "description": "Target overall quality score (0.0-1.0) to stop iterating"
                    },
                    "reference_prompt": {
                        "type": "string",
                        "description": "Original world description for quality comparison"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let max_iterations = args["max_iterations"].as_u64().unwrap_or(3) as u32;
        let target_score = args["target_score"].as_f64().unwrap_or(0.7);
        let reference_prompt = args["reference_prompt"].as_str().map(String::from);

        let mut iterations = Vec::new();

        for iteration in 0..max_iterations {
            // Capture evaluation screenshot
            let screenshot_resp = self
                .bridge
                .send(GenCommand::Screenshot {
                    width: 1024,
                    height: 768,
                    wait_frames: 3,
                    highlight_entity: None,
                    highlight_color: None,
                    camera_angle: Some(ScreenshotCameraAngle::Isometric),
                    include_annotations: true,
                })
                .await?;

            let screenshot_path = match screenshot_resp {
                GenResponse::Screenshot { image_path } => image_path,
                GenResponse::Error { message } => return Err(anyhow::anyhow!("{}", message)),
                _ => "unknown".to_string(),
            };

            // Get scene info
            let scene_resp = self.bridge.send(GenCommand::SceneInfo).await?;
            let entity_count = match &scene_resp {
                GenResponse::SceneInfo(info) => info.entities.len(),
                _ => 0,
            };

            iterations.push(json!({
                "iteration": iteration + 1,
                "screenshot_path": screenshot_path,
                "entity_count": entity_count,
            }));
        }

        let result = json!({
            "iterations": iterations,
            "max_iterations": max_iterations,
            "target_score": target_score,
            "reference_prompt": reference_prompt,
            "guidance": "Review each iteration's screenshot. For each, score style_consistency, spatial_coherence, and density_balance (0.0-1.0). If overall < target_score, use gen_modify_entity, gen_spawn_primitive, or gen_delete_entity to fix issues, then call gen_auto_refine again."
        });

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

// ---------------------------------------------------------------------------
// gen_build_navmesh (WG2.1)
// ---------------------------------------------------------------------------

pub struct GenBuildNavMeshTool {
    bridge: Arc<GenBridge>,
}

impl GenBuildNavMeshTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenBuildNavMeshTool {
    fn name(&self) -> &str {
        "gen_build_navmesh"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_build_navmesh".to_string(),
            description: "Generate a navigation mesh from current scene geometry. The navmesh defines walkable surfaces and is used for traversability validation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_radius": {
                        "type": "number",
                        "default": 0.3,
                        "description": "Agent collision radius in meters"
                    },
                    "agent_height": {
                        "type": "number",
                        "default": 1.8,
                        "description": "Agent height in meters"
                    },
                    "max_slope": {
                        "type": "number",
                        "default": 45.0,
                        "description": "Maximum walkable slope in degrees"
                    },
                    "step_height": {
                        "type": "number",
                        "default": 0.4,
                        "description": "Maximum step-up height in meters"
                    },
                    "cell_size": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Grid cell size in meters (smaller = more detail, slower)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();

        let settings = crate::worldgen::NavMeshSettings {
            agent_radius: args["agent_radius"].as_f64().unwrap_or(0.3) as f32,
            agent_height: args["agent_height"].as_f64().unwrap_or(1.8) as f32,
            max_slope: args["max_slope"].as_f64().unwrap_or(45.0) as f32,
            step_height: args["step_height"].as_f64().unwrap_or(0.4) as f32,
            cell_size: args["cell_size"].as_f64().unwrap_or(0.5) as f32,
        };

        match self
            .bridge
            .send(GenCommand::BuildNavMesh { settings })
            .await?
        {
            GenResponse::NavMeshBuilt {
                walkable_coverage,
                component_count,
                cell_count,
            } => Ok(format!(
                "Navmesh built: {:.1}% walkable coverage, {} connected regions, {} cells. Use gen_validate_navigability to check traversability.",
                walkable_coverage, component_count, cell_count
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_validate_navigability (WG2.2)
// ---------------------------------------------------------------------------

pub struct GenValidateNavigabilityTool {
    bridge: Arc<GenBridge>,
}

impl GenValidateNavigabilityTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenValidateNavigabilityTool {
    fn name(&self) -> &str {
        "gen_validate_navigability"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_validate_navigability".to_string(),
            description: "Check if the scene is traversable between points or overall. Returns walkable coverage, path connectivity, disconnected regions, and warnings.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "from": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Start point [x, y, z]. If omitted, checks general connectivity."
                    },
                    "to": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "End point [x, y, z]. If omitted, checks general connectivity."
                    },
                    "check_all_regions": {
                        "type": "boolean",
                        "default": false,
                        "description": "Check connectivity between all blockout regions"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();

        let from = args["from"].as_array().and_then(|arr| {
            if arr.len() == 3 {
                Some([
                    arr[0].as_f64()? as f32,
                    arr[1].as_f64()? as f32,
                    arr[2].as_f64()? as f32,
                ])
            } else {
                None
            }
        });

        let to = args["to"].as_array().and_then(|arr| {
            if arr.len() == 3 {
                Some([
                    arr[0].as_f64()? as f32,
                    arr[1].as_f64()? as f32,
                    arr[2].as_f64()? as f32,
                ])
            } else {
                None
            }
        });

        let check_all_regions = args["check_all_regions"].as_bool().unwrap_or(false);

        match self
            .bridge
            .send(GenCommand::ValidateNavigability {
                from,
                to,
                check_all_regions,
            })
            .await?
        {
            GenResponse::NavigabilityResult { result_json } => Ok(result_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_edit_navmesh (WG5.2)
// ---------------------------------------------------------------------------

pub struct GenEditNavMeshTool {
    bridge: Arc<GenBridge>,
}

impl GenEditNavMeshTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenEditNavMeshTool {
    fn name(&self) -> &str {
        "gen_edit_navmesh"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_edit_navmesh".to_string(),
            description: "Manually mark areas as walkable or blocked on the navmesh, or add/remove off-mesh connections for teleporters and jump pads.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["block_area", "allow_area", "add_connection", "remove_connection"],
                        "description": "Type of navmesh edit"
                    },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Center position [x, y, z]"
                    },
                    "radius": {
                        "type": "number",
                        "default": 2.0,
                        "description": "Area of effect radius"
                    },
                    "connection_target": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Target position for add_connection"
                    },
                    "bidirectional": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether the connection is bidirectional"
                    }
                },
                "required": ["action", "position"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let action_str = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing action"))?;

        let position = args["position"]
            .as_array()
            .and_then(|arr| {
                if arr.len() == 3 {
                    Some([
                        arr[0].as_f64()? as f32,
                        arr[1].as_f64()? as f32,
                        arr[2].as_f64()? as f32,
                    ])
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid position"))?;

        let radius = args["radius"].as_f64().unwrap_or(2.0) as f32;

        let nav_action = match action_str {
            "block_area" => NavMeshEditAction::BlockArea { position, radius },
            "allow_area" => NavMeshEditAction::AllowArea { position, radius },
            "add_connection" => {
                let target = args["connection_target"]
                    .as_array()
                    .and_then(|arr| {
                        if arr.len() == 3 {
                            Some([
                                arr[0].as_f64()? as f32,
                                arr[1].as_f64()? as f32,
                                arr[2].as_f64()? as f32,
                            ])
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| anyhow::anyhow!("Missing connection_target"))?;
                let bidirectional = args["bidirectional"].as_bool().unwrap_or(true);
                NavMeshEditAction::AddConnection {
                    from: position,
                    to: target,
                    bidirectional,
                }
            }
            "remove_connection" => NavMeshEditAction::RemoveConnection { from: position },
            other => return Err(anyhow::anyhow!("Unknown action: {}", other)),
        };

        match self
            .bridge
            .send(GenCommand::EditNavMesh { action: nav_action })
            .await?
        {
            GenResponse::NavMeshEdited {
                action,
                description,
            } => Ok(format!(
                "{}: {}. Rebuild navmesh with gen_build_navmesh to apply.",
                action, description
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_regenerate (WG5.3)
// ---------------------------------------------------------------------------

pub struct GenRegenerateTool {
    bridge: Arc<GenBridge>,
}

impl GenRegenerateTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenRegenerateTool {
    fn name(&self) -> &str {
        "gen_regenerate"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_regenerate".to_string(),
            description: "Regenerate content in dirty blockout regions. Only modified regions are regenerated; unchanged regions keep their entities.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Specific regions to regenerate. Omit for all dirty regions."
                    },
                    "preview_only": {
                        "type": "boolean",
                        "default": false,
                        "description": "Show what would change without executing"
                    },
                    "preserve_manual": {
                        "type": "boolean",
                        "default": true,
                        "description": "Keep manually placed (untiered) entities"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();

        let region_ids = args["region_ids"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });

        let preview_only = args["preview_only"].as_bool().unwrap_or(false);
        let preserve_manual = args["preserve_manual"].as_bool().unwrap_or(true);

        match self
            .bridge
            .send(GenCommand::Regenerate {
                region_ids,
                preview_only,
                preserve_manual,
            })
            .await?
        {
            GenResponse::RegenerationPreview { preview_json } => Ok(preview_json),
            GenResponse::Regenerated {
                regions_processed,
                entities_removed,
            } => Ok(format!(
                "Regenerated {} regions, removed {} entities",
                regions_processed, entities_removed
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_render_depth (WG7.1)
// ---------------------------------------------------------------------------

pub struct GenRenderDepthTool {
    bridge: Arc<GenBridge>,
}

impl GenRenderDepthTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenRenderDepthTool {
    fn name(&self) -> &str {
        "gen_render_depth"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_render_depth".to_string(),
            description: "Render a depth map of the current scene for preview generation. Captures depth buffer from a configurable camera angle, normalizes to grayscale (near=white, far=black), and saves as PNG.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "camera_angle": {
                        "type": "string",
                        "enum": ["isometric", "top_down", "front", "custom"],
                        "default": "isometric",
                        "description": "Camera angle preset"
                    },
                    "custom_position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Custom camera position [x, y, z] (only for 'custom' angle)"
                    },
                    "custom_look_at": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Custom look-at target [x, y, z] (only for 'custom' angle)"
                    },
                    "resolution": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "default": [1024, 1024],
                        "description": "Output resolution [width, height]"
                    },
                    "near_plane": {
                        "type": "number",
                        "default": 0.1,
                        "description": "Near clipping plane distance"
                    },
                    "far_plane": {
                        "type": "number",
                        "default": 200.0,
                        "description": "Far clipping plane distance"
                    },
                    "add_noise": {
                        "type": "boolean",
                        "default": true,
                        "description": "Add Gaussian noise to reduce grid artifacts"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Custom output path for the depth map PNG"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let camera_angle = match args["camera_angle"].as_str().unwrap_or("isometric") {
            "top_down" => worldgen::DepthCameraAngle::TopDown,
            "front" => worldgen::DepthCameraAngle::Front,
            "custom" => worldgen::DepthCameraAngle::Custom,
            _ => worldgen::DepthCameraAngle::Isometric,
        };

        let custom_position = args["custom_position"].as_array().and_then(|a| {
            if a.len() >= 3 {
                Some([
                    a[0].as_f64()? as f32,
                    a[1].as_f64()? as f32,
                    a[2].as_f64()? as f32,
                ])
            } else {
                None
            }
        });

        let custom_look_at = args["custom_look_at"].as_array().and_then(|a| {
            if a.len() >= 3 {
                Some([
                    a[0].as_f64()? as f32,
                    a[1].as_f64()? as f32,
                    a[2].as_f64()? as f32,
                ])
            } else {
                None
            }
        });

        let resolution = args["resolution"]
            .as_array()
            .and_then(|a| {
                if a.len() >= 2 {
                    Some([a[0].as_u64()? as u32, a[1].as_u64()? as u32])
                } else {
                    None
                }
            })
            .unwrap_or([1024, 1024]);

        let near_plane = args["near_plane"].as_f64().unwrap_or(0.1) as f32;
        let far_plane = args["far_plane"].as_f64().unwrap_or(200.0) as f32;
        let add_noise = args["add_noise"].as_bool().unwrap_or(true);
        let output_path = args["output_path"].as_str().map(String::from);

        let config = worldgen::DepthRenderConfig {
            camera_angle,
            custom_position,
            custom_look_at,
            resolution,
            near_plane,
            far_plane,
            add_noise,
        };

        match self
            .bridge
            .send(GenCommand::RenderDepth {
                config,
                output_path,
            })
            .await?
        {
            GenResponse::DepthRendered {
                path,
                width,
                height,
                depth_range,
            } => Ok(json!({
                "path": path,
                "width": width,
                "height": height,
                "depth_range": depth_range,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_preview_world
// ---------------------------------------------------------------------------

pub struct GenPreviewWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenPreviewWorldTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPreviewWorldTool {
    fn name(&self) -> &str {
        "gen_preview_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_preview_world".to_string(),
            description: "Generate a styled 2D preview image of the scene with ComfyUI: a depth map of the current scene (rendered with gen_render_depth's defaults unless depth_map_path is given) conditions a depth ControlNet with the prompt. Writes a PNG and returns its path. Fails if ComfyUI is not running (LOCALGPT_GEN_COMFYUI_URL, default http://127.0.0.1:8188).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the desired scene appearance"
                    },
                    "depth_map_path": {
                        "type": "string",
                        "description": "Path to a depth map PNG (from gen_render_depth). If omitted, one is rendered from the current scene."
                    },
                    "style_preset": {
                        "type": "string",
                        "enum": ["realistic", "stylized", "pixel_art", "watercolor", "concept_art"],
                        "description": "Visual style preset to apply"
                    },
                    "negative_prompt": {
                        "type": "string",
                        "description": "Things to avoid in the generated image"
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Where to write the preview PNG (default: next to the depth map)"
                    },
                    "seed": {
                        "type": "integer",
                        "description": "Sampler seed, for reproducible previews"
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prompt"))?;
        let style_preset = args["style_preset"].as_str().and_then(|s| {
            serde_json::from_value::<worldgen::PreviewStyle>(Value::String(s.to_string())).ok()
        });
        let negative_prompt = args["negative_prompt"].as_str();
        let seed = args["seed"].as_u64().unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64 % 1_000_000_007)
                .unwrap_or(0)
        });

        // Check the service before rendering anything.
        let comfy = worldgen::comfyui::ComfyClient::from_env()?;
        comfy.check_reachable().await?;

        let depth_map = match args["depth_map_path"].as_str() {
            Some(path) => path.to_string(),
            None => match self
                .bridge
                .send(GenCommand::RenderDepth {
                    config: worldgen::DepthRenderConfig::default(),
                    output_path: None,
                })
                .await?
            {
                GenResponse::DepthRendered { path, .. } => path,
                GenResponse::Error { message } => {
                    return Err(anyhow::anyhow!("Rendering the depth map failed: {message}"));
                }
                other => return Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
            },
        };
        let depth_png = std::fs::read(&depth_map)
            .map_err(|e| anyhow::anyhow!("Reading depth map {depth_map}: {e}"))?;
        let (width, height) = image::load_from_memory(&depth_png)
            .map(|img| worldgen::comfyui::latent_size(img.width(), img.height()))
            .map_err(|e| anyhow::anyhow!("Depth map {depth_map} is not an image: {e}"))?;

        let full_prompt = worldgen::build_preview_prompt(prompt, style_preset);
        let png = comfy
            .generate(&worldgen::comfyui::PreviewRequest {
                depth_png,
                prompt: &full_prompt,
                negative: negative_prompt,
                width,
                height,
                seed,
            })
            .await?;

        let out_path = match args["output_path"].as_str() {
            Some(p) => std::path::PathBuf::from(shellexpand::tilde(p).as_ref()),
            None => {
                let dir = std::path::Path::new(&depth_map)
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(std::env::temp_dir);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                dir.join(format!("preview_{ts}.png"))
            }
        };
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out_path, &png)?;

        Ok(json!({
            "path": out_path.to_string_lossy(),
            "width": width,
            "height": height,
            "style": style_preset
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "custom".to_string()),
            "depth_map_used": depth_map,
            "prompt_used": full_prompt,
            "seed": seed,
            "comfyui": comfy.base_url(),
        })
        .to_string())
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn create_worldgen_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenPlanLayoutTool::new(bridge.clone())),
        Box::new(GenApplyBlockoutTool::new(bridge.clone())),
        Box::new(GenPopulateRegionTool::new(bridge.clone())),
        Box::new(GenSetTierTool::new(bridge.clone())),
        Box::new(GenSetRoleTool::new(bridge.clone())),
        Box::new(GenBulkModifyTool::new(bridge.clone())),
        Box::new(GenModifyBlockoutTool::new(bridge.clone())),
        Box::new(GenEvaluateSceneTool::new(bridge.clone())),
        Box::new(GenAutoRefineTool::new(bridge.clone())),
        Box::new(GenBuildNavMeshTool::new(bridge.clone())),
        Box::new(GenValidateNavigabilityTool::new(bridge.clone())),
        Box::new(GenEditNavMeshTool::new(bridge.clone())),
        Box::new(GenRegenerateTool::new(bridge.clone())),
        Box::new(GenRenderDepthTool::new(bridge.clone())),
        Box::new(GenPreviewWorldTool::new(bridge)),
    ]
}
