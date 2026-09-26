//! Gen tools — implements the agent `Tool` trait for each Gen command.
//!
//! Each tool sends a `GenCommand` through the `GenBridge` and formats
//! the `GenResponse` as a string for the LLM.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use super::GenBridge;
use super::commands::*;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

/// Create all gen tools backed by the given bridge.
pub fn create_gen_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenSceneInfoTool::new(bridge.clone())),
        Box::new(GenScreenshotTool::new(bridge.clone())),
        Box::new(GenEntityInfoTool::new(bridge.clone())),
        Box::new(GenSpawnPrimitiveTool::new(bridge.clone())),
        Box::new(GenModifyEntityTool::new(bridge.clone())),
        Box::new(GenDeleteEntityTool::new(bridge.clone())),
        // Batch tools (more efficient than multiple single calls)
        Box::new(GenSpawnBatchTool::new(bridge.clone())),
        Box::new(GenModifyBatchTool::new(bridge.clone())),
        Box::new(GenDeleteBatchTool::new(bridge.clone())),
        Box::new(GenSetCameraTool::new(bridge.clone())),
        Box::new(GenSetLightTool::new(bridge.clone())),
        Box::new(GenSetEnvironmentTool::new(bridge.clone())),
        Box::new(GenSpawnMeshTool::new(bridge.clone())),
        Box::new(GenLoadGltfTool::new(bridge.clone())),
        Box::new(GenExportScreenshotTool::new(bridge.clone())),
        Box::new(GenExportGltfTool::new(bridge.clone())),
        // Audio tools
        Box::new(GenSetAmbienceTool::new(bridge.clone())),
        Box::new(GenAudioEmitterTool::new(bridge.clone())),
        Box::new(GenModifyAudioTool::new(bridge.clone())),
        Box::new(GenAudioInfoTool::new(bridge.clone())),
        // Behavior tools
        Box::new(GenAddBehaviorTool::new(bridge.clone())),
        Box::new(GenRemoveBehaviorTool::new(bridge.clone())),
        Box::new(GenListBehaviorsTool::new(bridge.clone())),
        Box::new(GenPauseBehaviorsTool::new(bridge.clone())),
        // World tools
        Box::new(GenSaveWorldTool::new(bridge.clone())),
        Box::new(GenLoadWorldTool::new(bridge.clone())),
        Box::new(GenExportWorldTool::new(bridge.clone())),
        Box::new(GenExportHtmlTool::new(bridge.clone())),
        Box::new(GenForkWorldTool::new(bridge.clone())),
        // Scene management
        Box::new(GenClearSceneTool::new(bridge.clone())),
        // Undo/Redo
        Box::new(GenUndoTool::new(bridge.clone())),
        Box::new(GenRedoTool::new(bridge.clone())),
        Box::new(GenUndoInfoTool::new(bridge.clone())),
        // Physics tools (P5)
        Box::new(GenSetPhysicsTool::new(bridge.clone())),
        Box::new(GenAddColliderTool::new(bridge.clone())),
        Box::new(GenAddJointTool::new(bridge.clone())),
        Box::new(GenAddForceTool::new(bridge.clone())),
        Box::new(GenSetGravityTool::new(bridge)),
    ]
}

// ===========================================================================
// gen_scene_info
// ===========================================================================

struct GenSceneInfoTool {
    bridge: Arc<GenBridge>,
}

