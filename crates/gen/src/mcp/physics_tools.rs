//! MCP tool handlers for P5: Physics Integration System.

use anyhow::Result;
use async_trait::async_trait;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use crate::physics;
use localgpt_world_agent::mcp::{Tool, ToolSchema};

// ---------------------------------------------------------------------------
// gen_set_physics
// ---------------------------------------------------------------------------

pub struct GenSetPhysicsTool {
    bridge: Arc<GenBridge>,
}

impl GenSetPhysicsTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
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
            name: "gen_set_physics".to_string(),
            description: "Enable physics on an entity with body type, mass, friction, and damping."
                .to_string(),
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
        let args: Value = serde_json::from_str(arguments)?;

        let entity_id = args["entity_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_id is required"))?
            .to_string();

        let body_type = match args["body_type"].as_str().unwrap_or("dynamic") {
            "static" => physics::BodyType::Static,
            "kinematic" => physics::BodyType::Kinematic,
            _ => physics::BodyType::Dynamic,
        };

        let params = physics::PhysicsParams {
            entity_id: entity_id.clone(),
            body_type,
            mass: args["mass"].as_f64().map(|v| v as f32),
            restitution: args["restitution"].as_f64().unwrap_or(0.3) as f32,
            friction: args["friction"].as_f64().unwrap_or(0.5) as f32,
            gravity_scale: args["gravity_scale"].as_f64().unwrap_or(1.0) as f32,
            linear_damping: args["linear_damping"].as_f64().unwrap_or(0.1) as f32,
            angular_damping: args["angular_damping"].as_f64().unwrap_or(0.1) as f32,
            lock_rotation: args["lock_rotation"].as_bool().unwrap_or(false),
        };

        let cmd = GenCommand::SetPhysics(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Physics enabled on '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Physics enabled on '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_collider
// ---------------------------------------------------------------------------

pub struct GenAddColliderTool {
    bridge: Arc<GenBridge>,
}

impl GenAddColliderTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
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
            name: "gen_add_collider".to_string(),
            description:
                "Add a collision shape to an entity. Supports box, sphere, capsule, cylinder, mesh."
                    .to_string(),
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
                        "description": "Collider shape"
                    },
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Dimensions [x, y, z] (auto-fit if omitted)"
                    },
                    "offset": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Offset from entity origin [x, y, z]"
                    },
                    "is_trigger": {
                        "type": "boolean",
                        "default": false,
                        "description": "Sensor (trigger only, no physics response)"
                    },
                    "visible_in_debug": {
                        "type": "boolean",
                        "default": true,
                        "description": "Show in debug view"
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

        let shape = match args["shape"].as_str().unwrap_or("box") {
            "sphere" => physics::ColliderShape::Sphere,
            "capsule" => physics::ColliderShape::Capsule,
            "cylinder" => physics::ColliderShape::Cylinder,
            "mesh" => physics::ColliderShape::Mesh,
            _ => physics::ColliderShape::Box,
        };

        let size = args["size"].as_array().map(|a| {
            Vec3::new(
                a[0].as_f64().unwrap_or(1.0) as f32,
                a[1].as_f64().unwrap_or(1.0) as f32,
                a[2].as_f64().unwrap_or(1.0) as f32,
            )
        });

        let offset = args["offset"]
            .as_array()
            .map(|a| {
                Vec3::new(
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or(Vec3::ZERO);

        let params = physics::ColliderParams {
            entity_id: entity_id.clone(),
            shape,
            size,
            offset,
            is_trigger: args["is_trigger"].as_bool().unwrap_or(false),
            visible_in_debug: args["visible_in_debug"].as_bool().unwrap_or(true),
        };

        let cmd = GenCommand::AddCollider(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Modified { name } => Ok(format!("Collider added to '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Collider added to '{}'", entity_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_joint
// ---------------------------------------------------------------------------

pub struct GenAddJointTool {
    bridge: Arc<GenBridge>,
}

impl GenAddJointTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
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
            name: "gen_add_joint".to_string(),
            description:
                "Create a physical constraint (joint) between two entities. Types: fixed, revolute, spherical, prismatic, spring."
                    .to_string(),
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
                        "default": [0, 0, 0],
                        "description": "Anchor on entity A (local space)"
                    },
                    "anchor_b": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Anchor on entity B (local space)"
                    },
                    "axis": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 1, 0],
                        "description": "Rotation/slide axis"
                    },
                    "limits": {
                        "type": "array",
                        "items": { "type": "number" },
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
                "required": ["entity_a", "entity_b", "joint_type"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let entity_a = args["entity_a"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_a is required"))?
            .to_string();
        let entity_b = args["entity_b"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("entity_b is required"))?
            .to_string();

        let joint_type = match args["joint_type"].as_str().unwrap_or("fixed") {
            "revolute" => physics::JointType::Revolute,
            "spherical" => physics::JointType::Spherical,
            "prismatic" => physics::JointType::Prismatic,
            "spring" => physics::JointType::Spring,
            _ => physics::JointType::Fixed,
        };

        let parse_vec3 = |key: &str, default: Vec3| -> Vec3 {
            args[key]
                .as_array()
                .map(|a| {
                    Vec3::new(
                        a[0].as_f64().unwrap_or(default.x as f64) as f32,
                        a[1].as_f64().unwrap_or(default.y as f64) as f32,
                        a[2].as_f64().unwrap_or(default.z as f64) as f32,
                    )
                })
                .unwrap_or(default)
        };

        let limits = args["limits"].as_array().map(|a| {
            Vec2::new(
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
            )
        });

        let params = physics::JointParams {
            entity_a: entity_a.clone(),
            entity_b: entity_b.clone(),
            joint_type,
            anchor_a: parse_vec3("anchor_a", Vec3::ZERO),
            anchor_b: parse_vec3("anchor_b", Vec3::ZERO),
            axis: parse_vec3("axis", Vec3::Y),
            limits,
            stiffness: args["stiffness"].as_f64().map(|v| v as f32),
            damping: args["damping"].as_f64().map(|v| v as f32),
        };

        let cmd = GenCommand::AddJoint(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!(
                "Joint '{}' created ({} <-> {})",
                name, entity_a, entity_b
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("Joint created ({} <-> {})", entity_a, entity_b)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_force
// ---------------------------------------------------------------------------

pub struct GenAddForceTool {
    bridge: Arc<GenBridge>,
}

impl GenAddForceTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
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
            name: "gen_add_force".to_string(),
            description: "Create a force field or apply an impulse. Types: directional, point_attract, point_repel, vortex, impulse.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "World position [x, y, z]"
                    },
                    "force_type": {
                        "type": "string",
                        "enum": ["directional", "point_attract", "point_repel", "vortex", "impulse"],
                        "default": "directional",
                        "description": "Force type"
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
                        "description": "Force direction [x, y, z] (directional type only)"
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
                        "description": "Affects player entity"
                    },
                    "continuous": {
                        "type": "boolean",
                        "default": true,
                        "description": "Continuous force (false = one-time)"
                    }
                },
                "required": ["position", "force_type"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let position = args["position"]
            .as_array()
            .map(|a| {
                Vec3::new(
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or(Vec3::ZERO);

        let force_type = match args["force_type"].as_str().unwrap_or("directional") {
            "point_attract" => physics::ForceType::PointAttract,
            "point_repel" => physics::ForceType::PointRepel,
            "vortex" => physics::ForceType::Vortex,
            "impulse" => physics::ForceType::Impulse,
            _ => physics::ForceType::Directional,
        };

        let direction = args["direction"].as_array().map(|a| {
            Vec3::new(
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
                a[2].as_f64().unwrap_or(1.0) as f32,
            )
        });

        let falloff = match args["falloff"].as_str().unwrap_or("none") {
            "linear" => physics::FalloffType::Linear,
            "quadratic" => physics::FalloffType::Quadratic,
            _ => physics::FalloffType::None,
        };

        let params = physics::ForceParams {
            position,
            force_type,
            strength: args["strength"].as_f64().unwrap_or(10.0) as f32,
            radius: args["radius"].as_f64().unwrap_or(5.0) as f32,
            direction,
            falloff,
            affects_player: args["affects_player"].as_bool().unwrap_or(true),
            continuous: args["continuous"].as_bool().unwrap_or(true),
        };

        let force_type_str = format!("{:?}", params.force_type);
        let cmd = GenCommand::AddForce(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!(
                "Force field '{}' ({}) created",
                name, force_type_str
            )),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok(format!("{} force field created", force_type_str)),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_gravity
// ---------------------------------------------------------------------------

pub struct GenSetGravityTool {
    bridge: Arc<GenBridge>,
}

impl GenSetGravityTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
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
            name: "gen_set_gravity".to_string(),
            description: "Control gravity direction and strength globally or per-entity/zone. Presets: earth (9.81), moon (1.62), mars (3.72), jupiter (24.79), zero (0).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity (global if omitted)"
                    },
                    "direction": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, -1, 0],
                        "description": "Gravity direction [x, y, z]"
                    },
                    "strength": {
                        "type": "number",
                        "default": 9.81,
                        "description": "Gravity strength (m/s^2)"
                    },
                    "zone_position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Create gravity zone at position [x, y, z]"
                    },
                    "zone_radius": {
                        "type": "number",
                        "description": "Gravity zone radius"
                    },
                    "transition_duration": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Transition duration in seconds"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let direction = args["direction"]
            .as_array()
            .map(|a| {
                Vec3::new(
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(-1.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or(Vec3::new(0.0, -1.0, 0.0));

        let zone_position = args["zone_position"].as_array().map(|a| {
            Vec3::new(
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
                a[2].as_f64().unwrap_or(0.0) as f32,
            )
        });

        let params = physics::GravityParams {
            entity_id: args["entity_id"].as_str().map(|s| s.to_string()),
            direction,
            strength: args["strength"].as_f64().unwrap_or(9.81) as f32,
            zone_position,
            zone_radius: args["zone_radius"].as_f64().map(|v| v as f32),
            transition_duration: args["transition_duration"].as_f64().unwrap_or(0.5) as f32,
        };

        let strength = params.strength;
        let cmd = GenCommand::SetGravity(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!(
                "Gravity zone '{}' created (strength: {} m/s^2)",
                name, strength
            )),
            GenResponse::EnvironmentSet => Ok(format!("Global gravity set to {} m/s^2", strength)),
            GenResponse::Modified { name } => Ok(format!("Gravity override set on '{}'", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Gravity configured".to_string()),
        }
    }
}

/// Create all P5 physics tools.
pub fn create_physics_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenSetPhysicsTool::new(bridge.clone())),
        Box::new(GenAddColliderTool::new(bridge.clone())),
        Box::new(GenAddJointTool::new(bridge.clone())),
        Box::new(GenAddForceTool::new(bridge.clone())),
        Box::new(GenSetGravityTool::new(bridge)),
    ]
}
