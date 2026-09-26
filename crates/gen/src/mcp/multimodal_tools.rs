//! MCP tool handlers for AI3: Multimodal input for image-guided world generation.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

// ---------------------------------------------------------------------------
// gen_image_to_layout (AI3.1)
// ---------------------------------------------------------------------------

pub struct GenImageToLayoutTool {
    bridge: Arc<GenBridge>,
}

impl GenImageToLayoutTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenImageToLayoutTool {
    fn name(&self) -> &str {
        "gen_image_to_layout"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_image_to_layout".to_string(),
            description: "Analyze a reference image and generate a blockout layout plan matching its spatial structure. Returns a BlockoutSpec JSON compatible with gen_apply_blockout. Use this to create worlds from concept art, screenshots, or photos.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "Path to reference image (PNG/JPEG) or base64-encoded image data"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Additional text guidance to supplement the image (e.g., 'focus on the castle in the background')"
                    },
                    "scale": {
                        "type": "string",
                        "enum": ["small", "medium", "large"],
                        "default": "medium",
                        "description": "Target world scale: small (single room ~10m²), medium (building/courtyard ~100m²), large (landscape ~1000m²)"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["match", "blockout", "stylized"],
                        "default": "match",
                        "description": "match: reproduce image style; blockout: gray-box only; stylized: interpret freely"
                    }
                },
                "required": ["image"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let image = args["image"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: image"))?;

        let prompt = args["prompt"].as_str().map(String::from);

        let scale = match args["scale"].as_str().unwrap_or("medium") {
            "small" => ImageLayoutScale::Small,
            "large" => ImageLayoutScale::Large,
            _ => ImageLayoutScale::Medium,
        };

        let style = match args["style"].as_str().unwrap_or("match") {
            "blockout" => ImageLayoutStyle::Blockout,
            "stylized" => ImageLayoutStyle::Stylized,
            _ => ImageLayoutStyle::Match,
        };

        let cmd = GenCommand::ImageToLayout {
            image: image.to_string(),
            prompt,
            scale,
            style,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::ImageLayoutAnalysis {
                plan_json,
                analysis_json,
            } => {
                let result = json!({
                    "plan": serde_json::from_str::<Value>(&plan_json).unwrap_or(Value::String(plan_json)),
                    "image_analysis": serde_json::from_str::<Value>(&analysis_json).unwrap_or(Value::String(analysis_json)),
                    "next_step": "Review the plan, then call gen_apply_blockout with the plan to create the 3D scene."
                });
                Ok(result.to_string())
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Image analysis completed".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_match_style (AI3.2)
// ---------------------------------------------------------------------------

pub struct GenMatchStyleTool {
    bridge: Arc<GenBridge>,
}

impl GenMatchStyleTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenMatchStyleTool {
    fn name(&self) -> &str {
        "gen_match_style"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_match_style".to_string(),
            description: "Adjust scene materials, lighting, and atmosphere to match the visual style of a reference image. Operates on an existing scene — use after building geometry.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "Path to style reference image or base64 data"
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["all", "lighting", "materials", "atmosphere"],
                        "default": "all",
                        "description": "Which aspects of the scene to adjust"
                    },
                    "intensity": {
                        "type": "number",
                        "default": 0.8,
                        "minimum": 0.0,
                        "maximum": 1.0,
                        "description": "How strongly to match the reference (0.0 = no change, 1.0 = full match)"
                    },
                    "entities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Specific entity IDs to restyle (default: all entities)"
                    }
                },
                "required": ["image"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let image = args["image"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: image"))?;

        let scope = match args["scope"].as_str().unwrap_or("all") {
            "lighting" => StyleMatchScope::Lighting,
            "materials" => StyleMatchScope::Materials,
            "atmosphere" => StyleMatchScope::Atmosphere,
            _ => StyleMatchScope::All,
        };

        let intensity = args["intensity"].as_f64().unwrap_or(0.8) as f32;

        let entities = args["entities"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

        let cmd = GenCommand::MatchStyle {
            image: image.to_string(),
            scope,
            intensity,
            entities,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::StyleMatched { changes_json } => Ok(changes_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Style matching completed".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_reference_board (AI3.3)
// ---------------------------------------------------------------------------

pub struct GenReferenceBoardTool {
    bridge: Arc<GenBridge>,
}

impl GenReferenceBoardTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenReferenceBoardTool {
    fn name(&self) -> &str {
        "gen_reference_board"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_reference_board".to_string(),
            description: "Manage reference images that influence world generation in the current session. Build a mood board of references (concept art, style guides, photos) that affects all subsequent generation, evaluation, and style-matching calls.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "remove", "list", "clear"],
                        "description": "Action to perform on the reference board"
                    },
                    "image": {
                        "type": "string",
                        "description": "Path or base64 image data (required for 'add')"
                    },
                    "label": {
                        "type": "string",
                        "description": "Label for the reference (e.g., 'color palette', 'architecture style')"
                    },
                    "weight": {
                        "type": "number",
                        "default": 1.0,
                        "minimum": 0.0,
                        "maximum": 2.0,
                        "description": "Influence weight — higher means more influence on generation"
                    },
                    "ref_id": {
                        "type": "string",
                        "description": "Reference ID (required for 'remove')"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let action_str = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: action"))?;

        let action = match action_str {
            "add" => {
                let image = args["image"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("'add' action requires 'image' parameter"))?;
                let label = args["label"].as_str().map(String::from);
                let weight = args["weight"].as_f64().unwrap_or(1.0) as f32;
                ReferenceBoardAction::Add {
                    image: image.to_string(),
                    label,
                    weight,
                }
            }
            "remove" => {
                let ref_id = args["ref_id"].as_str().ok_or_else(|| {
                    anyhow::anyhow!("'remove' action requires 'ref_id' parameter")
                })?;
                ReferenceBoardAction::Remove {
                    ref_id: ref_id.to_string(),
                }
            }
            "list" => ReferenceBoardAction::List,
            "clear" => ReferenceBoardAction::Clear,
            other => return Err(anyhow::anyhow!("Unknown action: {}", other)),
        };

        let cmd = GenCommand::ReferenceBoard { action };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::ReferenceBoardResult { result_json } => Ok(result_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Reference board updated".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_panorama_to_world (AI3.4)
// ---------------------------------------------------------------------------

pub struct GenPanoramaToWorldTool {
    bridge: Arc<GenBridge>,
}

impl GenPanoramaToWorldTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPanoramaToWorldTool {
    fn name(&self) -> &str {
        "gen_panorama_to_world"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_panorama_to_world".to_string(),
            description: "Plan a world layout from a 360° equirectangular panorama. Reads the sky and ground colours and the skyline at each bearing, and plans a blockout with a landmark region wherever something rises above the horizon (terrain, biome and time of day follow the image). Nothing is spawned: call gen_apply_blockout next. The image gives bearings and angles but no depth, so regions sit on one ring around the viewpoint.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image": {
                        "type": "string",
                        "description": "Path to an equirectangular panorama image, or its base64 data (optionally a data: URI)"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Additional guidance: layout style, density, or a biome to use instead of the one read from the ground colour"
                    },
                    "generate_beyond": {
                        "type": "boolean",
                        "default": false,
                        "description": "Also plan sparse walkable regions where the panorama shows open horizon"
                    }
                },
                "required": ["image"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let image = args["image"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: image"))?;

        let prompt = args["prompt"].as_str().map(String::from);
        let generate_beyond = args["generate_beyond"].as_bool().unwrap_or(false);

        let cmd = GenCommand::PanoramaToWorld {
            image: image.to_string(),
            prompt,
            generate_beyond,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::PanoramaWorldGenerated {
                world_name,
                regions_planned,
                spawn_point,
                analysis_json,
                notes,
            } => {
                let analysis: Value = serde_json::from_str(&analysis_json).unwrap_or(Value::Null);
                let result = json!({
                    "world_name": world_name,
                    "regions_planned": regions_planned,
                    "spawn_point": spawn_point,
                    "analysis": analysis,
                    "notes": notes,
                    "next_steps": [
                        "Review or adjust the plan, then call gen_apply_blockout to build it",
                        "Use gen_screenshot after applying to compare with the panorama"
                    ]
                });
                Ok(result.to_string())
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn create_multimodal_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenImageToLayoutTool::new(bridge.clone())),
        Box::new(GenMatchStyleTool::new(bridge.clone())),
        Box::new(GenReferenceBoardTool::new(bridge.clone())),
        Box::new(GenPanoramaToWorldTool::new(bridge)),
    ]
}