impl GenSceneInfoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSceneInfoTool {
    fn name(&self) -> &str {
        "gen_scene_info"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_scene_info".into(),
            description:
                "Get complete scene hierarchy with all entities, transforms, and materials.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::SceneInfo).await? {
            GenResponse::SceneInfo(data) => Ok(serde_json::to_string_pretty(&data)?),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_screenshot
// ===========================================================================

struct GenScreenshotTool {
    bridge: Arc<GenBridge>,
}

impl GenScreenshotTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenScreenshotTool {
    fn name(&self) -> &str {
        "gen_screenshot"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_screenshot".into(),
            description: "Capture a screenshot of the current scene. Supports entity highlighting, camera angle presets, and annotation overlays for visual evaluation.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "width": {
                        "type": "integer",
                        "default": 800,
                        "description": "Image width in pixels"
                    },
                    "height": {
                        "type": "integer",
                        "default": 600,
                        "description": "Image height in pixels"
                    },
                    "wait_frames": {
                        "type": "integer",
                        "default": 3,
                        "description": "Frames to wait before capture for render pipeline to process new geometry"
                    },
                    "highlight_entity": {
                        "type": "string",
                        "description": "Entity name to highlight with a distinct emissive glow in the screenshot"
                    },
                    "highlight_color": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 4,
                        "maxItems": 4,
                        "default": [1.0, 0.0, 0.0, 1.0],
                        "description": "Highlight color as [r, g, b, a] (default: red)"
                    },
                    "camera_angle": {
                        "type": "string",
                        "enum": ["current", "top_down", "isometric", "front", "entity_focus"],
                        "default": "current",
                        "description": "Camera angle preset: current (default), top_down, isometric, front, or entity_focus (frames the highlighted entity)"
                    },
                    "include_annotations": {
                        "type": "boolean",
                        "default": false,
                        "description": "Overlay entity names as labels in the screenshot"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let width = args["width"].as_u64().unwrap_or(800) as u32;
        let height = args["height"].as_u64().unwrap_or(600) as u32;
        let wait_frames = args["wait_frames"].as_u64().unwrap_or(3) as u32;
        let highlight_entity = args["highlight_entity"].as_str().map(String::from);
        let highlight_color = args["highlight_color"].as_array().and_then(|arr| {
            if arr.len() == 4 {
                Some([
                    arr[0].as_f64()? as f32,
                    arr[1].as_f64()? as f32,
                    arr[2].as_f64()? as f32,
                    arr[3].as_f64()? as f32,
                ])
            } else {
                None
            }
        });
        let camera_angle = args["camera_angle"]
            .as_str()
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok());
        let include_annotations = args["include_annotations"].as_bool().unwrap_or(false);

        match self
            .bridge
            .send(GenCommand::Screenshot {
                width,
                height,
                wait_frames,
                highlight_entity,
                highlight_color,
                camera_angle,
                include_annotations,
            })
            .await?
        {
            GenResponse::Screenshot { image_path } => {
                Ok(format!("Screenshot saved to: {}", image_path))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_entity_info
// ===========================================================================

struct GenEntityInfoTool {
    bridge: Arc<GenBridge>,
}

impl GenEntityInfoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenEntityInfoTool {
    fn name(&self) -> &str {
        "gen_entity_info"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_entity_info".into(),
            description: "Get detailed information about a specific entity by name.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Entity name to inspect"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing name"))?
            .to_string();

        match self.bridge.send(GenCommand::EntityInfo { name }).await? {
            GenResponse::EntityInfo(data) => Ok(serde_json::to_string_pretty(&data)?),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_spawn_primitive
// ===========================================================================

struct GenSpawnPrimitiveTool {
    bridge: Arc<GenBridge>,
}

impl GenSpawnPrimitiveTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSpawnPrimitiveTool {
    fn name(&self) -> &str {
        "gen_spawn_primitive"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_spawn_primitive".into(),
            description: "Spawn a 3D primitive shape with material and transform. Creates a fully visible object — no additional components needed.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique name for this entity (e.g., 'red_cube', 'table_leg_1')"
                    },
                    "shape": {
                        "type": "string",
                        "enum": ["Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane", "Pyramid", "Tetrahedron", "Icosahedron", "Wedge"],
                        "description": "Primitive shape type"
                    },
                    "dimensions": {
                        "type": "object",
                        "description": "Shape-specific dimensions. Cuboid: {x,y,z}. Sphere: {radius}. Cylinder: {radius, height}. Cone: {radius, height}. Torus: {major_radius, minor_radius}. Pyramid: {base_x, base_z, height}. Tetrahedron: {radius}. Icosahedron: {radius}. Wedge: {x, y, z}."
                    },
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0],
                        "description": "Position [x, y, z]"
                    },
                    "rotation_degrees": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0],
                        "description": "Euler angles in degrees (pitch, yaw, roll)"
                    },
                    "scale": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [1, 1, 1],
                        "description": "Scale [x, y, z]"
                    },
                    "color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0.8, 0.8, 0.8, 1.0],
                        "description": "RGBA color, 0.0-1.0"
                    },
                    "metallic": {
                        "type": "number",
                        "default": 0.0,
                        "minimum": 0,
                        "maximum": 1
                    },
                    "roughness": {
                        "type": "number",
                        "default": 0.5,
                        "minimum": 0,
                        "maximum": 1
                    },
                    "emissive": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0, 0],
                        "description": "Emissive RGBA color for glowing objects"
                    },
                    "alpha_mode": {
                        "type": "string",
                        "description": "Alpha/transparency mode: 'opaque' (default), 'blend' (transparent), 'mask:0.5' (cutout), 'add', 'multiply'"
                    },
                    "unlit": {
                        "type": "boolean",
                        "description": "If true, ignore lighting and render at full brightness. Good for UI planes and glow effects."
                    },
                    "parent": {
                        "type": "string",
                        "description": "Name of parent entity for hierarchy; position, rotation and scale are then relative to the parent. Omit for root-level."
                    }
                },
                "required": ["name", "shape"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let cmd = SpawnPrimitiveCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            shape: serde_json::from_value(args["shape"].clone())?,
            dimensions: args
                .get("dimensions")
                .and_then(|v| v.as_object())
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f as f32)))
                        .collect()
                })
                .unwrap_or_default(),
            position: parse_f32_array(&args["position"], [0.0, 0.0, 0.0]),
            rotation_degrees: parse_f32_array(&args["rotation_degrees"], [0.0, 0.0, 0.0]),
            scale: parse_f32_array(&args["scale"], [1.0, 1.0, 1.0]),
            color: parse_f32_4(&args["color"], [0.8, 0.8, 0.8, 1.0]),
            metallic: args["metallic"].as_f64().unwrap_or(0.0) as f32,
            roughness: args["roughness"].as_f64().unwrap_or(0.5) as f32,
            emissive: parse_f32_4(&args["emissive"], [0.0, 0.0, 0.0, 0.0]),
            alpha_mode: args
                .get("alpha_mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            unlit: args.get("unlit").and_then(|v| v.as_bool()),
            parent: args["parent"].as_str().map(|s| s.to_string()),
        };

        match self.bridge.send(GenCommand::SpawnPrimitive(cmd)).await? {
            GenResponse::Spawned { name, entity_id } => {
                Ok(format!("Spawned '{}' (entity_id: {})", name, entity_id))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_modify_entity
// ===========================================================================

struct GenModifyEntityTool {
    bridge: Arc<GenBridge>,
}

impl GenModifyEntityTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenModifyEntityTool {
    fn name(&self) -> &str {
        "gen_modify_entity"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_modify_entity".into(),
            description: "Modify properties of an existing entity. Only specified fields are changed; others remain unchanged.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of entity to modify"
                    },
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "New position [x, y, z]"
                    },
                    "rotation_degrees": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "New rotation [pitch, yaw, roll] in degrees"
                    },
                    "scale": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "New scale [x, y, z]"
                    },
                    "color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "New RGBA color"
                    },
                    "metallic": {"type": "number"},
                    "roughness": {"type": "number"},
                    "emissive": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "New emissive RGBA color"
                    },
                    "alpha_mode": {
                        "type": "string",
                        "enum": ["opaque", "blend", "add", "multiply"],
                        "description": "Alpha blending mode. Use 'mask:0.5' for alpha cutoff."
                    },
                    "unlit": {
                        "type": "boolean",
                        "description": "If true, material ignores lighting (flat shaded)"
                    },
                    "double_sided": {
                        "type": "boolean",
                        "description": "If true, render both sides of faces (for thin geometry like leaves)"
                    },
                    "reflectance": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Specular reflectance (default 0.5). 0.0 = no reflection, 1.0 = mirror-like"
                    },
                    "visible": {
                        "type": "boolean",
                        "description": "Show/hide entity"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Reparent to named entity, or null to unparent"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let cmd = ModifyEntityCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            position: parse_opt_f32_array(&args["position"]),
            rotation_degrees: parse_opt_f32_array(&args["rotation_degrees"]),
            scale: parse_opt_f32_array(&args["scale"]),
            color: parse_opt_f32_4(&args["color"]),
            metallic: args["metallic"].as_f64().map(|v| v as f32),
            roughness: args["roughness"].as_f64().map(|v| v as f32),
            emissive: parse_opt_f32_4(&args["emissive"]),
            alpha_mode: args
                .get("alpha_mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            unlit: args.get("unlit").and_then(|v| v.as_bool()),
            double_sided: args.get("double_sided").and_then(|v| v.as_bool()),
            reflectance: args
                .get("reflectance")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
            visible: args["visible"].as_bool(),
            parent: if args.get("parent").is_some() {
                Some(args["parent"].as_str().map(|s| s.to_string()))
            } else {
                None
            },
        };

        match self.bridge.send(GenCommand::ModifyEntity(cmd)).await? {
            GenResponse::Modified { name } => Ok(format!("Modified '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_delete_entity
// ===========================================================================

struct GenDeleteEntityTool {
    bridge: Arc<GenBridge>,
}

impl GenDeleteEntityTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenDeleteEntityTool {
    fn name(&self) -> &str {
        "gen_delete_entity"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_delete_entity".into(),
            description: "Delete an entity and all its children from the scene.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of entity to delete"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing name"))?
            .to_string();

        match self.bridge.send(GenCommand::DeleteEntity { name }).await? {
            GenResponse::Deleted { name } => Ok(format!("Deleted '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_spawn_batch
// ===========================================================================

struct GenSpawnBatchTool {
    bridge: Arc<GenBridge>,
}

impl GenSpawnBatchTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSpawnBatchTool {
    fn name(&self) -> &str {
        "gen_spawn_batch"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_spawn_batch".into(),
            description: "Spawn multiple primitives in a single call. All-or-nothing: if any item is invalid (duplicate name, missing parent), nothing is spawned.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entities": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {
                                    "type": "string",
                                    "description": "Unique name for this entity"
                                },
                                "shape": {
                                    "type": "string",
                                    "enum": ["Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane"],
                                    "description": "Primitive shape type"
                                },
                                "dimensions": {
                                    "type": "object",
                                    "description": "Shape-specific dimensions"
                                },
                                "position": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "default": [0, 0, 0],
                                    "description": "Position [x, y, z]"
                                },
                                "rotation_degrees": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "default": [0, 0, 0],
                                    "description": "Euler angles in degrees"
                                },
                                "scale": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "default": [1, 1, 1],
                                    "description": "Scale [x, y, z]"
                                },
                                "color": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "default": [0.8, 0.8, 0.8, 1.0],
                                    "description": "RGBA color"
                                },
                                "metallic": {"type": "number", "default": 0.0},
                                "roughness": {"type": "number", "default": 0.5},
                                "emissive": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "default": [0, 0, 0, 0],
                                    "description": "Emissive RGBA color"
                                },
                                "alpha_mode": {"type": "string"},
                                "unlit": {"type": "boolean"},
                                "parent": {"type": "string"}
                            },
                            "required": ["name", "shape"]
                        },
                        "description": "Array of entity specifications (same format as gen_spawn_primitive)"
                    },
                    "expected_revision": {
                        "type": "integer",
                        "description": "Scene revision this batch was planned against (from gen_scene_info). If the scene has changed since, nothing is applied."
                    }
                },
                "required": ["entities"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let entities_val = args
            .get("entities")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid entities array"))?;

        let mut entities = Vec::with_capacity(entities_val.len());
        for entity_val in entities_val {
            let cmd = SpawnPrimitiveCmd {
                name: entity_val["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing name in entity"))?
                    .to_string(),
                shape: serde_json::from_value(entity_val["shape"].clone())?,
                dimensions: entity_val
                    .get("dimensions")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f as f32)))
                            .collect()
                    })
                    .unwrap_or_default(),
                position: parse_f32_array(&entity_val["position"], [0.0, 0.0, 0.0]),
                rotation_degrees: parse_f32_array(&entity_val["rotation_degrees"], [0.0, 0.0, 0.0]),
                scale: parse_f32_array(&entity_val["scale"], [1.0, 1.0, 1.0]),
                color: parse_f32_4(&entity_val["color"], [0.8, 0.8, 0.8, 1.0]),
                metallic: entity_val["metallic"].as_f64().unwrap_or(0.0) as f32,
                roughness: entity_val["roughness"].as_f64().unwrap_or(0.5) as f32,
                emissive: parse_f32_4(&entity_val["emissive"], [0.0, 0.0, 0.0, 0.0]),
                alpha_mode: entity_val
                    .get("alpha_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                unlit: entity_val.get("unlit").and_then(|v| v.as_bool()),
                parent: entity_val["parent"].as_str().map(|s| s.to_string()),
            };
            entities.push(cmd);
        }

        match self
            .bridge
            .send(GenCommand::SpawnBatch {
                entities,
                expected_revision: args["expected_revision"].as_u64(),
            })
            .await?
        {
            GenResponse::BatchResult { results, revision } => {
                let success_count = results.iter().filter(|r| r.starts_with("Created:")).count();
                Ok(format!(
                    "Batch spawn: {} entities created (scene revision {})\n{}",
                    success_count,
                    revision,
                    results.join("\n")
                ))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_modify_batch
// ===========================================================================

struct GenModifyBatchTool {
    bridge: Arc<GenBridge>,
}

impl GenModifyBatchTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenModifyBatchTool {
    fn name(&self) -> &str {
        "gen_modify_batch"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_modify_batch".into(),
            description: "Modify multiple entities in a single call. All-or-nothing: if any entity is missing or listed twice, nothing is modified.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entities": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {
                                    "type": "string",
                                    "description": "Name of entity to modify"
                                },
                                "position": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "description": "New position [x, y, z]"
                                },
                                "rotation_degrees": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "description": "New rotation in degrees"
                                },
                                "scale": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "description": "New scale [x, y, z]"
                                },
                                "color": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "description": "New RGBA color"
                                },
                                "metallic": {"type": "number"},
                                "roughness": {"type": "number"},
                                "emissive": {
                                    "type": "array",
                                    "items": {"type": "number"},
                                    "description": "New emissive RGBA color"
                                },
                                "alpha_mode": {"type": "string"},
                                "unlit": {"type": "boolean"},
                                "double_sided": {"type": "boolean"},
                                "reflectance": {"type": "number"},
                                "visible": {"type": "boolean"},
                                "parent": {"type": "string"}
                            },
                            "required": ["name"]
                        },
                        "description": "Array of entity modifications"
                    },
                    "expected_revision": {
                        "type": "integer",
                        "description": "Scene revision this batch was planned against (from gen_scene_info). If the scene has changed since, nothing is applied."
                    }
                },
                "required": ["entities"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let entities_val = args
            .get("entities")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid entities array"))?;

        let mut entities = Vec::with_capacity(entities_val.len());
        for entity_val in entities_val {
            let cmd = ModifyEntityCmd {
                name: entity_val["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing name in entity"))?
                    .to_string(),
                position: parse_opt_f32_array(&entity_val["position"]),
                rotation_degrees: parse_opt_f32_array(&entity_val["rotation_degrees"]),
                scale: parse_opt_f32_array(&entity_val["scale"]),
                color: parse_opt_f32_4(&entity_val["color"]),
                metallic: entity_val["metallic"].as_f64().map(|v| v as f32),
                roughness: entity_val["roughness"].as_f64().map(|v| v as f32),
                emissive: parse_opt_f32_4(&entity_val["emissive"]),
                alpha_mode: entity_val
                    .get("alpha_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                unlit: entity_val.get("unlit").and_then(|v| v.as_bool()),
                double_sided: entity_val.get("double_sided").and_then(|v| v.as_bool()),
                reflectance: entity_val
                    .get("reflectance")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32),
                visible: entity_val["visible"].as_bool(),
                parent: if entity_val.get("parent").is_some() {
                    Some(entity_val["parent"].as_str().map(|s| s.to_string()))
                } else {
                    None
                },
            };
            entities.push(cmd);
        }

        match self
            .bridge
            .send(GenCommand::ModifyBatch {
                entities,
                expected_revision: args["expected_revision"].as_u64(),
            })
            .await?
        {
            GenResponse::BatchResult { results, revision } => {
                let success_count = results
                    .iter()
                    .filter(|r| r.starts_with("Modified:"))
                    .count();
                Ok(format!(
                    "Batch modify: {} entities modified (scene revision {})\n{}",
                    success_count,
                    revision,
                    results.join("\n")
                ))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_delete_batch
// ===========================================================================

struct GenDeleteBatchTool {
    bridge: Arc<GenBridge>,
}

impl GenDeleteBatchTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenDeleteBatchTool {
    fn name(&self) -> &str {
        "gen_delete_batch"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_delete_batch".into(),
            description: "Delete multiple entities in a single call. All-or-nothing: if any entity is missing, nothing is deleted.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "names": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Array of entity names to delete"
                    },
                    "expected_revision": {
                        "type": "integer",
                        "description": "Scene revision this batch was planned against (from gen_scene_info). If the scene has changed since, nothing is applied."
                    }
                },
                "required": ["names"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let names_val = args
            .get("names")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid names array"))?;

        let names: Vec<String> = names_val
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        if names.is_empty() {
            return Err(anyhow::anyhow!("Empty names array"));
        }

        match self
            .bridge
            .send(GenCommand::DeleteBatch {
                names,
                expected_revision: args["expected_revision"].as_u64(),
            })
            .await?
        {
            GenResponse::BatchResult { results, revision } => {
                let success_count = results.iter().filter(|r| r.starts_with("Deleted:")).count();
                Ok(format!(
                    "Batch delete: {} entities deleted (scene revision {})\n{}",
                    success_count,
                    revision,
                    results.join("\n")
                ))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_camera
// ===========================================================================

struct GenSetCameraTool {
    bridge: Arc<GenBridge>,
}

impl GenSetCameraTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetCameraTool {
    fn name(&self) -> &str {
        "gen_set_camera"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_camera".into(),
            description:
                "Set camera position and target. The camera always looks at the target point."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [5, 5, 5],
                        "description": "Camera position [x, y, z]"
                    },
                    "look_at": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0],
                        "description": "Point camera looks at [x, y, z]"
                    },
                    "fov_degrees": {
                        "type": "number",
                        "default": 45,
                        "description": "Vertical field of view"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();

        let cmd = CameraCmd {
            position: parse_f32_array(&args["position"], [5.0, 5.0, 5.0]),
            look_at: parse_f32_array(&args["look_at"], [0.0, 0.0, 0.0]),
            fov_degrees: args["fov_degrees"].as_f64().unwrap_or(45.0) as f32,
        };

        match self.bridge.send(GenCommand::SetCamera(cmd)).await? {
            GenResponse::CameraSet => Ok("Camera updated".to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_light
// ===========================================================================

struct GenSetLightTool {
    bridge: Arc<GenBridge>,
}

impl GenSetLightTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetLightTool {
    fn name(&self) -> &str {
        "gen_set_light"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_light".into(),
            description:
                "Add or update a light source. Lighting is the primary driver of visual quality."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Light name (e.g., 'sun', 'key_light', 'fill')"
                    },
                    "light_type": {
                        "type": "string",
                        "enum": ["directional", "point", "spot"],
                        "default": "directional"
                    },
                    "color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [1, 1, 1, 1],
                        "description": "RGBA light color"
                    },
                    "intensity": {
                        "type": "number",
                        "default": 1000,
                        "description": "Lumens for point/spot, lux for directional"
                    },
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "Position for point/spot lights [x, y, z]"
                    },
                    "direction": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "Direction for directional/spot lights [x, y, z]"
                    },
                    "shadows": {
                        "type": "boolean",
                        "default": true
                    },
                    "range": {
                        "type": "number",
                        "description": "Max range in world units for point/spot lights"
                    },
                    "outer_angle": {
                        "type": "number",
                        "description": "Outer cone angle in radians (spot lights only)"
                    },
                    "inner_angle": {
                        "type": "number",
                        "description": "Inner cone angle in radians (spot lights only)"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let cmd = SetLightCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            light_type: args
                .get("light_type")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(LightType::Directional),
            color: parse_f32_4(&args["color"], [1.0, 1.0, 1.0, 1.0]),
            intensity: args["intensity"].as_f64().unwrap_or(1000.0) as f32,
            position: parse_opt_f32_array(&args["position"]),
            direction: parse_opt_f32_array(&args["direction"]),
            shadows: args["shadows"].as_bool().unwrap_or(true),
            range: args.get("range").and_then(|v| v.as_f64()).map(|v| v as f32),
            outer_angle: args
                .get("outer_angle")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
            inner_angle: args
                .get("inner_angle")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
        };

        match self.bridge.send(GenCommand::SetLight(cmd)).await? {
            GenResponse::LightSet { name } => Ok(format!("Light '{}' set", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_environment
// ===========================================================================

struct GenSetEnvironmentTool {
    bridge: Arc<GenBridge>,
}

impl GenSetEnvironmentTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetEnvironmentTool {
    fn name(&self) -> &str {
        "gen_set_environment"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_environment".into(),
            description: "Set global environment: background color, ambient light.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "background_color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "RGBA background color"
                    },
                    "ambient_light": {
                        "type": "number",
                        "default": 80,
                        "description": "Ambient light brightness in Bevy GlobalAmbientLight units (the world format's ambient_intensity): 80 is a neutral fill, 20-60 for night or interiors, 150-300 for bright daylight. Values below 1 leave the scene unlit."
                    },
                    "ambient_color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [1, 1, 1, 1],
                        "description": "Ambient light RGBA color"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();

        let cmd = EnvironmentCmd {
            background_color: parse_opt_f32_4(&args["background_color"]),
            ambient_light: args["ambient_light"].as_f64().map(|v| v as f32),
            ambient_color: parse_opt_f32_4(&args["ambient_color"]),
        };

        match self.bridge.send(GenCommand::SetEnvironment(cmd)).await? {
            GenResponse::EnvironmentSet => Ok("Environment updated".to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_spawn_mesh
// ===========================================================================

struct GenSpawnMeshTool {
    bridge: Arc<GenBridge>,
}

impl GenSpawnMeshTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSpawnMeshTool {
    fn name(&self) -> &str {
        "gen_spawn_mesh"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_spawn_mesh".into(),
            description:
                "Create custom geometry from raw vertex data. Use when primitives are insufficient."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "vertices": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": {"type": "number"}
                        },
                        "description": "Array of [x,y,z] vertex positions"
                    },
                    "indices": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "description": "Triangle indices (groups of 3)"
                    },
                    "normals": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": {"type": "number"}
                        },
                        "description": "Per-vertex normals [x,y,z]. Auto-computed if omitted."
                    },
                    "uvs": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": {"type": "number"}
                        },
                        "description": "Per-vertex UV coordinates [u,v]"
                    },
                    "color": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0.8, 0.8, 0.8, 1.0]
                    },
                    "metallic": {"type": "number", "default": 0.0},
                    "roughness": {"type": "number", "default": 0.5},
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0],
                        "description": "World position [x, y, z]"
                    },
                    "rotation_degrees": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0],
                        "description": "Euler angles in degrees (pitch, yaw, roll)"
                    },
                    "scale": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [1, 1, 1],
                        "description": "Scale [x, y, z]"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Name of parent entity for hierarchy; position, rotation and scale are then relative to the parent. Omit for root-level."
                    },
                    "emissive": {
                        "type": "array",
                        "items": {"type": "number"},
                        "default": [0, 0, 0, 0],
                        "description": "Emissive RGBA color for glowing objects"
                    },
                    "alpha_mode": {
                        "type": "string",
                        "description": "Alpha/transparency mode: 'opaque' (default), 'blend' (transparent), 'mask:0.5' (cutout), 'add', 'multiply'"
                    },
                    "unlit": {
                        "type": "boolean",
                        "description": "If true, ignore lighting and render at full brightness"
                    },
                    "double_sided": {
                        "type": "boolean",
                        "description": "If true, render both sides of faces"
                    },
                    "reflectance": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Specular reflectance (default 0.5)"
                    }
                },
                "required": ["name", "vertices", "indices"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let vertices: Vec<[f32; 3]> = args["vertices"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing vertices"))?
            .iter()
            .map(parse_json_vec3)
            .collect();

        let indices: Vec<u32> = args["indices"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing indices"))?
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u32))
            .collect();

        let normals: Option<Vec<[f32; 3]>> = args
            .get("normals")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(parse_json_vec3).collect());

        let uvs: Option<Vec<[f32; 2]>> = args
            .get("uvs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(parse_json_vec2).collect());

        let cmd = RawMeshCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            vertices,
            indices,
            normals,
            uvs,
            color: parse_f32_4(&args["color"], [0.8, 0.8, 0.8, 1.0]),
            metallic: args["metallic"].as_f64().unwrap_or(0.0) as f32,
            roughness: args["roughness"].as_f64().unwrap_or(0.5) as f32,
            position: parse_f32_array(&args["position"], [0.0, 0.0, 0.0]),
            rotation_degrees: parse_f32_array(&args["rotation_degrees"], [0.0, 0.0, 0.0]),
            scale: parse_f32_array(&args["scale"], [1.0, 1.0, 1.0]),
            parent: args["parent"].as_str().map(|s| s.to_string()),
            emissive: parse_f32_4(&args["emissive"], [0.0, 0.0, 0.0, 0.0]),
            alpha_mode: args
                .get("alpha_mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            unlit: args.get("unlit").and_then(|v| v.as_bool()),
            double_sided: args.get("double_sided").and_then(|v| v.as_bool()),
            reflectance: args
                .get("reflectance")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
        };

        match self.bridge.send(GenCommand::SpawnMesh(cmd)).await? {
            GenResponse::Spawned { name, entity_id } => Ok(format!(
                "Spawned mesh '{}' (entity_id: {})",
                name, entity_id
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_load_gltf
// ===========================================================================

struct GenLoadGltfTool {
    bridge: Arc<GenBridge>,
}

impl GenLoadGltfTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenLoadGltfTool {
    fn name(&self) -> &str {
        "gen_load_gltf"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_load_gltf".into(),
            description: "Load a glTF/GLB file from disk into the scene. Optionally decompose into editable sub-objects. Searches in workspace/exports by default.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to glTF/GLB file. Can be absolute, relative, or just a filename to search in workspace."
                    },
                    "segment": {
                        "type": "boolean",
                        "default": false,
                        "description": "Decompose mesh into individually editable sub-objects via connected-component analysis"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?
            .to_string();
        let segment = args["segment"].as_bool().unwrap_or(false);

        match self
            .bridge
            .send(GenCommand::LoadGltf { path, segment })
            .await?
        {
            GenResponse::GltfLoaded { name, path } => {
                Ok(format!("Loaded '{}' from {}", name, path))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_export_screenshot
// ===========================================================================

struct GenExportScreenshotTool {
    bridge: Arc<GenBridge>,
}

impl GenExportScreenshotTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenExportScreenshotTool {
    fn name(&self) -> &str {
        "gen_export_screenshot"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_export_screenshot".into(),
            description: "Render a high-resolution image of the scene to a file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Output file path"
                    },
                    "width": {
                        "type": "integer",
                        "default": 1920
                    },
                    "height": {
                        "type": "integer",
                        "default": 1080
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?
            .to_string();
        let width = args["width"].as_u64().unwrap_or(1920) as u32;
        let height = args["height"].as_u64().unwrap_or(1080) as u32;

        match self
            .bridge
            .send(GenCommand::ExportScreenshot {
                path,
                width,
                height,
            })
            .await?
        {
            GenResponse::Screenshot { image_path } => {
                Ok(format!("Exported screenshot to: {}", image_path))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_export_gltf
// ===========================================================================

struct GenExportGltfTool {
    bridge: Arc<GenBridge>,
}

impl GenExportGltfTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenExportGltfTool {
    fn name(&self) -> &str {
        "gen_export_gltf"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_export_gltf".into(),
            description: "Export the current scene as a glTF binary (.glb) file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Output file path (.glb extension added if missing). If omitted, exports to {workspace}/exports/{timestamp}.glb"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match self.bridge.send(GenCommand::ExportGltf { path }).await? {
            GenResponse::Exported { path } => Ok(format!("Exported scene to: {}", path)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_ambience
// ===========================================================================

struct GenSetAmbienceTool {
    bridge: Arc<GenBridge>,
}

impl GenSetAmbienceTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetAmbienceTool {
    fn name(&self) -> &str {
        "gen_set_ambience"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_ambience".into(),
            description: "Set the global ambient soundscape. Replaces previous ambience. Each layer is a continuous procedural sound that loops with natural variation.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "layers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "description": "Layer name (e.g., 'wind', 'rain')"},
                                "sound": {
                                    "type": "object",
                                    "description": "Sound type with parameters. Types: wind {speed: 0-1, gustiness: 0-1}, rain {intensity: 0-1}, forest {bird_density: 0-1, wind: 0-1}, ocean {wave_size: 0-1}, cave {drip_rate: 0-1, resonance: 0-1}, stream {flow_rate: 0-1}, silence {}",
                                    "properties": {
                                        "type": {"type": "string", "enum": ["wind", "rain", "forest", "ocean", "cave", "stream", "silence"]}
                                    }
                                },
                                "volume": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.5}
                            },
                            "required": ["name", "sound"]
                        },
                        "description": "Ambient sound layers to mix together"
                    },
                    "master_volume": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "Master volume for all audio (default: 0.8)"
                    }
                },
                "required": ["layers"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let layers_val = args["layers"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing layers array"))?;

        let mut layers = Vec::new();
        for layer_val in layers_val {
            let name = layer_val["name"].as_str().unwrap_or("unnamed").to_string();
            let sound: AmbientSound = serde_json::from_value(layer_val["sound"].clone())?;
            let volume = layer_val["volume"].as_f64().unwrap_or(0.5) as f32;
            layers.push(AmbienceLayerDef {
                name,
                sound,
                volume,
            });
        }

        let master_volume = args["master_volume"].as_f64().map(|v| v as f32);

        let cmd = AmbienceCmd {
            layers,
            master_volume,
            reverb: None,
        };

        match self.bridge.send(GenCommand::SetAmbience(cmd)).await? {
            GenResponse::AmbienceSet => Ok("Ambient soundscape set".to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_audio_emitter
// ===========================================================================

struct GenAudioEmitterTool {
    bridge: Arc<GenBridge>,
}

impl GenAudioEmitterTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAudioEmitterTool {
    fn name(&self) -> &str {
        "gen_audio_emitter"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_audio_emitter".into(),
            description: "Create a spatial audio emitter. Sound gets louder as camera approaches and quieter when far away. Can attach to an existing entity or specify a position.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique name for this audio emitter"
                    },
                    "entity": {
                        "type": "string",
                        "description": "Name of existing entity to attach sound to"
                    },
                    "position": {
                        "type": "array",
                        "items": {"type": "number"},
                        "description": "Position [x, y, z] for standalone emitter (ignored if entity is specified)"
                    },
                    "sound": {
                        "type": "object",
                        "description": "Sound type. Types: water {turbulence: 0-1}, fire {intensity: 0-1, crackle: 0-1}, hum {frequency: Hz, warmth: 0-1}, wind {pitch: Hz}, custom {waveform: sine/saw/square/white_noise/pink_noise/brown_noise, filter_cutoff: Hz, filter_type: lowpass/highpass/bandpass}",
                        "properties": {
                            "type": {"type": "string", "enum": ["water", "fire", "hum", "wind", "custom"]}
                        }
                    },
                    "radius": {
                        "type": "number",
                        "default": 10.0,
                        "description": "Maximum audible distance from emitter"
                    },
                    "volume": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "default": 0.7,
                        "description": "Base volume at closest distance"
                    }
                },
                "required": ["name", "sound"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing name"))?
            .to_string();

        let sound: EmitterSound = serde_json::from_value(args["sound"].clone())?;

        let cmd = AudioEmitterCmd {
            name,
            entity: args["entity"].as_str().map(|s| s.to_string()),
            position: parse_opt_f32_array(&args["position"]),
            sound,
            radius: args["radius"].as_f64().unwrap_or(10.0) as f32,
            volume: args["volume"].as_f64().unwrap_or(0.7) as f32,
        };

        match self.bridge.send(GenCommand::SpawnAudioEmitter(cmd)).await? {
            GenResponse::AudioEmitterSpawned { name } => {
                Ok(format!("Audio emitter '{}' created", name))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_modify_audio
// ===========================================================================

struct GenModifyAudioTool {
    bridge: Arc<GenBridge>,
}

impl GenModifyAudioTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenModifyAudioTool {
    fn name(&self) -> &str {
        "gen_modify_audio"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_modify_audio".into(),
            description: "Modify an existing audio emitter's volume, radius, or sound parameters."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of audio emitter to modify"
                    },
                    "volume": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1,
                        "description": "New base volume"
                    },
                    "radius": {
                        "type": "number",
                        "description": "New audible radius"
                    },
                    "sound": {
                        "type": "object",
                        "description": "New sound type (replaces current)"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let cmd = ModifyAudioEmitterCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            volume: args["volume"].as_f64().map(|v| v as f32),
            radius: args["radius"].as_f64().map(|v| v as f32),
            sound: args
                .get("sound")
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
        };

        match self
            .bridge
            .send(GenCommand::ModifyAudioEmitter(cmd))
            .await?
        {
            GenResponse::AudioEmitterModified { name } => {
                Ok(format!("Audio emitter '{}' modified", name))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_audio_info
// ===========================================================================

struct GenAudioInfoTool {
    bridge: Arc<GenBridge>,
}

impl GenAudioInfoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAudioInfoTool {
    fn name(&self) -> &str {
        "gen_audio_info"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_audio_info".into(),
            description: "Get current audio state: active layers, emitters with positions/volumes."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::AudioInfo).await? {
            GenResponse::AudioInfoData(data) => Ok(serde_json::to_string_pretty(&data)?),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_add_behavior
// ===========================================================================

struct GenAddBehaviorTool {
    bridge: Arc<GenBridge>,
}

impl GenAddBehaviorTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddBehaviorTool {
    fn name(&self) -> &str {
        "gen_add_behavior"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_behavior".into(),
            description: "Add a continuous behavior to an entity. Behaviors animate entities automatically each frame. Multiple behaviors can be stacked on one entity.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Name of entity to add behavior to"
                    },
                    "behavior_id": {
                        "type": "string",
                        "description": "Optional unique ID for this behavior (auto-generated if omitted). Use to later remove specific behaviors."
                    },
                    "behavior": {
                        "type": "object",
                        "description": "Behavior definition. Types: orbit {center, center_point, radius, speed (deg/s), axis, phase, tilt}, spin {axis, speed}, bob {axis, amplitude, frequency, phase}, look_at {target}, pulse {min_scale, max_scale, frequency}, path_follow {waypoints: [[x,y,z],...], speed (u/s), mode: loop|ping_pong|once, orient_to_path}, bounce {height, gravity, damping, surface_y}",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["orbit", "spin", "bob", "look_at", "pulse", "path_follow", "bounce"]
                            }
                        },
                        "required": ["type"]
                    }
                },
                "required": ["entity", "behavior"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing entity"))?
            .to_string();

        let behavior_id = args["behavior_id"].as_str().map(|s| s.to_string());
        let behavior: BehaviorDef = serde_json::from_value(args["behavior"].clone())?;

        let cmd = AddBehaviorCmd {
            entity,
            behavior_id,
            behavior,
        };

        match self.bridge.send(GenCommand::AddBehavior(cmd)).await? {
            GenResponse::BehaviorAdded {
                entity,
                behavior_id,
            } => Ok(format!(
                "Added behavior '{}' to entity '{}'",
                behavior_id, entity
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_remove_behavior
// ===========================================================================

struct GenRemoveBehaviorTool {
    bridge: Arc<GenBridge>,
}

impl GenRemoveBehaviorTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenRemoveBehaviorTool {
    fn name(&self) -> &str {
        "gen_remove_behavior"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_remove_behavior".into(),
            description:
                "Remove behaviors from an entity. If behavior_id is specified, removes only that behavior; otherwise removes all behaviors from the entity."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Name of entity to remove behaviors from"
                    },
                    "behavior_id": {
                        "type": "string",
                        "description": "Specific behavior ID to remove (from gen_list_behaviors). Omit to remove all."
                    }
                },
                "required": ["entity"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing entity"))?
            .to_string();

        let behavior_id = args["behavior_id"].as_str().map(|s| s.to_string());

        match self
            .bridge
            .send(GenCommand::RemoveBehavior {
                entity,
                behavior_id,
            })
            .await?
        {
            GenResponse::BehaviorRemoved { entity, count } => {
                Ok(format!("Removed {} behavior(s) from '{}'", count, entity))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_list_behaviors
// ===========================================================================

struct GenListBehaviorsTool {
    bridge: Arc<GenBridge>,
}

impl GenListBehaviorsTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenListBehaviorsTool {
    fn name(&self) -> &str {
        "gen_list_behaviors"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_list_behaviors".into(),
            description: "List all active behaviors. Optionally filter by entity name.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Filter to specific entity name (optional)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let entity = args["entity"].as_str().map(|s| s.to_string());

        match self
            .bridge
            .send(GenCommand::ListBehaviors { entity })
            .await?
        {
            GenResponse::BehaviorList(data) => Ok(serde_json::to_string_pretty(&data)?),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_pause_behaviors
// ===========================================================================

struct GenPauseBehaviorsTool {
    bridge: Arc<GenBridge>,
}

impl GenPauseBehaviorsTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPauseBehaviorsTool {
    fn name(&self) -> &str {
        "gen_pause_behaviors"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_pause_behaviors".into(),
            description:
                "Pause or resume all behaviors. When paused, entities freeze in place. Useful for inspecting or modifying the scene mid-animation."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "paused": {
                        "type": "boolean",
                        "description": "true to pause, false to resume"
                    }
                },
                "required": ["paused"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let paused = args["paused"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("Missing paused"))?;

        match self
            .bridge
            .send(GenCommand::SetBehaviorsPaused { paused })
            .await?
        {
            GenResponse::BehaviorsPaused { paused } => {
                if paused {
                    Ok("All behaviors paused".to_string())
                } else {
                    Ok("All behaviors resumed".to_string())
                }
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_save_world
// ===========================================================================

struct GenSaveWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenSaveWorldTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSaveWorldTool {
    fn name(&self) -> &str {
        "gen_save_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_save_world".into(),
            description: "Save the current scene as a world skill. Creates a skill directory with world.ron (RON format manifest with all entities, behaviors, audio, tours inline) and SKILL.md. The world can be loaded later with gen_load_world or invoked as a skill.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "World/skill name (e.g., 'solar-system', 'medieval-village'). Used as directory name."
                    },
                    "description": {
                        "type": "string",
                        "description": "Brief description of the world for SKILL.md"
                    },
                    "path": {
                        "type": "string",
                        "description": "Custom output path. Default: {workspace}/skills/{name}/"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let cmd = SaveWorldCmd {
            name: args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?
                .to_string(),
            description: args["description"].as_str().map(|s| s.to_string()),
            path: args["path"].as_str().map(|s| s.to_string()),
        };

        match self.bridge.send(GenCommand::SaveWorld(cmd)).await? {
            GenResponse::WorldSaved {
                path,
                skill_name,
                warnings,
            } => {
                let mut msg = format!(
                    "World '{}' saved to: {}\nCan be loaded with gen_load_world or invoked as /{}",
                    skill_name, path, skill_name
                );
                if !warnings.is_empty() {
                    msg.push_str("\n\nValidation warnings:");
                    for w in &warnings {
                        msg.push_str(&format!("\n  - {}", w));
                    }
                }
                Ok(msg)
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_load_world
// ===========================================================================

struct GenLoadWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenLoadWorldTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenLoadWorldTool {
    fn name(&self) -> &str {
        "gen_load_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_load_world".into(),
            description: "Load a world skill. Restores the 3D scene, behaviors, audio, environment, and camera from a saved world directory. Clears existing scene by default.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to world skill directory, or just the skill name (searches {workspace}/skills/)"
                    },
                    "clear": {
                        "type": "boolean",
                        "description": "Clear existing scene before loading (default: true)",
                        "default": true
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?
            .to_string();
        let clear = args["clear"].as_bool().unwrap_or(true);

        match self
            .bridge
            .send(GenCommand::LoadWorld { path, clear })
            .await?
        {
            GenResponse::WorldLoaded {
                path,
                entities,
                behaviors,
            } => Ok(format!(
                "World loaded from: {}\n{} entities, {} behaviors restored",
                path, entities, behaviors
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_export_world
// ===========================================================================

struct GenExportWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenExportWorldTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenExportWorldTool {
    fn name(&self) -> &str {
        "gen_export_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_export_world".into(),
            description: "Export the current world to a glTF file for external viewers. Creates scene.glb or scene.gltf + scene.bin in the world's export/ directory. Use after gen_save_world to create portable 3D files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["glb", "gltf"],
                        "default": "glb",
                        "description": "Export format: 'glb' for single binary file (recommended), 'gltf' for human-readable JSON + BIN"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match self.bridge.send(GenCommand::ExportWorld { format }).await? {
            GenResponse::Exported { path } => Ok(format!(
                "World exported to: {}\n\nThis file can be opened in external 3D viewers like Blender, Unity, Unreal Engine.",
                path
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_export_html
// ===========================================================================

struct GenExportHtmlTool {
    bridge: Arc<GenBridge>,
}

impl GenExportHtmlTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenExportHtmlTool {
    fn name(&self) -> &str {
        "gen_export_html"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_export_html".into(),
            description: "Export the current world as a self-contained HTML file using Three.js. \
                Generates index.html in the world's export/ directory with 3D shapes, PBR materials, \
                lights, animated behaviors, and procedural audio via Web Audio API. \
                Requires a saved world (use gen_save_world first). \
                Open the exported file in any browser — no server required."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::ExportHtml).await? {
            GenResponse::Exported { path } => Ok(format!(
                "World exported to HTML: {}\n\n\
                Open this file in any web browser to view the interactive 3D scene.\n\
                Features: Three.js rendering, OrbitControls, animated behaviors, procedural audio.",
                path
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_fork_world
// ===========================================================================

struct GenForkWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenForkWorldTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenForkWorldTool {
    fn name(&self) -> &str {
        "gen_fork_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_fork_world".into(),
            description:
                "Fork (copy) an existing world skill to a new name with attribution metadata. \
                Creates a complete copy of the source world directory under a new skill name, \
                updating world.ron metadata and SKILL.md title."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Source world name or path to fork from"
                    },
                    "new_name": {
                        "type": "string",
                        "description": "Name for the forked world"
                    }
                },
                "required": ["source", "new_name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let source = args["source"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing source"))?
            .to_string();
        let new_name = args["new_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing new_name"))?
            .to_string();

        match self
            .bridge
            .send(GenCommand::ForkWorld { source, new_name })
            .await?
        {
            GenResponse::WorldSaved {
                path,
                skill_name,
                warnings,
            } => {
                let mut msg = format!(
                    "World forked as '{}' at: {}\nCan be loaded with gen_load_world or invoked as /{}",
                    skill_name, path, skill_name
                );
                if !warnings.is_empty() {
                    msg.push_str("\n\nWarnings:");
                    for w in &warnings {
                        msg.push_str(&format!("\n  - {}", w));
                    }
                }
                Ok(msg)
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_clear_scene
// ===========================================================================

struct GenClearSceneTool {
    bridge: Arc<GenBridge>,
}

impl GenClearSceneTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenClearSceneTool {
    fn name(&self) -> &str {
        "gen_clear_scene"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_clear_scene".into(),
            description: "Clear the 3D scene. Removes all entities, stops audio, and resets behaviors. Useful before loading a new world.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "keep_camera": {
                        "type": "boolean",
                        "description": "Keep the camera (default: true)",
                        "default": true
                    },
                    "keep_lights": {
                        "type": "boolean",
                        "description": "Keep lights (default: true)",
                        "default": true
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let keep_camera = args["keep_camera"].as_bool().unwrap_or(true);
        let keep_lights = args["keep_lights"].as_bool().unwrap_or(true);

        match self
            .bridge
            .send(GenCommand::ClearScene {
                keep_camera,
                keep_lights,
            })
            .await?
        {
            GenResponse::SceneCleared { removed_count } => {
                Ok(format!("Scene cleared: {} entities removed", removed_count))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// Undo/Redo tools
// ===========================================================================

struct GenUndoTool {
    bridge: Arc<GenBridge>,
}

impl GenUndoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenUndoTool {
    fn name(&self) -> &str {
        "gen_undo"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_undo".into(),
            description: "Undo the last scene edit (spawn, delete, or modify entity). Can be called multiple times to undo further back.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::Undo).await? {
            GenResponse::Undone { description } => Ok(format!("Undone: {}", description)),
            GenResponse::NothingToUndo => Ok("Nothing to undo".to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

struct GenRedoTool {
    bridge: Arc<GenBridge>,
}

impl GenRedoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenRedoTool {
    fn name(&self) -> &str {
        "gen_redo"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_redo".into(),
            description: "Redo a previously undone scene edit. Only available after gen_undo."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::Redo).await? {
            GenResponse::Redone { description } => Ok(format!("Redone: {}", description)),
            GenResponse::NothingToRedo => Ok("Nothing to redo".to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

struct GenUndoInfoTool {
    bridge: Arc<GenBridge>,
}

impl GenUndoInfoTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenUndoInfoTool {
    fn name(&self) -> &str {
        "gen_undo_info"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_undo_info".into(),
            description: "Show the current undo/redo stack state: how many operations can be undone and redone.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        match self.bridge.send(GenCommand::UndoInfo).await? {
            GenResponse::UndoInfoResult {
                undo_count,
                redo_count,
                entity_count,
                dirty_count,
            } => Ok(format!(
                "Undo: {} undoable, {} redoable | Scene: {} entities ({} unsaved)",
                undo_count, redo_count, entity_count, dirty_count
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_physics
// ===========================================================================

struct GenSetPhysicsTool {
    bridge: Arc<GenBridge>,
}

impl GenSetPhysicsTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetPhysicsTool {
    fn name(&self) -> &str {
        "gen_set_physics"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_physics".into(),
            description: "Enable physics simulation on an entity. Sets body type (dynamic/static/kinematic), mass, friction, bounciness, and damping.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "body_type": {
                        "type": "string",
                        "enum": ["dynamic", "static", "kinematic"],
                        "default": "dynamic",
                        "description": "Physics body type"
                    },
                    "mass": {
                        "type": "number",
                        "description": "Mass in kg (auto-calculated if omitted)"
                    },
                    "restitution": {
                        "type": "number",
                        "default": 0.3,
                        "description": "Bounciness (0-1)"
                    },
                    "friction": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Surface friction (0-1)"
                    },
                    "gravity_scale": {
                        "type": "number",
                        "default": 1.0,
                        "description": "Gravity multiplier"
                    },
                    "linear_damping": {
                        "type": "number",
                        "default": 0.1,
                        "description": "Linear air resistance"
                    },
                    "angular_damping": {
                        "type": "number",
                        "default": 0.1,
                        "description": "Angular air resistance"
                    },
                    "lock_rotation": {
                        "type": "boolean",
                        "default": false,
                        "description": "Prevent rotation"
                    }
                },
                "required": ["entity_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let params = crate::physics::PhysicsParams {
            entity_id: args["entity_id"].as_str().unwrap_or("").to_string(),
            body_type: match args["body_type"].as_str().unwrap_or("dynamic") {
                "static" => crate::physics::BodyType::Static,
                "kinematic" => crate::physics::BodyType::Kinematic,
                _ => crate::physics::BodyType::Dynamic,
            },
            mass: args["mass"].as_f64().map(|v| v as f32),
            restitution: args["restitution"].as_f64().unwrap_or(0.3) as f32,
            friction: args["friction"].as_f64().unwrap_or(0.5) as f32,
            gravity_scale: args["gravity_scale"].as_f64().unwrap_or(1.0) as f32,
            linear_damping: args["linear_damping"].as_f64().unwrap_or(0.1) as f32,
            angular_damping: args["angular_damping"].as_f64().unwrap_or(0.1) as f32,
            lock_rotation: args["lock_rotation"].as_bool().unwrap_or(false),
        };
        match self.bridge.send(GenCommand::SetPhysics(params)).await? {
            GenResponse::Modified { name } => Ok(format!("Physics enabled on '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_add_collider
// ===========================================================================

struct GenAddColliderTool {
    bridge: Arc<GenBridge>,
}

impl GenAddColliderTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddColliderTool {
    fn name(&self) -> &str {
        "gen_add_collider"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_collider".into(),
            description: "Add a collision shape to an entity. Shapes: box, sphere, capsule, cylinder, mesh. Can be a sensor (trigger-only, no physics response).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "shape": {
                        "type": "string",
                        "enum": ["box", "sphere", "capsule", "cylinder", "mesh"],
                        "default": "box",
                        "description": "Collider shape type"
                    },
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "description": "Dimensions [x, y, z] (auto-fit to mesh if omitted)"
                    },
                    "offset": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3,
                        "maxItems": 3,
                        "default": [0, 0, 0],
                        "description": "Offset from entity origin"
                    },
                    "is_trigger": {
                        "type": "boolean",
                        "default": false,
                        "description": "Sensor only (detect overlap, no physics response)"
                    }
                },
                "required": ["entity_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let size = args["size"].as_array().map(|arr| {
            bevy::math::Vec3::new(
                arr.first().and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                arr.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                arr.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            )
        });
        let offset_arr = parse_f32_array(&args["offset"], [0.0, 0.0, 0.0]);
        let params = crate::physics::ColliderParams {
            entity_id: args["entity_id"].as_str().unwrap_or("").to_string(),
            shape: match args["shape"].as_str().unwrap_or("box") {
                "sphere" => crate::physics::ColliderShape::Sphere,
                "capsule" => crate::physics::ColliderShape::Capsule,
                "cylinder" => crate::physics::ColliderShape::Cylinder,
                "mesh" => crate::physics::ColliderShape::Mesh,
                _ => crate::physics::ColliderShape::Box,
            },
            size,
            offset: bevy::math::Vec3::from_array(offset_arr),
            is_trigger: args["is_trigger"].as_bool().unwrap_or(false),
            visible_in_debug: true,
        };
        match self.bridge.send(GenCommand::AddCollider(params)).await? {
            GenResponse::Modified { name } => Ok(format!("Collider added to '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_add_joint
// ===========================================================================

struct GenAddJointTool {
    bridge: Arc<GenBridge>,
}

impl GenAddJointTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddJointTool {
    fn name(&self) -> &str {
        "gen_add_joint"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_joint".into(),
            description: "Create a physical joint/constraint between two entities. Types: fixed, revolute (hinge), spherical (ball), prismatic (slider), spring.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_a": {
                        "type": "string",
                        "description": "First entity name"
                    },
                    "entity_b": {
                        "type": "string",
                        "description": "Second entity name"
                    },
                    "joint_type": {
                        "type": "string",
                        "enum": ["fixed", "revolute", "spherical", "prismatic", "spring"],
                        "default": "fixed",
                        "description": "Joint type"
                    },
                    "anchor_a": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "default": [0, 0, 0],
                        "description": "Anchor on entity A (local space)"
                    },
                    "anchor_b": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "default": [0, 0, 0],
                        "description": "Anchor on entity B (local space)"
                    },
                    "axis": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "default": [0, 1, 0],
                        "description": "Rotation/slide axis"
                    },
                    "limits": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 2, "maxItems": 2,
                        "description": "Angle limits [min, max] in degrees"
                    },
                    "stiffness": {
                        "type": "number",
                        "description": "Spring stiffness"
                    },
                    "damping": {
                        "type": "number",
                        "description": "Spring damping"
                    }
                },
                "required": ["entity_a", "entity_b"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let anchor_a = parse_f32_array(&args["anchor_a"], [0.0, 0.0, 0.0]);
        let anchor_b = parse_f32_array(&args["anchor_b"], [0.0, 0.0, 0.0]);
        let axis = parse_f32_array(&args["axis"], [0.0, 1.0, 0.0]);
        let limits = args["limits"].as_array().map(|arr| {
            bevy::math::Vec2::new(
                arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            )
        });
        let params = crate::physics::JointParams {
            entity_a: args["entity_a"].as_str().unwrap_or("").to_string(),
            entity_b: args["entity_b"].as_str().unwrap_or("").to_string(),
            joint_type: match args["joint_type"].as_str().unwrap_or("fixed") {
                "revolute" => crate::physics::JointType::Revolute,
                "spherical" => crate::physics::JointType::Spherical,
                "prismatic" => crate::physics::JointType::Prismatic,
                "spring" => crate::physics::JointType::Spring,
                _ => crate::physics::JointType::Fixed,
            },
            anchor_a: bevy::math::Vec3::from_array(anchor_a),
            anchor_b: bevy::math::Vec3::from_array(anchor_b),
            axis: bevy::math::Vec3::from_array(axis),
            limits,
            stiffness: args["stiffness"].as_f64().map(|v| v as f32),
            damping: args["damping"].as_f64().map(|v| v as f32),
        };
        match self.bridge.send(GenCommand::AddJoint(params)).await? {
            GenResponse::Spawned { name, entity_id } => {
                Ok(format!("Joint created: '{}' (id: {})", name, entity_id))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_add_force
// ===========================================================================

struct GenAddForceTool {
    bridge: Arc<GenBridge>,
}

impl GenAddForceTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddForceTool {
    fn name(&self) -> &str {
        "gen_add_force"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_force".into(),
            description: "Create a force field or apply an impulse. Types: directional, point_attract, point_repel, vortex, impulse.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "description": "World position [x, y, z]"
                    },
                    "force_type": {
                        "type": "string",
                        "enum": ["directional", "point_attract", "point_repel", "vortex", "impulse"],
                        "default": "directional",
                        "description": "Force field type"
                    },
                    "strength": {
                        "type": "number",
                        "default": 10.0,
                        "description": "Force strength"
                    },
                    "radius": {
                        "type": "number",
                        "default": 5.0,
                        "description": "Area of effect radius"
                    },
                    "direction": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "description": "Force direction (directional type only)"
                    },
                    "falloff": {
                        "type": "string",
                        "enum": ["none", "linear", "quadratic"],
                        "default": "linear",
                        "description": "Distance falloff type"
                    },
                    "affects_player": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether force affects the player"
                    },
                    "continuous": {
                        "type": "boolean",
                        "default": true,
                        "description": "Continuous force (vs one-shot impulse)"
                    }
                },
                "required": ["position"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let pos = parse_f32_array(&args["position"], [0.0, 0.0, 0.0]);
        let dir = args["direction"].as_array().map(|arr| {
            bevy::math::Vec3::new(
                arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                arr.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            )
        });
        let params = crate::physics::ForceParams {
            position: bevy::math::Vec3::from_array(pos),
            force_type: match args["force_type"].as_str().unwrap_or("directional") {
                "point_attract" => crate::physics::ForceType::PointAttract,
                "point_repel" => crate::physics::ForceType::PointRepel,
                "vortex" => crate::physics::ForceType::Vortex,
                "impulse" => crate::physics::ForceType::Impulse,
                _ => crate::physics::ForceType::Directional,
            },
            strength: args["strength"].as_f64().unwrap_or(10.0) as f32,
            radius: args["radius"].as_f64().unwrap_or(5.0) as f32,
            direction: dir,
            falloff: match args["falloff"].as_str().unwrap_or("linear") {
                "none" => crate::physics::FalloffType::None,
                "quadratic" => crate::physics::FalloffType::Quadratic,
                _ => crate::physics::FalloffType::Linear,
            },
            affects_player: args["affects_player"].as_bool().unwrap_or(true),
            continuous: args["continuous"].as_bool().unwrap_or(true),
        };
        match self.bridge.send(GenCommand::AddForce(params)).await? {
            GenResponse::Spawned { name, entity_id } => Ok(format!(
                "Force field created: '{}' (id: {})",
                name, entity_id
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// gen_set_gravity
// ===========================================================================

struct GenSetGravityTool {
    bridge: Arc<GenBridge>,
}

impl GenSetGravityTool {
    fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetGravityTool {
    fn name(&self) -> &str {
        "gen_set_gravity"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_gravity".into(),
            description: "Set gravity direction and strength. Can affect the whole scene or create a localized gravity zone.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity (omit for global gravity)"
                    },
                    "direction": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "default": [0, -1, 0],
                        "description": "Gravity direction (normalized)"
                    },
                    "strength": {
                        "type": "number",
                        "default": 9.81,
                        "description": "Gravity strength in m/s²"
                    },
                    "zone_position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "minItems": 3, "maxItems": 3,
                        "description": "Create gravity zone at this position"
                    },
                    "zone_radius": {
                        "type": "number",
                        "description": "Gravity zone radius"
                    },
                    "transition_duration": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Transition time in seconds"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or_default();
        let dir = parse_f32_array(&args["direction"], [0.0, -1.0, 0.0]);
        let zone_pos =
            parse_opt_f32_array(&args["zone_position"]).map(bevy::math::Vec3::from_array);
        let params = crate::physics::GravityParams {
            entity_id: args["entity_id"].as_str().map(String::from),
            direction: bevy::math::Vec3::from_array(dir),
            strength: args["strength"].as_f64().unwrap_or(9.81) as f32,
            zone_position: zone_pos,
            zone_radius: args["zone_radius"].as_f64().map(|v| v as f32),
            transition_duration: args["transition_duration"].as_f64().unwrap_or(0.5) as f32,
        };
        match self.bridge.send(GenCommand::SetGravity(params)).await? {
            GenResponse::EnvironmentSet => Ok("Gravity updated".to_string()),
            GenResponse::Spawned { name, entity_id } => Ok(format!(
                "Gravity zone created: '{}' (id: {})",
                name, entity_id
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ===========================================================================
// JSON parsing helpers
// ===========================================================================

fn parse_f32_array(val: &Value, default: [f32; 3]) -> [f32; 3] {
    val.as_array()
        .map(|arr| {
            [
                arr.first()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[0] as f64) as f32,
                arr.get(1)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[1] as f64) as f32,
                arr.get(2)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[2] as f64) as f32,
            ]
        })
        .unwrap_or(default)
}

fn parse_f32_4(val: &Value, default: [f32; 4]) -> [f32; 4] {
    val.as_array()
        .map(|arr| {
            [
                arr.first()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[0] as f64) as f32,
                arr.get(1)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[1] as f64) as f32,
                arr.get(2)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[2] as f64) as f32,
                arr.get(3)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(default[3] as f64) as f32,
            ]
        })
        .unwrap_or(default)
}

fn parse_opt_f32_array(val: &Value) -> Option<[f32; 3]> {
    val.as_array().map(|arr| {
        [
            arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        ]
    })
}

fn parse_opt_f32_4(val: &Value) -> Option<[f32; 4]> {
    val.as_array().map(|arr| {
        [
            arr.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            arr.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        ]
    })
}

fn parse_json_vec3(val: &Value) -> [f32; 3] {
    let empty = vec![];
    let arr = val.as_array().unwrap_or(&empty);
    [
        arr.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        arr.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        arr.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
    ]
}

fn parse_json_vec2(val: &Value) -> [f32; 2] {
    let empty = vec![];
    let arr = val.as_array().unwrap_or(&empty);
    [
        arr.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        arr.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
    ]
}
