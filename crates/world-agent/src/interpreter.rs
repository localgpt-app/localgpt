//! The world agent interpreter, ported from LocalGPT Verse's `agent.rs` /
//! `agent_types.rs` (Apache-2.0) and first hardened in LocalGPT MD.
//!
//! The model builds a place's contents itself through tool calls:
//! primitives, real pack assets, lights. This is the pure, in-process
//! interpreter whose state is the place's entities in **platform-local
//! coordinates** (y = 0 is the platform surface): it applies
//! [`AgentCommand`]s to a name-keyed list of [`wt::WorldEntity`]s with no
//! Bevy, no bridge, no replay machinery. Verse (live scene) and MD
//! (compile-to-manifest) both consume it — the executor side that touches a
//! live Bevy scene stays app-side.
//!
//! The command types and interpreter are ungated so the cache contract is
//! testable without the model (Verse `agent_types.rs` precedent). Much of
//! the surface is only called under `feature = "llm"` — dead-code warnings
//! in other configurations are expected and allowed, as in Verse.
#![allow(dead_code)]

use std::collections::HashMap;

use localgpt_world_types as wt;
use serde::{Deserialize, Serialize};

use crate::assets::{self, AssetManifest};

/// Cap on entities per section — a region is one small platform, and this
/// bounds a runaway scatter loop the same way Verse's tier budgets do.
const MAX_ENTITIES: usize = 64;
/// Local positions stay on the platform's neighbourhood: this far from the
/// centre horizontally…
const MAX_RADIUS: f32 = 12.0;
/// …and between the surface (y = 0) and this high above it.
const MAX_HEIGHT: f32 = 25.0;
/// Point-light cap (lumens) and directional cap (lux) — Verse's no-Comfort
/// bounds, sized down for one small region.
const MAX_POINT_INTENSITY: f32 = 300_000.0;
const MAX_DIRECTIONAL_INTENSITY: f32 = 15_000.0;

// ---------------------------------------------------------------------------
// Commands (the model-facing protocol, serde-tagged like Verse's)
// ---------------------------------------------------------------------------

/// A single tool call, as parsed from the model. Also the sidecar-side shape
/// of a recorded call — kept serde so a session log stays inspectable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentCommand {
    SpawnPrimitive(SpawnPrimitiveCmd),
    PlaceAsset(PlaceAssetCmd),
    ScatterField(ScatterFieldCmd),
    ModifyEntity(ModifyEntityCmd),
    DeleteEntity { name: String },
    SetLight(SetLightCmd),
    SceneInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPrimitiveCmd {
    pub name: String,
    pub shape: PrimitiveShape,
    #[serde(default)]
    pub dimensions: HashMap<String, f32>,
    #[serde(default = "zero3")]
    pub position: [f32; 3],
    #[serde(default = "zero3")]
    pub rotation_degrees: [f32; 3],
    #[serde(default = "one3")]
    pub scale: [f32; 3],
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    #[serde(default)]
    pub metallic: f32,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    #[serde(default = "zero4")]
    pub emissive: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PrimitiveShape {
    Cuboid,
    Sphere,
    Cylinder,
    Cone,
    Torus,
    Plane,
}

/// Place one curated pack asset. Two-level vocabulary (Verse's): the model
/// names a semantic *kind*; the session resolves it to a concrete `asset`
/// file before the command is applied, so the build is self-contained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceAssetCmd {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// The resolved manifest file (relative to the models directory).
    #[serde(default)]
    pub asset: String,
    #[serde(default = "zero3")]
    pub position: [f32; 3],
    #[serde(default = "zero3")]
    pub rotation_degrees: [f32; 3],
    #[serde(default = "one_f")]
    pub scale: f32,
}

