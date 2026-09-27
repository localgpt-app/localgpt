//! LLM scene-construction agent — a simplified port of `localgpt-gen`'s
//! tool-calling pattern (PLAN.md M7, `llm` feature).
//!
//! Unlike the [`crate::recipe`] path (where the model emits one static JSON
//! object that the renderer interprets), this module makes the model a true
//! **agent**: it calls tools — `spawn_primitive`, `place_asset`, `set_light`,
//! ... — that actually construct the 3D world entity-by-entity, sees the
//! results, and iterates. The world is authored by the LLM, not just
//! parameterized by it.
//!
//! # Architecture (mirrors LocalGPT's `crates/gen/src/gen3d`, trimmed to essentials)
//!
//! ```text
//! ┌──────────────────────────┐   mpsc channels   ┌─────────────────────┐
//! │ Agent loop (tokio)       │ ◄────────────────►│ Bevy (main thread)  │
//! │  - GGUF via mistral.rs   │  AgentCommand ──►  │  - drains each frame│
//! │  - 7 tool schemas        │  ◄── AgentResponse │  - name registry    │
//! │  - executes tool_calls   │                    │  - spawns entities  │
//! └──────────────────────────┘                    └─────────────────────┘
//! ```
//!
//! Core tools: spawn_primitive, place_asset, modify_entity, delete_entity,
//! set_light, set_environment, scene_info.
//!
//! # Track scoping
//!
//! The worker analyzes tracks *ahead* of playback, so a live session usually
//! authors the world of the next track while another song plays. Every
//! agent-spawned entity therefore carries its track's content-hash id
//! ([`crate::agent_types::AgentEntity`]) and starts hidden;
//! [`sync_agent_scene_scope`] reveals (and lights) the current track's scene
//! and blacks out everyone else's, so a lookahead session never pops into the
//! world mid-song. When the track becomes current, `replay_cached_build`
//! rebuilds its cached [`SceneBuild`] deterministically — no LLM re-run.
//!
//! # Comfort
//!
//! Every emissive/light value the agent authors passes through the Comfort
//! gates at execution time (reduce-flashing caps emissive strength and light
//! intensity), the same contract the recipe path has always had.
//!
//! # Graceful degradation
//! No `llm` feature → module not compiled; feature but no model → the agent
//! returns an empty [`SceneBuild`] and the renderer keeps the rule-derived
//! world. Same contract as [`crate::ml`] / [`crate::demucs`].

#![allow(dead_code)]
#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::sync::Arc;

use bevy::log::{info, warn};
use serde_json::{Value, json};

// Re-export the always-compiled data types so callers can reach them via
// `crate::agent::SceneBuild` etc., while the types themselves live in the
// ungated `agent_types` module (so `TrackAnalysis` can carry them without the
// `llm` feature, and `world.rs` can exclude agent-owned entities from its
// queries in every feature config).
pub use crate::agent_types::{
    AgentAmbient, AgentCommand, AgentEntity, AgentLight, AgentResponse, EnvOverride,
    PrimitiveShape, SceneBuild, SectionScoped, parse_tool_call,
};

// The bridge's async channels are tokio mpsc (matches gen's pattern). The
// command/response types are plain structs so the Bevy side (sync) can move
// them across the frame boundary without an async runtime.
use tokio::sync::{Mutex, mpsc};

use crate::analysis::TrackAnalysis;
use crate::theme::moods;
use crate::world_assets::AssetManifest;

// ---------------------------------------------------------------------------
// Bridge — async agent ↔ sync Bevy
// ---------------------------------------------------------------------------

/// The agent's handle to send commands and await responses. Cloned cheaply
/// (Arc) so the agent loop and the tool layer share it.
pub struct AgentBridge {
    cmd_tx: mpsc::UnboundedSender<AgentCommand>,
    resp_rx: Mutex<mpsc::UnboundedReceiver<AgentResponse>>,
}

impl AgentBridge {
    /// Send a command and await Bevy's reply.
    pub async fn send(&self, cmd: AgentCommand) -> AgentResponse {
        if self.cmd_tx.send(cmd).is_err() {
            return AgentResponse::Error("Bevy side closed".into());
        }
        let mut rx = self.resp_rx.lock().await;
        rx.recv()
            .await
            .unwrap_or_else(|| AgentResponse::Error("no reply".into()))
    }
}

/// The bridge, published by `AgentPlugin` so the analysis worker can be handed
/// it at construction. A resource rather than a global: it is owned by the app,
/// dropped with it, and visible to anything that needs to know the tier is live.
#[derive(bevy::prelude::Resource)]
pub struct AgentBridgeHandle(pub Arc<AgentBridge>);

/// The Bevy-side channels, held as a resource and drained each frame.
pub struct AgentChannels {
    pub cmd_rx: mpsc::UnboundedReceiver<AgentCommand>,
    pub resp_tx: mpsc::UnboundedSender<AgentResponse>,
}

/// Create the matched (bridge, channels) pair — one per app instance.
pub fn create_channels() -> (Arc<AgentBridge>, AgentChannels) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (resp_tx, resp_rx) = mpsc::unbounded_channel();
    let bridge = Arc::new(AgentBridge {
        cmd_tx,
        resp_rx: Mutex::new(resp_rx),
    });
    (bridge, AgentChannels { cmd_rx, resp_tx })
}

// ---------------------------------------------------------------------------
// Tool schemas — the LLM-facing JSON-Schema for each of the 7 core tools
// ---------------------------------------------------------------------------

