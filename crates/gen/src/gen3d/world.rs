//! World skill save/load — serialize scenes as complete skill directories.
//!
//! ## Format (v1 — RON)
//!
//! ```text
//! world-name/
//!   SKILL.md           — Skill metadata + description
//!   world.ron          — WorldManifest with inline entities (parametric shapes preserved)
//!   history.jsonl      — Undo/redo history
//!   assets/
//!     meshes/
//!       tree.glb       — Copied mesh assets (relative paths in world.ron)
//!       rock.glb
//!   export/            — Generated on demand via gen_export_world tool
//!     scene.glb        — OR scene.gltf + scene.bin (format selectable)
//! ```

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use localgpt_world_types as wt;

use super::audio::AudioEngine;
use super::behaviors::{BehaviorState, EntityBehaviors};
use super::commands::*;
use super::compat;
use super::plugin::GenWorkspace;
use super::registry::*;

// ---------------------------------------------------------------------------
// Environment snapshot (passed from plugin.rs which has access to Bevy resources)
// ---------------------------------------------------------------------------

pub struct EnvironmentSnapshot {
    pub background_color: Option<[f32; 4]>,
    pub ambient_intensity: Option<f32>,
    pub ambient_color: Option<[f32; 4]>,
}

// ---------------------------------------------------------------------------
// Save world (new RON format)
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
    _mesh_handles: &Query<&Mesh3d>,
    _mesh_assets: &Assets<Mesh>,
    audio_engine: &AudioEngine,
    behaviors_query: &Query<&mut EntityBehaviors>,
    parametric_shapes: &Query<&ParametricShape>,
    gltf_sources: &Query<&GltfSource>,
    visibility_query: &Query<&Visibility>,
    directional_lights: &Query<&DirectionalLight>,
    point_lights: &Query<&PointLight>,
    spot_lights: &Query<&SpotLight>,
    projections: &Query<&Projection>,
    env_snapshot: &EnvironmentSnapshot,
    avatar: Option<&AvatarDef>,
    tours: &[TourDef],
    edit_history: &wt::EditHistory,
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

    // Create assets/meshes directory for localized mesh assets
    let meshes_dir = skill_dir.join("assets").join("meshes");
    if let Err(e) = std::fs::create_dir_all(&meshes_dir) {
        return GenResponse::Error {
            message: format!("Failed to create assets/meshes directory: {}", e),
        };
    }

    // Collect all external mesh paths and build path mapping
    let mut asset_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, entity) in registry.all_names() {
        if let Ok(gltf_src) = gltf_sources.get(entity) {
            asset_paths.insert(gltf_src.path.clone());
        }
    }

    // Copy each mesh to assets/meshes/ and build path mapping: original -> relative
    let mut path_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for original_path in asset_paths {
        let resolved = resolve_gltf_path(&original_path, &workspace.path);
        if let Some(resolved) = resolved {
            if let Some(filename) = resolved.file_name() {
                let filename_str = filename.to_string_lossy().into_owned();
                // Generate unique filename if there's a conflict
                let unique_filename = if meshes_dir.join(&filename_str).exists() {
                    // Add a unique suffix based on path hash to avoid collisions
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    original_path.hash(&mut hasher);
                    let hash = hasher.finish();
                    let stem = std::path::Path::new(&filename_str)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| filename_str.clone());
                    let ext = std::path::Path::new(&filename_str)
                        .extension()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "glb".to_string());
                    format!("{}_{}{}.{}", stem, hash % 10000, "", ext)
                } else {
                    filename_str
                };

                if let Err(e) = std::fs::copy(&resolved, meshes_dir.join(&unique_filename)) {
                    tracing::warn!("Failed to copy mesh asset '{}': {}", original_path, e);
                    // Keep original path if copy fails
                    path_map.insert(original_path.clone(), original_path);
                } else {
                    let relative_path = format!("assets/meshes/{}", unique_filename);
                    path_map.insert(original_path.clone(), relative_path);
                }
            } else {
                // Keep original path if we can't extract filename
                path_map.insert(original_path.clone(), original_path);
            }
        } else {
            // Keep original path if we can't resolve it
            path_map.insert(original_path.clone(), original_path);
        }
    }

    // Collect all entities into WorldEntity objects
    let mut world_entities: Vec<wt::WorldEntity> = Vec::new();
    let mut next_id: u64 = 1;

    for (name, bevy_entity) in registry.all_names() {
        // Skip infrastructure entities (camera, default scene objects)
        let Some(gen_ent) = gen_entities.get(bevy_entity).ok() else {
            continue;
        };

        // Skip camera — stored separately
        if gen_ent.entity_type == GenEntityType::Camera {
            continue;
        }

        let entity_id = gen_ent.world_id;
        if entity_id.0 >= next_id {
            next_id = entity_id.0 + 1;
        }

        let transform = transforms.get(bevy_entity).copied().unwrap_or_default();
        let euler = transform.rotation.to_euler(EulerRot::XYZ);

        let mut we = wt::WorldEntity::new(entity_id.0, name);
        we.transform = wt::WorldTransform {
            position: transform.translation.to_array(),
            rotation_degrees: [
                euler.0.to_degrees(),
                euler.1.to_degrees(),
                euler.2.to_degrees(),
            ],
            scale: transform.scale.to_array(),
            visible: visibility_query
                .get(bevy_entity)
                .map(|v| *v != Visibility::Hidden)
                .unwrap_or(true),
        };

        // Parent
        if let Ok(child_of) = parent_query.get(bevy_entity)
            && let Some(parent_name) = registry.get_name(child_of.parent())
            && let Some(parent_bevy) = registry.get_entity(parent_name)
            && let Ok(parent_gen) = gen_entities.get(parent_bevy)
        {
            we.parent = Some(parent_gen.world_id);
        }

        // Shape (from ParametricShape component — preserves dimensions!)
        if let Ok(param) = parametric_shapes.get(bevy_entity) {
            we.shape = Some(param.shape.clone());
        }

        // Mesh asset (imported glTF source path — use localized relative path)
        if let Ok(gltf_src) = gltf_sources.get(bevy_entity) {
            let relative_path = path_map.get(&gltf_src.path).unwrap_or(&gltf_src.path);
            we.mesh_asset = Some(wt::MeshAssetRef {
                path: relative_path.clone(),
                node: None,
            });
        }

        // Material
        if let Ok(mat_handle) = material_handles.get(bevy_entity)
            && let Some(mat) = material_assets.get(&mat_handle.0)
        {
            let c = mat.base_color.to_srgba();
            let e = mat.emissive;
            let alpha_mode = match mat.alpha_mode {
                AlphaMode::Opaque => None,
                AlphaMode::Mask(cutoff) => Some(wt::AlphaModeDef::Mask(cutoff)),
                AlphaMode::Blend => Some(wt::AlphaModeDef::Blend),
                AlphaMode::Add => Some(wt::AlphaModeDef::Add),
                AlphaMode::Multiply => Some(wt::AlphaModeDef::Multiply),
                _ => None,
            };
            we.material = Some(wt::MaterialDef {
                color: [c.red, c.green, c.blue, c.alpha],
                metallic: mat.metallic,
                roughness: mat.perceptual_roughness,
                emissive: [e.red, e.green, e.blue, e.alpha],
                alpha_mode,
                unlit: if mat.unlit { Some(true) } else { None },
                double_sided: if mat.double_sided { Some(true) } else { None },
                reflectance: if (mat.reflectance - 0.5).abs() > f32::EPSILON {
                    Some(mat.reflectance)
                } else {
                    None
                },
            });
        }

        // Light — extract from Bevy light components (any entity type can have a light)
        {
            if let Ok(dl) = directional_lights.get(bevy_entity) {
                let c = dl.color.to_srgba();
                let dir = transform.forward().as_vec3().to_array();
                we.light = Some(wt::LightDef {
                    light_type: wt::LightType::Directional,
                    color: [c.red, c.green, c.blue, c.alpha],
                    intensity: dl.illuminance,
                    direction: Some(dir),
                    shadows: dl.shadows_enabled,
                    range: None,
                    outer_angle: None,
                    inner_angle: None,
                });
            } else if let Ok(pl) = point_lights.get(bevy_entity) {
                let c = pl.color.to_srgba();
                we.light = Some(wt::LightDef {
                    light_type: wt::LightType::Point,
                    color: [c.red, c.green, c.blue, c.alpha],
                    intensity: pl.intensity,
                    direction: None,
                    shadows: pl.shadows_enabled,
                    range: Some(pl.range),
                    outer_angle: None,
                    inner_angle: None,
                });
            } else if let Ok(sl) = spot_lights.get(bevy_entity) {
                let c = sl.color.to_srgba();
                let dir = transform.forward().as_vec3().to_array();
                we.light = Some(wt::LightDef {
                    light_type: wt::LightType::Spot,
                    color: [c.red, c.green, c.blue, c.alpha],
                    intensity: sl.intensity,
                    direction: Some(dir),
                    shadows: sl.shadows_enabled,
                    range: Some(sl.range),
                    outer_angle: Some(sl.outer_angle),
                    inner_angle: Some(sl.inner_angle),
                });
            }
        }

        // Behaviors
        if let Ok(eb) = behaviors_query.get(bevy_entity) {
            for inst in &eb.behaviors {
                we.behaviors.push((&inst.def).into());
            }
        }

        // Audio emitter (check if this entity has audio attached)
        // Audio emitters are tracked in AudioEngine.emitter_meta by name.
        if let Some(meta) = audio_engine.emitter_meta.get(name) {
            let source: wt::AudioSource = (&meta.sound).into();
            we.audio = Some(wt::AudioDef {
                kind: wt::AudioKind::Sfx,
                source,
                volume: meta.base_volume,
                radius: Some(meta.radius),
                rolloff: wt::Rolloff::InverseSquare,
            });
        }

        world_entities.push(we);
    }

    // Collect ambient audio as root-level entities
    if let Some(ref ambience_cmd) = audio_engine.last_ambience {
        for layer in &ambience_cmd.layers {
            let source: wt::AudioSource = (&layer.sound).into();
            let mut we = wt::WorldEntity::new(next_id, format!("ambience_{}", layer.name));
            next_id += 1;
            we.audio = Some(wt::AudioDef {
                kind: wt::AudioKind::Ambient,
                source,
                volume: layer.volume,
                radius: None, // global
                rolloff: wt::Rolloff::InverseSquare,
            });
            world_entities.push(we);
        }
    }

    // Camera
    let camera_def = registry.get_entity("main_camera").and_then(|e| {
        transforms.get(e).ok().map(|t| {
            let forward = t.forward().as_vec3();
            let look_at = t.translation + forward * 10.0;
            let fov_degrees = projections
                .get(e)
                .ok()
                .and_then(|p| match p {
                    Projection::Perspective(pp) => Some(pp.fov.to_degrees()),
                    _ => None,
                })
                .unwrap_or(45.0);
            wt::CameraDef {
                position: t.translation.to_array(),
                look_at: look_at.to_array(),
                fov_degrees,
            }
        })
    });

    // Environment
    let environment =
        if env_snapshot.background_color.is_some() || env_snapshot.ambient_intensity.is_some() {
            Some(wt::EnvironmentDef {
                background_color: env_snapshot.background_color,
                ambient_intensity: env_snapshot.ambient_intensity,
                ambient_color: env_snapshot.ambient_color,
                fog_density: None,
                fog_color: None,
            })
        } else {
            None
        };

    // Build the manifest
    let manifest = wt::WorldManifest {
        version: 1,
        meta: wt::WorldMeta {
            name: cmd.name.clone(),
            description: cmd.description.clone(),
            biome: None,
            time_of_day: None,
        },
        environment,
        camera: camera_def,
        avatar: avatar.map(|a| a.into()),
        tours: tours.iter().map(|t| t.into()).collect(),
        entities: world_entities,
        creations: Vec::new(),
        next_entity_id: next_id,
    };

    // Validate before saving
    let validation_issues =
        wt::validation::validate_entities(&manifest.entities, &wt::WorldLimits::default());
    let warnings: Vec<String> = validation_issues
        .iter()
        .map(|i| i.message.clone())
        .collect();
    for issue in &validation_issues {
        match issue.severity {
            wt::Severity::Warning => tracing::warn!("Save validation: {}", issue.message),
            wt::Severity::Error => tracing::error!("Save validation: {}", issue.message),
        }
    }

    // Write world.ron
    let ron_str = ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default())
        .unwrap_or_else(|e| {
            tracing::error!("RON serialization failed: {}", e);
            String::new()
        });
    if let Err(e) = std::fs::write(skill_dir.join("world.ron"), &ron_str) {
        return GenResponse::Error {
            message: format!("Failed to write world.ron: {}", e),
        };
    }

    // Write SKILL.md
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

