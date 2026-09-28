//! AudioEngine resource, AudioEmitter component, and Bevy systems for
//! procedural environmental audio.
//!
//! Since the move to `localgpt-world-audio`, this module is the Bevy-facing
//! half of a two-part engine:
//!
//! - **This side** (Bevy main thread): the `AudioEngine` resource, the
//!   `AudioEmitter`/`SpatialAudioListener` components, the command handlers
//!   and the per-frame spatial system. It owns metadata — names, base
//!   volumes, radii, `last_ambience` for world save round-trips.
//! - **`localgpt_world_audio::Engine`** (dedicated thread): the kira mixer
//!   playing FunDSP graphs. It owns the device.
//!
//! The two halves talk over an mpsc channel of [`AudioUpdate`]s, drained
//! blocking on the audio thread. That replaces the previous hand-rolled
//! transport — a cpal stream plus a FunDSP `Net` rebuilt in place plus
//! `Shared<f32>` atomics threaded between threads — and with it the two
//! things kira now does instead: distance attenuation (an emitter is a
//! spatial sub-track, so moving the listener or the emitter is the whole of
//! it) and panning (tracks pan). The quadratic falloff Gen computed per
//! frame is now kira's min/max-distance curve; the audible difference is
//! small and the per-frame cost is a channel send instead of math per
//! emitter.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use bevy::prelude::*;
use localgpt_world_audio::wt;
use localgpt_world_audio::wt::AudioSource;

use super::commands::{
    AmbienceCmd, AudioEmitterCmd, AudioEmitterSummary, AudioInfoResponse, GenResponse,
    ModifyAudioEmitterCmd,
};
use super::registry::{GenEntity, NameRegistry};

// ---------------------------------------------------------------------------
// Bevy components
// ---------------------------------------------------------------------------

/// Marker component for entities that emit spatial audio.
#[derive(Component)]
pub struct AudioEmitter {
    #[allow(dead_code)]
    pub sound: AudioSource,
    pub radius: f32,
    pub volume: f32,
    pub emitter_name: String,
}

/// Marker for the spatial audio listener (attached to the camera).
#[derive(Component)]
pub struct SpatialAudioListener;

// ---------------------------------------------------------------------------
// AudioEngine resource (Bevy-side, Send+Sync)
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct AudioEngine {
    pub active: bool,
    master_volume: f32,
    /// Names of live emitters, mirroring the audio thread's map.
    emitter_names: HashSet<String>,
    update_tx: mpsc::Sender<AudioUpdate>,
    pub ambience_layer_names: Vec<String>,
    pub emitter_meta: HashMap<String, EmitterMeta>,
    /// Last ambience command for world save round-trip.
    pub last_ambience: Option<AmbienceCmd>,
    /// The song's position in seconds as the audio thread last saw it, as
    /// `f32` bits; NaN while no song is audibly playing.
    soundtrack_position: Arc<AtomicU32>,
}

pub struct EmitterMeta {
    pub sound_type: String,
    pub sound: AudioSource,
    pub base_volume: f32,
    pub radius: f32,
    pub attached_to: Option<String>,
    pub position: Option<[f32; 3]>,
}

impl AudioEngine {
    /// Play a one-shot sound emitter at a world position.
    ///
    /// Used by trigger systems (proximity, click) to fire PlaySoundAction.
    /// Creates a temporary emitter named "trigger_{sound}_{counter}" with a
    /// default volume (fire-and-forget).
    pub fn play_emitter_at(&mut self, sound_name: &str, position: Vec3) {
        let emitter_name = format!("trigger_{}_{}", sound_name, self.emitter_names.len());
        let volume = 0.6;

        // Infer the sound from the name, same vocabulary as auto-inference.
        let (sound, radius) =
            localgpt_world_audio::infer_emitter_from_name(sound_name).unwrap_or((
                AudioSource::Custom {
                    waveform: wt::WaveformType::WhiteNoise,
                    filter_cutoff: 2000.0,
                    filter_type: wt::FilterType::Lowpass,
                },
                10.0,
            ));
        let position = [position.x, position.y, position.z];

        let _ = self.update_tx.send(AudioUpdate::AddEmitter {
            name: emitter_name.clone(),
            sound: sound.clone(),
            volume,
            radius,
            position,
        });

        self.emitter_names.insert(emitter_name.clone());
        self.emitter_meta.insert(
            emitter_name,
            EmitterMeta {
                sound_type: sound_name.to_string(),
                sound,
                base_volume: volume,
                radius,
                attached_to: None,
                position: Some(position),
            },
        );
    }

