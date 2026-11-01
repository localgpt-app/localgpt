//! MCP tool handlers for P4: In-World Text & UI System.

use anyhow::Result;
use async_trait::async_trait;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use crate::ui;
use localgpt_world_agent::mcp::{Tool, ToolSchema};

// ---------------------------------------------------------------------------
// gen_add_sign
// ---------------------------------------------------------------------------

pub struct GenAddSignTool {
    bridge: Arc<GenBridge>,
}

impl GenAddSignTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddSignTool {
    fn name(&self) -> &str {
        "gen_add_sign"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_sign".to_string(),
            description: "Place readable text in the 3D world as a sign or billboard.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "World position [x, y, z]"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text content"
                    },
                    "font_size": {
                        "type": "number",
                        "default": 24.0,
                        "description": "Font size in world units"
                    },
                    "color": {
                        "type": "string",
                        "default": "#ffffff",
                        "description": "Text color (hex)"
                    },
                    "background_color": {
                        "type": "string",
                        "description": "Background color (hex with alpha, optional)"
                    },
                    "billboard": {
                        "type": "boolean",
                        "default": true,
                        "description": "Always face camera"
                    },
                    "max_width": {
                        "type": "number",
                        "description": "Word wrap width in world units"
                    },
                    "rotation": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Rotation (when billboard=false) [x, y, z]"
                    }
                },
                "required": ["position", "text"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let position = args["position"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("position is required"))?;

        let rotation = args["rotation"]
            .as_array()
            .map(|a| {
                Vec3::new(
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or(Vec3::ZERO);

        let params = ui::SignParams {
            position: Vec3::new(
                position[0].as_f64().unwrap_or(0.0) as f32,
                position[1].as_f64().unwrap_or(0.0) as f32,
                position[2].as_f64().unwrap_or(0.0) as f32,
            ),
            text: args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("text is required"))?
                .to_string(),
            font_size: args["font_size"].as_f64().unwrap_or(24.0) as f32,
            color: args["color"].as_str().unwrap_or("#ffffff").to_string(),
            background_color: args["background_color"].as_str().map(|s| s.to_string()),
            billboard: args["billboard"].as_bool().unwrap_or(true),
            max_width: args["max_width"].as_f64().map(|v| v as f32),
            rotation,
        };

        let cmd = GenCommand::AddSign(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Sign '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Sign created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_hud
// ---------------------------------------------------------------------------

pub struct GenAddHudTool {
    bridge: Arc<GenBridge>,
}

impl GenAddHudTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddHudTool {
    fn name(&self) -> &str {
        "gen_add_hud"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_hud".to_string(),
            description: "Add a persistent screen-space HUD element (score, health, timer, text)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "element_type": {
                        "type": "string",
                        "enum": ["score", "health", "text", "timer"],
                        "default": "score",
                        "description": "HUD element type"
                    },
                    "position": {
                        "type": "string",
                        "enum": ["top-left", "top-right", "bottom-left", "bottom-right", "center-top", "center-bottom"],
                        "default": "top-left",
                        "description": "Screen position"
                    },
                    "label": {
                        "type": "string",
                        "description": "Label prefix text"
                    },
                    "initial_value": {
                        "type": "string",
                        "default": "0",
                        "description": "Initial display value"
                    },
                    "font_size": {
                        "type": "number",
                        "default": 18.0,
                        "description": "Font size"
                    },
                    "color": {
                        "type": "string",
                        "default": "#ffffff",
                        "description": "Text color (hex)"
                    },
                    "id": {
                        "type": "string",
                        "description": "Unique ID for updates"
                    }
                },
                "required": ["element_type"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let element_type = match args["element_type"].as_str().unwrap_or("score") {
            "health" => ui::HudElementType::Health,
            "text" => ui::HudElementType::Text,
            "timer" => ui::HudElementType::Timer,
            _ => ui::HudElementType::Score,
        };

        let position = match args["position"].as_str().unwrap_or("top-left") {
            "top_right" | "top-right" => ui::HudPosition::TopRight,
            "bottom_left" | "bottom-left" => ui::HudPosition::BottomLeft,
            "bottom_right" | "bottom-right" => ui::HudPosition::BottomRight,
            "center_top" | "center-top" => ui::HudPosition::CenterTop,
            "center_bottom" | "center-bottom" => ui::HudPosition::CenterBottom,
            _ => ui::HudPosition::TopLeft,
        };

        let params = ui::HudParams {
            element_type,
            position,
            label: args["label"].as_str().map(|s| s.to_string()),
            initial_value: args["initial_value"].as_str().unwrap_or("0").to_string(),
            font_size: args["font_size"].as_f64().unwrap_or(18.0) as f32,
            color: args["color"].as_str().unwrap_or("#ffffff").to_string(),
            id: args["id"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::AddHud(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("HUD element '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("HUD element created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_label
// ---------------------------------------------------------------------------

pub struct GenAddLabelTool {
    bridge: Arc<GenBridge>,
}

impl GenAddLabelTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddLabelTool {
    fn name(&self) -> &str {
        "gen_add_label"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_label".to_string(),
            description:
                "Attach a floating name label to an entity that billboards toward the camera."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "text": {
                        "type": "string",
                        "description": "Label text"
                    },
                    "color": {
                        "type": "string",
                        "default": "#ffffff",
                        "description": "Text color (hex)"
                    },
                    "background_color": {
                        "type": "string",
                        "default": "#00000088",
                        "description": "Background color (hex with alpha)"
                    },
                    "offset_y": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Height above entity"
                    },
                    "font_size": {
                        "type": "number",
                        "default": 16.0,
                        "description": "Font size"
                    },
                    "visible_distance": {
                        "type": "number",
                        "default": 15.0,
                        "description": "Maximum visible distance"
                    }
                },
                "required": ["entity_id", "text"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let params = ui::LabelParams {
            entity_id: entity_id.clone(),
            text: args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("text is required"))?
                .to_string(),
            color: args["color"].as_str().unwrap_or("#ffffff").to_string(),
            background_color: args["background_color"]
                .as_str()
                .unwrap_or("#00000088")
                .to_string(),
            offset_y: args["offset_y"].as_f64().unwrap_or(0.5) as f32,
            font_size: args["font_size"].as_f64().unwrap_or(16.0) as f32,
            visible_distance: args["visible_distance"].as_f64().unwrap_or(15.0) as f32,
        };

        let cmd = GenCommand::AddLabel(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Label added to '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Label added to '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_tooltip
// ---------------------------------------------------------------------------

pub struct GenAddTooltipTool {
    bridge: Arc<GenBridge>,
}

impl GenAddTooltipTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddTooltipTool {
    fn name(&self) -> &str {
        "gen_add_tooltip"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_tooltip".to_string(),
            description:
                "Add a contextual tooltip to an entity that appears on proximity or look-at."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity name"
                    },
                    "text": {
                        "type": "string",
                        "description": "Tooltip text"
                    },
                    "trigger": {
                        "type": "string",
                        "enum": ["proximity", "look_at"],
                        "default": "proximity",
                        "description": "Trigger type"
                    },
                    "range": {
                        "type": "number",
                        "default": 3.0,
                        "description": "Trigger range"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["floating", "screen_center", "screen_bottom"],
                        "default": "floating",
                        "description": "Display style"
                    },
                    "color": {
                        "type": "string",
                        "default": "#ffffff",
                        "description": "Text color (hex)"
                    },
                    "duration": {
                        "type": "number",
                        "description": "Auto-dismiss after seconds"
                    }
                },
                "required": ["entity_id", "text"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let trigger = match args["trigger"].as_str().unwrap_or("proximity") {
            "look_at" => ui::TooltipTrigger::LookAt,
            _ => ui::TooltipTrigger::Proximity,
        };

        let style = match args["style"].as_str().unwrap_or("floating") {
            "screen_center" => ui::TooltipStyle::ScreenCenter,
            "screen_bottom" => ui::TooltipStyle::ScreenBottom,
            _ => ui::TooltipStyle::Floating,
        };

        let params = ui::TooltipParams {
            entity_id: entity_id.clone(),
            text: args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("text is required"))?
                .to_string(),
            trigger,
            range: args["range"].as_f64().unwrap_or(3.0) as f32,
            style,
            color: args["color"].as_str().unwrap_or("#ffffff").to_string(),
            duration: args["duration"].as_f64().map(|v| v as f32),
        };

        let cmd = GenCommand::AddTooltip(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Tooltip added to '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Tooltip added to '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_notification
// ---------------------------------------------------------------------------

pub struct GenAddNotificationTool {
    bridge: Arc<GenBridge>,
}

impl GenAddNotificationTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddNotificationTool {
    fn name(&self) -> &str {
        "gen_add_notification"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_notification".to_string(),
            description:
                "Show a transient notification message with animation (toast, banner, achievement)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Notification text"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["toast", "banner", "achievement"],
                        "default": "toast",
                        "description": "Display style"
                    },
                    "position": {
                        "type": "string",
                        "enum": ["top", "center", "bottom"],
                        "default": "top",
                        "description": "Screen position"
                    },
                    "duration": {
                        "type": "number",
                        "default": 3.0,
                        "description": "Display duration in seconds"
                    },
                    "color": {
                        "type": "string",
                        "default": "#ffffff",
                        "description": "Text color (hex)"
                    },
                    "icon": {
                        "type": "string",
                        "enum": ["none", "star", "coin", "key", "heart", "warning"],
                        "default": "none",
                        "description": "Icon type"
                    },
                    "sound": {
                        "type": "string",
                        "description": "Sound to play"
                    }
                },
                "required": ["text"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let style = match args["style"].as_str().unwrap_or("toast") {
            "banner" => ui::NotificationStyle::Banner,
            "achievement" => ui::NotificationStyle::Achievement,
            _ => ui::NotificationStyle::Toast,
        };

        let position = match args["position"].as_str().unwrap_or("top") {
            "center" => ui::NotificationPosition::Center,
            "bottom" => ui::NotificationPosition::Bottom,
            _ => ui::NotificationPosition::Top,
        };

        let icon = match args["icon"].as_str().unwrap_or("none") {
            "star" => ui::NotificationIcon::Star,
            "coin" => ui::NotificationIcon::Coin,
            "key" => ui::NotificationIcon::Key,
            "heart" => ui::NotificationIcon::Heart,
            "warning" => ui::NotificationIcon::Warning,
            _ => ui::NotificationIcon::None,
        };

        let params = ui::NotificationParams {
            text: args["text"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("text is required"))?
                .to_string(),
            style,
            position,
            duration: args["duration"].as_f64().unwrap_or(3.0) as f32,
            color: args["color"].as_str().unwrap_or("#ffffff").to_string(),
            icon,
            sound: args["sound"].as_str().map(|s| s.to_string()),
        };

        let cmd = GenCommand::AddNotification(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Notification '{}' shown", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Notification shown".to_string()),
        }
    }
}

/// Create all P4 UI tools.
pub fn create_ui_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenAddSignTool::new(bridge.clone())),
        Box::new(GenAddHudTool::new(bridge.clone())),
        Box::new(GenAddLabelTool::new(bridge.clone())),
        Box::new(GenAddTooltipTool::new(bridge.clone())),
        Box::new(GenAddNotificationTool::new(bridge)),
    ]
}
