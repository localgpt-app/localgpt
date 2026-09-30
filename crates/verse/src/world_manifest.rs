//! Export the current track's world as a `localgpt-world-types` manifest.
//!
//! A Verse world is a performance: the mood palette, the analysis sidecar
//! and the agent's scene build together describe a world that plays to a
//! song. This module writes that description in LocalGPT's shared world
//! format, so the same world renders in Gen, MD and the web viewer
//! (localgpt.world) — and performs there too, because the manifest carries a
//! `SoundtrackDef` (the analysis curves, never the audio file or the CLAP
//! embedding) and `ModulationDef`s that bind entities to it: emissive
//! entities pulse with the beat, hero placements breathe with the energy
//! curve, agent lights follow the drums stem.
//!
//! What is exported: the environment from the mood palette, a ground plane,
//! a sun, the rule-based world every track gets (the props
//! `world_assets::plan_world_props` places, with their bob and beacons, the
//! ground-cover field pebble by pebble, and the waveform skyline), and the
//! agent-authored scene (`SceneBuild`: primitives, placed CC0 assets, scatter
//! fields, lights, the environment override), replayed with the same
//! deterministic scatter as the live executor.
//!
//! Not exported yet: section scoping (`at_role`), the materialize rise,
//! particles, and the palette's timbre wash. Agent rotations are recorded as
//! given; the executor composes them YXZ, the format XYZ, which only differs
//! for compound rotations.
//!
//! `VERSE_EXPORT_WORLD=<dir>` writes `<dir>/<track id>.world.json` (the web
//! viewer's format) and `<dir>/<track id>.world.ron` (Gen's save format)
//! once per track as it becomes current; add `VERSE_EXPORT_ONLY=1` to export
//! the whole library as `<file stem>.world.*` without opening a window. Only
//! a track its folder's `music.json` licenses (the CC0 starter pack) ships
//! its audio; personal libraries export with no audio path, and the world
//! performs silently from its curves anywhere.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use localgpt_world_types as wt;

use crate::agent_types::{AgentCommand, SceneBuild};
use crate::analysis::{AnalysisStore, TrackAnalysis};
use crate::playback::{Playback, Track};
use crate::theme::{Arrangement, WorldMood};
use crate::world_assets::{
    AssetEmbeddings, AssetManifest, PropInputs, PropPlan, Tier, WorldAssets, WorldLayout,
};

/// Ground plane size, world units.
const GROUND_SIZE: f32 = 400.0;
/// Ambient brightness (Bevy `GlobalAmbientLight` units) for exported worlds.
const AMBIENT_BRIGHTNESS: f32 = 260.0;
/// Sun illuminance in lux.
const SUN_LUX: f32 = 6000.0;

/// Audio the world may ship (the CC0 starter pack, the creator's own music).
#[derive(Debug, Clone)]
pub struct AudioRef {
    /// Path relative to the world's `assets/` directory.
    pub path: String,
    /// License of the audio, e.g. `CC0-1.0`.
    pub license: String,
}

/// Everything the export reads.
pub struct ExportInput<'a> {
    pub track: &'a Track,
    pub analysis: &'a TrackAnalysis,
    pub mood: &'a WorldMood,
    /// The asset pack, for placement scales and tiers of placed assets.
    pub assets: Option<&'a AssetManifest>,
    /// The rule-based props, as [`crate::world_assets::plan_world_props`]
    /// places them for this track.
    pub props: Option<&'a PropPlan>,
    /// Audio to reference; `None` for a personal library.
    pub audio: Option<AudioRef>,
}

fn srgba(color: Color) -> [f32; 4] {
    let c = color.to_srgba();
    [c.red, c.green, c.blue, c.alpha]
}

/// A track of a pack whose `music.json` licenses it, such as the CC0 starter
/// pack: the metadata to show and the audio its world may ship.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct PackTrack {
    title: String,
    artist: String,
    file: String,
    license: String,
}

/// The `music.json` entry for a track, when its folder has one and the entry
/// names a license. Anything else is a personal library: no audio leaves with
/// its worlds.
fn pack_track(path: &Path) -> Option<PackTrack> {
    #[derive(serde::Deserialize)]
    struct Pack {
        tracks: Vec<PackTrack>,
    }
    let text = std::fs::read_to_string(path.parent()?.join("music.json")).ok()?;
    let pack: Pack = serde_json::from_str(&text).ok()?;
    let file = path.file_name()?.to_str()?;
    pack.tracks
        .into_iter()
        .find(|t| t.file == file && !t.license.trim().is_empty())
}

/// A track's world as the app places it — mood, seed and recipe from its
/// analysis, the rule-based props, the agent's build — with the pack's audio
/// when the pack licenses it.
pub fn export_world(
    track: &Track,
    analysis: &TrackAnalysis,
    assets: Option<&AssetManifest>,
    embeddings: &AssetEmbeddings,
) -> wt::WorldManifest {
    let mood = crate::analysis::world_mood(analysis);
    let recipe = crate::analysis::world_recipe(analysis);
    let seed = track
        .path
        .as_deref()
        .map_or(WorldLayout::default().seed, |p| {
            crate::analysis::layout_seed(analysis, p)
        });
    let plan = assets.map(|manifest| {
        crate::world_assets::plan_world_props(&PropInputs {
            mood,
            seed,
            recipe: recipe.as_ref(),
            manifest,
            track_embedding: analysis.embedding.as_deref(),
            embeddings,
            section_count: analysis.sections.len(),
        })
    });
    let pack = track.path.as_deref().and_then(pack_track);
    let mut track = track.clone();
    if let Some(pack) = &pack {
        track.title = pack.title.clone();
        track.artist = pack.artist.clone();
    }
    world_manifest(&ExportInput {
        track: &track,
        analysis,
        mood: &crate::theme::moods()[mood],
        assets,
        props: plan.as_ref(),
        audio: pack.map(|p| AudioRef {
            path: format!("music/{}", p.file),
            license: p.license,
        }),
    })
}