/// Scatter many assets of one kind around a position in one command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScatterFieldCmd {
    /// Unique prefix; instances are named `{name}_1` … `{name}_{count}`.
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// Resolved variant files, cycled across the instances (1..=4 entries).
    #[serde(default)]
    pub assets: Vec<String>,
    /// How many instances (already clamped 1..=48 at parse).
    pub count: u32,
    /// Scatter disk radius around `position` (clamped 0.5..=12 at parse).
    #[serde(default = "ten_f")]
    pub radius: f32,
    #[serde(default = "zero3")]
    pub position: [f32; 3],
    #[serde(default = "one_f")]
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyEntityCmd {
    pub name: String,
    pub position: Option<[f32; 3]>,
    pub rotation_degrees: Option<[f32; 3]>,
    pub scale: Option<[f32; 3]>,
    pub color: Option<[f32; 4]>,
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub emissive: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLightCmd {
    pub name: String,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    pub position: Option<[f32; 3]>,
    /// Direction for a directional light (a sun). None → point light.
    pub direction: Option<[f32; 3]>,
}

/// The interpreter's reply to one command, stringified for the model.
#[derive(Debug, Clone)]
pub enum AgentResponse {
    Spawned { name: String },
    AssetPlaced { name: String, asset: String },
    Scattered { name: String, count: usize },
    Modified { name: String },
    Deleted { name: String },
    LightSet { name: String },
    SceneInfo(String),
    Error(String),
}

impl AgentResponse {
    pub fn to_message(&self) -> String {
        match self {
            Self::Spawned { name } => format!("spawned '{name}'"),
            Self::AssetPlaced { name, asset } => format!("placed '{name}' ({asset})"),
            Self::Scattered { name, count } => format!("scattered {count} props as '{name}'"),
            Self::Modified { name } => format!("modified '{name}'"),
            Self::Deleted { name } => format!("deleted '{name}'"),
            Self::LightSet { name } => format!("light '{name}' set"),
            Self::SceneInfo(s) => s.clone(),
            Self::Error(e) => format!("error: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing (ported from Verse, minus at_role/set_environment)
// ---------------------------------------------------------------------------

/// Map a tool name + arguments JSON into an [`AgentCommand`]. `None` for an
/// unknown tool or malformed arguments — the session replies with an error
/// the model can correct.
pub fn parse_tool_call(name: &str, args: &str) -> Option<AgentCommand> {
    let args: serde_json::Value = serde_json::from_str(args).ok()?;
    match name {
        "spawn_primitive" => Some(AgentCommand::SpawnPrimitive(SpawnPrimitiveCmd {
            name: args["name"].as_str()?.into(),
            shape: serde_json::from_value(args["shape"].clone()).ok()?,
            dimensions: args
                .get("dimensions")
                .and_then(|v| v.as_object())
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f as f32)))
                        .collect()
                })
                .unwrap_or_default(),
            position: parse_arr3(&args["position"]),
            rotation_degrees: parse_arr3(&args["rotation_degrees"]),
            scale: parse_arr3_scale(&args["scale"]),
            color: parse_arr4(&args["color"]),
            metallic: args["metallic"].as_f64().unwrap_or(0.0) as f32,
            roughness: args["roughness"].as_f64().unwrap_or(0.5) as f32,
            emissive: parse_arr4(&args["emissive"]),
        })),
        "place_asset" => Some(AgentCommand::PlaceAsset(PlaceAssetCmd {
            name: args["name"].as_str()?.into(),
            kind: args["kind"].as_str().unwrap_or_default().to_lowercase(),
            asset: args["asset"].as_str().unwrap_or_default().to_string(),
            position: parse_arr3(&args["position"]),
            rotation_degrees: parse_arr3(&args["rotation_degrees"]),
            scale: args["scale"].as_f64().unwrap_or(1.0).clamp(0.05, 20.0) as f32,
        })),
        "scatter_field" => Some(AgentCommand::ScatterField(ScatterFieldCmd {
            name: args["name"].as_str()?.into(),
            kind: args["kind"].as_str().unwrap_or_default().to_lowercase(),
            assets: Vec::new(), // resolved (and recorded) by the session loop
            count: args["count"].as_i64().unwrap_or(12).clamp(1, 48) as u32,
            radius: args["radius"].as_f64().unwrap_or(10.0).clamp(0.5, 12.0) as f32,
            position: parse_arr3(&args["position"]),
            scale: args["scale"].as_f64().unwrap_or(1.0).clamp(0.05, 20.0) as f32,
        })),
        "modify_entity" => Some(AgentCommand::ModifyEntity(ModifyEntityCmd {
            name: args["name"].as_str()?.into(),
            position: args.get("position").and_then(parse_opt_arr3),
            rotation_degrees: args.get("rotation_degrees").and_then(parse_opt_arr3),
            scale: args.get("scale").and_then(parse_opt_arr3),
            color: args.get("color").and_then(parse_opt_arr4),
            metallic: args["metallic"].as_f64().map(|f| f as f32),
            roughness: args["roughness"].as_f64().map(|f| f as f32),
            emissive: args.get("emissive").and_then(parse_opt_arr4),
        })),
        "delete_entity" => Some(AgentCommand::DeleteEntity {
            name: args["name"].as_str()?.into(),
        }),
        "set_light" => Some(AgentCommand::SetLight(SetLightCmd {
            name: args["name"].as_str()?.into(),
            color: parse_arr4(&args["color"]),
            intensity: args["intensity"].as_f64().unwrap_or(1000.0) as f32,
            position: args.get("position").and_then(parse_opt_arr3),
            direction: args.get("direction").and_then(parse_opt_arr3),
        })),
        "scene_info" => Some(AgentCommand::SceneInfo),
        _ => None,
    }
}

/// Resolve a parsed command's kind-level references against the manifest —
/// the host half of the two-level vocabulary, run *in the session* so the
/// applied build carries concrete files. `Err(message)` becomes the
/// tool-error reply and the command is not applied (Verse's no-ghost rule).
pub fn resolve_agent_assets(
    cmd: AgentCommand,
    manifest: &AssetManifest,
    used: &mut Vec<String>,
) -> Result<AgentCommand, String> {
    match cmd {
        AgentCommand::PlaceAsset(mut c) => {
            if !c.asset.is_empty() {
                // A concrete file the model named anyway: keep it if known.
                if manifest.assets.iter().any(|a| a.file == c.asset) {
                    return Ok(AgentCommand::PlaceAsset(c));
                }
                if c.kind.is_empty() {
                    return Err(format!(
                        "unknown asset '{}' (name a 'kind' from place_asset's enum)",
                        c.asset
                    ));
                }
                c.asset = String::new(); // unknown file, known kind → resolve
            }
            if c.kind.is_empty() {
                return Err("place_asset needs a 'kind' from its enum".into());
            }
            match assets::resolve_kind(manifest, &c.kind, used) {
                Some(e) => {
                    c.asset = e.file.clone();
                    Ok(AgentCommand::PlaceAsset(c))
                }
                None => Err(format!(
                    "no '{kind}' in the asset pack (see place_asset's enum)",
                    kind = c.kind
                )),
            }
        }
        AgentCommand::ScatterField(mut c) => {
            if c.kind.is_empty() {
                return Err("scatter_field needs a 'kind' from its enum".into());
            }
            // Up to four variants, cycled across the instances — a field of
            // one model reads as clones; four read as a landscape.
            let mut variants: Vec<String> = Vec::new();
            for _ in 0..4 {
                match assets::resolve_kind(manifest, &c.kind, used) {
                    Some(e) if !variants.contains(&e.file) => variants.push(e.file.clone()),
                    _ => break,
                }
            }
            if variants.is_empty() {
                return Err(format!(
                    "no '{kind}' in the asset pack (see scatter_field's enum)",
                    kind = c.kind
                ));
            }
            c.assets = variants;
            Ok(AgentCommand::ScatterField(c))
        }
        other => Ok(other),
    }
}

// ---------------------------------------------------------------------------
// The interpreter — pure command execution into platform-local entities
// ---------------------------------------------------------------------------

/// A finished agent build: the section's entities (local coordinates) and
/// the model's closing description.
#[derive(Debug, Clone, Default)]
pub struct BuildOutput {
    pub entities: Vec<wt::WorldEntity>,
    pub description: Option<String>,
}

/// Applies [`AgentCommand`]s to an in-memory scene: a name-keyed list of
/// platform-local [`wt::WorldEntity`]s. All model-authored values are
/// clamped here — position to the platform neighbourhood, colours to 0..1,
/// light intensities to region-scale caps, counts to the entity budget — so
/// nothing out of range can reach the manifest even from a hand-edited log.
pub struct SceneInterpreter {
    section_key: String,
    entities: Vec<wt::WorldEntity>,
}

