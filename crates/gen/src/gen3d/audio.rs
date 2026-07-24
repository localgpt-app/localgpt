//! AudioEngine resource, AudioEmitter component, and Bevy systems for
//! procedural environmental audio.
//!
//! Architecture:
//! - Audio management thread: owns FunDSP `Net` frontend, processes graph updates
//! - cpal callback thread: owns `Net` backend, renders samples
//! - Bevy main thread: sends graph updates via channel, sets `Shared` params lock-free

#![allow(clippy::precedence)]

use std::collections::HashMap;
use std::sync::mpsc;

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use fundsp::prelude::*;

use super::audio_graphs;
use super::commands::*;
use super::registry::{GenEntity, NameRegistry};

// ---------------------------------------------------------------------------
// Bevy components
// ---------------------------------------------------------------------------

/// Marker component for entities that emit spatial audio.
#[derive(Component)]
pub struct AudioEmitter {
    #[allow(dead_code)]
    pub sound: EmitterSound,
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
    master_volume: Shared,
    emitter_params: HashMap<String, EmitterSharedParams>,
    layer_volumes: HashMap<String, Shared>,
    graph_tx: mpsc::Sender<AudioGraphUpdate>,
    pub ambience_layer_names: Vec<String>,
    pub emitter_meta: HashMap<String, EmitterMeta>,
    /// Last ambience command for world save round-trip.
    pub last_ambience: Option<AmbienceCmd>,
}

struct EmitterSharedParams {
    volume: Shared,
    pan: Shared,
}

pub struct EmitterMeta {
    pub sound_type: String,
    pub sound: EmitterSound,
    pub base_volume: f32,
    pub radius: f32,
    pub attached_to: Option<String>,
    pub position: Option<[f32; 3]>,
}

impl AudioEngine {
    /// Play a one-shot sound emitter at a world position.
    ///
    /// Used by trigger systems (proximity, click) to fire PlaySoundAction.
    /// Creates a temporary emitter named "trigger_{sound}_{counter}" with
    /// a default volume and no spatial tracking (fire-and-forget).
    pub fn play_emitter_at(&mut self, sound_name: &str, position: Vec3) {
        use super::commands::{EmitterSound, FilterType, WaveformType};

        let emitter_name = format!("trigger_{}_{}", sound_name, self.emitter_params.len());
        let volume = shared(0.6);
        let pan = shared(0.0); // center pan for one-shots

        // Infer EmitterSound from the name
        let sound = match sound_name {
            "water" => EmitterSound::Water { turbulence: 0.5 },
            "fire" => EmitterSound::Fire {
                intensity: 0.7,
                crackle: 0.5,
            },
            "hum" => EmitterSound::Hum {
                frequency: 220.0,
                warmth: 0.5,
            },
            "wind" => EmitterSound::Wind { pitch: 300.0 },
            _ => EmitterSound::Custom {
                waveform: WaveformType::WhiteNoise,
                filter_cutoff: 2000.0,
                filter_type: FilterType::Lowpass,
            },
        };

        let _ = self.graph_tx.send(AudioGraphUpdate::AddEmitter {
            name: emitter_name.clone(),
            sound: sound.clone(),
            volume_shared: volume.clone(),
            pan_shared: pan.clone(),
        });

        self.emitter_params
            .insert(emitter_name.clone(), EmitterSharedParams { volume, pan });
        self.emitter_meta.insert(
            emitter_name,
            EmitterMeta {
                sound_type: sound_name.to_string(),
                sound,
                base_volume: 0.6,
                radius: 10.0,
                attached_to: None,
                position: Some([position.x, position.y, position.z]),
            },
        );
    }

    /// Stop all audio: remove all emitters and clear ambience state.
    pub fn stop_all(&mut self) {
        let emitter_names: Vec<String> = self.emitter_params.keys().cloned().collect();
        for name in &emitter_names {
            let _ = self
                .graph_tx
                .send(AudioGraphUpdate::RemoveEmitter { name: name.clone() });
        }
        self.emitter_params.clear();
        self.emitter_meta.clear();
        self.ambience_layer_names.clear();
        self.layer_volumes.clear();
        self.last_ambience = None;
    }
}

/// Messages sent from Bevy to the audio management thread.
enum AudioGraphUpdate {
    SetAmbience {
        layers: Vec<(String, AmbientSound, f32, Shared)>,
        master_vol: Shared,
    },
    AddEmitter {
        name: String,
        sound: EmitterSound,
        volume_shared: Shared,
        pan_shared: Shared,
    },
    RemoveEmitter {
        name: String,
    },
    #[allow(dead_code)]
    Shutdown,
}