/// Build the manifest for a track's world.
pub fn world_manifest(input: &ExportInput<'_>) -> wt::WorldManifest {
    let ExportInput {
        track,
        analysis,
        mood,
        assets,
        props,
        audio,
    } = input;
    let recipe = analysis.recipe.as_ref();
    let build = analysis.build.as_ref();

    let name = recipe
        .map(|r| r.world_name.trim())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} — {}", mood.world_name, track.title));
    let mut manifest = wt::WorldManifest::new(name);
    manifest.meta.description = Some(build.and_then(|b| b.description.clone()).unwrap_or_else(
        || {
            format!(
                "A LocalGPT Verse world for \"{}\" by {}",
                track.title, track.artist
            )
        },
    ));
    manifest.meta.tags = Some(vec!["verse".to_string(), mood.id.to_string()]);
    manifest.meta.source = Some("verse".to_string());

    manifest.environment = Some(wt::EnvironmentDef {
        background_color: Some(srgba(mood.sky_bottom)),
        ambient_intensity: Some(AMBIENT_BRIGHTNESS),
        ambient_color: Some(srgba(mood.ambient)),
        fog_density: Some(0.012),
        fog_color: Some(srgba(mood.fog)),

        extra: ::std::collections::BTreeMap::new(),
    });
    manifest.camera = Some(wt::CameraDef {
        position: [0.0, 6.0, 22.0],
        look_at: [0.0, 2.0, 0.0],
        fov_degrees: 50.0,
    });
    manifest.avatar = Some(wt::AvatarDef {
        spawn_position: [0.0, 1.7, 22.0],
        spawn_look_at: [0.0, 2.0, 0.0],
        pov: wt::PointOfView::FirstPerson,
        movement_speed: 6.0,
        height: 1.7,
        model_entity: None,
    });

    let mut next_id = 1u64;
    let mut alloc = || {
        let id = next_id;
        next_id += 1;
        id
    };

    let mut ground = wt::WorldEntity::new(alloc(), "ground");
    ground.shape = Some(wt::Shape::Plane {
        x: GROUND_SIZE,
        z: GROUND_SIZE,
    });
    ground.material = Some(wt::MaterialDef {
        color: srgba(mood.ground),
        roughness: 0.95,
        ..Default::default()
    });
    manifest.entities.push(ground);

    let mut sun = wt::WorldEntity::new(alloc(), "sun");
    sun.transform.position = [0.0, 40.0, 0.0];
    sun.light = Some(wt::LightDef {
        light_type: wt::LightType::Directional,
        color: srgba(mood.accent.with_alpha(1.0)),
        intensity: SUN_LUX,
        direction: Some([-0.35, -1.0, -0.45]),
        shadows: true,
        ..Default::default()
    });
    manifest.entities.push(sun);

    if let Some(plan) = props {
        rule_props(plan, mood, &mut manifest, &mut alloc);
    }
    skyline_entities(&analysis.energy, mood, &mut manifest, &mut alloc);

    if let Some(build) = build {
        let track_id = track.id.as_deref().unwrap_or("");
        replay_build(build, track_id, *assets, &mut manifest, &mut alloc);
    }

    manifest.soundtrack = Some(soundtrack(track, analysis, audio.clone()));
    manifest.next_entity_id = next_id;
    manifest
}

