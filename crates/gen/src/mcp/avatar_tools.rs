//! MCP tool handlers for P1: Avatar & Character System.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::character::*;
use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

// ---------------------------------------------------------------------------
// gen_spawn_player
// ---------------------------------------------------------------------------

/// Tool: Spawn a controllable player character.
pub struct GenSpawnPlayerTool {
    bridge: Arc<GenBridge>,
}

impl GenSpawnPlayerTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSpawnPlayerTool {
    fn name(&self) -> &str {
        "gen_spawn_player"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_spawn_player".to_string(),
            description: "Spawn a controllable player character with movement, camera, and collision. Only one player is allowed; calling again replaces the previous player.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 1, 0],
                        "description": "Spawn position [x, y, z]"
                    },
                    "rotation": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Spawn rotation in degrees [pitch, yaw, roll]"
                    },
                    "walk_speed": {
                        "type": "number",
                        "default": 5.0,
                        "description": "Walk speed in units per second"
                    },
                    "run_speed": {
                        "type": "number",
                        "default": 10.0,
                        "description": "Run speed in units per second"
                    },
                    "jump_force": {
                        "type": "number",
                        "default": 8.0,
                        "description": "Jump force (upward velocity)"
                    },
                    "camera_mode": {
                        "type": "string",
                        "enum": ["first_person", "third_person"],
                        "default": "third_person",
                        "description": "Camera perspective mode"
                    },
                    "camera_distance": {
                        "type": "number",
                        "default": 5.0,
                        "description": "Camera distance for third-person mode"
                    },
                    "collision_radius": {
                        "type": "number",
                        "default": 0.3,
                        "description": "Collision capsule radius"
                    },
                    "collision_height": {
                        "type": "number",
                        "default": 1.8,
                        "description": "Collision capsule height"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let params = SpawnPlayerParams {
            position: args["position"]
                .as_array()
                .map(|a| {
                    [
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(1.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    ]
                })
                .unwrap_or([0.0, 1.0, 0.0]),
            rotation: args["rotation"]
                .as_array()
                .map(|a| {
                    [
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(0.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]),
            walk_speed: args["walk_speed"].as_f64().unwrap_or(5.0) as f32,
            run_speed: args["run_speed"].as_f64().unwrap_or(10.0) as f32,
            jump_force: args["jump_force"].as_f64().unwrap_or(8.0) as f32,
            camera_mode: args["camera_mode"]
                .as_str()
                .unwrap_or("third_person")
                .to_string(),
            camera_distance: args["camera_distance"].as_f64().unwrap_or(5.0) as f32,
            collision_radius: args["collision_radius"].as_f64().unwrap_or(0.3) as f32,
            collision_height: args["collision_height"].as_f64().unwrap_or(1.8) as f32,
        };

        let cmd = GenCommand::SpawnPlayer(params);
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::Spawned { name, entity_id } => Ok(format!(
                "Spawned player '{}' (entity_id: {})",
                name, entity_id
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Player spawned successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_spawn_point
// ---------------------------------------------------------------------------

/// Tool: Set a spawn/respawn location.
pub struct GenSetSpawnPointTool {
    bridge: Arc<GenBridge>,
}

impl GenSetSpawnPointTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetSpawnPointTool {
    fn name(&self) -> &str {
        "gen_set_spawn_point"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_spawn_point".to_string(),
            description: "Set a spawn/respawn location for the player. Only one spawn point can be the default.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Spawn position [x, y, z]"
                    },
                    "rotation": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Spawn rotation in degrees [pitch, yaw, roll]"
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional name for the spawn point"
                    },
                    "is_default": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether this is the default spawn point"
                    }
                },
                "required": ["position"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let params = SpawnPointParams {
            position: args["position"]
                .as_array()
                .map(|a| {
                    [
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(1.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    ]
                })
                .ok_or_else(|| anyhow::anyhow!("position is required"))?,
            rotation: args["rotation"]
                .as_array()
                .map(|a| {
                    [
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(0.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]),
            name: args["name"].as_str().map(|s| s.to_string()),
            is_default: args["is_default"].as_bool().unwrap_or(true),
        };

        let cmd = GenCommand::SetSpawnPoint(params);
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Created spawn point '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Spawn point created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_npc
// ---------------------------------------------------------------------------

/// Tool: Create a non-player character.
pub struct GenAddNpcTool {
    bridge: Arc<GenBridge>,
}

impl GenAddNpcTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddNpcTool {
    fn name(&self) -> &str {
        "gen_add_npc"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_npc".to_string(),
            description: "Create a non-player character with optional patrol or wander behavior."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Spawn position [x, y, z]"
                    },
                    "name": {
                        "type": "string",
                        "description": "NPC display name"
                    },
                    "model": {
                        "type": "string",
                        "default": "default_humanoid",
                        "description": "Model type: 'default_humanoid' or asset URL"
                    },
                    "behavior": {
                        "type": "string",
                        "enum": ["idle", "patrol", "wander"],
                        "default": "idle",
                        "description": "NPC behavior type"
                    },
                    "patrol_points": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": { "type": "number" }
                        },
                        "description": "Patrol waypoints (required if behavior is patrol)"
                    },
                    "patrol_speed": {
                        "type": "number",
                        "default": 3.0,
                        "description": "Movement speed"
                    },
                    "dialogue_id": {
                        "type": "string",
                        "description": "Optional dialogue ID reference"
                    }
                },
                "required": ["position", "name"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let position = args["position"]
            .as_array()
            .map(|a| {
                [
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                ]
            })
            .ok_or_else(|| anyhow::anyhow!("position is required"))?;

        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("name is required"))?
            .to_string();

        let patrol_points: Vec<[f32; 3]> = args["patrol_points"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        p.as_array().map(|a| {
                            [
                                a[0].as_f64().unwrap_or(0.0) as f32,
                                a[1].as_f64().unwrap_or(0.0) as f32,
                                a[2].as_f64().unwrap_or(0.0) as f32,
                            ]
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let params = SpawnNpcParams {
            position,
            name,
            model: args["model"]
                .as_str()
                .unwrap_or("default_humanoid")
                .to_string(),
            behavior: args["behavior"].as_str().unwrap_or("idle").to_string(),
            patrol_points,
            patrol_speed: args["patrol_speed"].as_f64().unwrap_or(3.0) as f32,
            dialogue_id: args["dialogue_id"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::SpawnNpc(params);
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::Spawned { name, entity_id } => {
                Ok(format!("Created NPC '{}' (entity_id: {})", name, entity_id))
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("NPC created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_npc_dialogue
// ---------------------------------------------------------------------------

/// Tool: Attach dialogue to an NPC.
pub struct GenSetNpcDialogueTool {
    bridge: Arc<GenBridge>,
}

impl GenSetNpcDialogueTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetNpcDialogueTool {
    fn name(&self) -> &str {
        "gen_set_npc_dialogue"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_npc_dialogue".to_string(),
            description: "Attach a branching conversation tree to an NPC.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "npc_id": {
                        "type": "string",
                        "description": "NPC entity name/ID"
                    },
                    "nodes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "text": { "type": "string" },
                                "speaker": { "type": "string" },
                                "choices": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "text": { "type": "string" },
                                            "next_node_id": { "type": "string" }
                                        }
                                    }
                                }
                            }
                        },
                        "description": "Dialogue nodes"
                    },
                    "start_node": {
                        "type": "string",
                        "description": "Starting node ID"
                    },
                    "trigger": {
                        "type": "string",
                        "enum": ["proximity", "click"],
                        "default": "click",
                        "description": "How dialogue is triggered"
                    },
                    "trigger_radius": {
                        "type": "number",
                        "default": 3.0,
                        "description": "Trigger radius"
                    }
                },
                "required": ["npc_id", "nodes", "start_node"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let npc_id = args["npc_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("npc_id is required"))?
            .to_string();

        let nodes: Vec<DialogueNodeDef> = args["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("nodes is required"))?
            .iter()
            .filter_map(|n| {
                Some(DialogueNodeDef {
                    id: n["id"].as_str()?.to_string(),
                    text: n["text"].as_str()?.to_string(),
                    speaker: n["speaker"].as_str().map(|s| s.to_string()),
                    choices: n["choices"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|c| {
                                    Some(DialogueChoiceDef {
                                        text: c["text"].as_str()?.to_string(),
                                        next_node_id: c["next_node_id"]
                                            .as_str()
                                            .map(|s| s.to_string()),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect();

        let start_node = args["start_node"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("start_node is required"))?
            .to_string();

        let params = SetDialogueParams {
            npc_id,
            nodes,
            start_node,
            trigger: args["trigger"].as_str().unwrap_or("click").to_string(),
            trigger_radius: args["trigger_radius"].as_f64().unwrap_or(3.0) as f32,
        };

        let cmd = GenCommand::SetNpcDialogue(params);
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::Modified { name } => Ok(format!("Set dialogue for NPC '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Dialogue set successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_camera_mode
// ---------------------------------------------------------------------------

/// Tool: Switch or configure camera mode.
pub struct GenSetCameraModeTool {
    bridge: Arc<GenBridge>,
}

impl GenSetCameraModeTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetCameraModeTool {
    fn name(&self) -> &str {
        "gen_set_camera_mode"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_camera_mode".to_string(),
            description: "Switch or configure the player camera mode with smooth transitions."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["first_person", "third_person", "top_down", "fixed"],
                        "description": "Camera mode"
                    },
                    "distance": {
                        "type": "number",
                        "default": 5.0,
                        "description": "Distance from player (third_person/top_down)"
                    },
                    "pitch": {
                        "type": "number",
                        "default": -20.0,
                        "description": "Initial pitch in degrees (top_down: -60 recommended)"
                    },
                    "fov": {
                        "type": "number",
                        "default": 60.0,
                        "description": "Field of view in degrees"
                    },
                    "transition_duration": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Seconds to blend between modes"
                    },
                    "fixed_position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Camera position (fixed mode only)"
                    },
                    "fixed_look_at": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Look-at target (fixed mode only)"
                    }
                },
                "required": ["mode"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let params = SetCameraModeParams {
            mode: args["mode"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("mode is required"))?
                .to_string(),
            distance: args["distance"].as_f64().unwrap_or(5.0) as f32,
            pitch: args["pitch"].as_f64().unwrap_or(-20.0) as f32,
            fov: args["fov"].as_f64().unwrap_or(60.0) as f32,
            transition_duration: args["transition_duration"].as_f64().unwrap_or(0.5) as f32,
            fixed_position: args["fixed_position"].as_array().map(|a| {
                [
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                ]
            }),
            fixed_look_at: args["fixed_look_at"].as_array().map(|a| {
                [
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                ]
            }),
        };

        let mode_str = params.mode.clone();
        let cmd = GenCommand::SetPlayerCameraMode(params);
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::CameraSet => Ok(format!("Camera mode set to '{}'", mode_str)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Camera mode changed successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_npc_brain (AI2.1)
// ---------------------------------------------------------------------------

/// Tool: Attach an AI brain to an NPC for autonomous behavior.
pub struct GenSetNpcBrainTool {
    bridge: Arc<GenBridge>,
}

impl GenSetNpcBrainTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetNpcBrainTool {
    fn name(&self) -> &str {
        "gen_set_npc_brain"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_npc_brain".to_string(),
            description: "Attach an AI brain to an entity (usually an NPC) for autonomous decision-making: every tick_rate seconds a local Ollama model (LOCALGPT_GEN_OLLAMA_URL, default http://localhost:11434) picks its next action from what it perceives. Fails if Ollama isn't running or the model isn't pulled.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "NPC entity name"
                    },
                    "personality": {
                        "type": "string",
                        "default": "a friendly villager",
                        "description": "Personality description for the NPC brain"
                    },
                    "model": {
                        "type": "string",
                        "default": "llama3.2:3b",
                        "description": "Ollama model name for the brain"
                    },
                    "tick_rate": {
                        "type": "number",
                        "default": 2.0,
                        "description": "Seconds between brain decisions"
                    },
                    "perception_radius": {
                        "type": "number",
                        "default": 15.0,
                        "description": "How far the NPC can perceive (meters)"
                    },
                    "goals": {
                        "type": "array",
                        "items": { "type": "string" },
                        "default": [],
                        "description": "List of goals for the NPC"
                    },
                    "knowledge": {
                        "type": "array",
                        "items": { "type": "string" },
                        "default": [],
                        "description": "Facts the NPC knows"
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
            .ok_or_else(|| anyhow::anyhow!("entity is required"))?
            .to_string();

        let config = crate::character::npc_brain::NpcBrainConfig {
            personality: args["personality"]
                .as_str()
                .unwrap_or("a friendly villager")
                .to_string(),
            model: args["model"].as_str().unwrap_or("llama3.2:3b").to_string(),
            tick_rate: args["tick_rate"].as_f64().unwrap_or(2.0) as f32,
            perception_radius: args["perception_radius"].as_f64().unwrap_or(15.0) as f32,
            goals: args["goals"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            knowledge: args["knowledge"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        };

        crate::character::npc_brain::check_ollama_model(
            &crate::character::npc_brain::ollama_url(),
            &config.model,
        )
        .await?;

        let cmd = GenCommand::SetNpcBrain { entity, config };
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::NpcBrainSet {
                entity,
                model,
                tick_rate,
            } => Ok(format!(
                "Set brain for NPC '{}' (model: {}, tick_rate: {}s)",
                entity, model, tick_rate
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            other => Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_npc_observe (AI2.2)
// ---------------------------------------------------------------------------

/// Tool: Make an NPC observe/perceive the scene from its point of view.
pub struct GenNpcObserveTool {
    bridge: Arc<GenBridge>,
}

impl GenNpcObserveTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenNpcObserveTool {
    fn name(&self) -> &str {
        "gen_npc_observe"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_npc_observe".to_string(),
            description: "List what an NPC (or any entity) can perceive: named entities inside its view cone and perception radius (its brain's radius, else 20 m), nearest first, with distance and bearing (0 = ahead, positive = right). With a question, the NPC's Ollama model answers it from that perception. This is a scene query, not a rendered image.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "NPC entity name"
                    },
                    "question": {
                        "type": "string",
                        "description": "Optional question for the NPC to answer about what it perceives"
                    },
                    "fov": {
                        "type": "number",
                        "default": 120.0,
                        "description": "View cone in degrees around the entity's forward (360 = all around)"
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
            .ok_or_else(|| anyhow::anyhow!("entity is required"))?
            .to_string();
        let question = args["question"].as_str().map(|s| s.to_string());
        let fov = args["fov"].as_f64().unwrap_or(120.0) as f32;

        let cmd = GenCommand::NpcObserve { entity, fov };
        let (entity, position, radius, perceived, model) = match self.bridge.send(cmd).await? {
            GenResponse::NpcObservation {
                entity,
                position,
                radius,
                perceived_json,
                model,
            } => (
                entity,
                position,
                radius,
                serde_json::from_str::<Value>(&perceived_json).unwrap_or(Value::Null),
                model,
            ),
            GenResponse::Error { message } => return Err(anyhow::anyhow!("{}", message)),
            other => return Err(anyhow::anyhow!("Unexpected response: {:?}", other)),
        };

        let mut result = json!({
            "entity": entity,
            "position": position,
            "radius": radius,
            "fov": fov,
            "perceived": perceived,
        });
        if let Some(question) = question {
            let model = model.unwrap_or_else(|| "llama3.2:3b".to_string());
            let prompt = format!(
                "You are '{entity}' at {position:?}. Within {radius} m in view you perceive \
                 (distance in m, bearing in degrees, 0 = ahead, positive = right):\n{perceived}\n\n\
                 Question: {question}"
            );
            match crate::character::npc_brain::ask_ollama(
                &crate::character::npc_brain::ollama_url(),
                &model,
                "You are an NPC in a 3D world. Answer briefly, in character, only from what you perceive.",
                &prompt,
            )
            .await
            {
                Ok(answer) => result["answer"] = json!(answer),
                Err(e) => {
                    result["answer"] = Value::Null;
                    result["answer_error"] = json!(format!("{model} via Ollama: {e:#}"));
                }
            }
        }
        Ok(result.to_string())
    }
}

// ---------------------------------------------------------------------------
// gen_set_npc_memory (AI2.3)
// ---------------------------------------------------------------------------

/// Tool: Configure persistent memory for an NPC.
pub struct GenSetNpcMemoryTool {
    bridge: Arc<GenBridge>,
}

impl GenSetNpcMemoryTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetNpcMemoryTool {
    fn name(&self) -> &str {
        "gen_set_npc_memory"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_npc_memory".to_string(),
            description: "Configure persistent memory for an NPC. Memories persist across save/load and influence brain decisions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity": {
                        "type": "string",
                        "description": "NPC entity name"
                    },
                    "capacity": {
                        "type": "integer",
                        "default": 50,
                        "description": "Maximum number of memories to retain"
                    },
                    "initial_memories": {
                        "type": "array",
                        "items": { "type": "string" },
                        "default": [],
                        "description": "Seed memories to give the NPC"
                    },
                    "auto_memorize": {
                        "type": "boolean",
                        "default": true,
                        "description": "Whether to automatically create memories from interactions"
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
            .ok_or_else(|| anyhow::anyhow!("entity is required"))?
            .to_string();

        let capacity = args["capacity"].as_u64().unwrap_or(50) as usize;

        let initial_memories: Vec<String> = args["initial_memories"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let auto_memorize = args["auto_memorize"].as_bool().unwrap_or(true);

        let cmd = GenCommand::SetNpcMemory {
            entity,
            capacity,
            initial_memories,
            auto_memorize,
        };
        let response = self.bridge.send(cmd).await?;

        match response {
            GenResponse::NpcMemorySet {
                entity,
                capacity,
                initial_count,
            } => Ok(format!(
                "Set memory for NPC '{}' (capacity: {}, initial memories: {})",
                entity, capacity, initial_count
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("NPC memory configured successfully".to_string()),
        }
    }
}

/// Create all P1 character tools (player, spawn points, NPCs, dialogue, camera)
/// plus AI2 NPC intelligence tools (brain, observe, memory).
pub fn create_character_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenSpawnPlayerTool::new(bridge.clone())),
        Box::new(GenSetSpawnPointTool::new(bridge.clone())),
        Box::new(GenAddNpcTool::new(bridge.clone())),
        Box::new(GenSetNpcDialogueTool::new(bridge.clone())),
        Box::new(GenSetCameraModeTool::new(bridge.clone())),
        // AI2 NPC Intelligence
        Box::new(GenSetNpcBrainTool::new(bridge.clone())),
        Box::new(GenNpcObserveTool::new(bridge.clone())),
        Box::new(GenSetNpcMemoryTool::new(bridge)),
    ]
}