// ---------------------------------------------------------------------------
// Audio thread
// ---------------------------------------------------------------------------

pub fn start_audio_engine() -> Option<AudioEngine> {
    let (graph_tx, graph_rx) = mpsc::channel::<AudioGraphUpdate>();
    let master_volume = Shared::new(0.8);
    let master_vol_clone = master_volume.clone();

    let thread_result = std::thread::Builder::new()
        .name("gen-audio".into())
        .spawn(move || {
            audio_thread_main(graph_rx, master_vol_clone);
        });

    match thread_result {
        Ok(_) => {
            tracing::info!("Audio engine started");
            Some(AudioEngine {
                active: true,
                master_volume,
                emitter_params: HashMap::new(),
                layer_volumes: HashMap::new(),
                graph_tx,
                ambience_layer_names: Vec::new(),
                emitter_meta: HashMap::new(),
                last_ambience: None,
            })
        }
        Err(e) => {
            tracing::error!("Failed to start audio thread: {}", e);
            None
        }
    }
}

/// State maintained on the audio thread for graph rebuilding.
struct AudioThreadState {
    ambience_layers: Vec<(String, AmbientSound, f32, Shared)>,
    emitters: HashMap<String, (EmitterSound, Shared, Shared)>,
    master_vol: Shared,
}

fn audio_thread_main(rx: mpsc::Receiver<AudioGraphUpdate>, master_vol: Shared) {
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            tracing::error!("No audio output device found");
            return;
        }
    };

    let supported_config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to get audio config: {}", e);
            return;
        }
    };

    let sample_rate = supported_config.sample_rate() as f64;
    let channels = supported_config.channels() as usize;
    let mut config = supported_config.config();

    // Request a larger buffer to avoid underruns from FunDSP graph processing
    config.buffer_size = cpal::BufferSize::Fixed(2048);

    tracing::info!(
        "Audio output: {} Hz, {} channels, buffer 2048",
        sample_rate,
        channels
    );

    // Build initial silent graph
    let mut net = Net::new(0, 2);
    let silence_id = net.push(Box::new(dc(0.0) | dc(0.0)));
    net.pipe_output(silence_id);
    net.set_sample_rate(sample_rate);
    net.allocate();

    // Get backend for cpal callback
    let mut backend = net.backend();
    backend.set_sample_rate(sample_rate);

    // Start cpal stream
    let _stream = match supported_config.sample_format() {
        cpal::SampleFormat::F32 => build_cpal_stream::<f32>(&device, &config, backend, channels),
        cpal::SampleFormat::I16 => build_cpal_stream::<i16>(&device, &config, backend, channels),
        cpal::SampleFormat::U16 => build_cpal_stream::<u16>(&device, &config, backend, channels),
        _ => {
            tracing::error!("Unsupported audio sample format");
            return;
        }
    };

    let Some(stream) = _stream else {
        tracing::error!("Failed to create audio stream");
        return;
    };

    if let Err(e) = stream.play() {
        tracing::error!("Failed to play audio stream: {}", e);
        return;
    }

    let mut state = AudioThreadState {
        ambience_layers: Vec::new(),
        emitters: HashMap::new(),
        master_vol,
    };

    // Process graph updates
    loop {
        match rx.recv() {
            Ok(AudioGraphUpdate::SetAmbience { layers, master_vol }) => {
                state.ambience_layers = layers;
                state.master_vol = master_vol;
                rebuild_graph(&mut net, &state, sample_rate);
            }
            Ok(AudioGraphUpdate::AddEmitter {
                name,
                sound,
                volume_shared,
                pan_shared,
            }) => {
                state
                    .emitters
                    .insert(name.clone(), (sound, volume_shared, pan_shared));
                rebuild_graph(&mut net, &state, sample_rate);
            }
            Ok(AudioGraphUpdate::RemoveEmitter { name }) => {
                state.emitters.remove(&name);
                rebuild_graph(&mut net, &state, sample_rate);
            }
            Ok(AudioGraphUpdate::Shutdown) | Err(_) => {
                tracing::info!("Audio thread shutting down");
                break;
            }
        }
    }
}