/// The rule-based world the app places for every track — mood props, recipe
/// accents and landmarks with their beacons, and the merged ground-cover field
/// pebble by pebble — as `populate_world_props` and `animate_props` show it.
fn rule_props(
    plan: &PropPlan,
    mood: &WorldMood,
    manifest: &mut wt::WorldManifest,
    alloc: &mut impl FnMut() -> u64,
) {
    let accent = mood.accent.to_linear();
    for (i, p) in plan.props.iter().enumerate() {
        let tier = match p.tier {
            Tier::Hero => "hero",
            Tier::Medium => "prop",
            Tier::Scatter => "cover",
        };
        let stem = Path::new(&p.file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut e = wt::WorldEntity::new(alloc(), format!("{tier}_{:02}_{stem}", i + 1));
        e.transform.position = p.pos.to_array();
        e.transform.rotation_degrees = [0.0, p.rot_y.to_degrees(), 0.0];
        e.transform.scale = [p.scale; 3];
        e.mesh_asset = Some(wt::MeshAssetRef::new(format!("models/{}", p.file)));
        if p.tier == Tier::Hero {
            // Heroes are monuments: they breathe with the song, never move.
            e.modulations.push(
                wt::ModulationDef::new(
                    wt::ModulationTarget::Scale,
                    wt::SignalSource::Energy,
                    [0.95, 1.08],
                )
                .with_smoothing(0.4),
            );
        } else {
            // A slow bob and spin (the app also swells the bob with the bass).
            let (bob, spin) = if p.tier == Tier::Medium {
                (0.15, 0.08)
            } else {
                (0.06, 0.2)
            };
            e.behaviors.push(wt::BehaviorDef::Bob {
                axis: [0.0, 1.0, 0.0],
                amplitude: bob,
                frequency: 1.0 / std::f32::consts::TAU,
                phase: p.motion_seed * 360.0,
            });
            e.behaviors.push(wt::BehaviorDef::Spin {
                axis: [0.0, 1.0, 0.0],
                speed: f32::to_degrees(spin),
            });
        }
        manifest.entities.push(e);

        if let Some(emissive) = p.beacon {
            // An unlit sphere above the landmark, breathing with the drums.
            let radius = (0.22 * p.scale).clamp(0.1, 0.6);
            let height = (2.6 * p.scale).clamp(2.0, 14.0);
            let glow = 1.4 * emissive;
            let mut b = wt::WorldEntity::new(alloc(), format!("beacon_{:02}_{stem}", i + 1));
            b.transform.position = (p.pos + Vec3::Y * height).to_array();
            b.shape = Some(wt::Shape::Sphere { radius });
            b.material = Some(wt::MaterialDef {
                color: srgba(mood.accent.with_alpha(1.0)),
                emissive: [
                    accent.red * glow,
                    accent.green * glow,
                    accent.blue * glow,
                    1.0,
                ],
                unlit: Some(true),
                ..Default::default()
            });
            b.modulations.push(
                wt::ModulationDef::new(
                    wt::ModulationTarget::Emissive,
                    wt::SignalSource::Stem(wt::StemKind::Drums),
                    [0.55, 1.1],
                )
                .with_smoothing(0.1),
            );
            manifest.entities.push(b);
        }
    }

    // The ground-cover field: chips for city grids, shards for rings and
    // terraces, pebbles elsewhere, tinted from the ground toward the accent.
    let shape = match plan.arrangement {
        Arrangement::Grid => wt::Shape::Cuboid {
            x: 1.0,
            y: 0.5,
            z: 1.0,
        },
        Arrangement::Rings | Arrangement::Terraces => wt::Shape::Tetrahedron {
            radius: 0.75_f32.sqrt(),
        },
        _ => wt::Shape::Icosahedron { radius: 1.0 },
    };
    let ground = mood.ground.to_linear();
    for (i, pebble) in plan.scatter.iter().enumerate() {
        let tint = LinearRgba::new(
            ground.red + (accent.red - ground.red) * pebble.mix,
            ground.green + (accent.green - ground.green) * pebble.mix,
            ground.blue + (accent.blue - ground.blue) * pebble.mix,
            1.0,
        );
        let mut e = wt::WorldEntity::new(alloc(), format!("ground_cover_{:03}", i + 1));
        e.transform.position = pebble.pos.to_array();
        e.transform.rotation_degrees = [0.0, pebble.rot_y.to_degrees(), 0.0];
        e.transform.scale = [pebble.scale; 3];
        e.shape = Some(shape.clone());
        e.material = Some(wt::MaterialDef {
            color: srgba(Color::from(tint)),
            roughness: 0.95,
            ..Default::default()
        });
        manifest.entities.push(e);
    }
}

/// The waveform skyline ([`crate::world_assets::skyline`]): stelae facing the
/// centre whose heights read the song's energy.
fn skyline_entities(
    curve: &[f32],
    mood: &WorldMood,
    manifest: &mut wt::WorldManifest,
    alloc: &mut impl FnMut() -> u64,
) {
    let accent = mood.accent.to_linear();
    for (k, stela) in crate::world_assets::skyline(curve).into_iter().enumerate() {
        let facing = Transform::from_translation(stela.pos)
            .looking_at(Vec3::new(0.0, stela.pos.y, 0.0), Vec3::Y);
        let (yaw, _, _) = facing.rotation.to_euler(EulerRot::YXZ);
        let mut e = wt::WorldEntity::new(alloc(), format!("skyline_{:02}", k + 1));
        e.transform.position = stela.pos.to_array();
        e.transform.rotation_degrees = [0.0, yaw.to_degrees(), 0.0];
        e.shape = Some(wt::Shape::Cuboid {
            x: 0.8,
            y: stela.height,
            z: 1.4,
        });
        e.material = Some(wt::MaterialDef {
            color: srgba(mood.ground),
            emissive: [
                accent.red * 0.22,
                accent.green * 0.22,
                accent.blue * 0.22,
                1.0,
            ],
            unlit: Some(true),
            ..Default::default()
        });
        manifest.entities.push(e);
    }
}

/// The agent's scene build as manifest entities, with the modulations that
/// make them perform.
fn replay_build(
    build: &SceneBuild,
    track_id: &str,
    assets: Option<&AssetManifest>,
    manifest: &mut wt::WorldManifest,
    alloc: &mut impl FnMut() -> u64,
) {
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut env_override: Option<wt::EnvironmentDef> = None;

    fn push(
        by_name: &mut HashMap<String, usize>,
        manifest: &mut wt::WorldManifest,
        entity: wt::WorldEntity,
    ) {
        by_name.insert(entity.name.0.clone(), manifest.entities.len());
        manifest.entities.push(entity);
    }

    for cmd in &build.commands {
        match cmd {
            AgentCommand::BeginSession { .. } | AgentCommand::SceneInfo => {}
            AgentCommand::SpawnPrimitive(c) => {
                if by_name.contains_key(&c.name) {
                    continue;
                }
                let mut e = wt::WorldEntity::new(alloc(), c.name.clone());
                e.transform.position = c.position;
                e.transform.rotation_degrees = c.rotation_degrees;
                e.transform.scale = c.scale;
                e.shape = Some(primitive_shape(c.shape, &c.dimensions));
                e.material = Some(wt::MaterialDef {
                    color: c.color,
                    metallic: c.metallic,
                    roughness: c.roughness,
                    emissive: c.emissive,
                    ..Default::default()
                });
                add_emissive_pulse(&mut e);
                push(&mut by_name, manifest, e);
            }
            AgentCommand::PlaceAsset(c) => {
                if by_name.contains_key(&c.name) || c.asset.is_empty() {
                    continue;
                }
                let entry = assets.and_then(|m| m.assets.iter().find(|a| a.file == c.asset));
                let mut e = wt::WorldEntity::new(alloc(), c.name.clone());
                e.transform.position = c.position;
                e.transform.rotation_degrees = c.rotation_degrees;
                let scale = entry.map_or(1.0, |a| a.placement_scale()) * c.scale.max(0.05);
                e.transform.scale = [scale; 3];
                e.mesh_asset = Some(wt::MeshAssetRef::new(format!("models/{}", c.asset)));
                if entry.is_some_and(|a| a.tier == Tier::Hero) {
                    e.modulations.push(
                        wt::ModulationDef::new(
                            wt::ModulationTarget::Scale,
                            wt::SignalSource::Energy,
                            [0.95, 1.08],
                        )
                        .with_smoothing(0.4),
                    );
                }
                push(&mut by_name, manifest, e);
            }
            AgentCommand::ScatterField(c) => {
                if by_name.contains_key(&c.name) || c.assets.is_empty() {
                    continue;
                }
                let entries: Vec<_> = c
                    .assets
                    .iter()
                    .map(|file| {
                        (
                            file.clone(),
                            assets
                                .and_then(|m| m.assets.iter().find(|a| &a.file == file))
                                .map_or(1.0, |a| a.placement_scale()),
                        )
                    })
                    .collect();
                // The executor's deterministic field (ARCHITECTURE R6).
                let seed = crate::world_assets::fold_seed(&format!("{}|{}", track_id, c.name));
                let offsets =
                    crate::world_assets::scatter_offsets(seed, c.count as usize, c.radius);
                let mut rng = seed ^ 0xA5A5_5EED_u64;
                let base = Vec3::from(c.position);
                for (i, off) in offsets.into_iter().enumerate() {
                    let (file, placement) = &entries[i % entries.len()];
                    let yaw = crate::world_assets::rand01(&mut rng) * std::f32::consts::TAU;
                    let scale = placement
                        * c.scale.max(0.05)
                        * (0.7 + crate::world_assets::rand01(&mut rng) * 0.7);
                    let mut e = wt::WorldEntity::new(alloc(), format!("{}_{}", c.name, i + 1));
                    e.transform.position = (base + off).to_array();
                    e.transform.rotation_degrees = [0.0, yaw.to_degrees(), 0.0];
                    e.transform.scale = [scale; 3];
                    e.mesh_asset = Some(wt::MeshAssetRef::new(format!("models/{file}")));
                    push(&mut by_name, manifest, e);
                }
                by_name.insert(c.name.clone(), usize::MAX);
            }
            AgentCommand::ModifyEntity(c) => {
                let Some(&idx) = by_name.get(&c.name) else {
                    continue;
                };
                let Some(e) = manifest.entities.get_mut(idx) else {
                    continue;
                };
                if let Some(p) = c.position {
                    e.transform.position = p;
                }
                if let Some(r) = c.rotation_degrees {
                    e.transform.rotation_degrees = r;
                }
                if let Some(s) = c.scale {
                    e.transform.scale = s;
                }
                if c.color.is_some()
                    || c.metallic.is_some()
                    || c.roughness.is_some()
                    || c.emissive.is_some()
                {
                    let mut m = e.material.take().unwrap_or_default();
                    if let Some(color) = c.color {
                        m.color = color;
                    }
                    if let Some(metallic) = c.metallic {
                        m.metallic = metallic;
                    }
                    if let Some(roughness) = c.roughness {
                        m.roughness = roughness;
                    }
                    if let Some(emissive) = c.emissive {
                        m.emissive = emissive;
                    }
                    e.material = Some(m);
                    e.modulations
                        .retain(|m| m.target != wt::ModulationTarget::Emissive);
                    add_emissive_pulse(e);
                }
            }
            AgentCommand::DeleteEntity { name } => {
                if let Some(idx) = by_name.remove(name)
                    && idx != usize::MAX
                {
                    manifest.entities.remove(idx);
                    for v in by_name.values_mut() {
                        if *v != usize::MAX && *v > idx {
                            *v -= 1;
                        }
                    }
                }
            }
            AgentCommand::SetLight(c) => {
                if by_name.contains_key(&c.name) {
                    continue;
                }
                let mut e = wt::WorldEntity::new(alloc(), c.name.clone());
                e.transform.position = c.position.unwrap_or([0.0, 6.0, 0.0]);
                let directional = c.direction.is_some();
                e.light = Some(wt::LightDef {
                    light_type: if directional {
                        wt::LightType::Directional
                    } else {
                        wt::LightType::Point
                    },
                    color: c.color,
                    intensity: c.intensity,
                    direction: c.direction,
                    shadows: directional,
                    range: (!directional).then_some(40.0),
                    ..Default::default()
                });
                e.modulations.push(
                    wt::ModulationDef::new(
                        wt::ModulationTarget::LightIntensity,
                        wt::SignalSource::Stem(wt::StemKind::Drums),
                        [0.6, 1.4],
                    )
                    .with_smoothing(0.1),
                );
                push(&mut by_name, manifest, e);
            }
            AgentCommand::SetEnvironment(c) => {
                env_override = Some(wt::EnvironmentDef {
                    background_color: Some(c.background_color),
                    ambient_intensity: Some(AMBIENT_BRIGHTNESS),
                    ambient_color: Some(c.ambient_light),
                    fog_density: manifest.environment.as_ref().and_then(|e| e.fog_density),
                    fog_color: Some(c.background_color),

                    extra: ::std::collections::BTreeMap::new(),
                });
            }
        }
    }
    if let Some(env) = env_override {
        manifest.environment = Some(env);
    }
}

/// Emissive entities pulse with the beat, the way `pulse_beacons` drives
/// the live world.
fn add_emissive_pulse(entity: &mut wt::WorldEntity) {
    let glows = entity
        .material
        .as_ref()
        .is_some_and(|m| m.emissive[0] > 0.0 || m.emissive[1] > 0.0 || m.emissive[2] > 0.0);
    if glows {
        entity.modulations.push(
            wt::ModulationDef::new(
                wt::ModulationTarget::Emissive,
                wt::SignalSource::Beat,
                [0.6, 1.6],
            )
            .with_smoothing(0.05),
        );
    }
}

/// The agent's primitive vocabulary as a format shape, with the executor's
/// default dimensions.
fn primitive_shape(
    shape: crate::agent_types::PrimitiveShape,
    dims: &HashMap<String, f32>,
) -> wt::Shape {
    use crate::agent_types::PrimitiveShape as P;
    let d = |key: &str, default: f32| dims.get(key).copied().unwrap_or(default);
    match shape {
        P::Cuboid => wt::Shape::Cuboid {
            x: d("x", 1.0),
            y: d("y", 1.0),
            z: d("z", 1.0),
        },
        P::Sphere => wt::Shape::Sphere {
            radius: d("radius", 0.5),
        },
        P::Cylinder => wt::Shape::Cylinder {
            radius: d("radius", 0.5),
            height: d("height", 1.0),
        },
        P::Cone => wt::Shape::Cone {
            radius: d("radius", 0.5),
            height: d("height", 1.0),
        },
        P::Torus => wt::Shape::Torus {
            major_radius: d("major_radius", 1.0),
            minor_radius: d("minor_radius", 0.25),
        },
        P::Plane => wt::Shape::Plane {
            x: d("x", 10.0),
            z: d("z", 10.0),
        },
    }
}

/// The track's analysis as a soundtrack definition. Never the embedding.
fn soundtrack(
    track: &Track,
    analysis: &TrackAnalysis,
    audio: Option<AudioRef>,
) -> wt::SoundtrackDef {
    let unit = |v: &[f32]| v.iter().map(|x| x.clamp(0.0, 1.0)).collect::<Vec<_>>();
    // `StemEnergy` is `[drums, bass, vocals, other]` (demucs::STEM_NAMES).
    let stems = analysis.stems.as_ref().map(|s| wt::StemCurves {
        drums: unit(&s[0]),
        bass: unit(&s[1]),
        vocals: unit(&s[2]),
        other: unit(&s[3]),
    });
    wt::SoundtrackDef {
        path: audio.as_ref().map(|a| a.path.clone()),
        license: audio.map(|a| a.license),
        title: Some(track.title.clone()),
        artist: Some(track.artist.clone()),
        duration: analysis.duration.max(track.duration).max(0.0),
        bpm: analysis.bpm.max(0.0),
        beat_offset: analysis.beat_offset.max(0.0),
        sections: unit(&analysis.sections),
        energy: unit(&analysis.energy),
        stems,
    }
}

// ---------------------------------------------------------------------------
// `VERSE_EXPORT_WORLD=<dir>`: write the current track's world once
// ---------------------------------------------------------------------------

/// Where exports go (`VERSE_EXPORT_WORLD`).
#[derive(Resource)]
pub struct ExportDir(pub PathBuf);

/// Writes each track's world the first time it is current with an analysis.
pub struct WorldExportPlugin;

impl Plugin for WorldExportPlugin {
    fn build(&self, app: &mut App) {
        if let Some(dir) = std::env::var_os("VERSE_EXPORT_WORLD") {
            app.insert_resource(ExportDir(PathBuf::from(dir)))
                .add_systems(Update, export_current_track);
        }
    }
}

fn export_current_track(
    dir: Res<ExportDir>,
    playback: Res<Playback>,
    analysis: Option<Res<AnalysisStore>>,
    assets: Option<Res<WorldAssets>>,
    embeddings: Option<Res<AssetEmbeddings>>,
    mut done: Local<HashSet<String>>,
) {
    let Some(analysis) = analysis else {
        return;
    };
    let Some(track) = playback.queue.get(playback.current) else {
        return;
    };
    let Some(id) = track.id.as_deref() else {
        return;
    };
    if done.contains(id) {
        return;
    }
    let Some(track_analysis) = analysis.get(id) else {
        return;
    };
    let neutral = AssetEmbeddings::default();
    let manifest = export_world(
        track,
        track_analysis,
        assets.as_deref().and_then(|a| a.manifest.as_ref()),
        embeddings.as_deref().unwrap_or(&neutral),
    );
    done.insert(id.to_string());
    if let Err(e) = write_manifest(&dir.0, id, &manifest) {
        warn!("World export failed for {}: {e}", track.title);
    } else {
        info!(
            "Exported world for {} — {} entities → {}",
            track.title,
            manifest.entities.len(),
            dir.0.join(format!("{id}.world.json")).display()
        );
    }
}

/// Write `<dir>/<id>.world.json` and `<dir>/<id>.world.ron`.
pub fn write_manifest(
    dir: &std::path::Path,
    id: &str,
    manifest: &wt::WorldManifest,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(manifest).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(format!("{id}.world.json")), json)?;
    let ron = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .map_err(std::io::Error::other)?;
    std::fs::write(dir.join(format!("{id}.world.ron")), ron)
}

