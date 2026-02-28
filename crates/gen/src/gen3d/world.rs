//! World skill save/load — serialize scenes as complete skill directories.
//!
//! A world skill directory contains:
//! - `SKILL.md`       — Skill metadata + description
//! - `world.toml`     — Manifest tying everything together
//! - `scene.glb`      — glTF geometry & materials
//! - `behaviors.toml` — Declarative behavior definitions
//! - `audio.toml`     — Ambience + spatial audio emitters
//! - `tours.toml`     — Guided tours (optional)

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::audio::AudioEngine;
use super::behaviors::{self, BehaviorState, EntityBehaviors};
use super::commands::*;
use super::plugin::GenWorkspace;
use super::registry::*;

// ---------------------------------------------------------------------------
// TOML data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct WorldManifest {
    world: WorldMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    environment: Option<EnvironmentDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<CameraDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar: Option<AvatarDef>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorldMeta {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default = "default_scene_file")]
    scene: String,
    #[serde(default = "default_behaviors_file")]
    behaviors: String,
    #[serde(default = "default_audio_file")]
    audio: String,
    #[serde(default = "default_tours_file")]
    tours: String,
}

fn default_scene_file() -> String {
    "scene.glb".to_string()
}
fn default_behaviors_file() -> String {
    "behaviors.toml".to_string()
}
fn default_audio_file() -> String {
    "audio.toml".to_string()
}
fn default_tours_file() -> String {
    "tours.toml".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct EnvironmentDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background_color: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ambient_intensity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ambient_color: Option<[f32; 4]>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CameraDef {
    position: [f32; 3],
    look_at: [f32; 3],
    #[serde(default = "default_fov")]
    fov_degrees: f32,
}

fn default_fov() -> f32 {
    45.0
}

#[derive(Debug, Serialize, Deserialize)]
struct BehaviorsFile {
    #[serde(default)]
    behaviors: Vec<BehaviorEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BehaviorEntry {
    entity: String,
    #[serde(flatten)]
    behavior: BehaviorDef,
}

#[derive(Debug, Serialize, Deserialize)]
struct AudioFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ambience: Option<AmbienceDef>,
    #[serde(default)]
    emitters: Vec<AudioEmitterCmd>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AmbienceDef {
    layers: Vec<AmbienceLayerDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    master_volume: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToursFile {
    #[serde(default)]
    tours: Vec<TourDef>,
}

// ---------------------------------------------------------------------------
// Environment snapshot (passed from plugin.rs which has access to Bevy resources)
// ---------------------------------------------------------------------------

pub struct EnvironmentSnapshot {
    pub background_color: Option<[f32; 4]>,
    pub ambient_intensity: Option<f32>,
    pub ambient_color: Option<[f32; 4]>,
}

// ---------------------------------------------------------------------------
// Save world
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn handle_save_world(
    cmd: SaveWorldCmd,
    workspace: &GenWorkspace,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    parent_query: &Query<&ChildOf>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
    mesh_handles: &Query<&Mesh3d>,
    mesh_assets: &Assets<Mesh>,
    audio_engine: &AudioEngine,
    behaviors_query: &Query<&mut EntityBehaviors>,
    env_snapshot: &EnvironmentSnapshot,
    avatar: Option<&AvatarDef>,
    tours: &[TourDef],
) -> GenResponse {
    // Resolve output directory
    let skill_dir = if let Some(ref path) = cmd.path {
        PathBuf::from(shellexpand::tilde(path).as_ref())
    } else {
        workspace.path.join("skills").join(&cmd.name)
    };

    if let Err(e) = std::fs::create_dir_all(&skill_dir) {
        return GenResponse::Error {
            message: format!("Failed to create skill directory: {}", e),
        };
    }

    // 1. Export scene.glb
    let scene_path = skill_dir.join("scene.glb");
    let glb_result = export_scene_glb(
        &scene_path,
        registry,
        transforms,
        gen_entities,
        parent_query,
        material_handles,
        material_assets,
        mesh_handles,
        mesh_assets,
    );
    if let Err(e) = glb_result {
        return GenResponse::Error {
            message: format!("Failed to export scene: {}", e),
        };
    }

    // 2. Write behaviors.toml
    let all_behaviors = behaviors::collect_all_behaviors(registry, behaviors_query);
    let behaviors_file = BehaviorsFile {
        behaviors: all_behaviors
            .iter()
            .flat_map(|(entity, defs)| {
                defs.iter().map(move |def| BehaviorEntry {
                    entity: entity.clone(),
                    behavior: def.clone(),
                })
            })
            .collect(),
    };
    let behaviors_toml = toml::to_string_pretty(&behaviors_file).unwrap_or_else(|_| String::new());
    if let Err(e) = std::fs::write(skill_dir.join("behaviors.toml"), &behaviors_toml) {
        return GenResponse::Error {
            message: format!("Failed to write behaviors.toml: {}", e),
        };
    }

    // 3. Write audio.toml
    let audio_file = collect_audio_state(audio_engine);
    let audio_toml = toml::to_string_pretty(&audio_file).unwrap_or_else(|_| String::new());
    if let Err(e) = std::fs::write(skill_dir.join("audio.toml"), &audio_toml) {
        return GenResponse::Error {
            message: format!("Failed to write audio.toml: {}", e),
        };
    }

    // 4. Gather camera info
    let camera_def = registry.get_entity("main_camera").and_then(|e| {
        transforms.get(e).ok().map(|t| {
            let forward = t.forward().as_vec3();
            let look_at = t.translation + forward * 10.0;
            CameraDef {
                position: t.translation.to_array(),
                look_at: look_at.to_array(),
                fov_degrees: 45.0,
            }
        })
    });

    // 5. Write world.toml
    let manifest = WorldManifest {
        world: WorldMeta {
            name: cmd.name.clone(),
            description: cmd.description.clone(),
            scene: "scene.glb".to_string(),
            behaviors: "behaviors.toml".to_string(),
            audio: "audio.toml".to_string(),
            tours: "tours.toml".to_string(),
        },
        environment: if env_snapshot.background_color.is_some()
            || env_snapshot.ambient_intensity.is_some()
        {
            Some(EnvironmentDef {
                background_color: env_snapshot.background_color,
                ambient_intensity: env_snapshot.ambient_intensity,
                ambient_color: env_snapshot.ambient_color,
            })
        } else {
            None
        },
        camera: camera_def,
        avatar: avatar.cloned(),
    };
    let manifest_toml = toml::to_string_pretty(&manifest).unwrap_or_else(|_| String::new());
    if let Err(e) = std::fs::write(skill_dir.join("world.toml"), &manifest_toml) {
        return GenResponse::Error {
            message: format!("Failed to write world.toml: {}", e),
        };
    }

    // 6. Write tours.toml (only if tours exist)
    if !tours.is_empty() {
        let tours_file = ToursFile {
            tours: tours.to_vec(),
        };
        let tours_toml = toml::to_string_pretty(&tours_file).unwrap_or_else(|_| String::new());
        if let Err(e) = std::fs::write(skill_dir.join("tours.toml"), &tours_toml) {
            return GenResponse::Error {
                message: format!("Failed to write tours.toml: {}", e),
            };
        }
    }

    // 7. Write SKILL.md
    let description = cmd.description.as_deref().unwrap_or("A generated 3D world");
    let skill_md = format!(
        r#"---
name: "{name}"
description: "{description}"
user-invocable: true
metadata:
  type: "world"
useWhen:
  - contains: "{name}"
---
# {name}

{description}

This is a gen world skill. Load it with `gen_load_world` to restore the 3D scene,
behaviors, audio, avatar, and tours.
"#,
        name = cmd.name,
        description = description,
    );
    if let Err(e) = std::fs::write(skill_dir.join("SKILL.md"), &skill_md) {
        return GenResponse::Error {
            message: format!("Failed to write SKILL.md: {}", e),
        };
    }

    GenResponse::WorldSaved {
        path: skill_dir.to_string_lossy().into_owned(),
        skill_name: cmd.name,
    }
}

// ---------------------------------------------------------------------------
// Load world
// ---------------------------------------------------------------------------

/// Result of parsing a world skill directory (returned to plugin.rs for ECS application).
pub struct WorldLoadResult {
    pub world_path: String,
    pub scene_path: Option<String>,
    pub behaviors: Vec<(String, Vec<BehaviorDef>)>,
    pub ambience: Option<AmbienceCmd>,
    pub emitters: Vec<AudioEmitterCmd>,
    pub environment: Option<EnvironmentCmd>,
    pub camera: Option<CameraCmd>,
    pub avatar: Option<AvatarDef>,
    pub tours: Vec<TourDef>,
    pub entity_count: usize,
    pub behavior_count: usize,
}

pub fn handle_load_world(
    path: &str,
    workspace: &GenWorkspace,
    _behavior_state: &mut BehaviorState,
) -> Result<WorldLoadResult, String> {
    let world_dir = resolve_world_path(path, &workspace.path)
        .ok_or_else(|| format!("World skill not found: {}", path))?;

    // Read world.toml
    let manifest_path = world_dir.join("world.toml");
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read world.toml: {}", e))?;
    let manifest: WorldManifest =
        toml::from_str(&manifest_str).map_err(|e| format!("Failed to parse world.toml: {}", e))?;

    // Resolve scene.glb path
    let scene_path = {
        let p = world_dir.join(&manifest.world.scene);
        if p.exists() {
            Some(p.to_string_lossy().into_owned())
        } else {
            None
        }
    };

    // Read behaviors.toml
    let behaviors_path = world_dir.join(&manifest.world.behaviors);
    let mut behaviors: Vec<(String, Vec<BehaviorDef>)> = Vec::new();
    if behaviors_path.exists() {
        let s = std::fs::read_to_string(&behaviors_path)
            .map_err(|e| format!("Failed to read behaviors.toml: {}", e))?;
        let file: BehaviorsFile =
            toml::from_str(&s).map_err(|e| format!("Failed to parse behaviors.toml: {}", e))?;

        // Group by entity
        let mut map: std::collections::HashMap<String, Vec<BehaviorDef>> =
            std::collections::HashMap::new();
        for entry in file.behaviors {
            map.entry(entry.entity).or_default().push(entry.behavior);
        }
        behaviors = map.into_iter().collect();
    }

    // Read audio.toml
    let audio_path = world_dir.join(&manifest.world.audio);
    let mut ambience: Option<AmbienceCmd> = None;
    let mut emitters: Vec<AudioEmitterCmd> = Vec::new();
    if audio_path.exists() {
        let s = std::fs::read_to_string(&audio_path)
            .map_err(|e| format!("Failed to read audio.toml: {}", e))?;
        let audio_file: AudioFile =
            toml::from_str(&s).map_err(|e| format!("Failed to parse audio.toml: {}", e))?;

        if let Some(amb) = audio_file.ambience {
            ambience = Some(AmbienceCmd {
                layers: amb.layers,
                master_volume: amb.master_volume,
                reverb: None,
            });
        }
        emitters = audio_file.emitters;
    }

    // Environment
    let environment = manifest.environment.map(|env| EnvironmentCmd {
        background_color: env.background_color,
        ambient_light: env.ambient_intensity,
        ambient_color: env.ambient_color,
    });

    // Camera
    let camera = manifest.camera.map(|cam| CameraCmd {
        position: cam.position,
        look_at: cam.look_at,
        fov_degrees: cam.fov_degrees,
    });

    // Avatar
    let avatar = manifest.avatar;

    // Read tours.toml
    let tours_path = world_dir.join(&manifest.world.tours);
    let mut tours: Vec<TourDef> = Vec::new();
    if tours_path.exists() {
        let s = std::fs::read_to_string(&tours_path)
            .map_err(|e| format!("Failed to read tours.toml: {}", e))?;
        let tours_file: ToursFile =
            toml::from_str(&s).map_err(|e| format!("Failed to parse tours.toml: {}", e))?;
        tours = tours_file.tours;
    }

    let behavior_count: usize = behaviors.iter().map(|(_, v)| v.len()).sum();

    Ok(WorldLoadResult {
        world_path: world_dir.to_string_lossy().into_owned(),
        scene_path,
        entity_count: 0, // Will be counted after scene loads
        behavior_count,
        behaviors,
        ambience,
        emitters,
        environment,
        camera,
        avatar,
        tours,
    })
}

/// Resolve a world skill path:
/// 1. As-is (absolute or relative)
/// 2. {workspace}/skills/{name}
/// 3. {workspace}/skills/{name}/ (with trailing slash)
fn resolve_world_path(path: &str, workspace: &Path) -> Option<PathBuf> {
    let expanded = shellexpand::tilde(path).into_owned();

    // 1. As-is
    let p = PathBuf::from(&expanded);
    if p.is_dir() && p.join("world.toml").exists() {
        return Some(p);
    }

    // 2. {workspace}/skills/{name}
    let skill_path = workspace.join("skills").join(&expanded);
    if skill_path.is_dir() && skill_path.join("world.toml").exists() {
        return Some(skill_path);
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_audio_state(engine: &AudioEngine) -> AudioFile {
    AudioFile {
        ambience: engine.last_ambience.as_ref().map(|cmd| AmbienceDef {
            layers: cmd.layers.clone(),
            master_volume: cmd.master_volume,
        }),
        emitters: engine
            .emitter_meta
            .iter()
            .map(|(name, meta)| AudioEmitterCmd {
                name: name.clone(),
                entity: meta.attached_to.clone(),
                position: meta.position,
                sound: meta.sound.clone(),
                radius: meta.radius,
                volume: meta.base_volume,
            })
            .collect(),
    }
}

/// Export scene geometry to GLB. Delegates to `gltf_export::export_glb`.
#[allow(clippy::too_many_arguments)]
fn export_scene_glb(
    output_path: &Path,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    parent_query: &Query<&ChildOf>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
    mesh_handles: &Query<&Mesh3d>,
    mesh_assets: &Assets<Mesh>,
) -> Result<(), String> {
    super::gltf_export::export_glb(
        output_path,
        registry,
        transforms,
        gen_entities,
        parent_query,
        material_handles,
        material_assets,
        mesh_handles,
        mesh_assets,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_serialization_roundtrip() {
        let avatar = AvatarDef {
            spawn_position: [1.0, 1.8, 5.0],
            spawn_look_at: [0.0, 1.5, 0.0],
            pov: PointOfView::FirstPerson,
            movement_speed: 3.0,
            height: 1.8,
            model_entity: Some("player".to_string()),
        };

        let manifest = WorldManifest {
            world: WorldMeta {
                name: "test".to_string(),
                description: None,
                scene: default_scene_file(),
                behaviors: default_behaviors_file(),
                audio: default_audio_file(),
                tours: default_tours_file(),
            },
            environment: None,
            camera: None,
            avatar: Some(avatar),
        };

        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        let parsed: WorldManifest = toml::from_str(&toml_str).unwrap();
        let a = parsed.avatar.unwrap();
        assert_eq!(a.pov, PointOfView::FirstPerson);
        assert_eq!(a.spawn_position, [1.0, 1.8, 5.0]);
        assert_eq!(a.model_entity.as_deref(), Some("player"));
    }

    #[test]
    fn tours_serialization_roundtrip() {
        let tours = ToursFile {
            tours: vec![TourDef {
                name: "grand_tour".to_string(),
                description: Some("A tour".to_string()),
                waypoints: vec![
                    TourWaypoint {
                        position: [0.0, 1.0, 5.0],
                        look_at: [0.0, 1.5, 0.0],
                        description: Some("Start".to_string()),
                        pause_duration: 3.0,
                    },
                    TourWaypoint {
                        position: [10.0, 1.0, 0.0],
                        look_at: [5.0, 2.0, -5.0],
                        description: None,
                        pause_duration: 0.0,
                    },
                ],
                speed: 2.0,
                mode: TourMode::Walk,
                autostart: false,
                loop_tour: true,
                pov: Some(PointOfView::ThirdPerson),
            }],
        };

        let toml_str = toml::to_string_pretty(&tours).unwrap();
        let parsed: ToursFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.tours.len(), 1);
        assert_eq!(parsed.tours[0].name, "grand_tour");
        assert_eq!(parsed.tours[0].waypoints.len(), 2);
        assert_eq!(parsed.tours[0].mode, TourMode::Walk);
        assert!(parsed.tours[0].loop_tour);
    }

    #[test]
    fn backward_compatible_manifest_without_avatar_or_tours() {
        let toml_str = r#"
[world]
name = "legacy_world"
scene = "scene.glb"
behaviors = "behaviors.toml"
audio = "audio.toml"

[camera]
position = [5.0, 5.0, 5.0]
look_at = [0.0, 0.0, 0.0]
fov_degrees = 45.0
"#;
        let parsed: WorldManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.world.name, "legacy_world");
        assert!(parsed.avatar.is_none());
        assert_eq!(parsed.world.tours, "tours.toml");
    }

    #[test]
    fn default_avatar_values() {
        let avatar = AvatarDef::default();
        assert_eq!(avatar.pov, PointOfView::ThirdPerson);
        assert_eq!(avatar.movement_speed, 5.0);
        assert_eq!(avatar.height, 1.8);
        assert!(avatar.model_entity.is_none());
    }

    #[test]
    fn tour_modes_serialize_correctly() {
        let walk_tour = TourDef {
            name: "walk".to_string(),
            description: None,
            waypoints: vec![],
            speed: 2.0,
            mode: TourMode::Walk,
            autostart: false,
            loop_tour: false,
            pov: None,
        };
        let fly_tour = TourDef {
            mode: TourMode::Fly,
            name: "fly".to_string(),
            ..walk_tour.clone()
        };
        let tp_tour = TourDef {
            mode: TourMode::Teleport,
            name: "tp".to_string(),
            ..walk_tour.clone()
        };

        let file = ToursFile {
            tours: vec![walk_tour, fly_tour, tp_tour],
        };
        let toml_str = toml::to_string_pretty(&file).unwrap();
        assert!(toml_str.contains("\"walk\""));
        assert!(toml_str.contains("\"fly\""));
        assert!(toml_str.contains("\"teleport\""));

        let parsed: ToursFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.tours[0].mode, TourMode::Walk);
        assert_eq!(parsed.tours[1].mode, TourMode::Fly);
        assert_eq!(parsed.tours[2].mode, TourMode::Teleport);
    }
}