impl SceneInterpreter {
    /// `section_key` (the section's hash hex) salts scatter geometry, so a
    /// cached build replays to the identical field.
    pub fn new(section_key: &str) -> Self {
        Self {
            section_key: section_key.to_string(),
            entities: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// The built entities in local coordinates (ids are local placeholders;
    /// `draft::compile_with` renumbers them into the section's id band).
    pub fn entities(&self) -> &[wt::WorldEntity] {
        &self.entities
    }

    pub fn into_entities(self) -> Vec<wt::WorldEntity> {
        self.entities
    }

    /// Apply one command, returning the model-facing reply. `manifest` is
    /// consulted for span normalization; without it (or for a file that left
    /// the pack) placements keep scale 1.0 so the entity still renders.
    pub fn apply(&mut self, cmd: AgentCommand, manifest: Option<&AssetManifest>) -> AgentResponse {
        match cmd {
            AgentCommand::SpawnPrimitive(c) => self.spawn_primitive(c),
            AgentCommand::PlaceAsset(c) => self.place_asset(c, manifest),
            AgentCommand::ScatterField(c) => self.scatter_field(c, manifest),
            AgentCommand::ModifyEntity(c) => self.modify_entity(c),
            AgentCommand::DeleteEntity { name } => self.delete(&name),
            AgentCommand::SetLight(c) => self.set_light(c),
            AgentCommand::SceneInfo => AgentResponse::SceneInfo(self.scene_info()),
        }
    }

    fn next_id(&self) -> u64 {
        self.entities.len() as u64 + 1
    }

    fn push(&mut self, name: &str, mut entity: wt::WorldEntity) -> Result<(), String> {
        if self.entities.iter().any(|e| e.name.0 == name) {
            return Err(format!("name '{name}' already exists"));
        }
        if self.entities.len() >= MAX_ENTITIES {
            return Err(format!(
                "the region is full ({MAX_ENTITIES} entities) — delete something first"
            ));
        }
        entity.id = wt::EntityId(self.next_id());
        entity.name = wt::EntityName::new(name);
        entity.transform.position = clamp_local(entity.transform.position);
        self.entities.push(entity);
        Ok(())
    }

    fn spawn_primitive(&mut self, c: SpawnPrimitiveCmd) -> AgentResponse {
        let mut entity = wt::WorldEntity::new(0, c.name.clone());
        entity.transform.position = c.position;
        entity.transform.rotation_degrees = c.rotation_degrees;
        entity.transform.scale = c.scale.map(|v| v.clamp(0.05, 20.0));
        entity.shape = Some(primitive_shape(&c.shape, &c.dimensions));
        entity.material = Some(wt::MaterialDef {
            color: clamp_rgba(c.color),
            metallic: c.metallic.clamp(0.0, 1.0),
            roughness: c.roughness.clamp(0.01, 1.0),
            emissive: clamp_rgba(c.emissive),
            ..Default::default()
        });
        match self.push(&c.name, entity) {
            Ok(()) => AgentResponse::Spawned { name: c.name },
            Err(e) => AgentResponse::Error(e),
        }
    }

    fn place_asset(&mut self, c: PlaceAssetCmd, manifest: Option<&AssetManifest>) -> AgentResponse {
        if c.asset.is_empty() {
            return AgentResponse::Error(format!(
                "no '{kind}' asset resolved (see place_asset's enum)",
                kind = c.kind
            ));
        }
        let scale = manifest
            .and_then(|m| m.assets.iter().find(|a| a.file == c.asset))
            .map(|e| e.placement_scale())
            .unwrap_or(1.0)
            * c.scale.max(0.05);
        let mut entity = wt::WorldEntity::new(0, c.name.clone());
        entity.transform.position = c.position;
        entity.transform.rotation_degrees = c.rotation_degrees;
        entity.transform.scale = [scale, scale, scale];
        entity.mesh_asset = Some(wt::MeshAssetRef {
            path: assets::mesh_path(&c.asset),
            node: None,
        });
        let asset = c.asset.clone();
        match self.push(&c.name, entity) {
            Ok(()) => AgentResponse::AssetPlaced {
                name: c.name,
                asset,
            },
            Err(e) => AgentResponse::Error(e),
        }
    }

    fn scatter_field(
        &mut self,
        c: ScatterFieldCmd,
        manifest: Option<&AssetManifest>,
    ) -> AgentResponse {
        if c.assets.is_empty() {
            return AgentResponse::Error(format!(
                "no '{kind}' assets resolved (see scatter_field's enum)",
                kind = c.kind
            ));
        }
        let count = c.count as usize;
        // Reject the whole field before placing any of it (no half-fields).
        if self.entities.len() + count > MAX_ENTITIES {
            return AgentResponse::Error(format!(
                "the region is full ({MAX_ENTITIES} entities) — this field of {count} does not fit"
            ));
        }
        // Replay fallback: a pack change can empty the recorded variants.
        let variants: &[String] = &c.assets;
        let entry_scale = |file: &str| {
            manifest
                .and_then(|m| m.assets.iter().find(|a| a.file == file))
                .map(|e| e.placement_scale())
                .unwrap_or(1.0)
        };
        let seed = assets::fold_seed(&format!("{}|{}", self.section_key, c.name));
        let offsets = assets::scatter_offsets(seed, count, c.radius);
        let mut rng = seed ^ 0xA5A5_5EED;
        for (i, offset) in offsets.into_iter().enumerate() {
            let file = &variants[i % variants.len()];
            let scale =
                entry_scale(file) * c.scale.max(0.05) * (0.7 + assets::rand01(&mut rng) * 0.7);
            let name = format!("{}_{}", c.name, i + 1);
            let mut entity = wt::WorldEntity::new(0, name.clone());
            entity.transform.position = [
                c.position[0] + offset[0],
                c.position[1] + offset[1],
                c.position[2] + offset[2],
            ];
            entity.transform.rotation_degrees = [0.0, assets::rand01(&mut rng) * 360.0, 0.0];
            entity.transform.scale = [scale, scale, scale];
            entity.mesh_asset = Some(wt::MeshAssetRef {
                path: assets::mesh_path(file),
                node: None,
            });
            if let Err(e) = self.push(&name, entity) {
                return AgentResponse::Error(e);
            }
        }
        AgentResponse::Scattered {
            name: c.name,
            count,
        }
    }

    fn modify_entity(&mut self, c: ModifyEntityCmd) -> AgentResponse {
        let Some(entity) = self.entities.iter_mut().find(|e| e.name.0 == c.name) else {
            return AgentResponse::Error(format!("no entity named '{}'", c.name));
        };
        if let Some(p) = c.position {
            entity.transform.position = clamp_local(p);
        }
        if let Some(r) = c.rotation_degrees {
            entity.transform.rotation_degrees = r;
        }
        if let Some(s) = c.scale {
            entity.transform.scale = s.map(|v| v.clamp(0.05, 20.0));
        }
        // Material-ish patches apply to whatever the entity wears; patching
        // a placement's (absent) material materializes one with defaults.
        if c.color.is_some()
            || c.metallic.is_some()
            || c.roughness.is_some()
            || c.emissive.is_some()
        {
            let mut material = entity.material.clone().unwrap_or_default();
            if let Some(color) = c.color {
                material.color = clamp_rgba(color);
            }
            if let Some(metallic) = c.metallic {
                material.metallic = metallic.clamp(0.0, 1.0);
            }
            if let Some(roughness) = c.roughness {
                material.roughness = roughness.clamp(0.01, 1.0);
            }
            if let Some(emissive) = c.emissive {
                material.emissive = clamp_rgba(emissive);
            }
            entity.material = Some(material);
        }
        AgentResponse::Modified { name: c.name }
    }

    fn delete(&mut self, name: &str) -> AgentResponse {
        match self.entities.iter().position(|e| e.name.0 == name) {
            Some(index) => {
                self.entities.remove(index);
                AgentResponse::Deleted {
                    name: name.to_string(),
                }
            }
            None => AgentResponse::Error(format!("no entity named '{name}'")),
        }
    }

    fn set_light(&mut self, c: SetLightCmd) -> AgentResponse {
        let directional = c.direction.is_some();
        let light = wt::LightDef {
            light_type: if directional {
                wt::LightType::Directional
            } else {
                wt::LightType::Point
            },
            color: clamp_rgba(c.color),
            intensity: if directional {
                c.intensity.clamp(0.0, MAX_DIRECTIONAL_INTENSITY)
            } else {
                c.intensity.clamp(0.0, MAX_POINT_INTENSITY)
            },
            direction: c.direction,
            shadows: false,
            ..Default::default()
        };
        // Name reuse updates the existing light instead of stacking one.
        if let Some(entity) = self.entities.iter_mut().find(|e| e.name.0 == c.name) {
            entity.light = Some(light);
            if let Some(position) = c.position {
                entity.transform.position = clamp_local(position);
            }
        } else {
            let mut entity = wt::WorldEntity::new(0, c.name.clone());
            entity.transform.position = c.position.unwrap_or([0.0, 6.0, 0.0]);
            entity.light = Some(light);
            if let Err(e) = self.push(&c.name, entity) {
                return AgentResponse::Error(e);
            }
        }
        AgentResponse::LightSet { name: c.name }
    }

    fn scene_info(&self) -> String {
        let mut out = format!("Region ({} entities):\n", self.entities.len());
        for e in &self.entities {
            let what = if let Some(mesh) = &e.mesh_asset {
                mesh.path.clone()
            } else if let Some(shape) = &e.shape {
                shape_label(shape).to_string()
            } else if e.light.is_some() {
                "light".to_string()
            } else {
                "empty".to_string()
            };
            let p = e.transform.position;
            out.push_str(&format!(
                "  - {} [{}] at ({:.1}, {:.1}, {:.1})\n",
                e.name.0, what, p[0], p[1], p[2]
            ));
        }
        out
    }
}

fn shape_label(shape: &wt::Shape) -> &'static str {
    match shape {
        wt::Shape::Cuboid { .. } => "cuboid",
        wt::Shape::Sphere { .. } => "sphere",
        wt::Shape::Cylinder { .. } => "cylinder",
        wt::Shape::Cone { .. } => "cone",
        wt::Shape::Capsule { .. } => "capsule",
        wt::Shape::Torus { .. } => "torus",
        wt::Shape::Plane { .. } => "plane",
        wt::Shape::Pyramid { .. } => "pyramid",
        wt::Shape::Tetrahedron { .. } => "tetrahedron",
        wt::Shape::Icosahedron { .. } => "icosahedron",
        wt::Shape::Wedge { .. } => "wedge",
    }
}

/// Dimensions map → [`wt::Shape`], with per-shape defaults for anything the
/// model left out (Verse's vocabulary: Cuboid{x,y,z}, Sphere{radius},
/// Cylinder/Cone{radius,height}, Torus{major_radius,minor_radius},
/// Plane{x,z}).
fn primitive_shape(shape: &PrimitiveShape, d: &HashMap<String, f32>) -> wt::Shape {
    let get = |key: &str, default: f32| d.get(key).copied().unwrap_or(default).max(0.01);
    match shape {
        PrimitiveShape::Cuboid => wt::Shape::Cuboid {
            x: get("x", 1.0),
            y: get("y", 1.0),
            z: get("z", 1.0),
        },
        PrimitiveShape::Sphere => wt::Shape::Sphere {
            radius: get("radius", 0.5),
        },
        PrimitiveShape::Cylinder => wt::Shape::Cylinder {
            radius: get("radius", 0.5),
            height: get("height", 1.0),
        },
        PrimitiveShape::Cone => wt::Shape::Cone {
            radius: get("radius", 0.5),
            height: get("height", 1.0),
        },
        PrimitiveShape::Torus => wt::Shape::Torus {
            major_radius: get("major_radius", 1.0),
            minor_radius: get("minor_radius", 0.25),
        },
        PrimitiveShape::Plane => wt::Shape::Plane {
            x: get("x", 10.0),
            z: get("z", 10.0),
        },
    }
}

/// Keep a local position on the platform's neighbourhood: within
/// `MAX_RADIUS` horizontally (a hard radial clamp keeps the region round)
/// and between the surface and `MAX_HEIGHT`. Also applied by
/// `draft::compile_with` when placing cached/overridden entities, so the
/// clamp holds no matter who authored them.
pub fn clamp_local(position: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = position.map(|v| if v.is_finite() { v } else { 0.0 });
    let dist = (x * x + z * z).sqrt();
    let (x, z) = if dist > MAX_RADIUS {
        let k = MAX_RADIUS / dist;
        (x * k, z * k)
    } else {
        (x, z)
    };
    [x, y.clamp(0.0, MAX_HEIGHT), z]
}

fn clamp_rgba([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [
        finite(r).clamp(0.0, 1.0),
        finite(g).clamp(0.0, 1.0),
        finite(b).clamp(0.0, 1.0),
        finite(a).clamp(0.0, 1.0),
    ]
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.5 }
}

// --- parse helpers (Verse's) ------------------------------------------------

fn parse_arr3(v: &serde_json::Value) -> [f32; 3] {
    let a = v.as_array();
    [
        a.and_then(|a| a.first())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        a.and_then(|a| a.get(1))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        a.and_then(|a| a.get(2))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
    ]
}

fn parse_arr3_scale(v: &serde_json::Value) -> [f32; 3] {
    let a = v.as_array();
    [
        a.and_then(|a| a.first())
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32,
        a.and_then(|a| a.get(1))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32,
        a.and_then(|a| a.get(2))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32,
    ]
}

fn parse_arr4(v: &serde_json::Value) -> [f32; 4] {
    let a = v.as_array();
    [
        a.and_then(|a| a.first())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        a.and_then(|a| a.get(1))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        a.and_then(|a| a.get(2))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32,
        a.and_then(|a| a.get(3))
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32,
    ]
}

fn parse_opt_arr3(v: &serde_json::Value) -> Option<[f32; 3]> {
    v.as_array().filter(|a| a.len() == 3).map(|_| parse_arr3(v))
}

fn parse_opt_arr4(v: &serde_json::Value) -> Option<[f32; 4]> {
    v.as_array().filter(|a| a.len() == 4).map(|_| parse_arr4(v))
}

// --- serde defaults ---------------------------------------------------------

fn zero3() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}
fn one3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn zero4() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.0]
}
fn default_color() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}
fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn default_roughness() -> f32 {
    0.5
}
fn default_intensity() -> f32 {
    1000.0
}
fn one_f() -> f32 {
    1.0
}
fn ten_f() -> f32 {
    10.0
}

