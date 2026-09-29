//! MCP tool handlers for P2: Interaction & Trigger System.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use crate::interaction;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;

// ---------------------------------------------------------------------------
// gen_add_trigger
// ---------------------------------------------------------------------------

/// Tool: Add an interaction trigger and action.
pub struct GenAddTriggerTool {
    bridge: Arc<GenBridge>,
}

impl GenAddTriggerTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddTriggerTool {
    fn name(&self) -> &str {
        "gen_add_trigger"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_trigger".to_string(),
            description: "Add an interaction trigger and action to an entity; saved with the world. Combines an event (start, proximity, click, area enter/exit, collision, timer) with an action (show text, show/enable, hide/disable, toggle visibility, remove/destroy, animate, teleport; Gen-only: play sound, toggle state, add score, spawn). A second trigger for the same event on an entity replaces the first.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "trigger_type": {
                        "type": "string",
                        "enum": ["start", "proximity", "click", "area_enter", "area_exit", "collision", "timer"],
                        "description": "Type of trigger"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["show_text", "show", "enable", "hide", "disable", "toggle", "remove", "destroy", "animate", "teleport", "play_sound", "toggle_state", "add_score", "spawn"],
                        "description": "Action to perform"
                    },
                    "radius": { "type": "number", "default": 3.0, "description": "Trigger radius" },
                    "cooldown": { "type": "number", "default": 1.0, "description": "Cooldown between triggers" },
                    "interval": { "type": "number", "description": "Timer interval (timer trigger only)" },
                    "max_distance": { "type": "number", "default": 5.0, "description": "Max click distance" },
                    "once": { "type": "boolean", "default": false, "description": "Fire only once" },
                    "destination": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Teleport destination [x, y, z] (teleport action)"
                    },
                    "text": { "type": "string", "description": "Text content (show_text action)" },
                    "amount": { "type": "integer", "default": 1, "description": "Score amount (add_score action)" },
                    "state_key": { "type": "string", "description": "State key (toggle_state action)" },
                    "category": { "type": "string", "default": "points", "description": "Score category (add_score action)" },
                    "requires_item": { "type": "string", "description": "Item ID required in player inventory for this trigger to fire" },
                    "duration": { "type": "number", "description": "Seconds for animate and show_text" },
                    "volume": {
                        "type": "object",
                        "description": "Area for area_enter/area_exit, in the entity's own frame: {\"shape\": \"box\", \"half_extents\": [x, y, z]} or {\"shape\": \"sphere\", \"radius\": r}. Default: the entity's shape box, or a sphere of radius 3"
                    }
                },
                "required": ["entity_id", "trigger_type", "action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let params = interaction::AddTriggerParams {
            entity_id: entity_id.clone(),
            trigger_type: match args["trigger_type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("trigger_type is required"))?
            {
                "start" => interaction::TriggerType::Start,
                "click" => interaction::TriggerType::Click,
                "area_enter" => interaction::TriggerType::AreaEnter,
                "area_exit" => interaction::TriggerType::AreaExit,
                "collision" => interaction::TriggerType::Collision,
                "timer" => interaction::TriggerType::Timer,
                _ => interaction::TriggerType::Proximity,
            },
            action: match args["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("action is required"))?
            {
                "teleport" => interaction::TriggerAction::Teleport,
                "play_sound" => interaction::TriggerAction::PlaySound,
                "show_text" => interaction::TriggerAction::ShowText,
                "toggle_state" => interaction::TriggerAction::ToggleState,
                "spawn" => interaction::TriggerAction::Spawn,
                "destroy" | "remove" => interaction::TriggerAction::Destroy,
                "toggle" => interaction::TriggerAction::Toggle,
                "add_score" => interaction::TriggerAction::AddScore,
                "enable" | "show" => interaction::TriggerAction::Enable,
                "disable" | "hide" => interaction::TriggerAction::Disable,
                _ => interaction::TriggerAction::Animate,
            },
            radius: args["radius"].as_f64().map(|v| v as f32),
            cooldown: args["cooldown"].as_f64().map(|v| v as f32),
            interval: args["interval"].as_f64().map(|v| v as f32),
            max_distance: args["max_distance"].as_f64().map(|v| v as f32),
            prompt_text: args["prompt_text"].as_str().map(|s| s.to_string()),
            once: args["once"].as_bool().unwrap_or(false),
            destination: args["destination"].as_array().map(|a| {
                [
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                ]
            }),
            text: args["text"].as_str().map(|s| s.to_string()),
            amount: args["amount"].as_i64().map(|v| v as i32),
            state_key: args["state_key"].as_str().map(|s| s.to_string()),
            category: args["category"].as_str().map(|s| s.to_string()),
            requires_item: args["requires_item"].as_str().map(|s| s.to_string()),
            duration: args["duration"].as_f64().map(|v| v as f32),
            volume: match args.get("volume") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    serde_json::from_value(v.clone())
                        .map_err(|e| anyhow::anyhow!("volume: {e}"))?,
                ),
            },
        };

        let cmd = GenCommand::AddTrigger(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Trigger added to entity '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Trigger added to entity '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_teleporter
// ---------------------------------------------------------------------------

/// Tool: Create a portal that teleports the player.
pub struct GenAddTeleporterTool {
    bridge: Arc<GenBridge>,
}

impl GenAddTeleporterTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddTeleporterTool {
    fn name(&self) -> &str {
        "gen_add_teleporter"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_teleporter".to_string(),
            description:
                "Create a portal that teleports the player to a destination when they step into it."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Portal position [x, y, z]"
                    },
                    "destination": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Teleport destination [x, y, z]"
                    },
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [2, 3, 2],
                        "description": "Portal trigger size [x, y, z]"
                    },
                    "effect": {
                        "type": "string",
                        "enum": ["none", "fade", "particles"],
                        "default": "fade",
                        "description": "Teleport effect"
                    },
                    "sound": {
                        "type": "string",
                        "description": "Optional sound effect"
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional label text above portal"
                    }
                },
                "required": ["position", "destination"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let position = args["position"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("position is required"))?;
        let destination = args["destination"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("destination is required"))?;

        let size = args["size"]
            .as_array()
            .map(|a| {
                [
                    a[0].as_f64().unwrap_or(2.0) as f32,
                    a[1].as_f64().unwrap_or(3.0) as f32,
                    a[2].as_f64().unwrap_or(2.0) as f32,
                ]
            })
            .unwrap_or([2.0, 3.0, 2.0]);

        let params = interaction::TeleporterParams {
            position: [
                position[0].as_f64().unwrap_or(0.0) as f32,
                position[1].as_f64().unwrap_or(0.0) as f32,
                position[2].as_f64().unwrap_or(0.0) as f32,
            ],
            destination: [
                destination[0].as_f64().unwrap_or(0.0) as f32,
                destination[1].as_f64().unwrap_or(0.0) as f32,
                destination[2].as_f64().unwrap_or(0.0) as f32,
            ],
            size,
            effect: match args["effect"].as_str().unwrap_or("none") {
                "fade" => interaction::TeleportEffect::Fade,
                "particles" => interaction::TeleportEffect::Particles,
                _ => interaction::TeleportEffect::None,
            },
            sound: args["sound"].as_str().map(|s| s.to_string()),
            label: args["label"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::AddTeleporter(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Teleporter '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Teleporter created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_collectible
// ---------------------------------------------------------------------------

/// Tool: Make an entity collectible.
pub struct GenAddCollectibleTool {
    bridge: Arc<GenBridge>,
}

impl GenAddCollectibleTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddCollectibleTool {
    fn name(&self) -> &str {
        "gen_add_collectible"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_collectible".to_string(),
            description: "Make an entity collectible with score value, pickup effects, and optional respawning.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "value": {
                        "type": "integer",
                        "default": 1,
                        "description": "Score value"
                    },
                    "category": {
                        "type": "string",
                        "default": "points",
                        "description": "Score category"
                    },
                    "pickup_sound": {
                        "type": "string",
                        "description": "Optional pickup sound"
                    },
                    "pickup_effect": {
                        "type": "string",
                        "enum": ["none", "sparkle", "dissolve"],
                        "default": "sparkle",
                        "description": "Pickup visual effect"
                    },
                    "respawn_time": {
                        "type": "number",
                        "description": "Seconds until respawn (null = no respawn)"
                    }
                },
                "required": ["entity_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let params = interaction::CollectibleParams {
            entity_id: entity_id.clone(),
            value: args["value"].as_i64().unwrap_or(1) as i32,
            category: args["category"].as_str().unwrap_or("points").to_string(),
            pickup_sound: args["pickup_sound"].as_str().map(|s| s.to_string()),
            pickup_effect: match args["pickup_effect"].as_str().unwrap_or("sparkle") {
                "none" => interaction::PickupEffect::None,
                "dissolve" => interaction::PickupEffect::Dissolve,
                _ => interaction::PickupEffect::Sparkle,
            },
            respawn_time: args["respawn_time"].as_f64().map(|v| v as f32),
        };

        let cmd = GenCommand::AddCollectible(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Entity '{}' is now collectible", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Entity '{}' is now collectible", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_door
// ---------------------------------------------------------------------------

/// Tool: Add interactive door behavior.
pub struct GenAddDoorTool {
    bridge: Arc<GenBridge>,
}

impl GenAddDoorTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddDoorTool {
    fn name(&self) -> &str {
        "gen_add_door"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_door".to_string(),
            description: "Add interactive door behavior to an entity with open/close logic and optional key requirement.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity (should have hinge at origin)"
                    },
                    "trigger": {
                        "type": "string",
                        "enum": ["proximity", "click"],
                        "default": "proximity",
                        "description": "How door is triggered"
                    },
                    "open_angle": {
                        "type": "number",
                        "default": 90.0,
                        "description": "Door opening angle in degrees"
                    },
                    "open_duration": {
                        "type": "number",
                        "default": 1.5,
                        "description": "Animation duration"
                    },
                    "auto_close": {
                        "type": "boolean",
                        "default": true,
                        "description": "Auto-close after delay"
                    },
                    "auto_close_delay": {
                        "type": "number",
                        "default": 3.0,
                        "description": "Seconds before auto-close"
                    },
                    "requires_key": {
                        "type": "string",
                        "description": "Required key item name"
                    }
                },
                "required": ["entity_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let params = interaction::DoorParams {
            entity_id: entity_id.clone(),
            trigger: match args["trigger"].as_str().unwrap_or("proximity") {
                "click" => interaction::DoorTrigger::Click,
                _ => interaction::DoorTrigger::Proximity,
            },
            open_angle: args["open_angle"].as_f64().unwrap_or(90.0) as f32,
            open_duration: args["open_duration"].as_f64().unwrap_or(1.5) as f32,
            auto_close: args["auto_close"].as_bool().unwrap_or(true),
            auto_close_delay: args["auto_close_delay"].as_f64().unwrap_or(3.0) as f32,
            sound_open: args["sound_open"].as_str().map(|s| s.to_string()),
            sound_close: args["sound_close"].as_str().map(|s| s.to_string()),
            requires_key: args["requires_key"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::AddDoor(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Door behavior added to '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Door behavior added to '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_link_entities
// ---------------------------------------------------------------------------

/// Tool: Wire one entity's event to another's action.
pub struct GenLinkEntitiesTool {
    bridge: Arc<GenBridge>,
}

impl GenLinkEntitiesTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenLinkEntitiesTool {
    fn name(&self) -> &str {
        "gen_link_entities"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_link_entities".to_string(),
            description: "Wire one entity's event to trigger another entity's action. Enables chain reactions and puzzle logic.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "source_id": {
                        "type": "string",
                        "description": "Source entity name"
                    },
                    "source_event": {
                        "type": "string",
                        "description": "Event type: 'clicked', 'state_changed:is_active', 'proximity_entered', etc."
                    },
                    "target_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "target_action": {
                        "type": "string",
                        "description": "Action to perform: 'toggle_state:is_open', 'play_animation:open', 'enable', 'disable', 'destroy'"
                    },
                    "condition": {
                        "type": "string",
                        "description": "Optional boolean expression: 'source.is_active AND other.is_active'"
                    }
                },
                "required": ["source_id", "source_event", "target_id", "target_action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let source_id = args["source_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("source_id is required"))?
            .to_string();
        let target_id = args["target_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("target_id is required"))?
            .to_string();

        let params = interaction::LinkEntitiesParams {
            source_id: source_id.clone(),
            source_event: args["source_event"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("source_event is required"))?
                .to_string(),
            target_id: target_id.clone(),
            target_action: args["target_action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("target_action is required"))?
                .to_string(),
            condition: args["condition"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::LinkEntities(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Linked '{}' -> '{}'", source_id, name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Linked '{}' -> '{}'", source_id, target_id)),
        }
    }
}

/// Create all P2 interaction tools.
pub fn create_interaction_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenAddTriggerTool::new(bridge.clone())),
        Box::new(GenAddTeleporterTool::new(bridge.clone())),
        Box::new(GenAddCollectibleTool::new(bridge.clone())),
        Box::new(GenAddDoorTool::new(bridge.clone())),
        Box::new(GenLinkEntitiesTool::new(bridge)),
    ]
}
