//! MCP tool handlers for multi-file iterative world generation and sync/drift detection.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use localgpt_world_agent::mcp::{Tool, ToolSchema};

// ---------------------------------------------------------------------------
// gen_write_world_plan
// ---------------------------------------------------------------------------

pub struct GenWriteWorldPlanTool {
    bridge: Arc<GenBridge>,
}

impl GenWriteWorldPlanTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenWriteWorldPlanTool {
    fn name(&self) -> &str {
        "gen_write_world_plan"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_write_world_plan".to_string(),
            description: "Create SKILL.md, world.md, and root world.ron from a structured world plan. This is the first step in iterative world generation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "World name"
                    },
                    "description": {
                        "type": "string",
                        "description": "World description"
                    },
                    "generation_strategy": {
                        "type": "string",
                        "enum": ["blockout", "manual", "hybrid"],
                        "description": "How the world should be generated"
                    },
                    "regions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "center": {
                                    "type": "array",
                                    "items": { "type": "number" }
                                },
                                "size": {
                                    "type": "array",
                                    "items": { "type": "number" }
                                },
                                "estimated_entities": { "type": "integer" }
                            },
                            "required": ["id"]
                        },
                        "description": "Region definitions with id, center, size, and estimated entity count"
                    },
                    "constraints": {
                        "type": "object",
                        "description": "Generation constraints (e.g., max entities, performance budget)"
                    },
                    "environment": {
                        "type": "object",
                        "description": "Environment settings (lighting, fog, sky)"
                    },
                    "camera": {
                        "type": "object",
                        "description": "Initial camera configuration"
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
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
            .to_string();

        let description = args["description"].as_str().map(|s| s.to_string());
        let generation_strategy = args["generation_strategy"]
            .as_str()
            .unwrap_or("blockout")
            .to_string();
        let regions = args
            .get("regions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let constraints = args.get("constraints").cloned();
        let environment = args.get("environment").cloned();
        let camera = args.get("camera").cloned();

        let cmd = GenCommand::WriteWorldPlan {
            name,
            description,
            generation_strategy,
            regions,
            constraints,
            environment,
            camera,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::WorldPlanWritten {
                skill_md_path,
                world_md_path,
                world_ron_path,
            } => Ok(json!({
                "skill_md": skill_md_path,
                "world_md": world_md_path,
                "world_ron": world_ron_path,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("World plan written".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_write_region
// ---------------------------------------------------------------------------

pub struct GenWriteRegionTool {
    bridge: Arc<GenBridge>,
}

impl GenWriteRegionTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenWriteRegionTool {
    fn name(&self) -> &str {
        "gen_write_region"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_write_region".to_string(),
            description: "Write a .md + .ron file pair for a region. Files are held in memory until gen_save_world flushes them, unless flush=true.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region_id": {
                        "type": "string",
                        "description": "Region identifier"
                    },
                    "design_intent": {
                        "type": "string",
                        "description": "Design intent for the .md file describing this region"
                    },
                    "entities": {
                        "type": "array",
                        "description": "Entity definitions for the .ron file"
                    },
                    "bounds": {
                        "type": "object",
                        "description": "Optional bounding box for the region",
                        "properties": {
                            "center": {
                                "type": "array",
                                "items": { "type": "number" }
                            },
                            "size": {
                                "type": "array",
                                "items": { "type": "number" }
                            }
                        }
                    },
                    "flush": {
                        "type": "boolean",
                        "default": false,
                        "description": "Write files to disk immediately instead of buffering"
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
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: region_id"))?
            .to_string();

        let design_intent = args["design_intent"].as_str().unwrap_or("");
        let entities = args.get("entities").cloned().unwrap_or(json!([]));
        let bounds = args.get("bounds").cloned();
        let flush = args["flush"].as_bool().unwrap_or(false);

        // Build .md content from design_intent
        let md_content = if let Some(b) = &bounds {
            format!(
                "# Region: {}\n\n{}\n\n## Bounds\n\n```json\n{}\n```\n",
                region_id,
                design_intent,
                serde_json::to_string_pretty(b).unwrap_or_default()
            )
        } else {
            format!("# Region: {}\n\n{}\n", region_id, design_intent)
        };

        // Serialize entities to RON string for .ron content
        let ron_content = serde_json::to_string_pretty(&entities)?;

        let cmd = GenCommand::WriteRegion {
            region_id,
            md_content,
            ron_content,
            flush,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::RegionWritten {
                region_id,
                md_path,
                ron_path,
            } => Ok(json!({
                "region_id": region_id,
                "md_path": md_path,
                "ron_path": ron_path,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Region written".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_load_region
// ---------------------------------------------------------------------------

pub struct GenLoadRegionTool {
    bridge: Arc<GenBridge>,
}

impl GenLoadRegionTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenLoadRegionTool {
    fn name(&self) -> &str {
        "gen_load_region"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_load_region".to_string(),
            description: "Load a region .ron file and spawn its entities into the live Bevy scene."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region_id": {
                        "type": "string",
                        "description": "Region identifier to load"
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

        let cmd = GenCommand::LoadRegion {
            path: region_id.to_string(),
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::RegionLoaded {
                region_id,
                entity_count,
            } => Ok(json!({
                "region_id": region_id,
                "entity_count": entity_count,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Region loaded".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_unload_region
// ---------------------------------------------------------------------------

pub struct GenUnloadRegionTool {
    bridge: Arc<GenBridge>,
}

impl GenUnloadRegionTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenUnloadRegionTool {
    fn name(&self) -> &str {
        "gen_unload_region"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_unload_region".to_string(),
            description: "Remove all entities belonging to a region from the live scene. Used before re-generating a region.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region_id": {
                        "type": "string",
                        "description": "Region identifier to unload"
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
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: region_id"))?
            .to_string();

        let cmd = GenCommand::UnloadRegion { region_id };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::RegionUnloaded {
                region_id,
                entities_removed,
            } => Ok(json!({
                "region_id": region_id,
                "entities_removed": entities_removed,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Region unloaded".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_persist_blockout
// ---------------------------------------------------------------------------

pub struct GenPersistBlockoutTool {
    bridge: Arc<GenBridge>,
}

impl GenPersistBlockoutTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenPersistBlockoutTool {
    fn name(&self) -> &str {
        "gen_persist_blockout"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_persist_blockout".to_string(),
            description:
                "Save the current BlockoutSpec to layout/blockout.md and layout/blockout.ron."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        let cmd = GenCommand::PersistBlockout;
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BlockoutPersisted { md_path, ron_path } => Ok(json!({
                "md_path": md_path,
                "ron_path": ron_path,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Blockout persisted".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_write_behaviors
// ---------------------------------------------------------------------------

pub struct GenWriteBehaviorsTool {
    bridge: Arc<GenBridge>,
}

impl GenWriteBehaviorsTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenWriteBehaviorsTool {
    fn name(&self) -> &str {
        "gen_write_behaviors"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_write_behaviors".to_string(),
            description: "Write a behavior library .md + .ron file pair.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Behavior library name"
                    },
                    "behaviors": {
                        "type": "object",
                        "description": "Mapping of behavior name to behavior definition"
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
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
            .to_string();

        let behaviors = args.get("behaviors").cloned().unwrap_or(json!({}));

        // Build .md content
        let md_content = format!(
            "# Behaviors: {}\n\n```json\n{}\n```\n",
            name,
            serde_json::to_string_pretty(&behaviors)?
        );

        // Serialize to RON-compatible string
        let ron_content = serde_json::to_string_pretty(&behaviors)?;

        let cmd = GenCommand::WriteBehaviors {
            name: name.clone(),
            md_content,
            ron_content,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::BehaviorsWritten {
                name,
                md_path,
                ron_path,
            } => Ok(json!({
                "name": name,
                "md_path": md_path,
                "ron_path": ron_path,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Behaviors written".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_write_audio
// ---------------------------------------------------------------------------

pub struct GenWriteAudioTool {
    bridge: Arc<GenBridge>,
}

impl GenWriteAudioTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenWriteAudioTool {
    fn name(&self) -> &str {
        "gen_write_audio"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_write_audio".to_string(),
            description: "Write an audio spec .md + .ron file pair.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Audio spec name"
                    },
                    "ambience": {
                        "type": "array",
                        "description": "Ambient sound definitions"
                    },
                    "emitters": {
                        "type": "array",
                        "description": "Sound emitter definitions"
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
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
            .to_string();

        let ambience = args.get("ambience").cloned().unwrap_or(json!([]));
        let emitters = args.get("emitters").cloned().unwrap_or(json!([]));

        let audio_spec = json!({
            "ambience": ambience,
            "emitters": emitters,
        });

        // Build .md content
        let md_content = format!(
            "# Audio: {}\n\n```json\n{}\n```\n",
            name,
            serde_json::to_string_pretty(&audio_spec)?
        );

        // Serialize to RON-compatible string
        let ron_content = serde_json::to_string_pretty(&audio_spec)?;

        let cmd = GenCommand::WriteAudio {
            name: name.clone(),
            md_content,
            ron_content,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::AudioWritten {
                name,
                md_path,
                ron_path,
            } => Ok(json!({
                "name": name,
                "md_path": md_path,
                "ron_path": ron_path,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Audio written".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_check_drift
// ---------------------------------------------------------------------------

pub struct GenCheckDriftTool {
    bridge: Arc<GenBridge>,
}

impl GenCheckDriftTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenCheckDriftTool {
    fn name(&self) -> &str {
        "gen_check_drift"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_check_drift".to_string(),
            description: "Compare .md files, .ron files, and the live Bevy scene to detect inconsistencies. Returns a drift report showing which domains are in sync, which have drifted, and what changed.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Check a specific domain (e.g., 'region/forest', 'audio', 'behaviors') or omit for all"
                    },
                    "detail_level": {
                        "type": "string",
                        "enum": ["summary", "structural", "full"],
                        "default": "structural",
                        "description": "Level of detail in the drift report"
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let domain = args["domain"].as_str().map(|s| s.to_string());
        let detail_level = args["detail_level"]
            .as_str()
            .unwrap_or("structural")
            .to_string();

        let cmd = GenCommand::CheckDrift {
            domain,
            detail_level,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::DriftReport(report) => Ok(serde_json::to_string_pretty(&report)?),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Drift check completed".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_sync
// ---------------------------------------------------------------------------

pub struct GenSyncTool {
    bridge: Arc<GenBridge>,
}

impl GenSyncTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSyncTool {
    fn name(&self) -> &str {
        "gen_sync"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_sync".to_string(),
            description: "Reconcile drift between .md, .ron, and the live scene. Specify which representation is the source of truth. Preview mode shows what would change without applying.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Domain to sync (e.g., 'region/forest', 'audio', 'behaviors')"
                    },
                    "source": {
                        "type": "string",
                        "enum": ["md", "ron", "scene"],
                        "description": "Which representation is the source of truth"
                    },
                    "preview": {
                        "type": "boolean",
                        "default": true,
                        "description": "If true, show what would change without applying"
                    },
                    "resolve_conflicts": {
                        "type": "object",
                        "description": "Per-field conflict resolution (field name → source to use)"
                    }
                },
                "required": ["domain", "source"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let domain = args["domain"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: domain"))?
            .to_string();

        let source = args["source"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: source"))?
            .to_string();

        let preview = args["preview"].as_bool().unwrap_or(true);

        let resolve_conflicts = args
            .get("resolve_conflicts")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<HashMap<String, String>>()
            });

        let cmd = GenCommand::Sync {
            domain,
            source,
            preview,
            resolve_conflicts,
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::SyncPreview { domain, changes } => Ok(json!({
                "mode": "preview",
                "domain": domain,
                "changes": changes,
            })
            .to_string()),
            GenResponse::SyncApplied {
                domain,
                files_updated,
            } => Ok(json!({
                "mode": "applied",
                "domain": domain,
                "files_updated": files_updated,
            })
            .to_string()),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Sync completed".to_string()),
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
            description: "Report current generation phase and progress.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        let cmd = GenCommand::GenerationStatus {
            task_id: None,
            action: "status".to_string(),
        };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::GenerationStatusResult { status_json } => Ok(status_json),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("{}".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn create_multifile_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenWriteWorldPlanTool::new(bridge.clone())),
        Box::new(GenWriteRegionTool::new(bridge.clone())),
        Box::new(GenLoadRegionTool::new(bridge.clone())),
        Box::new(GenUnloadRegionTool::new(bridge.clone())),
        Box::new(GenPersistBlockoutTool::new(bridge.clone())),
        Box::new(GenWriteBehaviorsTool::new(bridge.clone())),
        Box::new(GenWriteAudioTool::new(bridge.clone())),
        Box::new(GenCheckDriftTool::new(bridge.clone())),
        Box::new(GenSyncTool::new(bridge.clone())),
        Box::new(GenGenerationStatusTool::new(bridge)),
    ]
}