// ---------------------------------------------------------------------------
// `VERSE_EXPORT_ONLY=1`: export a whole library without opening a window
// ---------------------------------------------------------------------------

/// Export every track under `music_dir` (walked like an import) to
/// `out_dir` as `<file stem>.world.json` and `.world.ron`. Analyses come
/// from the sidecar cache, else run now and are cached, as the app would.
/// Returns the JSON files written.
pub fn export_library(music_dir: &Path, out_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let assets = crate::world_assets::read_manifest_from_disk();
    let embeddings = AssetEmbeddings::default();
    let mut written = Vec::new();
    for entry in walkdir::WalkDir::new(music_dir)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if !entry.file_type().is_file() || !crate::audio::is_audio(path) {
            continue;
        }
        let Some(track) = crate::audio::read_track(path) else {
            continue;
        };
        let Some(id) = track.id.clone() else {
            continue;
        };
        let Some(analysis) = crate::analysis::analysis_for(&id, path) else {
            eprintln!("Analysis failed for {}", path.display());
            continue;
        };
        let manifest = export_world(&track, &analysis, assets.as_ref(), &embeddings);
        let stem = path
            .file_stem()
            .map_or(id.clone(), |s| s.to_string_lossy().into_owned());
        write_manifest(out_dir, &stem, &manifest)?;
        written.push(out_dir.join(format!("{stem}.world.json")));
    }
    Ok(written)
}

