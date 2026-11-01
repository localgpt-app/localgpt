//! MCP tool handlers for P3: Terrain & Landscape System.

use anyhow::Result;
use async_trait::async_trait;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::gen3d::GenBridge;
use crate::gen3d::commands::*;
use crate::terrain;
use localgpt_world_agent::mcp::{Tool, ToolSchema};

// ---------------------------------------------------------------------------
// gen_add_terrain
// ---------------------------------------------------------------------------

pub struct GenAddTerrainTool {
    bridge: Arc<GenBridge>,
}

impl GenAddTerrainTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddTerrainTool {
    fn name(&self) -> &str {
        "gen_add_terrain"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_terrain".to_string(),
            description: "Generate procedural terrain from noise with automatic collision mesh."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [100, 100],
                        "description": "Terrain size in meters [x, z]"
                    },
                    "resolution": {
                        "type": "integer",
                        "default": 128,
                        "description": "Vertices per side"
                    },
                    "height_scale": {
                        "type": "number",
                        "default": 20.0,
                        "description": "Maximum height in meters"
                    },
                    "noise_type": {
                        "type": "string",
                        "enum": ["perlin", "simplex", "flat"],
                        "default": "perlin",
                        "description": "Noise algorithm"
                    },
                    "noise_octaves": {
                        "type": "integer",
                        "default": 4,
                        "description": "Number of noise octaves"
                    },
                    "noise_frequency": {
                        "type": "number",
                        "default": 0.02,
                        "description": "Noise frequency"
                    },
                    "seed": {
                        "type": "integer",
                        "description": "Random seed (optional)"
                    },
                    "material": {
                        "type": "string",
                        "enum": ["grass", "sand", "snow", "rock", "custom"],
                        "default": "grass",
                        "description": "Material preset"
                    },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "World position [x, y, z]"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let size = args["size"]
            .as_array()
            .map(|a| {
                Vec2::new(
                    a[0].as_f64().unwrap_or(100.0) as f32,
                    a[1].as_f64().unwrap_or(100.0) as f32,
                )
            })
            .unwrap_or(Vec2::splat(100.0));

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

        let noise_type = match args["noise_type"].as_str().unwrap_or("perlin") {
            "simplex" => terrain::NoiseType::Simplex,
            "flat" => terrain::NoiseType::Flat,
            _ => terrain::NoiseType::Perlin,
        };

        let material = match args["material"].as_str().unwrap_or("grass") {
            "sand" => terrain::TerrainMaterial::Sand,
            "snow" => terrain::TerrainMaterial::Snow,
            "rock" => terrain::TerrainMaterial::Rock,
            "custom" => terrain::TerrainMaterial::Custom,
            _ => terrain::TerrainMaterial::Grass,
        };

        let params = terrain::TerrainParams {
            size,
            resolution: args["resolution"].as_u64().unwrap_or(128) as u32,
            height_scale: args["height_scale"].as_f64().unwrap_or(20.0) as f32,
            noise_type,
            noise_octaves: args["noise_octaves"].as_u64().unwrap_or(4) as usize,
            noise_frequency: args["noise_frequency"].as_f64().unwrap_or(0.02) as f32,
            seed: args["seed"].as_u64().map(|v| v as u32),
            material,
            position,
        };