To export for external viewers, use `gen_export_world` with format "glb" or "gltf".
"#,
        name = cmd.name,
        description = description,
    );
    if let Err(e) = std::fs::write(skill_dir.join("SKILL.md"), &skill_md) {
        return GenResponse::Error {
            message: format!("Failed to write SKILL.md: {}", e),
        };
    }

    // Write undo history as JSONL (one WorldEdit per line)
    if !edit_history.edits.is_empty() {
        let mut lines = String::new();
        for edit in &edit_history.edits {
            if let Ok(line) = serde_json::to_string(edit) {
                lines.push_str(&line);
                lines.push('\n');
            }
        }
        // Also store cursor position as a metadata line
        lines.push_str(&format!("{{\"_cursor\":{}}}\n", edit_history.cursor));
        if let Err(e) = std::fs::write(skill_dir.join("history.jsonl"), &lines) {
            // Non-fatal — undo history is optional
            tracing::warn!("Failed to write history.jsonl: {}", e);
        }
    }

    GenResponse::WorldSaved {
        path: skill_dir.to_string_lossy().into_owned(),
        skill_name: cmd.name,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Load world (RON format)
// ---------------------------------------------------------------------------

/// Result of parsing a world skill directory (returned to plugin.rs for ECS application).
pub struct WorldLoadResult {
    pub world_path: String,
    /// Entities to spawn directly from WorldManifest.
    pub world_entities: Vec<wt::WorldEntity>,
    pub ambience: Option<AmbienceCmd>,
    pub emitters: Vec<AudioEmitterCmd>,
    pub environment: Option<EnvironmentCmd>,
    pub camera: Option<CameraCmd>,
    pub avatar: Option<AvatarDef>,
    pub tours: Vec<TourDef>,
    pub entity_count: usize,
    pub behavior_count: usize,
    /// Restored edit history from history.jsonl (if present).
    pub edit_history: Option<wt::EditHistory>,
}

pub fn handle_load_world(
    path: &str,
    workspace: &GenWorkspace,
    _behavior_state: &mut BehaviorState,
) -> Result<WorldLoadResult, String> {
    let world_dir = resolve_world_path(path, &workspace.path)
        .ok_or_else(|| format!("World skill not found: {}", path))?;

    let ron_path = world_dir.join("world.ron");
    if !ron_path.exists() {
        return Err(format!(
            "No world.ron found in {}. Save a world first with gen_save_world.",
            world_dir.display()
        ));
    }
    load_ron_world(&world_dir, &ron_path)
}

/// Load a world from the new RON format.
fn load_ron_world(world_dir: &Path, ron_path: &Path) -> Result<WorldLoadResult, String> {
    let ron_str = std::fs::read_to_string(ron_path)
        .map_err(|e| format!("Failed to read world.ron: {}", e))?;
    let manifest: wt::WorldManifest =
        ron::from_str(&ron_str).map_err(|e| format!("Failed to parse world.ron: {}", e))?;

    // Check version compatibility
    if let Err(e) = manifest.check_version() {
        return Err(format!(
            "World '{}' is incompatible: {}. \
             Try regenerating the world or using an older version of localgpt-gen.",
            manifest.meta.name, e
        ));
    }

    // Extract ambient audio from entities (kind == Ambient, radius == None)
    let mut ambience_layers: Vec<AmbienceLayerDef> = Vec::new();
    let mut emitters: Vec<AudioEmitterCmd> = Vec::new();
    let mut scene_entities: Vec<wt::WorldEntity> = Vec::new();

    for entity in &manifest.entities {
        if let Some(ref audio) = entity.audio {
            if audio.kind == wt::AudioKind::Ambient && audio.radius.is_none() {
                // This is an ambient layer — extract to legacy format for audio engine
                if let Some(ambient_sound) = compat::audio_source_to_ambient(&audio.source) {
                    ambience_layers.push(AmbienceLayerDef {
                        name: entity
                            .name
                            .as_str()
                            .strip_prefix("ambience_")
                            .unwrap_or(entity.name.as_str())
                            .to_string(),
                        sound: ambient_sound,
                        volume: audio.volume,
                    });
                }
                // Don't include ambient-only entities in scene spawn
                if entity.shape.is_none() && entity.light.is_none() {
                    continue;
                }
            } else if let Some(emitter_sound) = compat::audio_source_to_emitter(&audio.source) {
                // Spatial emitter
                emitters.push(AudioEmitterCmd {
                    name: entity.name.as_str().to_string(),
                    entity: Some(entity.name.as_str().to_string()),
                    position: Some(entity.transform.position),
                    sound: emitter_sound,
                    radius: audio.radius.unwrap_or(20.0),
                    volume: audio.volume,
                });
            }
        }
        scene_entities.push(entity.clone());
    }

    // Count behaviors across all entities
    let behavior_count: usize = scene_entities.iter().map(|e| e.behaviors.len()).sum();

    let ambience = if ambience_layers.is_empty() {
        None
    } else {
        Some(AmbienceCmd {
            layers: ambience_layers,
            master_volume: None,
            reverb: None,
        })
    };

    let environment = manifest.environment.as_ref().map(|e| EnvironmentCmd {
        background_color: e.background_color,
        ambient_light: e.ambient_intensity,
        ambient_color: e.ambient_color,
    });

    let camera = manifest.camera.as_ref().map(|c| CameraCmd {
        position: c.position,
        look_at: c.look_at,
        fov_degrees: c.fov_degrees,
    });

    let avatar = manifest.avatar.as_ref().map(|a| a.into());
    let tours: Vec<TourDef> = manifest.tours.iter().map(|t| t.into()).collect();
    let entity_count = scene_entities.len();

    // Load edit history from history.jsonl if present
    let edit_history = load_edit_history(&world_dir.join("history.jsonl"));

    Ok(WorldLoadResult {
        world_path: world_dir.to_string_lossy().into_owned(),
        world_entities: scene_entities,
        ambience,
        emitters,
        environment,
        camera,
        avatar,
        tours,
        entity_count,
        behavior_count,
        edit_history,
    })
}

/// Load edit history from a JSONL file. Returns None if file doesn't exist or has errors.
fn load_edit_history(path: &std::path::Path) -> Option<wt::EditHistory> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut edits = Vec::new();
    let mut cursor = 0usize;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Check for cursor metadata line
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(c) = val.get("_cursor").and_then(|v| v.as_u64())
        {
            cursor = c as usize;
            continue;
        }
        // Parse as WorldEdit
        match serde_json::from_str::<wt::WorldEdit>(line) {
            Ok(edit) => edits.push(edit),
            Err(e) => {
                tracing::warn!("Skipping malformed history line: {}", e);
            }
        }
    }

    if edits.is_empty() {
        return None;
    }

    // Clamp cursor to valid range
    let cursor = cursor.min(edits.len());

    Some(wt::EditHistory { edits, cursor })
}