/// The core tool definitions, as mistral.rs `Tool`s ready for `set_tools`.
/// Each maps 1:1 to an [`AgentCommand`] variant executed by [`AgentExecutor`].
///
/// With a manifest, `place_asset` and `scatter_field` join the set. Both speak
/// the **two-level vocabulary**: their `kind` parameter is an enum of the
/// manifest's semantic kinds (`rock`, `tree`, `lamp`, …) — a small, stable,
/// clean-token list that a local GGUF can hold reliably — while the host
/// resolves each call to a concrete variant, preferring the track's mood and
/// rotating so repeats differ. Diversity scales with the pool, not the enum
/// (which a long file-name list would only degrade). Without a manifest both
/// tools are omitted and the agent builds from primitives only.
pub fn tool_schemas(manifest: Option<&AssetManifest>) -> Vec<mistralrs::Tool> {
    use mistralrs::{Function, Tool, ToolType};
    /// helper: build a Function from name/description/parameters JSON.
    fn f(name: &str, desc: &str, params: Value) -> Tool {
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
                    "emissive": {"type": "array", "items": {"type":"number"}, "default": [0,0,0,0], "description": "Glow color RGBA"},
                    "at_role": {"type": "string", "enum": ["intro","verse","chorus","drop","bridge","outro"], "description": "Song section this structure appears in (stays hidden until then)"}
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
            "set_environment",
            "Set the background color and ambient light.",
            json!({
                "type": "object",
                "properties": {
                    "background_color": {"type": "array", "items": {"type":"number"}, "default": [0.04,0.05,0.07,1]},
                    "ambient_light": {"type": "array", "items": {"type":"number"}, "default": [0.3,0.3,0.4,1]}
                }
            }),
        ),
        f(
            "scene_info",
            "List all currently-spawned entities and lights, so you can review and iterate on the world you are building.",
            json!({"type":"object","properties":{}}),
        ),
    ];
    if let Some(manifest) = manifest {
        // The enum is the *kind* list; the description carries example names
        // per kind so the model knows what each kind looks like without a
        // per-file enum (long file-name enums are exactly what a small local
        // model handles worst).
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
                     fits this world and varies it on repeats. Prefer these over primitives."
                ),
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Unique name for this placement (e.g. 'gate_1')"},
                        "kind": {"type": "string", "enum": kinds, "description": "What to place (the app picks the model)"},
                        "position": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                        "rotation_degrees": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                        "scale": {"type": "number", "default": 1.0, "description": "Uniform scale multiplier"},
                        "at_role": {"type": "string", "enum": ["intro","verse","chorus","drop","bridge","outro"], "description": "Song section this asset appears in (e.g. a gateway for the bridge, monuments on the chorus)"}
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
                     it is the cheapest way to make a world feel dense."
                ),
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Unique prefix; instances are named name_1 … name_N"},
                        "kind": {"type": "string", "enum": kinds, "description": "What to scatter (variants mix automatically)"},
                        "count": {"type": "integer", "minimum": 1, "maximum": 48, "default": 12},
                        "radius": {"type": "number", "minimum": 0.5, "maximum": 120, "default": 10, "description": "Disk radius in metres around position"},
                        "position": {"type": "array", "items": {"type":"number"}, "default": [0,0,0]},
                        "scale": {"type": "number", "default": 1.0, "description": "Uniform scale multiplier"},
                        "at_role": {"type": "string", "enum": ["intro","verse","chorus","drop","bridge","outro"], "description": "Song section this field appears in"}
                    },
                    "required": ["name", "kind"]
                }),
            ),
        );
    }
    tools
}