        let cmd = GenCommand::AddTerrain(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Terrain '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Terrain created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_water
// ---------------------------------------------------------------------------

pub struct GenAddWaterTool {
    bridge: Arc<GenBridge>,
}

impl GenAddWaterTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddWaterTool {
    fn name(&self) -> &str {
        "gen_add_water"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_water".to_string(),
            description: "Create a transparent animated water plane at a specified height."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "height": {
                        "type": "number",
                        "default": 0.0,
                        "description": "Water plane Y height"
                    },
                    "size": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [100, 100],
                        "description": "Water plane size [x, z]"
                    },
                    "color": {
                        "type": "string",
                        "default": "#2389da",
                        "description": "Water color (hex)"
                    },
                    "opacity": {
                        "type": "number",
                        "default": 0.7,
                        "description": "Transparency (0-1)"
                    },
                    "wave_speed": {
                        "type": "number",
                        "default": 1.0,
                        "description": "Wave animation speed"
                    },
                    "wave_height": {
                        "type": "number",
                        "default": 0.3,
                        "description": "Wave height amplitude"
                    },
                    "position": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "Center position [x, y, z]"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let size = args["size"]
            .as_array()
            .map(|a| {
                Vec2::new(
                    a[0].as_f64().unwrap_or(100.0) as f32,
                    a[1].as_f64().unwrap_or(100.0) as f32,
                )
            })
            .unwrap_or(Vec2::splat(100.0));

        let position = args["position"].as_array().map(|a| {
            Vec3::new(
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
                a[2].as_f64().unwrap_or(0.0) as f32,
            )
        });

        let params = terrain::WaterParams {
            height: args["height"].as_f64().unwrap_or(0.0) as f32,
            size,
            color: args["color"].as_str().unwrap_or("#2389da").to_string(),
            opacity: args["opacity"].as_f64().unwrap_or(0.7) as f32,
            wave_speed: args["wave_speed"].as_f64().unwrap_or(1.0) as f32,
            wave_height: args["wave_height"].as_f64().unwrap_or(0.3) as f32,
            position,
        };

        let cmd = GenCommand::AddWater(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Water '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Water plane created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_path
// ---------------------------------------------------------------------------

pub struct GenAddPathTool {
    bridge: Arc<GenBridge>,
}

impl GenAddPathTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddPathTool {
    fn name(&self) -> &str {
        "gen_add_path"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_path".to_string(),
            description: "Create a walkable path between waypoints with terrain-conforming mesh."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "points": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": { "type": "number" }
                        },
                        "description": "Waypoints [[x,y,z], ...]"
                    },
                    "width": {
                        "type": "number",
                        "default": 2.0,
                        "description": "Path width in meters"
                    },
                    "material": {
                        "type": "string",
                        "enum": ["stone", "dirt", "wood", "cobblestone", "custom"],
                        "default": "stone",
                        "description": "Path material"
                    },
                    "curved": {
                        "type": "boolean",
                        "default": true,
                        "description": "Use smooth curves between points"
                    },
                    "raised": {
                        "type": "number",
                        "default": 0.02,
                        "description": "Height above terrain"
                    },
                    "border": {
                        "type": "boolean",
                        "default": false,
                        "description": "Add stone border edges"
                    }
                },
                "required": ["points"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let points: Vec<Vec3> = args["points"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("points is required"))?
            .iter()
            .filter_map(|p| {
                p.as_array().map(|a| {
                    Vec3::new(
                        a[0].as_f64().unwrap_or(0.0) as f32,
                        a[1].as_f64().unwrap_or(0.0) as f32,
                        a[2].as_f64().unwrap_or(0.0) as f32,
                    )
                })
            })
            .collect();

        let material = match args["material"].as_str().unwrap_or("stone") {
            "dirt" => terrain::PathMaterial::Dirt,
            "wood" => terrain::PathMaterial::Wood,
            "cobblestone" => terrain::PathMaterial::Cobblestone,
            "custom" => terrain::PathMaterial::Custom,
            _ => terrain::PathMaterial::Stone,
        };

        let params = terrain::PathParams {
            points,
            width: args["width"].as_f64().unwrap_or(2.0) as f32,
            material,
            curved: args["curved"].as_bool().unwrap_or(true),
            raised: args["raised"].as_f64().unwrap_or(0.02) as f32,
            border: args["border"].as_bool().unwrap_or(false),
        };

        let cmd = GenCommand::AddPath(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Path '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Path created successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_add_foliage
// ---------------------------------------------------------------------------

pub struct GenAddFoliageTool {
    bridge: Arc<GenBridge>,
}

impl GenAddFoliageTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenAddFoliageTool {
    fn name(&self) -> &str {
        "gen_add_foliage"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_add_foliage".to_string(),
            description: "Scatter vegetation (trees, bushes, grass, flowers, rocks) across terrain using Poisson disk sampling.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "foliage_type": {
                        "type": "string",
                        "enum": ["tree", "bush", "grass", "flower", "rock"],
                        "default": "tree",
                        "description": "Type of foliage"
                    },
                    "center": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0, 0, 0],
                        "description": "Center of placement area [x, y, z]"
                    },
                    "radius": {
                        "type": "number",
                        "default": 30.0,
                        "description": "Placement area radius"
                    },
                    "density": {
                        "type": "number",
                        "default": 0.5,
                        "description": "Density (0-1, items per area)"
                    },
                    "scale_range": {
                        "type": "array",
                        "items": { "type": "number" },
                        "default": [0.8, 1.2],
                        "description": "Random scale range [min, max]"
                    },
                    "seed": {
                        "type": "integer",
                        "description": "Random seed"
                    },
                    "avoid_paths": {
                        "type": "boolean",
                        "default": true,
                        "description": "Avoid placing on paths"
                    },
                    "avoid_water": {
                        "type": "boolean",
                        "default": true,
                        "description": "Avoid placing in water"
                    },
                    "max_slope": {
                        "type": "number",
                        "default": 30.0,
                        "description": "Maximum terrain slope (degrees)"
                    }
                },
                "required": ["foliage_type"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let foliage_type = match args["foliage_type"].as_str().unwrap_or("tree") {
            "bush" => terrain::FoliageType::Bush,
            "grass" => terrain::FoliageType::Grass,
            "flower" => terrain::FoliageType::Flower,
            "rock" => terrain::FoliageType::Rock,
            _ => terrain::FoliageType::Tree,
        };

        let center = args["center"]
            .as_array()
            .map(|a| {
                Vec3::new(
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or(Vec3::ZERO);

        let scale_range = args["scale_range"]
            .as_array()
            .map(|a| {
                Vec2::new(
                    a[0].as_f64().unwrap_or(0.8) as f32,
                    a[1].as_f64().unwrap_or(1.2) as f32,
                )
            })
            .unwrap_or(Vec2::new(0.8, 1.2));

        let params = terrain::FoliageParams {
            foliage_type,
            area: terrain::FoliageArea {
                center,
                radius: args["radius"].as_f64().unwrap_or(30.0) as f32,
            },
            density: args["density"].as_f64().unwrap_or(0.5) as f32,
            scale_range,
            seed: args["seed"].as_u64(),
            avoid_paths: args["avoid_paths"].as_bool().unwrap_or(true),
            avoid_water: args["avoid_water"].as_bool().unwrap_or(true),
            max_slope: args["max_slope"].as_f64().unwrap_or(30.0) as f32,
        };

        let cmd = GenCommand::AddFoliage(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::Spawned { name, .. } => Ok(format!("Foliage '{}' created", name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Foliage scattered successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_set_sky
// ---------------------------------------------------------------------------

pub struct GenSetSkyTool {
    bridge: Arc<GenBridge>,
}

impl GenSetSkyTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenSetSkyTool {
    fn name(&self) -> &str {
        "gen_set_sky"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_set_sky".to_string(),
            description: "Configure sky, sun direction, ambient light, and fog.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "preset": {
                        "type": "string",
                        "enum": ["day", "sunset", "night", "overcast", "custom"],
                        "default": "day",
                        "description": "Sky preset"
                    },
                    "sun_altitude": {
                        "type": "number",
                        "description": "Sun angle above horizon (0-90 degrees)"
                    },
                    "sun_azimuth": {
                        "type": "number",
                        "description": "Sun compass direction (0=north)"
                    },
                    "sun_intensity": {
                        "type": "number",
                        "description": "Sun brightness multiplier"
                    },
                    "ambient_color": {
                        "type": "string",
                        "description": "Ambient light color (hex)"
                    },
                    "ambient_intensity": {
                        "type": "number",
                        "description": "Ambient light brightness"
                    },
                    "fog_enabled": {
                        "type": "boolean",
                        "default": false,
                        "description": "Enable distance fog"
                    },
                    "fog_color": {
                        "type": "string",
                        "default": "#c8d0d8",
                        "description": "Fog color (hex)"
                    },
                    "fog_start": {
                        "type": "number",
                        "default": 50.0,
                        "description": "Fog start distance"
                    },
                    "fog_end": {
                        "type": "number",
                        "default": 200.0,
                        "description": "Fog end distance"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let preset = match args["preset"].as_str().unwrap_or("day") {
            "sunset" => terrain::SkyPreset::Sunset,
            "night" => terrain::SkyPreset::Night,
            "overcast" => terrain::SkyPreset::Overcast,
            "custom" => terrain::SkyPreset::Custom,
            _ => terrain::SkyPreset::Day,
        };

        let params = terrain::SkyParams {
            preset,
            sun_altitude: args["sun_altitude"].as_f64().map(|v| v as f32),
            sun_azimuth: args["sun_azimuth"].as_f64().map(|v| v as f32),
            sun_intensity: args["sun_intensity"].as_f64().map(|v| v as f32),
            ambient_color: args["ambient_color"].as_str().map(|s| s.to_string()),
            ambient_intensity: args["ambient_intensity"].as_f64().map(|v| v as f32),
            fog_enabled: args["fog_enabled"].as_bool().unwrap_or(false),
            fog_color: args["fog_color"].as_str().unwrap_or("#c8d0d8").to_string(),
            fog_start: args["fog_start"].as_f64().unwrap_or(50.0) as f32,
            fog_end: args["fog_end"].as_f64().unwrap_or(200.0) as f32,
        };

        let preset_name = format!("{:?}", params.preset);
        let cmd = GenCommand::SetSky(params);
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::EnvironmentSet => Ok(format!("Sky set to '{}' preset", preset_name)),
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Sky configured successfully".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// gen_query_terrain_height
// ---------------------------------------------------------------------------

pub struct GenQueryTerrainHeightTool {
    bridge: Arc<GenBridge>,
}

impl GenQueryTerrainHeightTool {
    pub fn new(bridge: Arc<GenBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Tool for GenQueryTerrainHeightTool {
    fn name(&self) -> &str {
        "gen_query_terrain_height"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "gen_query_terrain_height".to_string(),
            description: "Query terrain height at one or more (x, z) coordinates. Returns the world Y height for each point, enabling accurate entity placement on terrain.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "points": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": { "type": "number" },
                            "minItems": 2,
                            "maxItems": 2
                        },
                        "description": "Array of [x, z] coordinates to query"
                    },
                    "x": {
                        "type": "number",
                        "description": "Single point X coordinate (shortcut for one point)"
                    },
                    "z": {
                        "type": "number",
                        "description": "Single point Z coordinate (shortcut for one point)"
                    }
                },
                "required": []
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;

        let mut points: Vec<[f32; 2]> = Vec::new();

        // Support batch: {"points": [[x,z], [x,z], ...]}
        if let Some(pts) = args["points"].as_array() {
            for p in pts {
                if let Some(arr) = p.as_array() {
                    let x = arr[0].as_f64().unwrap_or(0.0) as f32;
                    let z = arr[1].as_f64().unwrap_or(0.0) as f32;
                    points.push([x, z]);
                }
            }
        }

        // Support single: {"x": 10, "z": 20}
        if points.is_empty()
            && let (Some(x), Some(z)) = (args["x"].as_f64(), args["z"].as_f64())
        {
            points.push([x as f32, z as f32]);
        }

        if points.is_empty() {
            return Err(anyhow::anyhow!(
                "Provide either 'points' array or 'x'+'z' for a single query"
            ));
        }

        let cmd = GenCommand::QueryTerrainHeight { points };
        let response = self.bridge.send(cmd).await?;
        match response {
            GenResponse::TerrainHeights { heights } => {
                let results: Vec<Value> = heights
                    .iter()
                    .map(|[x, y, z]| json!({"x": *x, "y": *y, "z": *z}))
                    .collect();
                Ok(serde_json::to_string_pretty(&json!({"heights": results}))?)
            }
            GenResponse::Error { message } => Err(anyhow::anyhow!("{}", message)),
            _ => Ok("Unexpected response".to_string()),
        }
    }
}

/// Create all P3 terrain tools.
pub fn create_terrain_tools(bridge: Arc<GenBridge>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GenAddTerrainTool::new(bridge.clone())),
        Box::new(GenAddWaterTool::new(bridge.clone())),
        Box::new(GenAddPathTool::new(bridge.clone())),
        Box::new(GenAddFoliageTool::new(bridge.clone())),
        Box::new(GenSetSkyTool::new(bridge.clone())),
        Box::new(GenQueryTerrainHeightTool::new(bridge)),
    ]
}
