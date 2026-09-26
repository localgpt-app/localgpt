//! MCP tool handlers for AI-powered 3D asset generation (AI1).

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::asset_gen::*;
use crate::gen3d::commands::*;
use crate::gen3d::model_server;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

// ---------------------------------------------------------------------------
// gen_generate_asset
// ---------------------------------------------------------------------------

pub struct GenGenerateAssetTool {
    bridge: Arc<GenBridge>,
}

impl GenGenerateAssetTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenGenerateAssetTool {
    fn name(&self) -> &str {
        "gen_generate_asset"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_generate_asset".to_string(),
            description: "Generate a 3D mesh from a text prompt on the local asset model server (LOCALGPT_GEN_MODEL_SERVER, default http://127.0.0.1:8741). Fails immediately if no server is running. Otherwise returns a task ID at once; the entity is not in the scene until the task completes, when it spawns at the given position (check with gen_generation_status).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the 3D asset to generate (e.g., 'a weathered wooden barrel', 'medieval stone well')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Entity name for the generated asset in the scene"
                    },
                    "reference_image": {
                        "type": "string",
                        "description": "Optional path to a reference image to guide generation"
                    },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "World position [x, y, z] to place the asset when generated"
                    },
                    "scale": {
                        "type": "number",
                        "default": 1.0,
                        "description": "Uniform scale factor for the generated asset"
                    },
                    "model": {
                        "type": "string",
                        "enum": ["tripo_sg", "hunyuan3d", "hunyuan3d_mini", "step1x"],
                        "default": "tripo_sg",
                        "description": "Generation model: tripo_sg (~8GB VRAM, 30s), hunyuan3d (~10GB, 60s), hunyuan3d_mini (~5.5GB, 45s), step1x (~16GB, 90s)"
                    },
                    "quality": {
                        "type": "string",
                        "enum": ["draft", "standard", "high"],
                        "default": "standard",
                        "description": "Quality preset: draft (fast), standard (balanced), high (slow, detailed)"
                    },
                    "pbr": {
                        "type": "boolean",
                        "default": true,
                        "description": "Generate PBR textures (albedo, normal, roughness, metallic)"
                    }
                },
                "required": ["prompt", "name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prompt"))?
            .to_string();

        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
            .to_string();

        let reference_image = args["reference_image"].as_str().map(String::from);

        let position = args["position"]
            .as_array()
            .and_then(|a| {
                if a.len() >= 3 {
                    Some([
                        a[0].as_f64()? as f32,
                        a[1].as_f64()? as f32,
                        a[2].as_f64()? as f32,
                    ])
                } else {
                    None
                }
            })
            .unwrap_or([0.0, 0.0, 0.0]);

        let scale = args["scale"].as_f64().unwrap_or(1.0) as f32;

        let model = match args["model"].as_str().unwrap_or("tripo_sg") {
            "hunyuan3d" => GenerationModel::Hunyuan3d,
            "hunyuan3d_mini" => GenerationModel::Hunyuan3dMini,
            "step1x" => GenerationModel::Step1x,
            _ => GenerationModel::TripoSG,
        };

        let quality = match args["quality"].as_str().unwrap_or("standard") {
            "draft" => GenerationQuality::Draft,
            "high" => GenerationQuality::High,
            _ => GenerationQuality::Standard,
        };

        let pbr = args["pbr"].as_bool().unwrap_or(true);

        model_server::check_health(&model_server::server_url()).await?;

        let cmd = GenCommand::GenerateAsset {
            prompt,
            name,
            reference_image,
            position,
            scale,
            model,
            quality,
            pbr,
        };

        match self.bridge.send(cmd).await? {
            GenResponse::AssetGenerating {
                task_id,
                estimated_seconds,
                message,
            } => Ok(json!({
                "task_id": task_id,
                "estimated_seconds": estimated_seconds,
                "message": message,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_generate_texture
// ---------------------------------------------------------------------------

pub struct GenGenerateTextureTool {
    bridge: Arc<GenBridge>,
}

impl GenGenerateTextureTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenGenerateTextureTool {
    fn name(&self) -> &str {
        "gen_generate_texture"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_generate_texture".to_string(),
            description: "Generate PBR texture maps for an existing entity on the local asset model server. Fails immediately if no server is running. Returns a task ID at once; when the task completes the maps replace the entity's material textures (and are saved with the world). Check with gen_generation_status.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "Name of the existing entity to apply textures to"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Text description of the desired texture (e.g., 'mossy stone bricks', 'polished oak wood')"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["realistic", "stylized", "pixel_art", "hand_painted", "toon"],
                        "default": "realistic",
                        "description": "Texture style preset"
                    },
                    "resolution": {
                        "type": "integer",
                        "enum": [512, 1024, 2048, 4096],
                        "default": 1024,
                        "description": "Texture resolution in pixels (width = height)"
                    }
                },
                "required": ["entity", "prompt"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: entity"))?
            .to_string();

        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prompt"))?
            .to_string();

        let style = match args["style"].as_str().unwrap_or("realistic") {
            "stylized" => TextureStyle::Stylized,
            "pixel_art" => TextureStyle::PixelArt,
            "hand_painted" => TextureStyle::HandPainted,
            "toon" => TextureStyle::Toon,
            _ => TextureStyle::Realistic,
        };

        let resolution = args["resolution"].as_u64().unwrap_or(1024) as u32;

        model_server::check_health(&model_server::server_url()).await?;

        let cmd = GenCommand::GenerateTexture {
            entity,
            prompt,
            style,
            resolution,
        };

        match self.bridge.send(cmd).await? {
            GenResponse::TextureGenerating {
                task_id,
                estimated_seconds,
                message,
            } => Ok(json!({
                "task_id": task_id,
                "estimated_seconds": estimated_seconds,
                "message": message,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_generation_status
// ---------------------------------------------------------------------------

pub struct GenGenerationStatusTool {
    bridge: Arc<GenBridge>,
}

impl GenGenerationStatusTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenGenerationStatusTool {
    fn name(&self) -> &str {
        "gen_generation_status"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_generation_status".to_string(),
            description: "Check the status of AI asset generation tasks. Can list all tasks, get status of a specific task, or cancel a queued/generating task.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["status", "cancel", "list"],
                        "default": "status",
                        "description": "Action: 'status'/'list' to view tasks, 'cancel' to cancel a task"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (required for 'cancel', optional for 'status' to filter)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let action = args["action"].as_str().unwrap_or("status").to_string();

        let task_id = args["task_id"].as_str().map(String::from);

        let cmd = GenCommand::GenerationStatus { task_id, action };

        match self.bridge.send(cmd).await? {
            GenResponse::GenerationStatusResult { status_json } => Ok(status_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn create_asset_gen_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenGenerateAssetTool::new(bridge.clone())),
        Box::new(GenGenerateTextureTool::new(bridge.clone())),
        Box::new(GenGenerationStatusTool::new(bridge)),
    ]
}