/// `VERSE_EXPORT_ONLY=1 VERSE_EXPORT_WORLD=<dir>`: run [`export_library`] over
/// `VERSE_IMPORT` (default: the starter pack) and return the exit code.
pub fn export_only() -> i32 {
    let Some(out_dir) = std::env::var_os("VERSE_EXPORT_WORLD").map(PathBuf::from) else {
        eprintln!("VERSE_EXPORT_ONLY needs VERSE_EXPORT_WORLD=<dir> for the worlds");
        return 2;
    };
    let music_dir = std::env::var_os("VERSE_IMPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::world_assets::asset_root().join("music"));
    match export_library(&music_dir, &out_dir) {
        Ok(files) => {
            for file in &files {
                println!("{}", file.display());
            }
            eprintln!(
                "Exported {} worlds from {}",
                files.len(),
                music_dir.display()
            );
            0
        }
        Err(e) => {
            eprintln!("World export failed: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_types::{PlaceAssetCmd, ScatterFieldCmd, SetLightCmd, SpawnPrimitiveCmd};
    use crate::world_assets::AssetEntry;

    fn track() -> Track {
        Track {
            title: "Amber Drift".into(),
            artist: "LocalGPT".into(),
            album: None,
            duration: 30.0,
            mood: 0,
            section: String::new(),
            path: None,
            id: Some("deadbeef".into()),
        }
    }

    fn analysis(build: Option<SceneBuild>) -> TrackAnalysis {
        TrackAnalysis {
            version: 2,
            duration: 30.0,
            bpm: 120.0,
            beat_offset: 0.2,
            sections: vec![0.0, 0.5],
            energy: (0..31).map(|i| (i % 10) as f32 / 10.0).collect(),
            centroid_hz: None,
            mood: 0,
            mood_id: None,
            loudness_lufs: None,
            pinned_mood: None,
            pinned_mood_id: None,
            pinned_seed: None,
            embedding: Some(vec![0.5; 512]),
            stems: None,
            recipe: None,
            build,
        }
    }

    fn pack() -> AssetManifest {
        AssetManifest {
            version: 2,
            assets: vec![AssetEntry {
                id: "rock_a".into(),
                name: "Rock A".into(),
                file: "rock_a.glb".into(),
                kind: "rock".into(),
                tier: Tier::Hero,
                mood: 0,
                mood_id: None,
                scale: 1.0,
                dims: Some([2.0, 1.0, 2.0]),
                license: "CC0".into(),
                author: "Poly Haven".into(),
                source: String::new(),
            }],
        }
    }

    fn build() -> SceneBuild {
        SceneBuild {
            commands: vec![
                AgentCommand::SpawnPrimitive(SpawnPrimitiveCmd {
                    name: "beacon".into(),
                    shape: crate::agent_types::PrimitiveShape::Sphere,
                    dimensions: HashMap::from([("radius".to_string(), 0.8)]),
                    position: [0.0, 3.0, 0.0],
                    rotation_degrees: [0.0; 3],
                    scale: [1.0; 3],
                    color: [0.1, 0.1, 0.1, 1.0],
                    metallic: 0.0,
                    roughness: 0.5,
                    emissive: [2.0, 1.0, 0.2, 1.0],
                    at_role: None,
                }),
                AgentCommand::PlaceAsset(PlaceAssetCmd {
                    name: "monolith".into(),
                    kind: "rock".into(),
                    asset: "rock_a.glb".into(),
                    position: [4.0, 0.0, -3.0],
                    rotation_degrees: [0.0, 30.0, 0.0],
                    scale: 1.2,
                    at_role: None,
                }),
                AgentCommand::ScatterField(ScatterFieldCmd {
                    name: "pebbles".into(),
                    kind: "rock".into(),
                    assets: vec!["rock_a.glb".into()],
                    count: 5,
                    radius: 6.0,
                    position: [0.0, 0.0, 4.0],
                    scale: 0.5,
                    at_role: None,
                }),
                AgentCommand::SetLight(SetLightCmd {
                    name: "lamp".into(),
                    color: [1.0, 0.9, 0.8, 1.0],
                    intensity: 5000.0,
                    position: Some([0.0, 5.0, 0.0]),
                    direction: None,
                }),
                AgentCommand::DeleteEntity {
                    name: "pebbles_3".into(),
                },
            ],
            description: Some("a lonely beacon".into()),
        }
    }

    #[test]
    fn manifest_validates_and_performs() {
        let t = track();
        let a = analysis(Some(build()));
        let pack = pack();
        let m = world_manifest(&ExportInput {
            track: &t,
            analysis: &a,
            mood: &crate::theme::moods()[0],
            assets: Some(&pack),
            props: None,
            audio: None,
        });
        let issues = wt::validate_manifest(&m, &wt::WorldLimits::default());
        assert!(
            issues.iter().all(|i| i.severity != wt::Severity::Error),
            "{issues:?}"
        );
        let names: Vec<&str> = m.entities.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"ground") && names.contains(&"sun"));
        assert!(
            names.contains(&"beacon") && names.contains(&"monolith") && names.contains(&"lamp")
        );
        assert!(names.contains(&"pebbles_1") && !names.contains(&"pebbles_3"));
        assert_eq!(
            names.iter().filter(|n| n.starts_with("pebbles_")).count(),
            4
        );
        let unique: HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "names are unique");
        assert!(m.next_entity_id > m.entities.iter().map(|e| e.id.0).max().unwrap());

        let beacon = m
            .entities
            .iter()
            .find(|e| e.name.as_str() == "beacon")
            .unwrap();
        assert!(matches!(
            beacon.modulations[0].signal,
            wt::SignalSource::Beat
        ));
        let monolith = m
            .entities
            .iter()
            .find(|e| e.name.as_str() == "monolith")
            .unwrap();
        assert_eq!(
            monolith.mesh_asset.as_ref().unwrap().path,
            "models/rock_a.glb"
        );
        assert!(
            (monolith.transform.scale[0] - 7.0 / 2.0 * 1.2).abs() < 1e-4,
            "hero span 7 / dims 2"
        );
        assert!(matches!(
            monolith.modulations[0].target,
            wt::ModulationTarget::Scale
        ));
        let lamp = m
            .entities
            .iter()
            .find(|e| e.name.as_str() == "lamp")
            .unwrap();
        assert_eq!(
            lamp.light.as_ref().unwrap().light_type,
            wt::LightType::Point
        );

        let st = m.soundtrack.as_ref().unwrap();
        assert!(
            st.path.is_none() && st.license.is_none(),
            "personal library ships no audio"
        );
        assert_eq!(st.title.as_deref(), Some("Amber Drift"));
        assert_eq!(st.bpm, 120.0);
        assert_eq!(st.energy.len(), 31);
        assert_eq!(m.meta.description.as_deref(), Some("a lonely beacon"));
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("embedding"),
            "the CLAP embedding never leaves the sidecar"
        );
    }

    #[test]
    fn scatter_replays_like_the_executor() {
        let t = track();
        let a = analysis(Some(build()));
        let pack = pack();
        let input = ExportInput {
            track: &t,
            analysis: &a,
            mood: &crate::theme::moods()[0],
            assets: Some(&pack),
            props: None,
            audio: None,
        };
        let first = world_manifest(&input);
        let second = world_manifest(&input);
        assert_eq!(first, second, "deterministic");
        let seed = crate::world_assets::fold_seed("deadbeef|pebbles");
        let offsets = crate::world_assets::scatter_offsets(seed, 5, 6.0);
        let p1 = first
            .entities
            .iter()
            .find(|e| e.name.as_str() == "pebbles_1")
            .unwrap();
        let expected = (Vec3::new(0.0, 0.0, 4.0) + offsets[0]).to_array();
        assert_eq!(p1.transform.position, expected);
    }

    #[test]
    fn rule_world_without_build_still_exports() {
        let t = track();
        let a = analysis(None);
        let m = world_manifest(&ExportInput {
            track: &t,
            analysis: &a,
            mood: &crate::theme::moods()[1],
            assets: None,
            props: None,
            audio: Some(AudioRef {
                path: "music/amber-drift.mp3".into(),
                license: "CC0-1.0".into(),
            }),
        });
        assert_eq!(m.entities.len(), 2 + 28, "ground, sun and the skyline");
        assert!(m.meta.name.contains("Amber Drift"));
        let st = m.soundtrack.as_ref().unwrap();
        assert_eq!(st.path.as_deref(), Some("music/amber-drift.mp3"));
        assert_eq!(st.license.as_deref(), Some("CC0-1.0"));
        assert!(
            wt::validate_manifest(&m, &wt::WorldLimits::default())
                .iter()
                .all(|i| i.severity != wt::Severity::Error)
        );
    }

    /// A pack with every tier for mood 0.
    fn full_pack() -> AssetManifest {
        let entry = |file: &str, kind: &str, tier: Tier| AssetEntry {
            id: file.trim_end_matches(".glb").into(),
            name: file.into(),
            file: file.into(),
            kind: kind.into(),
            tier,
            mood: 0,
            mood_id: None,
            scale: 1.0,
            dims: Some([1.0, 1.0, 1.0]),
            license: "CC0".into(),
            author: "Poly Haven".into(),
            source: String::new(),
        };
        AssetManifest {
            version: 2,
            assets: vec![
                entry("rock_a.glb", "rock", Tier::Hero),
                entry("tree_a.glb", "tree", Tier::Hero),
                entry("lamp_a.glb", "lamp", Tier::Medium),
                entry("crate_a.glb", "crate", Tier::Medium),
                entry("pebble_a.glb", "pebble", Tier::Scatter),
            ],
        }
    }

    fn rule_world(seed: u64) -> (PropPlan, wt::WorldManifest) {
        let (t, a, pack) = (track(), analysis(None), full_pack());
        let neutral = AssetEmbeddings::default();
        let plan = crate::world_assets::plan_world_props(&PropInputs {
            mood: 0,
            seed,
            recipe: None,
            manifest: &pack,
            track_embedding: None,
            embeddings: &neutral,
            section_count: a.sections.len(),
        });
        let m = world_manifest(&ExportInput {
            track: &t,
            analysis: &a,
            mood: &crate::theme::moods()[0],
            assets: Some(&pack),
            props: Some(&plan),
            audio: None,
        });
        (plan, m)
    }

    #[test]
    fn exports_the_rule_based_world() {
        let (plan, m) = rule_world(7);
        let issues = wt::validate_manifest(&m, &wt::WorldLimits::default());
        assert!(
            issues.iter().all(|i| i.severity != wt::Severity::Error),
            "{issues:?}"
        );
        let names: Vec<&str> = m.entities.iter().map(|e| e.name.as_str()).collect();
        let unique: HashSet<&&str> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "names are unique");
        let named = |prefix: &str| {
            m.entities
                .iter()
                .filter(|e| e.name.as_str().starts_with(prefix))
                .collect::<Vec<_>>()
        };

        let heroes = named("hero_");
        let props = named("prop_");
        assert_eq!(heroes.len(), 15, "the hero budget");
        assert_eq!(props.len(), 25, "the prop budget");
        assert_eq!(heroes.len() + props.len(), plan.props.len());
        for hero in &heroes {
            assert!(
                hero.mesh_asset
                    .as_ref()
                    .unwrap()
                    .path
                    .starts_with("models/")
            );
            assert!(hero.behaviors.is_empty(), "heroes stay still");
            assert_eq!(hero.modulations[0].target, wt::ModulationTarget::Scale);
        }
        for prop in &props {
            assert!(matches!(prop.behaviors[0], wt::BehaviorDef::Bob { .. }));
            assert!(matches!(prop.behaviors[1], wt::BehaviorDef::Spin { .. }));
        }
        assert_eq!(plan.scatter.len(), 27 * 4, "scatter budget × field boost");
        assert_eq!(named("ground_cover_").len(), plan.scatter.len());
        assert_eq!(named("skyline_").len(), 28);
    }

    #[test]
    fn the_same_track_places_the_same_world() {
        assert_eq!(rule_world(7).1, rule_world(7).1);
        assert_ne!(rule_world(7).1, rule_world(8).1, "the seed moves things");
    }

    #[test]
    fn only_a_licensed_pack_track_ships_its_audio() {
        let dir = std::env::temp_dir().join(format!("verse-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("music.json"),
            r#"{"version": 1, "tracks": [
                {"title": "Amber Drift", "artist": "Reverie", "file": "a.mp3", "license": "CC0-1.0"},
                {"title": "Unlicensed", "artist": "Someone", "file": "b.mp3", "license": " "}
            ]}"#,
        )
        .unwrap();
        assert_eq!(pack_track(&dir.join("a.mp3")).unwrap().license, "CC0-1.0");
        assert!(pack_track(&dir.join("b.mp3")).is_none(), "no license");
        assert!(pack_track(&dir.join("c.mp3")).is_none(), "not in the pack");

        let neutral = AssetEmbeddings::default();
        let mut t = track();
        t.path = Some(dir.join("a.mp3"));
        let m = export_world(&t, &analysis(None), None, &neutral);
        let st = m.soundtrack.unwrap();
        assert_eq!(st.path.as_deref(), Some("music/a.mp3"));
        assert_eq!(st.license.as_deref(), Some("CC0-1.0"));

        t.path = Some(dir.join("b.mp3"));
        let m = export_world(&t, &analysis(None), None, &neutral);
        assert!(
            m.soundtrack.unwrap().path.is_none(),
            "a personal track ships no audio"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_manifest_writes_both_formats() {
        let dir = std::env::temp_dir().join(format!("verse-export-{}", std::process::id()));
        let m = wt::WorldManifest::new("tmp");
        write_manifest(&dir, "abc", &m).unwrap();
        assert!(dir.join("abc.world.json").is_file());
        assert!(dir.join("abc.world.ron").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