/// Resolve a parsed command's kind-level asset references against the
/// manifest — the host half of the two-level vocabulary, run *in the session*
/// so the recorded [`SceneBuild`] carries the concrete files and replays
/// exactly. `used` is the session's rotation state (shared with
/// [`crate::world_assets::resolve_kind`]); `mood` is the track's, so variants
/// fit the world being authored. `Err(message)` becomes the tool-error reply
/// and the command is *not* recorded (a failed call must not ghost into the
/// replayed build).
fn resolve_agent_assets(
    cmd: AgentCommand,
    manifest: &AssetManifest,
    mood: usize,
    used: &mut Vec<String>,
) -> Result<AgentCommand, String> {
    match cmd {
        AgentCommand::PlaceAsset(mut c) => {
            // A concrete file (legacy behaviour, or a model that names one
            // anyway): keep it, backfilling the kind for the record.
            if !c.asset.is_empty() {
                if let Some(e) = manifest.assets.iter().find(|a| a.file == c.asset) {
                    if c.kind.is_empty() {
                        c.kind = e.kind.clone();
                    }
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
            match crate::world_assets::resolve_kind(manifest, &c.kind, Some(mood), used) {
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
            // Up to four mood-preferred variants, cycled across the instances
            // — a field of one model reads as clones; four read as a landscape.
            let mut variants: Vec<String> = Vec::new();
            for _ in 0..4 {
                match crate::world_assets::resolve_kind(manifest, &c.kind, Some(mood), used) {
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

// `parse_tool_call` and its helpers live in the shared world-agent crate
// (re-exported via `agent_types`); Verse's session uses it below.

// ---------------------------------------------------------------------------
// Bevy-side execution — name registry + frame drain
// ---------------------------------------------------------------------------

use bevy::prelude::{
    AmbientLight, Assets, Color, Commands, DirectionalLight, Entity, Local, Mesh, PointLight,
    Query, Res, ResMut, StandardMaterial, Visibility, With, Without,
};

/// Everything the executor needs from the app that it cannot own: the asset
/// server + manifest (for `place_asset`), and the Comfort gates every
/// emissive/light value must pass.
pub struct ExecDeps<'a> {
    pub asset_server: &'a bevy::asset::AssetServer,
    pub assets: &'a crate::world_assets::WorldAssets,
    pub comfort: &'a crate::Comfort,
}

/// Name → Entity registry. Lets modify/delete reference entities by the names
/// the LLM chose. Held as a Resource alongside the channels.
#[derive(bevy::prelude::Resource, Default)]
pub struct NameRegistry {
    pub map: HashMap<String, Entity>,
}

impl NameRegistry {
    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }
}

/// The Bevy-side resource: the channels to drain, the name registries, and the
/// session's track scope. Created once at startup and held for the app's
/// lifetime.
#[derive(bevy::prelude::Resource)]
pub struct AgentExecutor {
    pub channels: AgentChannels,
    pub registry: NameRegistry,
    /// Named lights (registry-style, so `set_light` with a used name updates
    /// instead of stacking — and `clear_scene` can despawn them).
    lights: HashMap<String, Entity>,
    /// The single ambient-light entity `set_environment` owns.
    ambient: Option<Entity>,
    /// The track-scoped background the environment set, applied via
    /// [`EnvOverride`] only while its track is current.
    env: Option<(String, Color)>,
    /// The content-hash id of the track whose session/replay is executing.
    /// Every spawned entity is stamped with it (track scoping — module docs).
    session_track: String,
    /// Kind-resolution rotation state for the executor's *fallback* path (a
    /// replayed build whose recorded variant has left the manifest). Cleared
    /// per session/replay, like the rest of the scope.
    kind_used: Vec<String>,
}

impl AgentExecutor {
    /// Build the executor over a drained channel pair (see
    /// [`create_channels`]). The only constructor: the registries start empty
    /// and the scope starts unowned.
    pub fn new(channels: AgentChannels) -> Self {
        Self {
            channels,
            registry: NameRegistry::default(),
            lights: HashMap::new(),
            ambient: None,
            env: None,
            session_track: String::new(),
            kind_used: Vec::new(),
        }
    }
}

impl AgentExecutor {
    /// Drain pending commands and execute them against the world. Called once
    /// per frame. Non-blocking (`try_recv`) so an idle agent costs nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn drain(
        &mut self,
        commands: &mut Commands,
        meshes: &mut ResMut<Assets<Mesh>>,
        materials: &mut ResMut<Assets<StandardMaterial>>,
        deps: ExecDeps<'_>,
        agent_entities: &Query<&AgentEntity>,
        lights_q: &Query<Entity, With<AgentLight>>,
        ambient_q: &Query<Entity, With<AgentAmbient>>,
    ) {
        // Reconcile the registries against despawned entities (e.g. a mood
        // change may have cleared agent entities). Cheap: walks the registries.
        self.registry
            .map
            .retain(|_, e| agent_entities.get(*e).is_ok());
        self.lights.retain(|_, e| lights_q.get(*e).is_ok());
        if let Some(a) = self.ambient
            && ambient_q.get(a).is_err()
        {
            self.ambient = None;
        }

        while let Ok(cmd) = self.channels.cmd_rx.try_recv() {
            let resp = self.execute(cmd, commands, meshes, materials, &deps);
            let _ = self.channels.resp_tx.send(resp);
        }
    }

    fn execute(
        &mut self,
        cmd: AgentCommand,
        commands: &mut Commands,
        meshes: &mut ResMut<Assets<Mesh>>,
        materials: &mut ResMut<Assets<StandardMaterial>>,
        deps: &ExecDeps<'_>,
    ) -> AgentResponse {
        use bevy::prelude::*;
        let track = self.session_track.clone();
        match cmd {
            AgentCommand::BeginSession { track: t } => {
                self.session_track = t;
                self.kind_used.clear();
                AgentResponse::SessionBegun
            }
            AgentCommand::SpawnPrimitive(c) => {
                if self.registry.contains(&c.name) {
                    return AgentResponse::Error(format!("'{}' already exists", c.name));
                }
                // Comfort: reduce-flashing caps emissive strength.
                let mut emissive = c.emissive;
                if deps.comfort.reduce_flashing {
                    for e in &mut emissive {
                        *e = (*e).clamp(0.0, 0.3);
                    }
                }
                let mesh = build_primitive_mesh(c.shape, &c.dimensions, meshes);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgba(c.color[0], c.color[1], c.color[2], c.color[3]),
                    metallic: c.metallic,
                    perceptual_roughness: c.roughness,
                    emissive: LinearRgba::new(emissive[0], emissive[1], emissive[2], emissive[3]),
                    ..default()
                });
                let entity = commands
                    .spawn((
                        AgentEntity {
                            name: c.name.clone(),
                            track: track.clone(),
                        },
                        Name::new(c.name.clone()),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform {
                            translation: Vec3::from(c.position),
                            rotation: Quat::from_euler(
                                EulerRot::YXZ,
                                c.rotation_degrees[1].to_radians(),
                                c.rotation_degrees[0].to_radians(),
                                c.rotation_degrees[2].to_radians(),
                            ),
                            scale: Vec3::from(c.scale),
                        },
                        // Revealed by `sync_agent_scene_scope` when (and only
                        // when) this track is the one playing.
                        Visibility::Hidden,
                    ))
                    .id();
                if let Some(role) = c.at_role {
                    commands
                        .entity(entity)
                        .insert(crate::agent_types::SectionScoped {
                            role,
                            track: track.clone(),
                        });
                }
                self.registry.map.insert(c.name.clone(), entity);
                AgentResponse::Spawned { name: c.name }
            }
            AgentCommand::PlaceAsset(c) => {
                if self.registry.contains(&c.name) {
                    return AgentResponse::Error(format!("'{}' already exists", c.name));
                }
                let Some(manifest) = &deps.assets.manifest else {
                    return AgentResponse::Error("no asset pack bundled".into());
                };
                // The recorded file decides; the kind is the fallback when a
                // replayed build's variant has left the manifest (pool grown
                // or pruned since it was cached).
                let entry = manifest
                    .assets
                    .iter()
                    .find(|a| a.file == c.asset)
                    .or_else(|| {
                        (!c.kind.is_empty())
                            .then(|| {
                                crate::world_assets::resolve_kind(
                                    manifest,
                                    &c.kind,
                                    None,
                                    &mut self.kind_used,
                                )
                            })
                            .flatten()
                    });
                let Some(entry) = entry else {
                    return AgentResponse::Error(format!(
                        "unknown asset '{}' (see place_asset's kinds)",
                        c.asset
                    ));
                };
                let handle: Handle<_> = deps.asset_server.load(
                    bevy::gltf::GltfAssetLabel::Scene(0)
                        .from_asset(format!("models/{}", entry.file)),
                );
                let entity = commands
                    .spawn((
                        AgentEntity {
                            name: c.name.clone(),
                            track: track.clone(),
                        },
                        Name::new(c.name.clone()),
                        WorldAssetRoot(handle),
                        Transform::from_translation(Vec3::from(c.position))
                            .with_rotation(Quat::from_euler(
                                EulerRot::YXZ,
                                c.rotation_degrees[1].to_radians(),
                                c.rotation_degrees[0].to_radians(),
                                c.rotation_degrees[2].to_radians(),
                            ))
                            .with_scale(Vec3::splat(entry.placement_scale() * c.scale.max(0.05))),
                        Visibility::Hidden,
                    ))
                    .id();
                if let Some(role) = c.at_role {
                    commands
                        .entity(entity)
                        .insert(crate::agent_types::SectionScoped {
                            role,
                            track: track.clone(),
                        });
                }
                self.registry.map.insert(c.name.clone(), entity);
                // The shared protocol's reply carries the resolved file
                // (MD's shape); Verse's executor names it back to the model.
                AgentResponse::AssetPlaced {
                    name: c.name,
                    asset: entry.file.clone(),
                }
            }
            AgentCommand::ScatterField(c) => {
                if self.registry.contains(&c.name) {
                    return AgentResponse::Error(format!("'{}' already exists", c.name));
                }
                let Some(manifest) = &deps.assets.manifest else {
                    return AgentResponse::Error("no asset pack bundled".into());
                };
                // Recorded variants decide; resolve fresh only when absent
                // (never happens live — the session resolves before recording).
                let mut variants: Vec<&crate::world_assets::AssetEntry> = c
                    .assets
                    .iter()
                    .filter_map(|f| manifest.assets.iter().find(|a| &a.file == f))
                    .collect();
                if variants.is_empty() && !c.kind.is_empty() {
                    let mut used = Vec::new();
                    for _ in 0..4 {
                        match crate::world_assets::resolve_kind(manifest, &c.kind, None, &mut used)
                        {
                            Some(e) if !variants.iter().any(|v| v.file == e.file) => {
                                variants.push(e)
                            }
                            _ => break,
                        }
                    }
                }
                if variants.is_empty() {
                    return AgentResponse::Error(format!(
                        "no '{}' assets in the pack (see scatter_field's kinds)",
                        c.kind
                    ));
                }
                // Deterministic field: seed from (track, name) — a cached
                // build replays to the identical scatter (ARCHITECTURE R6).
                let base = Vec3::from(c.position);
                let seed =
                    crate::world_assets::fold_seed(&format!("{}|{}", self.session_track, c.name));
                let offsets =
                    crate::world_assets::scatter_offsets(seed, c.count as usize, c.radius);
                let mut rng = seed ^ 0xA5A5_5EED_u64;
                let count = offsets.len();
                for (i, off) in offsets.into_iter().enumerate() {
                    let entry = variants[i % variants.len()];
                    let handle: Handle<_> = deps.asset_server.load(
                        bevy::gltf::GltfAssetLabel::Scene(0)
                            .from_asset(format!("models/{}", entry.file)),
                    );
                    let instance = format!("{}_{}", c.name, i + 1);
                    let entity = commands
                        .spawn((
                            AgentEntity {
                                name: instance.clone(),
                                track: track.clone(),
                            },
                            Name::new(instance.clone()),
                            WorldAssetRoot(handle),
                            Transform::from_translation(base + off)
                                .with_rotation(Quat::from_rotation_y(
                                    crate::world_assets::rand01(&mut rng) * std::f32::consts::TAU,
                                ))
                                .with_scale(Vec3::splat(
                                    entry.placement_scale()
                                        * c.scale.max(0.05)
                                        * (0.7 + crate::world_assets::rand01(&mut rng) * 0.7),
                                )),
                            // Revealed by `sync_agent_scene_scope` when (and
                            // only when) this track is the one playing.
                            Visibility::Hidden,
                        ))
                        .id();
                    // Distance culling by tier (ARCHITECTURE R7): a dense
                    // field of heroes still needs its skyline, but scatter-
                    // tier fields drop out inside the fog band like the
                    // world's own ground cover.
                    if let Some(range) = entry.tier.visibility_range() {
                        commands.entity(entity).insert(range);
                    }
                    if let Some(role) = c.at_role {
                        commands
                            .entity(entity)
                            .insert(crate::agent_types::SectionScoped {
                                role,
                                track: track.clone(),
                            });
                    }
                    self.registry.map.insert(instance, entity);
                }
                AgentResponse::Scattered {
                    name: c.name,
                    count,
                }
            }
            AgentCommand::ModifyEntity(c) => {
                let Some(&entity) = self.registry.map.get(&c.name) else {
                    return AgentResponse::Error(format!("'{}' not found", c.name));
                };
                // Partial transform patch: keep existing fields where the
                // command doesn't supply one. Spawn a fresh Transform only when
                // both are present; otherwise fall back to a deferred queue
                // that mutates the live component.
                let mut ecmd = commands.entity(entity);
                if let (Some(pos), Some(scale)) = (c.position, c.scale) {
                    ecmd.insert(Transform {
                        translation: Vec3::from(pos),
                        scale: Vec3::from(scale),
                        ..default()
                    });
                } else {
                    ecmd.queue(move |mut entity: EntityWorldMut| {
                        if let Some(mut tf) = entity.get_mut::<Transform>() {
                            if let Some(pos) = c.position {
                                tf.translation = Vec3::from(pos);
                            }
                            if let Some(scale) = c.scale {
                                tf.scale = Vec3::from(scale);
                            }
                        }
                    });
                }
                AgentResponse::Modified { name: c.name }
            }
            AgentCommand::DeleteEntity { name } => {
                if let Some(entity) = self.registry.map.remove(&name) {
                    commands.entity(entity).despawn();
                    AgentResponse::Deleted { name }
                } else {
                    AgentResponse::Error(format!("'{}' not found", name))
                }
            }
            AgentCommand::SetLight(c) => {
                // Comfort: reduce-flashing caps light intensity.
                let intensity = if deps.comfort.reduce_flashing {
                    c.intensity.clamp(0.0, 4000.0)
                } else {
                    c.intensity.clamp(0.0, 1_000_000.0)
                };
                if let Some(&entity) = self.lights.get(&c.name) {
                    // Update the existing light in place (color, intensity,
                    // transform) — no stacking.
                    let color = Color::srgba(c.color[0], c.color[1], c.color[2], c.color[3]);
                    let pos = c.position;
                    let dir = c.direction;
                    commands.entity(entity).insert(AgentLight {
                        name: c.name.clone(),
                        track: track.clone(),
                        base_intensity: intensity,
                    });
                    commands.entity(entity).queue(move |mut e: EntityWorldMut| {
                        if let Some(mut l) = e.get_mut::<PointLight>() {
                            l.color = color;
                            l.intensity = 0.0; // revealed by sync_agent_scene_scope
                        }
                        if let Some(mut l) = e.get_mut::<DirectionalLight>() {
                            l.color = color;
                            l.illuminance = 0.0;
                        }
                        if let Some(mut tf) = e.get_mut::<Transform>() {
                            if let Some(p) = pos {
                                tf.translation = Vec3::from(p);
                            }
                            if let Some(d) = dir {
                                let d = Vec3::from(d).normalize_or_zero();
                                if d != Vec3::ZERO {
                                    tf.look_to(d, Vec3::Y);
                                }
                            }
                        }
                    });
                    return AgentResponse::LightSet { name: c.name };
                }
                // A directional light if direction is set; else a point light.
                // Spawned dark (intensity 0) — `sync_agent_scene_scope` turns
                // it on when its track is current.
                let color = Color::srgba(c.color[0], c.color[1], c.color[2], c.color[3]);
                let entity = if let Some(dir) = c.direction {
                    let dir_v = Vec3::from(dir).normalize_or_zero();
                    commands
                        .spawn((
                            AgentLight {
                                name: c.name.clone(),
                                track: track.clone(),
                                base_intensity: intensity,
                            },
                            DirectionalLight {
                                color,
                                illuminance: 0.0,
                                ..default()
                            },
                            Transform::from_xyz(0.0, 10.0, 0.0).looking_to(dir_v, Vec3::Y),
                        ))
                        .id()
                } else {
                    commands
                        .spawn((
                            AgentLight {
                                name: c.name.clone(),
                                track: track.clone(),
                                base_intensity: intensity,
                            },
                            PointLight {
                                color,
                                intensity: 0.0,
                                ..default()
                            },
                            Transform::from_translation(Vec3::from(
                                c.position.unwrap_or([0.0, 5.0, 0.0]),
                            )),
                        ))
                        .id()
                };
                self.lights.insert(c.name.clone(), entity);
                AgentResponse::LightSet { name: c.name }
            }
            AgentCommand::SetEnvironment(c) => {
                // Comfort: reduce-flashing calms the ambient too.
                let lum = (c.ambient_light[0] * 0.3
                    + c.ambient_light[1] * 0.5
                    + c.ambient_light[2] * 0.2)
                    .clamp(0.0, 1.0);
                let brightness = 260.0
                    * lum
                    * if deps.comfort.reduce_flashing {
                        0.6
                    } else {
                        1.0
                    };
                let color = Color::srgba(
                    c.ambient_light[0],
                    c.ambient_light[1],
                    c.ambient_light[2],
                    c.ambient_light[3],
                );
                if let Some(entity) = self.ambient {
                    // Update the single owned ambient entity in place.
                    commands.entity(entity).insert(AgentAmbient {
                        track: track.clone(),
                        base_brightness: brightness,
                    });
                    commands.entity(entity).queue(move |mut e: EntityWorldMut| {
                        if let Some(mut a) = e.get_mut::<AmbientLight>() {
                            a.color = color;
                            a.brightness = 0.0; // revealed by sync_agent_scene_scope
                        }
                    });
                } else {
                    let entity = commands
                        .spawn((
                            AgentAmbient {
                                track: track.clone(),
                                base_brightness: brightness,
                            },
                            AmbientLight {
                                color,
                                brightness: 0.0,
                                ..default()
                            },
                        ))
                        .id();
                    self.ambient = Some(entity);
                }
                // The background is track-scoped: applied via EnvOverride only
                // while this track is current (see sync_agent_scene_scope).
                self.env = Some((
                    track.clone(),
                    Color::srgba(
                        c.background_color[0],
                        c.background_color[1],
                        c.background_color[2],
                        c.background_color[3],
                    ),
                ));
                AgentResponse::EnvironmentSet
            }
            AgentCommand::SceneInfo => {
                let mut summary = format!("Scene (track {}):\n", self.session_track);
                for name in self.registry.map.keys() {
                    summary.push_str(&format!("  - {name}\n"));
                }
                for name in self.lights.keys() {
                    summary.push_str(&format!("  - light {name}\n"));
                }
                if self.registry.map.is_empty() && self.lights.is_empty() {
                    summary.push_str("  (empty)");
                }
                AgentResponse::SceneInfo(summary)
            }
        }
    }

    /// Despawn every agent-spawned entity — primitives, placed assets,
    /// lights, the ambient — and clear the registries. Used before replaying a
    /// cached [`SceneBuild`] so the scene is rebuilt clean (spawn rejects
    /// duplicate names, so a stale scene would block replay).
    pub fn clear_scene(
        &mut self,
        commands: &mut Commands,
        agent_entities: &Query<&AgentEntity>,
        lights: &Query<Entity, With<AgentLight>>,
    ) {
        for &e in self.registry.map.values() {
            if agent_entities.get(e).is_ok() {
                commands.entity(e).despawn();
            }
        }
        for e in lights.iter() {
            commands.entity(e).despawn();
        }
        self.registry.map.clear();
        self.lights.clear();
        if let Some(a) = self.ambient {
            commands.entity(a).despawn();
        }
        self.ambient = None;
        self.env = None;
        self.kind_used.clear();
    }

    /// Replay a cached [`SceneBuild`] — iterate its commands through `execute`
    /// without the LLM, bridge, or async runtime. Deterministic: the same
    /// build → the same world. Call [`clear_scene`] first. Returns the count
    /// applied.
    pub fn replay(
        &mut self,
        build: &SceneBuild,
        track: &str,
        commands: &mut Commands,
        meshes: &mut ResMut<Assets<Mesh>>,
        materials: &mut ResMut<Assets<StandardMaterial>>,
        deps: ExecDeps<'_>,
    ) -> usize {
        self.session_track = track.to_string();
        self.kind_used.clear();
        let mut n = 0;
        for cmd in &build.commands {
            // Report-only or session-scoped commands have no effect on replay.
            if matches!(
                cmd,
                AgentCommand::SceneInfo | AgentCommand::BeginSession { .. }
            ) {
                continue;
            }
            self.execute(cmd.clone(), commands, meshes, materials, &deps);
            n += 1;
        }
        n
    }
}

/// Build the Bevy mesh for a primitive from its shape + dimensions.
fn build_primitive_mesh(
    shape: PrimitiveShape,
    dims: &HashMap<String, f32>,
    meshes: &mut ResMut<Assets<Mesh>>,
) -> bevy::asset::Handle<Mesh> {
    use bevy::prelude::*;
    match shape {
        PrimitiveShape::Cuboid => {
            let x = dims.get("x").copied().unwrap_or(1.0);
            let y = dims.get("y").copied().unwrap_or(1.0);
            let z = dims.get("z").copied().unwrap_or(1.0);
            meshes.add(Cuboid::new(x, y, z))
        }
        PrimitiveShape::Sphere => {
            let r = dims.get("radius").copied().unwrap_or(0.5);
            meshes.add(Sphere::new(r).mesh().uv(32, 18))
        }
        PrimitiveShape::Cylinder => {
            let r = dims.get("radius").copied().unwrap_or(0.5);
            let h = dims.get("height").copied().unwrap_or(1.0);
            meshes.add(Cylinder::new(r, h))
        }
        PrimitiveShape::Cone => {
            let r = dims.get("radius").copied().unwrap_or(0.5);
            let h = dims.get("height").copied().unwrap_or(1.0);
            meshes.add(Cone {
                radius: r,
                height: h,
            })
        }
        PrimitiveShape::Torus => {
            let major = dims.get("major_radius").copied().unwrap_or(1.0);
            let minor = dims.get("minor_radius").copied().unwrap_or(0.25);
            meshes.add(Torus::new(minor, major))
        }
        PrimitiveShape::Plane => {
            let x = dims.get("x").copied().unwrap_or(10.0);
            let z = dims.get("z").copied().unwrap_or(10.0);
            meshes.add(Plane3d::new(Vec3::Y, Vec2::new(x / 2.0, z / 2.0)))
        }
    }
}

/// The frame drain system. Registered in `plugins.rs`; runs every frame, costs
/// nothing when the agent is idle.
#[allow(clippy::too_many_arguments)]
pub fn drain_agent_commands(
    mut executor: ResMut<AgentExecutor>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<bevy::asset::AssetServer>,
    assets: Res<crate::world_assets::WorldAssets>,
    comfort: Res<crate::Comfort>,
    agent_entities: Query<&AgentEntity>,
    lights: Query<Entity, With<AgentLight>>,
    ambient: Query<Entity, With<AgentAmbient>>,
) {
    executor.drain(
        &mut commands,
        &mut meshes,
        &mut materials,
        ExecDeps {
            asset_server: &asset_server,
            assets: &assets,
            comfort: &comfort,
        },
        &agent_entities,
        &lights,
        &ambient,
    );
}

/// The segment a section role lands in, for `at_role` placement timing: the
/// recipe's choreography mapping when it resolved one, else the positional
/// shape of a song (mirrors `world::positional_role`, inverted).
fn role_segment(
    role: crate::recipe::SectionRole,
    segments: usize,
    moments: &[(usize, crate::recipe::SectionMoment)],
) -> usize {
    if let Some((idx, _)) = moments.iter().find(|(_, m)| m.at_role == role) {
        return *idx;
    }
    use crate::recipe::SectionRole as R;
    let n = segments.max(1);
    match role {
        R::Intro => 0,
        R::Outro => n - 1,
        R::Chorus | R::Drop => n / 2,
        R::Bridge => (n * 3) / 4,
        R::Verse => 1.min(n - 1),
    }
}

/// Reveal the current track's agent scene and black out everyone else's.
///
/// Entities spawned during a lookahead session carry that session's track id
/// and start hidden/dark; this system flips them on exactly when their track
/// becomes the playing one, and applies the track-scoped background via
/// [`EnvOverride`] (which `world.rs`'s palette wash reads). One frame of
/// latency at worst — it runs every frame after the drain/replay systems.
#[allow(clippy::too_many_arguments)]
pub fn sync_agent_scene_scope(
    playback: Res<crate::playback::Playback>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    executor: Res<AgentExecutor>,
    mut env_override: ResMut<EnvOverride>,
    mut meshes_q: Query<(&AgentEntity, Option<&SectionScoped>, &mut Visibility)>,
    mut point_q: Query<(&AgentLight, &mut PointLight), Without<DirectionalLight>>,
    mut dir_q: Query<(&AgentLight, &mut DirectionalLight), Without<PointLight>>,
    mut ambient_q: Query<(&AgentAmbient, &mut AmbientLight)>,
) {
    let current = playback
        .queue
        .get(playback.current % playback.queue.len().max(1))
        .and_then(|t| t.id.clone());
    let is_current = |track: &str| current.as_deref() == Some(track);

    let segments = playback.sections.len();
    let current_segment = playback
        .sections
        .iter()
        .filter(|&&s| s <= playback.fraction())
        .count()
        .saturating_sub(1);
    for (agent, scoped, mut vis) in &mut meshes_q {
        // Track scope first; then section scope — an `at_role` placement is
        // visible only inside its section of the current track.
        let visible = is_current(&agent.track)
            && match scoped {
                None => true,
                Some(s) => {
                    current_segment == role_segment(s.role, segments, &active_recipe.moments)
                }
            };
        *vis = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (light, mut l) in &mut point_q {
        l.intensity = if is_current(&light.track) {
            light.base_intensity
        } else {
            0.0
        };
    }
    for (light, mut l) in &mut dir_q {
        l.illuminance = if is_current(&light.track) {
            light.base_intensity
        } else {
            0.0
        };
    }
    for (ambient, mut a) in &mut ambient_q {
        a.brightness = if is_current(&ambient.track) {
            ambient.base_brightness
        } else {
            0.0
        };
    }
    env_override.background = executor
        .env
        .as_ref()
        .filter(|(track, _)| is_current(track))
        .map(|(_, color)| *color);
}

/// Replay a cached `SceneBuild` when the current track changes and has a build
/// stored in its analysis sidecar (PLAN.md M7 — no LLM re-run). Clears the
/// agent scene first, then iterates the cached commands. No-op when the current
/// track has no cached build or hasn't changed since the last replay.
#[allow(clippy::too_many_arguments)]
pub fn replay_cached_build(
    mut executor: ResMut<AgentExecutor>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<bevy::asset::AssetServer>,
    assets: Res<crate::world_assets::WorldAssets>,
    comfort: Res<crate::Comfort>,
    analysis: Res<crate::analysis::AnalysisStore>,
    playback: Res<crate::playback::Playback>,
    agent_entities: Query<&AgentEntity>,
    lights: Query<Entity, With<AgentLight>>,
    mut last: Local<Option<Option<String>>>,
) {
    let current_id = playback
        .queue
        .get(playback.current % playback.queue.len().max(1))
        .and_then(|t| t.id.clone());
    // Re-replay only when the current track id changes.
    if *last == Some(current_id.clone()) {
        return;
    }
    *last = Some(current_id.clone());

    let Some(id) = current_id.as_deref() else {
        return;
    };
    let Some(build) = analysis.get(id).and_then(|a| a.build.as_ref()) else {
        // No cached build for this track — nothing to replay. Any previously
        // replayed entities stay until the next track with a build clears them.
        return;
    };
    executor.clear_scene(&mut commands, &agent_entities, &lights);
    let n = executor.replay(
        build,
        id,
        &mut commands,
        &mut meshes,
        &mut materials,
        ExecDeps {
            asset_server: &asset_server,
            assets: &assets,
            comfort: &comfort,
        },
    );
    info!("Replayed {n} cached agent commands for track {id}");
}

// ---------------------------------------------------------------------------
// Agent session — runs the model in a tool-calling loop until it stops calling
// ---------------------------------------------------------------------------

/// Run one agent session: the model gets the track's analysis as context, then
/// loops calling tools to build a world, until it emits a message with no
/// tool_calls (it's done — that message is captured as the build's
/// description) or the step budget is exhausted. Each tool_call is parsed into
/// an [`AgentCommand`], sent to Bevy via the bridge, and the result is fed
/// back. Returns the ordered commands issued (the [`SceneBuild`]).
///
/// `track_id` scopes everything the session spawns to that track (see the
/// module docs on track scoping); `manifest` supplies the asset vocabulary for
/// the `place_asset` tool (`None` = primitives only). `cancel` is the worker's
/// cooperative shutdown flag — checked between turns and between tool calls; a
/// generation already inside mistral.rs finishes (see
/// `AnalysisStore::shutdown` for the detach backstop).
///
/// Blocks the calling thread on a dedicated tokio runtime (the analysis worker
/// is a plain std::thread — no async pollution of the rest of the app).
pub fn run_session(
    model: &mut mistralrs::Model,
    bridge: Arc<AgentBridge>,
    analysis: &TrackAnalysis,
    track_id: &str,
    manifest: Option<&AssetManifest>,
    cancel: &crate::analysis::WorkerCancel,
) -> Option<SceneBuild> {
    use mistralrs::{RequestBuilder, TextMessageRole, ToolChoice};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| warn!("agent: tokio runtime failed: {e}"))
        .ok()?;

    rt.block_on(async move {
        let tools = tool_schemas(manifest);
        let mut build = SceneBuild::default();
        // Session-wide kind-rotation state: every place_asset/scatter_field
        // consumes from it, so repeats within a world differ (the host half
        // of the two-level vocabulary).
        let mut asset_used: Vec<String> = Vec::new();

        // Seed the conversation with the world-design brief, scoped to this
        // track so the executor stamps its entities correctly. Not recorded —
        // `replay` sets its own scope.
        bridge
            .send(AgentCommand::BeginSession {
                track: track_id.to_string(),
            })
            .await;
        let mut messages = RequestBuilder::new()
            .add_message(
                TextMessageRole::User,
                build_system_prompt(analysis, manifest),
            )
            .set_tools(tools)
            .set_tool_choice(ToolChoice::Auto);

        'steps: for step in 0..MAX_AGENT_STEPS {
            // Wrap-up nudge: give the agent a few calls' warning before the
            // budget ends, so it reviews and closes with a description
            // instead of being cut off mid-build (which loses the capture).
            if step == MAX_AGENT_STEPS.saturating_sub(6) {
                messages = messages.add_message(
                    mistralrs::TextMessageRole::User,
                    "You are nearing your tool budget. Use the remaining calls wisely \
                     (scene_info if you must check), then FINISH by replying with a short \
                     description of the world — no more tool calls."
                        .to_string(),
                );
            }
            if cancel.is_cancelled() {
                info!("agent: cancelled at step {step} — keeping what was built");
                break;
            }
            let response = match model.send_chat_request(messages.clone()).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("agent: chat request failed at step {step}: {e}");
                    break;
                }
            };
            let Some(message) = response.choices.first().map(|c| &c.message) else {
                break;
            };
            let Some(tool_calls) = &message.tool_calls else {
                // No tool calls → the agent is done. Its closing description is
                // part of the build (cached, logged, and there for future UI).
                build.description = message.content.clone().filter(|s| !s.trim().is_empty());
                break;
            };

            // Execute every tool_call in this turn, recording each command.
            // Then feed the assistant turn + tool results back into the convo.
            messages = messages.add_message_with_tool_call(
                TextMessageRole::Assistant,
                message.content.as_deref().unwrap_or("").to_string(),
                tool_calls.clone(),
            );

            for call in tool_calls {
                if cancel.is_cancelled() {
                    info!("agent: cancelled mid-turn at step {step}");
                    break 'steps;
                }
                let name = &call.function.name;
                let args = &call.function.arguments;
                match parse_tool_call(name, args) {
                    Some(cmd) => {
                        // Resolve kind-level references to concrete manifest
                        // files *before* recording, so the cached SceneBuild
                        // replays exactly (and the model's error reply never
                        // ghosts into the build).
                        let cmd = if let Some(manifest) = manifest {
                            match resolve_agent_assets(
                                cmd,
                                manifest,
                                analysis.mood,
                                &mut asset_used,
                            ) {
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
                        build.commands.push(cmd.clone());
                        let resp = bridge.send(cmd).await;
                        messages = messages.add_tool_message(resp.to_message(), call.id.clone());
                    }
                    None => {
                        messages = messages.add_tool_message(
                            format!("error: unknown tool '{name}'"),
                            call.id.clone(),
                        );
                    }
                }
            }
        }

        if build.is_empty() {
            warn!("agent: session produced no commands — keeping rule-derived world");
            None
        } else {
            info!(
                "agent: session produced {} commands{}",
                build.commands.len(),
                build
                    .description
                    .as_deref()
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default()
            );
            Some(build)
        }
    })
}

/// Cap on agent turns per session — bounds LLM cost on a single track. The
/// prompt asks for 8–16 structures; 24 turns leaves room for review + revise.
const MAX_AGENT_STEPS: usize = 24;

/// The system prompt: tells the model what it is, gives it the track's mood/
/// BPM/energy as context, and instructs it to build a world with the tools.
fn build_system_prompt(analysis: &TrackAnalysis, manifest: Option<&AssetManifest>) -> String {
    let mood = moods()
        .get(analysis.mood)
        .map(|m| m.world_name)
        .unwrap_or("UNKNOWN");
    let bpm = if analysis.bpm > 0.0 {
        format!("{:.0}", analysis.bpm)
    } else {
        "unknown".into()
    };
    let assets = if manifest.is_some() {
        "place_asset places curated CC0 models by *kind* (rock, tree, lamp, statue, …) — the \
         app picks the concrete model to fit this world and varies it on repeats, so prefer \
         kinds over hand-built primitives. scatter_field is the richness multiplier: one call \
         scatters a whole field of one kind — use it for ground cover and anything you want \
         more than a few of instead of repeating place_asset. "
    } else {
        ""
    };
    format!(
        "You are a 3D world designer for a music visualizer. Build an immersive world that \
matches this song by calling the tools. {assets}\
Call scene_info to review your work and iterate.\n\n\
Song context: mood = {mood}, tempo = {bpm} BPM, {n} sections.\n\
Keep it tasteful and performant: 8-16 structures plus one or two scatter fields is plenty. \
Place a ground plane only if the world feels empty, a few hero structures, and accent \
lighting that suits the mood. When you are done, reply with a short description of the \
world instead of calling more tools.",
        n = analysis.sections.len().max(1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schemas_are_complete_functions() {
        let schemas = tool_schemas(None);
        assert_eq!(schemas.len(), 6, "core toolset without a manifest");
        for s in &schemas {
            assert!(!s.function.name.is_empty());
            assert!(s.function.description.is_some());
            assert!(s.function.parameters.is_some());
        }
    }

    #[test]
    fn place_asset_tool_requires_a_manifest() {
        let manifest: AssetManifest = serde_json::from_str(
            r#"{"version":1,"assets":[
                {"id":"u1","name":"Rock Arch","file":"rock_arch.glb","tier":"hero",
                 "kind":"rock","mood":0,"license":"CC0","author":"a","source":"s"},
                {"id":"u2","name":"Street Lamp","file":"street_lamp.glb","tier":"medium",
                 "kind":"lamp","mood":1,"license":"CC0","author":"a","source":"s"}
            ]}"#,
        )
        .expect("parses");
        let schemas = tool_schemas(Some(&manifest));
        assert_eq!(
            schemas.len(),
            8,
            "place_asset + scatter_field join with a manifest"
        );
        let place = schemas.iter().find(|t| t.function.name == "place_asset");
        assert!(place.is_some(), "place_asset present");
        let params = place.unwrap().function.parameters.clone().unwrap();
        let json = serde_json::to_value(&params).unwrap();
        // The enum is the *kind* vocabulary (small, stable), not the file list.
        let allowed = &json["properties"]["kind"]["enum"];
        assert_eq!(allowed[0].as_str(), Some("rock"));
        assert_eq!(allowed[1].as_str(), Some("lamp"));
        // The description carries example names so intent maps onto kinds.
        let desc = place.unwrap().function.description.clone().unwrap();
        assert!(desc.contains("Rock Arch") && desc.contains("Street Lamp"));
        // scatter_field shares the kind enum.
        let scatter = schemas
            .iter()
            .find(|t| t.function.name == "scatter_field")
            .expect("scatter_field present");
        let sparams = scatter.function.parameters.clone().unwrap();
        let sjson = serde_json::to_value(&sparams).unwrap();
        assert_eq!(
            sjson["properties"]["kind"]["enum"][0].as_str(),
            Some("rock")
        );
    }

    #[test]
    fn session_resolution_rotates_and_records() {
        let manifest: AssetManifest = serde_json::from_str(
            r#"{"version":2,"assets":[
                {"id":"r1","name":"Boulder A","file":"boulder_a.glb","tier":"hero",
                 "kind":"rock","mood":0,"license":"CC0","author":"a","source":"s"},
                {"id":"r2","name":"Boulder B","file":"boulder_b.glb","tier":"hero",
                 "kind":"rock","mood":0,"license":"CC0","author":"a","source":"s"},
                {"id":"t1","name":"Pine","file":"pine.glb","tier":"hero",
                 "kind":"tree","mood":2,"license":"CC0","author":"a","source":"s"}
            ]}"#,
        )
        .expect("parses");
        let mut used = Vec::new();
        // Two rock asks rotate; the tree resolves in its own mood.
        let cmd = parse_tool_call(
            "place_asset",
            r#"{"name":"r1","kind":"rock","position":[1,0,-4]}"#,
        )
        .unwrap();
        let resolved = resolve_agent_assets(cmd, &manifest, 0, &mut used)
            .expect("resolves")
            .clone();
        match resolved {
            AgentCommand::PlaceAsset(c) => {
                assert_eq!(c.asset, "boulder_a.glb");
                assert_eq!(c.kind, "rock");
            }
            _ => panic!("wrong variant"),
        }
        let cmd = parse_tool_call("place_asset", r#"{"name":"r2","kind":"Rock"}"#).unwrap();
        let resolved = resolve_agent_assets(cmd, &manifest, 0, &mut used)
            .expect("resolves")
            .clone();
        match resolved {
            AgentCommand::PlaceAsset(c) => assert_eq!(c.asset, "boulder_b.glb"),
            _ => panic!("wrong variant"),
        }
        // A kind absent from the pack is a tool error, not a silent guess.
        let cmd = parse_tool_call("place_asset", r#"{"name":"x","kind":"crystal"}"#).unwrap();
        assert!(resolve_agent_assets(cmd, &manifest, 0, &mut used).is_err());
        // scatter_field resolves up to four variants of the kind.
        let cmd = parse_tool_call(
            "scatter_field",
            r#"{"name":"field","kind":"rock","count":9,"radius":6}"#,
        )
        .unwrap();
        let resolved = resolve_agent_assets(cmd, &manifest, 2, &mut used)
            .expect("resolves")
            .clone();
        match resolved {
            AgentCommand::ScatterField(c) => {
                assert!(!c.assets.is_empty());
                assert!(c.assets.iter().all(|f| f.ends_with(".glb")));
                assert_eq!(c.count, 9);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_tool_call_round_trips_spawn() {
        let args = r#"{"name":"tower","shape":"Cuboid","dimensions":{"x":2,"y":8,"z":2},"position":[0,4,0]}"#;
        let cmd = parse_tool_call("spawn_primitive", args).expect("parses");
        match cmd {
            AgentCommand::SpawnPrimitive(s) => {
                assert_eq!(s.name, "tower");
                assert_eq!(s.shape, PrimitiveShape::Cuboid);
                assert_eq!(s.position, [0.0, 4.0, 0.0]);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_tool_call_round_trips_place_asset() {
        // Kind form (the schema the model sees).
        let args = r#"{"name":"gate","kind":"rock","position":[0,0,-6],"scale":1.5}"#;
        let cmd = parse_tool_call("place_asset", args).expect("parses");
        match cmd {
            AgentCommand::PlaceAsset(p) => {
                assert_eq!(p.kind, "rock");
                assert_eq!(p.asset, "");
                assert_eq!(p.scale, 1.5);
            }
            _ => panic!("wrong variant"),
        }
        // Legacy/robustness: a model naming a concrete file still parses and
        // is resolved (or backfilled) by the session loop.
        let args = r#"{"name":"gate","asset":"rock_arch.glb"}"#;
        let cmd = parse_tool_call("place_asset", args).expect("parses");
        match cmd {
            AgentCommand::PlaceAsset(p) => assert_eq!(p.asset, "rock_arch.glb"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_tool_call_round_trips_scatter_field() {
        let args = r#"{"name":"pebbles","kind":"shell","count":200,"radius":900,"scale":3}"#;
        let cmd = parse_tool_call("scatter_field", args).expect("parses");
        match cmd {
            AgentCommand::ScatterField(c) => {
                assert_eq!(c.kind, "shell");
                assert_eq!(c.count, 48, "count clamps to the budget cap");
                assert_eq!(c.radius, 120.0, "radius clamps");
                assert!(c.assets.is_empty(), "resolved by the session loop");
            }
            _ => panic!("wrong variant"),
        }
        // Missing kind → unresolvable (the session reports the tool error).
        let cmd = parse_tool_call("scatter_field", r#"{"name":"x","count":4}"#).unwrap();
        match cmd {
            AgentCommand::ScatterField(c) => assert_eq!(c.kind, ""),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn parse_tool_call_rejects_unknown() {
        assert!(parse_tool_call("nope", "{}").is_none());
    }

    #[test]
    fn response_messages_are_readable() {
        assert_eq!(
            AgentResponse::Spawned { name: "x".into() }.to_message(),
            "spawned 'x'"
        );
        assert_eq!(
            AgentResponse::AssetPlaced {
                name: "y".into(),
                asset: "rock.glb".into(),
            }
            .to_message(),
            "placed 'y' (rock.glb)"
        );
        assert_eq!(
            AgentResponse::Scattered {
                name: "pebbles".into(),
                count: 12
            }
            .to_message(),
            "scattered 12 props as 'pebbles'"
        );
        assert!(
            AgentResponse::Error("bad".into())
                .to_message()
                .contains("bad")
        );
    }
}