// ---------------------------------------------------------------------------
// The session (llm feature) — the tool-calling loop, ported from Verse
// ---------------------------------------------------------------------------

#[cfg(feature = "llm")]
mod session {
    use mistralrs::{RequestBuilder, TextMessageRole, ToolChoice};
    use tracing::{info, warn};

    use super::parse_tool_call;
    use super::{BuildOutput, SceneInterpreter, resolve_agent_assets};
    use crate::assets::AssetManifest;

    /// Cap on agent turns per section — bounds LLM cost and keeps a session
    /// inside a couple of minutes on the verified Metal setup. The prompt
    /// asks for 8–14 structures; 14 turns leaves room for review + revise.
    pub const MAX_AGENT_STEPS: usize = 14;

    /// Per-chat-request cap, matching the recipe tier's.
    const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

    /// Run one agent session for a section: the model builds the region by
    /// calling tools; each call is parsed, asset-resolved, applied to a
    /// [`SceneInterpreter`], and answered. No tool calls = done (the closing
    /// message is the description). `None` when nothing was built — the
    /// caller falls back to the recipe tier.
    pub fn run_session(
        model: &mut mistralrs::Model,
        section_key: &str,
        heading: &str,
        excerpt: &str,
        genre: &str,
        manifest: Option<&AssetManifest>,
    ) -> Option<BuildOutput> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("agent: tokio runtime failed: {e}"))
            .ok()?;

        rt.block_on(async move {
            let tools = tool_schemas(manifest);
            let mut scene = SceneInterpreter::new(section_key);
            // Session-wide kind rotation: repeats within a region differ.
            let mut asset_used: Vec<String> = Vec::new();
            let mut description = None;

            let mut messages = RequestBuilder::new()
                .add_message(
                    TextMessageRole::User,
                    build_system_prompt(heading, excerpt, genre, manifest),
                )
                .set_tools(tools)
                .set_tool_choice(ToolChoice::Auto);

            'steps: for step in 0..MAX_AGENT_STEPS {
                // Wrap-up nudge a few calls before the budget ends, so the
                // model reviews and closes instead of being cut off.
                if step == MAX_AGENT_STEPS.saturating_sub(4) {
                    messages = messages.add_message(
                        TextMessageRole::User,
                        "You are nearing your tool budget. Use the remaining calls wisely \
                         (scene_info if you must check), then FINISH by replying with a \
                         short description of this place — no more tool calls."
                            .to_string(),
                    );
                }
                let response = match tokio::time::timeout(
                    REQUEST_TIMEOUT,
                    model.send_chat_request(messages.clone()),
                )
                .await
                {
                    Ok(Ok(response)) => response,
                    Ok(Err(e)) => {
                        warn!("agent: chat request failed at step {step}: {e}");
                        break;
                    }
                    Err(_) => {
                        warn!(
                            "agent: chat request timed out at step {step} ({}s)",
                            REQUEST_TIMEOUT.as_secs()
                        );
                        break;
                    }
                };
                let Some(message) = response.choices.first().map(|c| &c.message) else {
                    break;
                };
                let Some(tool_calls) = &message.tool_calls else {
                    // Done: the closing description is part of the build.
                    description = message.content.clone().filter(|s| !s.trim().is_empty());
                    break;
                };
                messages = messages.add_message_with_tool_call(
                    TextMessageRole::Assistant,
                    message.content.as_deref().unwrap_or("").to_string(),
                    tool_calls.clone(),
                );

                for call in tool_calls {
                    let name = &call.function.name;
                    let args = &call.function.arguments;
                    let cmd = match parse_tool_call(name, args) {
                        Some(cmd) => cmd,
                        None => {
                            messages = messages.add_tool_message(
                                format!("error: unknown tool '{name}'"),
                                call.id.clone(),
                            );
                            continue;
                        }
                    };
                    // Resolve kinds to concrete files before applying, so the
                    // build is self-contained and errors never ghost in.
                    let cmd = if let Some(manifest) = manifest {
                        match resolve_agent_assets(cmd, manifest, &mut asset_used) {
                            Ok(cmd) => cmd,
                            Err(e) => {
                                messages = messages
                                    .add_tool_message(format!("error: {e}"), call.id.clone());
                                continue;
                            }
                        }
                    } else {
                        cmd
                    };
                    let reply = scene.apply(cmd, manifest);
                    messages = messages.add_tool_message(reply.to_message(), call.id.clone());
                    if matches!(reply, super::AgentResponse::Error(_))
                        && scene.is_empty()
                        && step + 1 == MAX_AGENT_STEPS
                    {
                        break 'steps;
                    }
                }
            }

            if scene.is_empty() {
                warn!("agent: session for \"{heading}\" built nothing — recipe tier next");
                None
            } else {
                info!(
                    "agent: built {} entities for \"{heading}\"{}",
                    scene.len(),
                    description
                        .as_deref()
                        .map(|d| format!(" — {d}"))
                        .unwrap_or_default()
                );
                Some(BuildOutput {
                    entities: scene.into_entities(),
                    description,
                })
            }
        })
    }

    /// The system prompt (sent as the first User message, Verse's
    /// discipline): the platform contract, the section's prose, and the
    /// budget.
    fn build_system_prompt(
        heading: &str,
        excerpt: &str,
        genre: &str,
        manifest: Option<&AssetManifest>,
    ) -> String {
        let assets = if manifest.is_some() {
            "place_asset places curated CC0 models by *kind* (rock, tree, lamp, statue, …) — \
             the app picks the concrete model and varies it on repeats, so prefer kinds over \
             hand-built primitives. scatter_field is the richness multiplier: one call \
             scatters a whole field of one kind — use it for ground cover and anything you \
             want more than a few of. "
        } else {
            ""
        };
        format!(
            "You are a 3D place designer. Build ONE place that reflects one section of a \
document by calling the tools. {assets}\
The stage: a circular platform of radius 6 already exists at the origin — a ground strip and \
the world's layout are already built. Work in platform-local coordinates: x/z across the \
platform (keep within ~10 of the centre), y up from the surface (y = 0 sits ON the platform). \
Call scene_info to review your work and iterate.\n\n\
Section (genre: {genre}): \"{heading}\".\nSection text: \"{excerpt}\".\n\
Let the text decide everything: a shore wants water-edge things, a library wants shelves and \
warm lamps, an observatory wants brass and a clear view up. Colours are muted and \
desaturated (dusk light, weathered stone, deep water — never pure primaries, never neon). \
Keep it tasteful: 6-12 structures plus one or two scatter fields is plenty; one or two \
lights. When you are done, reply with a short description of the place instead of calling \
more tools.",
        )
    }

    /// The tool schemas (ported from Verse, minus set_environment and
    /// at_role; radius clamped to the platform's scale).
    pub fn tool_schemas(manifest: Option<&AssetManifest>) -> Vec<mistralrs::Tool> {
        use mistralrs::{Function, Tool, ToolType};
        use serde_json::json;

        fn f(name: &str, desc: &str, params: serde_json::Value) -> Tool {
            Tool {
                tp: ToolType::Function,
                function: Function {
                    description: Some(desc.into()),
                    name: name.into(),
                    parameters: Some(serde_json::from_value(params).unwrap_or_default()),
                },
            }
        }

        let mut tools = vec![
            f(
                "spawn_primitive",
                "Spawn a 3D primitive shape (Cuboid/Sphere/Cylinder/Cone/Torus/Plane) with a material and transform. Use for structures the asset pack lacks; combine several to compose towers, platforms, frames.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Unique name for this entity (e.g. 'tower_base', 'crystal_1')"},
                        "shape": {"type": "string", "enum": ["Cuboid","Sphere","Cylinder","Cone","Torus","Plane"]},
                        "dimensions": {"type": "object", "description": "Cuboid:{x,y,z}. Sphere:{radius}. Cylinder:{radius,height}. Cone:{radius,height}. Torus:{major_radius,minor_radius}. Plane:{x,z}."},
                        "position": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                        "rotation_degrees": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                        "scale": {"type": "array", "items": {"type":"number"}, "default": [1,1,1]},
                        "color": {"type": "array", "items": {"type":"number"}, "default": [0.8,0.8,0.8,1.0], "description": "RGBA 0-1"},
                        "metallic": {"type": "number", "default": 0.0, "minimum": 0, "maximum": 1},
                        "roughness": {"type": "number", "default": 0.5, "minimum": 0, "maximum": 1},
                        "emissive": {"type": "array", "items": {"type":"number"}, "default": [0,0,0,0], "description": "Glow color RGBA"}
                    },
                    "required": ["name", "shape"]
                }),
            ),
            f(
                "modify_entity",
                "Partially update an existing entity by name. Only provided fields change.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "position": {"type": "array", "items": {"type":"number"}},
                        "scale": {"type": "array", "items": {"type":"number"}},
                        "color": {"type": "array", "items": {"type":"number"}},
                        "emissive": {"type": "array", "items": {"type":"number"}}
                    },
                    "required": ["name"]
                }),
            ),
            f(
                "delete_entity",
                "Delete an entity by name.",
                json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
            ),
            f(
                "set_light",
                "Add or update a named light. Omit direction for a point light; provide it for a directional (sun) light. Reusing a name updates that light instead of adding another.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "color": {"type": "array", "items": {"type":"number"}, "default": [1,1,1,1]},
                        "intensity": {"type": "number", "default": 1000},
                        "position": {"type": "array", "items": {"type":"number"}},
                        "direction": {"type": "array", "items": {"type":"number"}, "description": "Direction vector for a sun/directional light"}
                    },
                    "required": ["name"]
                }),
            ),
            f(
                "scene_info",
                "List all currently-placed entities and lights, so you can review and iterate on the place you are building.",
                json!({"type":"object","properties":{}}),
            ),
        ];
        if let Some(manifest) = manifest {
            // The enum is the *kind* list; the description carries example
            // names per kind so the model knows what each kind looks like
            // without a per-file enum (long file-name enums are exactly what
            // a small local model handles worst).
            let kinds = manifest.kinds();
            let listed = kinds
                .iter()
                .map(|k| {
                    let examples: Vec<&str> = manifest
                        .assets
                        .iter()
                        .filter(|a| a.kind == *k)
                        .take(3)
                        .map(|a| a.name.as_str())
                        .collect();
                    format!("{k} ({})", examples.join(", "))
                })
                .collect::<Vec<_>>()
                .join("; ");
            tools.insert(
                1,
                f(
                    "place_asset",
                    &format!(
                        "Place one of the app's curated CC0 3D models — real scanned props, grouped \
                         by kind: {listed}. You name the kind; the app picks a concrete model that \
                         fits and varies it on repeats. Prefer these over primitives."
                    ),
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Unique name for this placement (e.g. 'gate_1')"},
                            "kind": {"type": "string", "enum": kinds, "description": "What to place (the app picks the model)"},
                            "position": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                            "rotation_degrees": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                            "scale": {"type": "number", "default": 1.0, "description": "Uniform scale multiplier"}
                        },
                        "required": ["name", "kind"]
                    }),
                ),
            );
            tools.insert(
                2,
                f(
                    "scatter_field",
                    &format!(
                        "Scatter many instances of one kind across a disk in a single call — a field \
                         of rocks, a drift of shells, rows of barrels. Kinds: {listed}. Use this \
                         instead of repeated place_asset whenever you want more than ~4 of something; \
                         it is the cheapest way to make the place feel dense."
                    ),
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Unique prefix; instances are named name_1 … name_N"},
                            "kind": {"type": "string", "enum": kinds, "description": "What to scatter (variants mix automatically)"},
                            "count": {"type": "integer", "minimum": 1, "maximum": 48, "default": 12},
                            "radius": {"type": "number", "minimum": 0.5, "maximum": 12, "default": 6, "description": "Disk radius in metres around position"},
                            "position": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                            "scale": {"type": "number", "default": 1.0, "description": "Uniform scale multiplier"}
                        },
                        "required": ["name", "kind"]
                    }),
                ),
            );
        }
        tools
    }
}