/// Rebuild the entire audio graph from current state.
/// Uses Net's commit() for glitch-free transition.
fn rebuild_graph(net: &mut Net, state: &AudioThreadState, sample_rate: f64) {
    // Remove all existing nodes
    let ids: Vec<NodeId> = net.ids().copied().collect();
    for id in ids {
        net.remove(id);
    }

    let mut all_stereo_ids: Vec<NodeId> = Vec::new();

    // Build ambience layers
    for (_name, sound, base_vol, vol_shared) in &state.ambience_layers {
        vol_shared.set(*base_vol);
        let graph = audio_graphs::build_ambient_graph(sound);
        let stereo_id = add_mono_source_stereo(net, graph, vol_shared, &state.master_vol);
        all_stereo_ids.push(stereo_id);
    }

    // Build emitters
    for (sound, vol_shared, _pan_shared) in state.emitters.values() {
        let graph = audio_graphs::build_emitter_graph(sound);
        let stereo_id = add_mono_source_stereo(net, graph, vol_shared, &state.master_vol);
        all_stereo_ids.push(stereo_id);
    }

    // Sum all stereo outputs into the net output
    if all_stereo_ids.is_empty() {
        let sid = net.push(Box::new(dc(0.0) | dc(0.0)));
        net.pipe_output(sid);
    } else if all_stereo_ids.len() == 1 {
        net.pipe_output(all_stereo_ids[0]);
    } else {
        // Chain stereo sums: sum pairs iteratively
        let mut current = all_stereo_ids[0];
        for &next in &all_stereo_ids[1..] {
            current = sum_two_stereo(net, current, next);
        }
        net.pipe_output(current);
    }

    net.set_sample_rate(sample_rate);
    net.allocate();
    net.commit();
    tracing::debug!(
        "Audio graph rebuilt: {} ambience layers, {} emitters",
        state.ambience_layers.len(),
        state.emitters.len()
    );
}

/// Add a mono source to the Net, multiply by volume and master volume,
/// then pan to stereo. Returns the stereo output node ID.
fn add_mono_source_stereo(
    net: &mut Net,
    source: Box<dyn AudioUnit>,
    volume: &Shared,
    master_vol: &Shared,
) -> NodeId {
    let id_src = net.push(source);
    let id_vol = net.push(Box::new(var(volume)));
    let id_mvol = net.push(Box::new(var(master_vol)));

    // Multiply: source * volume
    let id_mul1 = net.push(Box::new(pass() * pass()));
    net.set_source(id_mul1, 0, Source::Local(id_src, 0));
    net.set_source(id_mul1, 1, Source::Local(id_vol, 0));

    // Multiply: (source*volume) * master_volume
    let id_mul2 = net.push(Box::new(pass() * pass()));
    net.set_source(id_mul2, 0, Source::Local(id_mul1, 0));
    net.set_source(id_mul2, 1, Source::Local(id_mvol, 0));

    // Pan mono to stereo (center)
    let id_pan = net.push(Box::new(pan(0.0)));
    net.pipe_all(id_mul2, id_pan);

    id_pan
}

/// Sum two stereo nodes into one stereo output.
/// Uses (pass() + pass()) | (pass() + pass()) — 4 inputs, 2 outputs.
fn sum_two_stereo(net: &mut Net, a: NodeId, b: NodeId) -> NodeId {
    let id_sum = net.push(Box::new((pass() + pass()) | (pass() + pass())));
    net.set_source(id_sum, 0, Source::Local(a, 0)); // left A
    net.set_source(id_sum, 1, Source::Local(b, 0)); // left B
    net.set_source(id_sum, 2, Source::Local(a, 1)); // right A
    net.set_source(id_sum, 3, Source::Local(b, 1)); // right B
    id_sum
}