    /// Stop all audio: remove all emitters, clear ambience state, and stop
    /// the world's song.
    pub fn stop_all(&mut self) {
        for name in self.emitter_names.drain() {
            let _ = self.update_tx.send(AudioUpdate::RemoveEmitter { name });
        }
        let _ = self.update_tx.send(AudioUpdate::StopSoundtrack);
        self.emitter_meta.clear();
        self.ambience_layer_names.clear();
        self.last_ambience = None;
    }

    /// Master volume as stored on the Bevy side (the thread applies it).
    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    /// Stream a world's song from `path`, looping, in place of any other.
    pub fn play_soundtrack(&mut self, path: PathBuf) {
        let _ = self.update_tx.send(AudioUpdate::PlaySoundtrack { path });
    }

    pub fn stop_soundtrack(&mut self) {
        let _ = self.update_tx.send(AudioUpdate::StopSoundtrack);
    }

    pub fn pause_soundtrack(&mut self, paused: bool) {
        let _ = self.update_tx.send(AudioUpdate::PauseSoundtrack(paused));
    }

    /// Where the song is, in seconds, while it is audibly playing.
    pub fn soundtrack_position(&self) -> Option<f32> {
        let seconds = f32::from_bits(self.soundtrack_position.load(Ordering::Relaxed));
        (!seconds.is_nan()).then_some(seconds)
    }
}

// ---------------------------------------------------------------------------
// Messages sent from Bevy to the audio thread
// ---------------------------------------------------------------------------

enum AudioUpdate {
    SetAmbience {
        layers: Vec<(String, AudioSource, f32)>,
        master_volume: Option<f32>,
    },
    AddEmitter {
        name: String,
        sound: AudioSource,
        volume: f32,
        radius: f32,
        position: [f32; 3],
    },
    SetEmitterVolume {
        name: String,
        volume: f32,
    },
    SetEmitterRadius {
        name: String,
        radius: f32,
    },
    RemoveEmitter {
        name: String,
    },
    SetListener {
        position: [f32; 3],
        orientation: [f32; 4],
    },
    SetEmitterPosition {
        name: String,
        position: [f32; 3],
    },
    PlaySoundtrack {
        path: PathBuf,
    },
    StopSoundtrack,
    PauseSoundtrack(bool),
}

// ---------------------------------------------------------------------------
// Audio thread
// ---------------------------------------------------------------------------

/// What the thread remembers per emitter, so radius and sound changes can
/// rebuild a voice without a round-trip to the Bevy side.
struct ThreadEmitter {
    sound: AudioSource,
    volume: f32,
    radius: f32,
    position: [f32; 3],
}

pub fn start_audio_engine() -> Option<AudioEngine> {
    let (update_tx, update_rx) = mpsc::channel::<AudioUpdate>();
    let soundtrack_position = Arc::new(AtomicU32::new(f32::NAN.to_bits()));
    let position = Arc::clone(&soundtrack_position);

    let thread_result = std::thread::Builder::new()
        .name("gen-audio".into())
        .spawn(move || {
            audio_thread_main(update_rx, &position);
        });

    match thread_result {
        Ok(_) => {
            tracing::info!("Audio engine started");
            Some(AudioEngine {
                active: true,
                master_volume: 0.8,
                emitter_names: HashSet::new(),
                update_tx,
                ambience_layer_names: Vec::new(),
                emitter_meta: HashMap::new(),
                last_ambience: None,
                soundtrack_position,
            })
        }
        Err(e) => {
            tracing::error!("Failed to start audio thread: {}", e);
            None
        }
    }
}

