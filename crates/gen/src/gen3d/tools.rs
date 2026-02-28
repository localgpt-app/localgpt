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
        // Scene management
        Box::new(GenClearSceneTool::new(bridge)),
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
            description: "Capture the current viewport as an image. Use after spawning/modifying entities to see results.".into(),
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

        match self
            .bridge
            .send(GenCommand::Screenshot {
                width,
                height,
                wait_frames,
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
                        "enum": ["Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane"],
                        "description": "Primitive shape type"
                    },
                    "dimensions": {
                        "type": "object",
                        "description": "Shape-specific dimensions. Cuboid: {x,y,z}. Sphere: {radius}. Cylinder: {radius, height}. Cone: {radius, height}. Torus: {major_radius, minor_radius}."
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
                    "parent": {
                        "type": "string",
                        "description": "Name of parent entity for hierarchy. Omit for root-level."
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
                        "default": 0.1,
                        "description": "Ambient light intensity 0.0-1.0"
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
            description: "Load a glTF/GLB file from disk into the scene. Searches in workspace/exports by default.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to glTF/GLB file. Can be absolute, relative, or just a filename to search in workspace."
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

        match self.bridge.send(GenCommand::LoadGltf { path }).await? {
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
            description: "Save the current scene as a world skill. Creates a skill directory with scene.glb, behaviors.toml, audio.toml, world.toml, and SKILL.md. The world can be loaded later with gen_load_world or invoked as a skill.".into(),
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
            GenResponse::WorldSaved { path, skill_name } => Ok(format!(
                "World '{}' saved to: {}\nCan be loaded with gen_load_world or invoked as /{}",
                skill_name, path, skill_name
            )),
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