/// Resolve a world skill path by looking for `world.ron`.
fn resolve_world_path(path: &str, workspace: &Path) -> Option<PathBuf> {
    let expanded = shellexpand::tilde(path).into_owned();

    // 1. As-is
    let p = PathBuf::from(&expanded);
    if p.is_dir() && p.join("world.ron").exists() {
        return Some(p);
    }

    // 2. {workspace}/skills/{name}
    let skill_path = workspace.join("skills").join(&expanded);
    if skill_path.is_dir() && skill_path.join("world.ron").exists() {
        return Some(skill_path);
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_manifest_roundtrip() {
        let mut manifest = wt::WorldManifest::new("test_world");
        manifest.meta.description = Some("A test".to_string());
        manifest.environment = Some(wt::EnvironmentDef {
            background_color: Some([0.1, 0.1, 0.2, 1.0]),
            ambient_intensity: Some(0.3),
            ambient_color: None,
            fog_density: None,
            fog_color: None,
        });
        manifest.camera = Some(wt::CameraDef::default());
        manifest.avatar = Some(wt::AvatarDef::default());
        manifest.entities.push(
            wt::WorldEntity::new(1, "cube").with_shape(wt::Shape::Cuboid {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        manifest.next_entity_id = 2;

        let ron_str =
            ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default()).unwrap();
        let back: wt::WorldManifest = ron::from_str(&ron_str).unwrap();
        assert_eq!(back.meta.name, "test_world");
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.entities[0].name.as_str(), "cube");
        assert!(back.entities[0].shape.is_some());
    }

    #[test]
    fn ron_manifest_with_behaviors_and_audio() {
        let mut manifest = wt::WorldManifest::new("campfire_scene");

        // A campfire entity with shape + light + audio + behavior
        let campfire = wt::WorldEntity::new(1, "campfire")
            .at([5.0, 0.0, 3.0])
            .with_shape(wt::Shape::Cone {
                radius: 0.5,
                height: 1.0,
            })
            .with_material(wt::MaterialDef {
                color: [0.8, 0.3, 0.1, 1.0],
                ..Default::default()
            })
            .with_behavior(wt::BehaviorDef::Pulse {
                min_scale: 0.9,
                max_scale: 1.1,
                frequency: 0.5,
            })
            .with_audio(wt::AudioDef {
                kind: wt::AudioKind::Sfx,
                source: wt::AudioSource::Fire {
                    intensity: 0.8,
                    crackle: 0.5,
                },
                volume: 0.7,
                radius: Some(15.0),
                rolloff: wt::Rolloff::InverseSquare,
            });

        manifest.entities.push(campfire);
        manifest.next_entity_id = 2;

        let ron_str =
            ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default()).unwrap();
        let back: wt::WorldManifest = ron::from_str(&ron_str).unwrap();
        let e = &back.entities[0];
        assert_eq!(e.name.as_str(), "campfire");
        assert!(e.shape.is_some());
        assert!(e.material.is_some());
        assert_eq!(e.behaviors.len(), 1);
        assert!(e.audio.is_some());
    }

    #[test]
    fn history_jsonl_roundtrip() {
        let mut history = wt::EditHistory::new();

        // Push a couple of edits
        let entity1 = wt::WorldEntity::new(1, "cube");
        history.push(
            wt::EditOp::spawn(entity1.clone()),
            wt::EditOp::delete(entity1.id),
            Some("test".to_string()),
        );

        let entity2 = wt::WorldEntity::new(2, "sphere");
        history.push(
            wt::EditOp::spawn(entity2.clone()),
            wt::EditOp::delete(entity2.id),
            None,
        );

        // Undo one to set cursor != edits.len()
        history.undo();
        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 1);

        // Serialize to JSONL format (same as save handler)
        let mut lines = String::new();
        for edit in &history.edits {
            let line = serde_json::to_string(edit).unwrap();
            lines.push_str(&line);
            lines.push('\n');
        }
        lines.push_str(&format!("{{\"_cursor\":{}}}\n", history.cursor));

        // Write to temp file and load back
        let dir = std::env::temp_dir().join("localgpt_test_history");
        let _ = std::fs::create_dir_all(&dir);
        let history_path = dir.join("history.jsonl");
        std::fs::write(&history_path, &lines).unwrap();

        let loaded = load_edit_history(&history_path).unwrap();
        assert_eq!(loaded.edits.len(), 2);
        assert_eq!(loaded.cursor, 1); // cursor restored
        assert_eq!(loaded.undo_count(), 1);
        assert_eq!(loaded.redo_count(), 1);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