fn build_cpal_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut backend: NetBackend,
    channels: usize,
) -> Option<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let mut next_value = move || backend.get_stereo();

    let stream = device
        .build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let (left, right) = next_value();
                    for (i, sample) in frame.iter_mut().enumerate() {
                        *sample = if i % 2 == 0 {
                            T::from_sample(left)
                        } else {
                            T::from_sample(right)
                        };
                    }
                }
            },
            |err| tracing::warn!("Audio stream: {}", err),
            None,
        )
        .ok()?;

    Some(stream)
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
            let (tx, _rx) = mpsc::channel();
            commands.insert_resource(AudioEngine {
                active: false,
                master_volume: Shared::new(0.0),
                emitter_params: HashMap::new(),
                layer_volumes: HashMap::new(),
                graph_tx: tx,
                ambience_layer_names: Vec::new(),
                emitter_meta: HashMap::new(),
                last_ambience: None,
            });
        }
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
        engine.master_volume.set(vol.clamp(0.0, 1.0));
    }

    let mut layers = Vec::new();
    let mut layer_volumes = HashMap::new();
    let mut layer_names = Vec::new();

    for layer_def in &cmd.layers {
        let vol_shared = Shared::new(layer_def.volume);
        layer_volumes.insert(layer_def.name.clone(), vol_shared.clone());
        layers.push((
            layer_def.name.clone(),
            layer_def.sound.clone(),
            layer_def.volume,
            vol_shared,
        ));
        layer_names.push(layer_def.name.clone());
    }

    engine.layer_volumes = layer_volumes;
    engine.ambience_layer_names = layer_names;
    engine.last_ambience = Some(cmd.clone());

    let _ = engine.graph_tx.send(AudioGraphUpdate::SetAmbience {
        layers,
        master_vol: engine.master_volume.clone(),
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

    let vol_shared = Shared::new(cmd.volume);
    let pan_shared = Shared::new(0.0);

    engine.emitter_params.insert(
        cmd.name.clone(),
        EmitterSharedParams {
            volume: vol_shared.clone(),
            pan: pan_shared.clone(),
        },
    );

    engine.emitter_meta.insert(
        cmd.name.clone(),
        EmitterMeta {
            sound_type: emitter_sound_type_name(&cmd.sound),
            sound: cmd.sound.clone(),
            base_volume: cmd.volume,
            radius: cmd.radius,
            attached_to: cmd.entity.clone(),
            position: cmd.position,
        },
    );

    let _ = engine.graph_tx.send(AudioGraphUpdate::AddEmitter {
        name: cmd.name.clone(),
        sound: cmd.sound.clone(),
        volume_shared: vol_shared,
        pan_shared,
    });

    // Attach to existing entity or spawn standalone
    if let Some(ref entity_name) = cmd.entity {
        if let Some(entity) = registry.get_entity(entity_name) {
            bevy_commands.entity(entity).insert(AudioEmitter {
                sound: cmd.sound.clone(),
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
                    sound: cmd.sound.clone(),
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
    if !engine.emitter_params.contains_key(&cmd.name) {
        return GenResponse::Error {
            message: format!("Audio emitter '{}' not found", cmd.name),
        };
    }

    if let Some(vol) = cmd.volume
        && let Some(meta) = engine.emitter_meta.get_mut(&cmd.name)
    {
        meta.base_volume = vol;
    }
    if let Some(radius) = cmd.radius
        && let Some(meta) = engine.emitter_meta.get_mut(&cmd.name)
    {
        meta.radius = radius;
    }

    if let Some(ref new_sound) = cmd.sound {
        // Remove and re-add with new sound
        let params = engine.emitter_params.get(&cmd.name).unwrap();
        let vol_shared = params.volume.clone();
        let pan_shared = params.pan.clone();

        let _ = engine.graph_tx.send(AudioGraphUpdate::RemoveEmitter {
            name: cmd.name.clone(),
        });
        let _ = engine.graph_tx.send(AudioGraphUpdate::AddEmitter {
            name: cmd.name.clone(),
            sound: new_sound.clone(),
            volume_shared: vol_shared,
            pan_shared,
        });

        if let Some(meta) = engine.emitter_meta.get_mut(&cmd.name) {
            meta.sound_type = emitter_sound_type_name(new_sound);
            meta.sound = new_sound.clone();
        }
    }

    GenResponse::AudioEmitterModified { name: cmd.name }
}

pub fn handle_remove_audio_emitter(name: &str, engine: &mut AudioEngine) -> GenResponse {
    engine.emitter_params.remove(name);
    engine.emitter_meta.remove(name);

    let _ = engine.graph_tx.send(AudioGraphUpdate::RemoveEmitter {
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
        master_volume: engine.master_volume.value(),
    })
}

/// Update spatial audio based on camera distance to emitters.
/// Lock-free — sets Shared params directly.
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

    let listener_pos = listener_transform.translation;
    let listener_right = listener_transform.right().as_vec3();

    for (emitter_transform, emitter) in emitter_query.iter() {
        let Some(params) = engine.emitter_params.get(&emitter.emitter_name) else {
            continue;
        };

        let emitter_pos = emitter_transform.translation;
        let to_emitter = emitter_pos - listener_pos;
        let distance = to_emitter.length();

        // Volume: quadratic falloff within radius
        let attenuation = if distance < 1.0 {
            1.0
        } else if distance > emitter.radius {
            0.0
        } else {
            let t = 1.0 - (distance - 1.0) / (emitter.radius - 1.0).max(0.01);
            t * t
        };

        params.volume.set(emitter.volume * attenuation);

        // Stereo pan
        if distance > 0.01 {
            let dir = to_emitter.normalize();
            let pan_value = dir.dot(listener_right).clamp(-1.0, 1.0);
            params.pan.set(pan_value);
        }
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

        let Some((sound, radius)) = audio_graphs::infer_emitter_from_name(name.as_str()) else {
            continue;
        };

        let emitter_name = format!("{}_audio", name.as_str());
        if engine.emitter_params.contains_key(&emitter_name) {
            continue;
        }

        let base_volume = 0.6;
        let pos = transform.translation.to_array();

        let vol_shared = Shared::new(base_volume);
        let pan_shared = Shared::new(0.0);

        engine.emitter_params.insert(
            emitter_name.clone(),
            EmitterSharedParams {
                volume: vol_shared.clone(),
                pan: pan_shared.clone(),
            },
        );

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

        let _ = engine.graph_tx.send(AudioGraphUpdate::AddEmitter {
            name: emitter_name.clone(),
            sound: sound.clone(),
            volume_shared: vol_shared,
            pan_shared,
        });

        commands.entity(entity).insert(AudioEmitter {
            sound,
            radius,
            volume: base_volume,
            emitter_name,
        });

        tracing::debug!("Auto-inferred audio emitter for entity '{}'", name.as_str());
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn emitter_sound_type_name(sound: &EmitterSound) -> String {
    match sound {
        EmitterSound::Water { .. } => "water".to_string(),
        EmitterSound::Fire { .. } => "fire".to_string(),
        EmitterSound::Hum { .. } => "hum".to_string(),
        EmitterSound::Wind { .. } => "wind".to_string(),
        EmitterSound::Custom { .. } => "custom".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emitter_sound_type_name() {
        assert_eq!(
            emitter_sound_type_name(&EmitterSound::Water { turbulence: 0.5 }),
            "water"
        );
        assert_eq!(
            emitter_sound_type_name(&EmitterSound::Fire {
                intensity: 0.5,
                crackle: 0.3
            }),
            "fire"
        );
        assert_eq!(
            emitter_sound_type_name(&EmitterSound::Hum {
                frequency: 120.0,
                warmth: 0.5
            }),
            "hum"
        );
        assert_eq!(
            emitter_sound_type_name(&EmitterSound::Wind { pitch: 400.0 }),
            "wind"
        );
        assert_eq!(
            emitter_sound_type_name(&EmitterSound::Custom {
                waveform: WaveformType::Sine,
                filter_cutoff: 1000.0,
                filter_type: FilterType::Lowpass,
            }),
            "custom"
        );
    }

    #[test]
    fn test_audio_emitter_component() {
        let emitter = AudioEmitter {
            sound: EmitterSound::Water { turbulence: 0.8 },
            radius: 15.0,
            volume: 0.6,
            emitter_name: "fountain_audio".to_string(),
        };
        assert_eq!(emitter.radius, 15.0);
        assert_eq!(emitter.volume, 0.6);
        assert_eq!(emitter.emitter_name, "fountain_audio");
    }

    #[test]
    fn test_emitter_meta() {
        let meta = EmitterMeta {
            sound_type: "fire".to_string(),
            sound: EmitterSound::Fire {
                intensity: 0.7,
                crackle: 0.4,
            },
            base_volume: 0.5,
            radius: 10.0,
            attached_to: Some("campfire".to_string()),
            position: Some([1.0, 0.0, 2.0]),
        };
        assert_eq!(meta.sound_type, "fire");
        assert_eq!(meta.base_volume, 0.5);
        assert_eq!(meta.attached_to, Some("campfire".to_string()));
        assert_eq!(meta.position, Some([1.0, 0.0, 2.0]));
    }

    #[test]
    fn test_emitter_meta_standalone() {
        let meta = EmitterMeta {
            sound_type: "water".to_string(),
            sound: EmitterSound::Water { turbulence: 0.3 },
            base_volume: 0.8,
            radius: 20.0,
            attached_to: None,
            position: None,
        };
        assert!(meta.attached_to.is_none());
        assert!(meta.position.is_none());
    }
}