// The session's external surface. `tool_schemas` and `MAX_AGENT_STEPS`
// are internal to the session loop (their tests live in the module too).
#[cfg(feature = "llm")]
pub use session::run_session;

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> AssetManifest {
        serde_json::from_str::<AssetManifest>(
            r#"{"version":2,"assets":[
                {"name":"Boulder","file":"boulder.glb","kind":"rock","tier":"hero",
                 "mood":0,"scale":1.0,"dims":[2.0,2.0,2.0],"license":"CC0","author":"x"},
                {"name":"Pebble","file":"pebble.glb","kind":"rock","tier":"scatter",
                 "mood":0,"scale":1.0,"dims":[0.5,0.5,0.5],"license":"CC0","author":"x"},
                {"name":"Slab","file":"slab.glb","kind":"rock","tier":"medium",
                 "mood":0,"scale":1.0,"dims":[1.0,1.0,1.0],"license":"CC0","author":"x"},
                {"name":"Palm","file":"palm.glb","kind":"tree","tier":"medium",
                 "mood":0,"scale":1.0,"dims":[4.0,8.0,4.0],"license":"CC0","author":"x"}
            ]}"#,
        )
        .unwrap()
    }

    fn spawn(name: &str) -> AgentCommand {
        parse_tool_call(
            "spawn_primitive",
            &format!(r#"{{"name":"{name}","shape":"Cuboid","dimensions":{{"x":2,"y":3,"z":2}},"position":[1,0,-2],"color":[9,0.2,0.1,1],"emissive":[0,0.5,0,1]}}"#),
        )
        .unwrap()
    }

    #[test]
    fn parse_covers_every_tool_with_clamps() {
        assert!(matches!(
            parse_tool_call("scene_info", "{}").unwrap(),
            AgentCommand::SceneInfo
        ));
        let AgentCommand::PlaceAsset(c) =
            parse_tool_call("place_asset", r#"{"name":"g","kind":"ROCK","scale":99}"#).unwrap()
        else {
            panic!("wrong variant");
        };
        assert_eq!(c.kind, "rock"); // lowercased
        assert_eq!(c.scale, 20.0); // clamped
        let AgentCommand::ScatterField(s) = parse_tool_call(
            "scatter_field",
            r#"{"name":"f","kind":"rock","count":500,"radius":99}"#,
        )
        .unwrap() else {
            panic!("wrong variant");
        };
        assert_eq!(s.count, 48);
        assert_eq!(s.radius, 12.0);
        assert!(parse_tool_call("set_environment", "{}").is_none()); // dropped tool
        assert!(parse_tool_call("nope", "{}").is_none());
        assert!(parse_tool_call("spawn_primitive", "not json").is_none());
    }

    #[test]
    fn interpreter_spawn_scene_info_modify_delete() {
        let mut scene = SceneInterpreter::new("key");
        let reply = scene.apply(spawn("tower"), None);
        assert_eq!(reply.to_message(), "spawned 'tower'");
        // Clamped colour, clamped-free position.
        let tower = scene.entities()[0].clone();
        assert_eq!(tower.material.unwrap().color, [1.0, 0.2, 0.1, 1.0]);
        assert_eq!(tower.transform.position, [1.0, 0.0, -2.0]);
        assert!(matches!(
            tower.shape,
            Some(wt::Shape::Cuboid {
                x: 2.0,
                y: 3.0,
                z: 2.0
            })
        ));

        // Duplicate name → error.
        assert!(matches!(
            scene.apply(spawn("tower"), None),
            AgentResponse::Error(_)
        ));

        // scene_info mentions the entity.
        let AgentResponse::SceneInfo(info) = scene.apply(AgentCommand::SceneInfo, None) else {
            panic!("wrong variant");
        };
        assert!(info.contains("tower") && info.contains("cuboid"));

        // Modify applies material + transform patches.
        scene.apply(
            parse_tool_call(
                "modify_entity",
                r#"{"name":"tower","position":[500,0,0],"color":[0,0,0,1]}"#,
            )
            .unwrap(),
            None,
        );
        let tower = scene.entities()[0].clone();
        assert_eq!(tower.transform.position[0], MAX_RADIUS); // radial clamp
        assert_eq!(tower.material.unwrap().color, [0.0, 0.0, 0.0, 1.0]);

        // Delete.
        scene.apply(
            parse_tool_call("delete_entity", r#"{"name":"tower"}"#).unwrap(),
            None,
        );
        assert!(scene.is_empty());
        assert!(matches!(
            scene.apply(
                parse_tool_call("delete_entity", r#"{"name":"tower"}"#).unwrap(),
                None
            ),
            AgentResponse::Error(_)
        ));
    }

    #[test]
    fn interpreter_enforces_the_entity_budget() {
        let mut scene = SceneInterpreter::new("k");
        for i in 0..MAX_ENTITIES {
            scene.apply(spawn(&format!("e{i}")), None);
        }
        assert_eq!(scene.len(), MAX_ENTITIES);
        assert!(matches!(
            scene.apply(spawn("overflow"), None),
            AgentResponse::Error(e) if e.contains("full")
        ));
    }

    #[test]
    fn place_asset_and_scatter_are_deterministic() {
        let manifest = manifest();
        let mut used = Vec::new();
        let place = resolve_agent_assets(
            parse_tool_call(
                "place_asset",
                r#"{"name":"gate","kind":"rock","position":[0,0,0],"scale":1.0}"#,
            )
            .unwrap(),
            &manifest,
            &mut used,
        )
        .unwrap();
        let mut a = SceneInterpreter::new("sec");
        a.apply(place, Some(&manifest));
        let gate = a.entities()[0].clone();
        assert_eq!(gate.mesh_asset.as_ref().unwrap().path, "models/boulder.glb");
        assert_eq!(gate.transform.scale, [3.5, 3.5, 3.5]); // hero span 7 / 2.0

        let scatter = resolve_agent_assets(
            parse_tool_call(
                "scatter_field",
                r#"{"name":"pebbles","kind":"rock","count":6,"radius":4}"#,
            )
            .unwrap(),
            &manifest,
            &mut used,
        )
        .unwrap();
        a.apply(scatter, Some(&manifest));
        assert_eq!(a.len(), 7); // gate + 6 pebbles
        let names: Vec<&str> = a.entities()[1..]
            .iter()
            .map(|e| e.name.0.as_str())
            .collect();
        assert_eq!(names[0], "pebbles_1");
        // Identical inputs → identical fields, seed and all.
        let mut b = SceneInterpreter::new("sec");
        let mut used2 = Vec::new();
        for cmd in [
            parse_tool_call("place_asset", r#"{"name":"gate","kind":"rock"}"#).unwrap(),
            parse_tool_call(
                "scatter_field",
                r#"{"name":"pebbles","kind":"rock","count":6,"radius":4}"#,
            )
            .unwrap(),
        ] {
            b.apply(
                resolve_agent_assets(cmd, &manifest, &mut used2).unwrap(),
                Some(&manifest),
            );
        }
        assert_eq!(a.entities(), b.entities());
        // Different section key → different field geometry.
        let mut c = SceneInterpreter::new("other");
        c.apply(
            resolve_agent_assets(
                parse_tool_call(
                    "scatter_field",
                    r#"{"name":"pebbles","kind":"rock","count":6,"radius":4}"#,
                )
                .unwrap(),
                &manifest,
                &mut Vec::new(),
            )
            .unwrap(),
            Some(&manifest),
        );
        assert_ne!(
            a.entities()[2].transform.position,
            c.entities()[1].transform.position
        );
    }

    #[test]
    fn scatter_that_does_not_fit_is_rejected_whole() {
        let manifest = manifest();
        let mut scene = SceneInterpreter::new("k");
        let cmd = resolve_agent_assets(
            parse_tool_call(
                "scatter_field",
                r#"{"name":"a_lot","kind":"rock","count":48,"radius":6}"#,
            )
            .unwrap(),
            &manifest,
            &mut Vec::new(),
        )
        .unwrap();
        // 48 fits exactly within the 64-entity budget.
        assert!(matches!(
            scene.apply(cmd, Some(&manifest)),
            AgentResponse::Scattered { .. }
        ));
        assert_eq!(scene.len(), 48);
        let cmd = resolve_agent_assets(
            parse_tool_call(
                "scatter_field",
                r#"{"name":"more","kind":"tree","count":20,"radius":6}"#,
            )
            .unwrap(),
            &manifest,
            &mut Vec::new(),
        )
        .unwrap();
        // 48 + 20 > 64: rejected whole, nothing placed.
        assert!(matches!(
            scene.apply(cmd, Some(&manifest)),
            AgentResponse::Error(e) if e.contains("full")
        ));
        assert_eq!(scene.len(), 48);
    }

    #[test]
    fn lights_update_by_name_and_clamp() {
        let mut scene = SceneInterpreter::new("k");
        scene.apply(
            parse_tool_call(
                "set_light",
                r#"{"name":"lamp","intensity":5000000,"position":[0,4,0]}"#,
            )
            .unwrap(),
            None,
        );
        let lamp = scene.entities()[0].clone();
        assert_eq!(
            lamp.light.as_ref().unwrap().light_type,
            wt::LightType::Point
        );
        assert_eq!(lamp.light.as_ref().unwrap().intensity, MAX_POINT_INTENSITY);
        // Same name → update, not a second light.
        scene.apply(
            parse_tool_call(
                "set_light",
                r#"{"name":"lamp","intensity":100,"color":[1,0,0,1]}"#,
            )
            .unwrap(),
            None,
        );
        assert_eq!(scene.len(), 1);
        assert_eq!(scene.entities()[0].light.as_ref().unwrap().intensity, 100.0);
        // Direction given → directional.
        scene.apply(
            parse_tool_call(
                "set_light",
                r#"{"name":"sun","direction":[-0.3,-1,-0.2],"intensity":99000}"#,
            )
            .unwrap(),
            None,
        );
        let sun = scene.entities()[1].clone();
        let light = sun.light.unwrap();
        assert_eq!(light.light_type, wt::LightType::Directional);
        assert_eq!(light.intensity, MAX_DIRECTIONAL_INTENSITY);
        assert_eq!(light.direction, Some([-0.3, -1.0, -0.2]));
    }

    #[test]
    fn resolve_reports_unknown_kinds_without_ghosting() {
        let manifest = manifest();
        let cmd = parse_tool_call("place_asset", r#"{"name":"x","kind":"boat"}"#).unwrap();
        assert!(resolve_agent_assets(cmd, &manifest, &mut Vec::new()).is_err());
        let cmd = parse_tool_call("scatter_field", r#"{"name":"x","kind":""}"#).unwrap();
        assert!(resolve_agent_assets(cmd, &manifest, &mut Vec::new()).is_err());
        // Up to 4 variants recorded for scatter.
        let cmd = parse_tool_call("scatter_field", r#"{"name":"x","kind":"rock"}"#).unwrap();
        let AgentCommand::ScatterField(c) =
            resolve_agent_assets(cmd, &manifest, &mut Vec::new()).unwrap()
        else {
            panic!("wrong variant");
        };
        assert_eq!(c.assets.len(), 3); // the synthetic pack has 3 rocks
    }
}