/// How often the thread wakes, when nothing is sent, to report where the
/// song is — about two frames.
const SOUNDTRACK_POLL: Duration = Duration::from_millis(30);

fn audio_thread_main(rx: mpsc::Receiver<AudioUpdate>, soundtrack_position: &AtomicU32) {
    // No output device is not an error — a headless runner or a machine with
    // no sound card gets a silent world, and the channel quietly drains.
    let Some(mut engine) = localgpt_world_audio::Engine::new() else {
        return;
    };
    engine.set_master_volume(0.8);

    let mut emitters: HashMap<String, ThreadEmitter> = HashMap::new();

    loop {
        let update = match rx.recv_timeout(SOUNDTRACK_POLL) {
            Ok(update) => Some(update),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(update) = update {
            apply_update(&mut engine, &mut emitters, update);
        }
        let position = engine.soundtrack_position().unwrap_or(f32::NAN);
        soundtrack_position.store(position.to_bits(), Ordering::Relaxed);
    }
    // The loop ends when every sender drops — the resource going away at
    // shutdown — which drops the engine and stops the stream.
}

fn apply_update(
    engine: &mut localgpt_world_audio::Engine,
    emitters: &mut HashMap<String, ThreadEmitter>,
    update: AudioUpdate,
) {
    {
        match update {
            AudioUpdate::SetAmbience {
                layers,
                master_volume,
            } => {
                if let Some(volume) = master_volume {
                    engine.set_master_volume(volume);
                }
                engine.set_ambience(&layers);
            }
            AudioUpdate::AddEmitter {
                name,
                sound,
                volume,
                radius,
                position,
            } => {
                engine.remove_emitter(&name);
                engine.add_emitter(&name, &sound, volume, position, radius);
                emitters.insert(
                    name,
                    ThreadEmitter {
                        sound,
                        volume,
                        radius,
                        position,
                    },
                );
            }
            AudioUpdate::SetEmitterVolume { name, volume } => {
                if let Some(thread_emitter) = emitters.get_mut(&name) {
                    thread_emitter.volume = volume;
                }
                engine.set_emitter_volume(&name, volume);
            }
            AudioUpdate::SetEmitterRadius { name, radius } => {
                // Distances are baked into the spatial track at build time,
                // so a radius change rebuilds the voice in place.
                if let Some(thread_emitter) = emitters.get_mut(&name) {
                    thread_emitter.radius = radius;
                    let sound = thread_emitter.sound.clone();
                    let volume = thread_emitter.volume;
                    let position = thread_emitter.position;
                    engine.remove_emitter(&name);
                    engine.add_emitter(&name, &sound, volume, position, radius);
                }
            }
            AudioUpdate::RemoveEmitter { name } => {
                engine.remove_emitter(&name);
                emitters.remove(&name);
            }
            AudioUpdate::SetListener {
                position,
                orientation,
            } => {
                engine.set_listener(position, orientation);
            }
            AudioUpdate::SetEmitterPosition { name, position } => {
                if let Some(thread_emitter) = emitters.get_mut(&name) {
                    thread_emitter.position = position;
                }
                engine.set_emitter_position(&name, position);
            }
            AudioUpdate::PlaySoundtrack { path } => {
                if let Err(e) = engine.play_soundtrack(&path, 1.0) {
                    tracing::warn!("soundtrack: {e}");
                }
            }
            AudioUpdate::StopSoundtrack => engine.stop_soundtrack(),
            AudioUpdate::PauseSoundtrack(paused) => engine.pause_soundtrack(paused),
        }
    }
}

// ---------------------------------------------------------------------------
// Bevy systems
// ---------------------------------------------------------------------------

pub fn init_audio_engine(mut commands: Commands) {
    match start_audio_engine() {
        Some(engine) => {
            commands.insert_resource(engine);
        }
        None => {
            tracing::warn!("Audio engine not available — continuing without audio");
            let (tx, rx) = mpsc::channel();
            // Keep the receiver alive so sends never block; nothing reads it.
            std::mem::forget(rx);
            commands.insert_resource(AudioEngine {
                active: false,
                master_volume: 0.0,
                emitter_names: HashSet::new(),
                update_tx: tx,
                ambience_layer_names: Vec::new(),
                emitter_meta: HashMap::new(),
                last_ambience: None,
                soundtrack_position: Arc::new(AtomicU32::new(f32::NAN.to_bits())),
            });
        }
    }
}

/// Keep the modulation clock on the song while it plays: modulations follow
/// what is heard, not a wall clock that drifts from it.
pub fn sync_soundtrack_clock(
    audio: Option<Res<AudioEngine>>,
    mut soundtrack: ResMut<localgpt_world_bevy::modulation::Soundtrack>,
) {
    let position = audio.and_then(|audio| audio.soundtrack_position());
    if soundtrack.position != position {
        soundtrack.position = position;
    }
}

// ---------------------------------------------------------------------------
// Command handlers (called from plugin.rs process_gen_commands)
// ---------------------------------------------------------------------------

pub fn handle_set_ambience(cmd: AmbienceCmd, engine: &mut AudioEngine) -> GenResponse {
    if !engine.active {
        return GenResponse::AmbienceSet;
    }

    if let Some(vol) = cmd.master_volume {
        engine.master_volume = vol.clamp(0.0, 1.0);
    }

    let mut layers = Vec::new();
    let mut layer_names = Vec::new();
    for layer_def in &cmd.layers {
        layers.push((
            layer_def.name.clone(),
            AudioSource::from(&layer_def.sound),
            layer_def.volume,
        ));
        layer_names.push(layer_def.name.clone());
    }

    engine.ambience_layer_names = layer_names;
    engine.last_ambience = Some(cmd.clone());

    let _ = engine.update_tx.send(AudioUpdate::SetAmbience {
        layers,
        master_volume: Some(engine.master_volume),
    });

    GenResponse::AmbienceSet
}

pub fn handle_spawn_audio_emitter(
    cmd: AudioEmitterCmd,
    engine: &mut AudioEngine,
    bevy_commands: &mut Commands,
    registry: &mut NameRegistry,
    next_entity_id: &mut super::registry::NextEntityId,
) -> GenResponse {
    if !engine.active {
        return GenResponse::AudioEmitterSpawned {
            name: cmd.name.clone(),
        };
    }

    let sound = AudioSource::from(&cmd.sound);

    engine.emitter_names.insert(cmd.name.clone());
    engine.emitter_meta.insert(
        cmd.name.clone(),
        EmitterMeta {
            sound_type: emitter_sound_type_name(&sound),
            sound: sound.clone(),
            base_volume: cmd.volume,
            radius: cmd.radius,
            attached_to: cmd.entity.clone(),
            position: cmd.position,
        },
    );

    let _ = engine.update_tx.send(AudioUpdate::AddEmitter {
        name: cmd.name.clone(),
        sound,
        volume: cmd.volume,
        radius: cmd.radius,
        // A position-less emitter attached to an entity gets its position
        // from `spatial_audio_update` on the next frame; until then it sits
        // at the origin, as the old engine's default pan/volume did.
        position: cmd.position.unwrap_or([0.0, 0.0, 0.0]),
    });

    // Attach to existing entity or spawn standalone
    if let Some(ref entity_name) = cmd.entity {
        if let Some(entity) = registry.get_entity(entity_name) {
            bevy_commands.entity(entity).insert(AudioEmitter {
                sound: AudioSource::from(&cmd.sound),
                radius: cmd.radius,
                volume: cmd.volume,
                emitter_name: cmd.name.clone(),
            });
        }
    } else if let Some(pos) = cmd.position {
        let wid = next_entity_id.alloc();
        let entity = bevy_commands
            .spawn((
                Transform::from_translation(bevy::math::Vec3::from_array(pos)),
                Name::new(cmd.name.clone()),
                GenEntity {
                    entity_type: super::registry::GenEntityType::AudioEmitter,
                    world_id: wid,
                },
                AudioEmitter {
                    sound: AudioSource::from(&cmd.sound),
                    radius: cmd.radius,
                    volume: cmd.volume,
                    emitter_name: cmd.name.clone(),
                },
            ))
            .id();
        registry.insert_with_id(cmd.name.clone(), entity, wid);
    }

    GenResponse::AudioEmitterSpawned { name: cmd.name }
}

pub fn handle_modify_audio_emitter(
    cmd: ModifyAudioEmitterCmd,
    engine: &mut AudioEngine,
) -> GenResponse {
    if !engine.emitter_names.contains(&cmd.name) {
        return GenResponse::Error {
            message: format!("Audio emitter '{}' not found", cmd.name),
        };
    }

    if let Some(vol) = cmd.volume
        && let Some(meta) = engine.emitter_meta.get_mut(&cmd.name)
    {
        meta.base_volume = vol;
        let _ = engine.update_tx.send(AudioUpdate::SetEmitterVolume {
            name: cmd.name.clone(),
            volume: vol,
        });
    }
    if let Some(radius) = cmd.radius {
        if let Some(meta) = engine.emitter_meta.get_mut(&cmd.name) {
            meta.radius = radius;
        }
        let _ = engine.update_tx.send(AudioUpdate::SetEmitterRadius {
            name: cmd.name.clone(),
            radius,
        });
    }

    if let Some(ref new_sound) = cmd.sound {
        // Rebuild the voice with the new graph, keeping volume and position.
        let new_source = AudioSource::from(new_sound);
        let _ = engine.update_tx.send(AudioUpdate::RemoveEmitter {
            name: cmd.name.clone(),
        });
        let (volume, position) = engine
            .emitter_meta
            .get(&cmd.name)
            .map(|meta| (meta.base_volume, meta.position.unwrap_or([0.0, 0.0, 0.0])))
            .unwrap_or((0.6, [0.0, 0.0, 0.0]));
        let _ = engine.update_tx.send(AudioUpdate::AddEmitter {
            name: cmd.name.clone(),
            sound: new_source.clone(),
            volume,
            radius: engine
                .emitter_meta
                .get(&cmd.name)
                .map(|meta| meta.radius)
                .unwrap_or(10.0),
            position,
        });

        if let Some(meta) = engine.emitter_meta.get_mut(&cmd.name) {
            meta.sound_type = emitter_sound_type_name(&new_source);
            meta.sound = new_source;
        }
    }

    GenResponse::AudioEmitterModified { name: cmd.name }
}

pub fn handle_remove_audio_emitter(name: &str, engine: &mut AudioEngine) -> GenResponse {
    engine.emitter_names.remove(name);
    engine.emitter_meta.remove(name);

    let _ = engine.update_tx.send(AudioUpdate::RemoveEmitter {
        name: name.to_string(),
    });

    GenResponse::AudioEmitterRemoved {
        name: name.to_string(),
    }
}

pub fn handle_audio_info(engine: &AudioEngine) -> GenResponse {
    let emitters = engine
        .emitter_meta
        .iter()
        .map(|(name, meta)| AudioEmitterSummary {
            name: name.clone(),
            sound_type: meta.sound_type.clone(),
            volume: meta.base_volume,
            radius: meta.radius,
            position: meta.position,
            attached_to: meta.attached_to.clone(),
        })
        .collect();

    GenResponse::AudioInfoData(AudioInfoResponse {
        active: engine.active,
        ambience_layers: engine.ambience_layer_names.clone(),
        emitters,
        master_volume: engine.master_volume(),
    })
}

/// Forward camera and emitter transforms to the audio thread.
///
/// The old system computed quadratic attenuation and stereo panning here,
/// per emitter per frame; the kira spatial tracks do both from the positions
/// alone, so this is now just movement, shipped as channel messages.
pub fn spatial_audio_update(
    engine: Res<AudioEngine>,
    listener_query: Query<&Transform, With<SpatialAudioListener>>,
    emitter_query: Query<(&Transform, &AudioEmitter)>,
) {
    if !engine.active {
        return;
    }

    let Ok(listener_transform) = listener_query.single() else {
        return;
    };

    let position = listener_transform.translation.to_array();
    let rotation = listener_transform.rotation;
    let _ = engine.update_tx.send(AudioUpdate::SetListener {
        position,
        orientation: [rotation.x, rotation.y, rotation.z, rotation.w],
    });

    for (emitter_transform, emitter) in emitter_query.iter() {
        let _ = engine.update_tx.send(AudioUpdate::SetEmitterPosition {
            name: emitter.emitter_name.clone(),
            position: emitter_transform.translation.to_array(),
        });
    }
}

/// Auto-infer audio emitters for newly spawned entities.
pub fn auto_infer_audio(
    mut commands: Commands,
    mut engine: ResMut<AudioEngine>,
    query: Query<(Entity, &Name, &GenEntity, &Transform), Without<AudioEmitter>>,
) {
    if !engine.active {
        return;
    }

    for (entity, name, gen_entity, transform) in query.iter() {
        match gen_entity.entity_type {
            super::registry::GenEntityType::Primitive | super::registry::GenEntityType::Mesh => {}
            _ => continue,
        }

        let Some((sound, radius)) = localgpt_world_audio::infer_emitter_from_name(name.as_str())
        else {
            continue;
        };

        let emitter_name = format!("{}_audio", name.as_str());
        if engine.emitter_names.contains(&emitter_name) {
            continue;
        }

        let base_volume = 0.6;
        let pos = transform.translation.to_array();

        engine.emitter_names.insert(emitter_name.clone());
        engine.emitter_meta.insert(
            emitter_name.clone(),
            EmitterMeta {
                sound_type: emitter_sound_type_name(&sound),
                sound: sound.clone(),
                base_volume,
                radius,
                attached_to: Some(name.as_str().to_string()),
                position: Some(pos),
            },
        );

        let _ = engine.update_tx.send(AudioUpdate::AddEmitter {
            name: emitter_name.clone(),
            sound: sound.clone(),
            volume: base_volume,
            radius,
            position: pos,
        });

        commands.entity(entity).insert(AudioEmitter {
            sound,
            radius,
            volume: base_volume,
            emitter_name,
        });
    }
}

/// The short name of a sound, for the inspector and `gen_audio_info`.
fn emitter_sound_type_name(sound: &AudioSource) -> String {
    match sound {
        AudioSource::Water { .. } => "water".to_string(),
        AudioSource::Fire { .. } => "fire".to_string(),
        AudioSource::Hum { .. } => "hum".to_string(),
        AudioSource::WindEmitter { .. } => "wind".to_string(),
        AudioSource::Custom { .. } => "custom".to_string(),
        // Ambient-only shapes never reach an emitter, but the format's enum
        // is shared, so name them rather than panic.
        AudioSource::Wind { .. } => "wind".to_string(),
        AudioSource::Rain { .. } => "rain".to_string(),
        AudioSource::Forest { .. } => "forest".to_string(),
        AudioSource::Ocean { .. } => "ocean".to_string(),
        AudioSource::Cave { .. } => "cave".to_string(),
        AudioSource::Stream { .. } => "stream".to_string(),
        AudioSource::Abc { .. } => "abc".to_string(),
        AudioSource::File { .. } => "file".to_string(),
        AudioSource::Silence => "silence".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emitter_sound_type_name() {
        assert_eq!(
            emitter_sound_type_name(&AudioSource::Water { turbulence: 0.5 }),
            "water"
        );
        assert_eq!(
            emitter_sound_type_name(&AudioSource::Fire {
                intensity: 0.7,
                crackle: 0.5
            }),
            "fire"
        );
        assert_eq!(
            emitter_sound_type_name(&AudioSource::Hum {
                frequency: 220.0,
                warmth: 0.5
            }),
            "hum"
        );
        assert_eq!(
            emitter_sound_type_name(&AudioSource::WindEmitter { pitch: 300.0 }),
            "wind"
        );
        assert_eq!(
            emitter_sound_type_name(&AudioSource::Custom {
                waveform: wt::WaveformType::WhiteNoise,
                filter_cutoff: 2000.0,
                filter_type: wt::FilterType::Lowpass
            }),
            "custom"
        );
    }
}
