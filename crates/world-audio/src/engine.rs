//! One kira `AudioManager` playing the format's audio: ambience layers,
//! spatial emitters, and files.
//!
//! This is what replaces Gen's hand-rolled engine — a management thread owning
//! a FunDSP `Net` frontend, a cpal callback owning the backend, and `Shared`
//! atomics threaded between them — and what lets one process also play Verse's
//! soundtrack, because kira decodes files on the same mixer.
//!
//! Two things Gen did by hand are kira's job here:
//!
//! * **Distance attenuation.** Gen recomputed per-emitter volume from the
//!   listener each frame. An emitter is a *spatial* sub-track, so moving the
//!   listener or the emitter is enough.
//! * **Panning.** Gen wrapped each graph in its own pan node. Tracks pan.
//!
//! Volume is amplitude here, as everywhere else in the format, and converted to
//! kira's decibels at the edge ([`amplitude_to_db`]).

use std::collections::HashMap;

use kira::listener::ListenerHandle;
use kira::track::{SpatialTrackBuilder, SpatialTrackHandle, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};
use localgpt_world_types as wt;
use tracing::{debug, warn};

use crate::source::{Fundsp, FundspHandle};

/// The sample rate graphs are built for. kira retunes a voice if the device
/// disagrees (see [`crate::source::FundspSound::process`]).
const GRAPH_RATE: f64 = 48_000.0;

/// Amplitude (0.0..=1.0, the format's unit) to kira's decibels. Zero and below
/// are silence rather than `-inf`, which would poison the mixer's arithmetic.
pub fn amplitude_to_db(amplitude: f32) -> Decibels {
    if amplitude <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * amplitude.log10())
    }
}

fn instant() -> Tween {
    Tween {
        duration: std::time::Duration::ZERO,
        ..Default::default()
    }
}

/// One playing ambience layer: its track, and the voice on it.
struct Layer {
    name: String,
    track: TrackHandle,
    /// Dropping this stops the voice, so it is held for exactly that.
    _voice: FundspHandle,
}

/// One playing spatial emitter.
struct Emitter {
    track: SpatialTrackHandle,
    _voice: FundspHandle,
}

/// The audio device, the mixer and everything currently sounding.
pub struct Engine {
    manager: AudioManager,
    listener: ListenerHandle,
    ambience: Vec<Layer>,
    emitters: HashMap<String, Emitter>,
}

impl Engine {
    /// Opens the default output device. `None` when there is none — a headless
    /// CI runner, a machine with no sound card — which every caller treats as
    /// "the world is silent" rather than an error, exactly as Gen's
    /// `start_audio_engine` did.
    pub fn new() -> Option<Self> {
        let mut manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
        {
            Ok(manager) => manager,
            Err(e) => {
                warn!("audio: no output device ({e}) — worlds will be silent");
                return None;
            }
        };
        // Emitters are spatial sub-tracks, so they need a listener even before
        // the caller has a camera to put it on.
        let listener = match manager.add_listener(
            mint::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            mint::Quaternion {
                s: 1.0,
                v: mint::Vector3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            },
        ) {
            Ok(listener) => listener,
            Err(e) => {
                warn!("audio: no listener ({e}) — worlds will be silent");
                return None;
            }
        };
        Some(Self {
            manager,
            listener,
            ambience: Vec::new(),
            emitters: HashMap::new(),
        })
    }

    /// The mixer, for callers with their own playback layers to add — Verse's
    /// soundtrack track and its analysis tap effect, for instance. Everything
    /// this crate maps goes through the same manager, so one process has one
    /// device and one mixer no matter how many kinds of sound it plays.
    pub fn manager(&mut self) -> &mut AudioManager {
        &mut self.manager
    }

    /// Master volume, as amplitude.
    pub fn set_master_volume(&mut self, amplitude: f32) {
        self.manager
            .main_track()
            .set_volume(amplitude_to_db(amplitude), instant());
    }

    /// Where the ears are. An emitter's distance falloff follows from this.
    pub fn set_listener(&mut self, position: [f32; 3], orientation: [f32; 4]) {
        self.listener.set_position(
            mint::Vector3 {
                x: position[0],
                y: position[1],
                z: position[2],
            },
            instant(),
        );
        self.listener.set_orientation(
            mint::Quaternion {
                s: orientation[3],
                v: mint::Vector3 {
                    x: orientation[0],
                    y: orientation[1],
                    z: orientation[2],
                },
            },
            instant(),
        );
    }

    /// Replace every ambience layer. Non-synthesis sources are skipped, so a
    /// `Silence` layer is the absence of a voice rather than a silent one.
    pub fn set_ambience(&mut self, layers: &[(String, wt::AudioSource, f32)]) {
        self.ambience.clear();
        for (name, source, volume) in layers {
            let Some(graph) = crate::graphs::build(source) else {
                debug!("audio: ambience layer {name} is not synthesis — skipped");
                continue;
            };
            let Ok(mut track) = self
                .manager
                .add_sub_track(TrackBuilder::new().volume(amplitude_to_db(*volume)))
            else {
                warn!("audio: mixer is full — ambience layer {name} dropped");
                continue;
            };
            match track.play(Fundsp::new(graph, GRAPH_RATE)) {
                Ok(voice) => self.ambience.push(Layer {
                    name: name.clone(),
                    track,
                    _voice: voice,
                }),
                Err(e) => warn!("audio: ambience layer {name} would not play ({e})"),
            }
        }
    }

    /// The ambience layers currently sounding, in order.
    pub fn ambience_names(&self) -> Vec<&str> {
        self.ambience.iter().map(|l| l.name.as_str()).collect()
    }

    /// Volume of one ambience layer, as amplitude.
    pub fn set_ambience_volume(&mut self, name: &str, amplitude: f32) -> bool {
        if let Some(layer) = self.ambience.iter_mut().find(|l| l.name == name) {
            layer
                .track
                .set_volume(amplitude_to_db(amplitude), instant());
            true
        } else {
            false
        }
    }

    /// Add a spatial emitter. `radius` is where it fades out; kira attenuates
    /// by listener distance from there, which Gen did per frame by hand.
    pub fn add_emitter(
        &mut self,
        name: &str,
        source: &wt::AudioSource,
        volume: f32,
        position: [f32; 3],
        radius: f32,
    ) -> bool {
        let Some(graph) = crate::graphs::build(source) else {
            debug!("audio: emitter {name} is not synthesis — skipped");
            return false;
        };
        let builder = SpatialTrackBuilder::new()
            .volume(amplitude_to_db(volume))
            .distances(kira::track::SpatialTrackDistances {
                min_distance: 1.0,
                max_distance: radius.max(1.0),
            });
        let Ok(mut track) = self.manager.add_spatial_sub_track(
            &self.listener,
            mint::Vector3 {
                x: position[0],
                y: position[1],
                z: position[2],
            },
            builder,
        ) else {
            warn!("audio: mixer is full — emitter {name} dropped");
            return false;
        };
        match track.play(Fundsp::new(graph, GRAPH_RATE)) {
            Ok(voice) => {
                self.emitters.insert(
                    name.to_string(),
                    Emitter {
                        track,
                        _voice: voice,
                    },
                );
                true
            }
            Err(e) => {
                warn!("audio: emitter {name} would not play ({e})");
                false
            }
        }
    }

    /// Move an emitter. Returns false when there is no such emitter.
    pub fn set_emitter_position(&mut self, name: &str, position: [f32; 3]) -> bool {
        if let Some(emitter) = self.emitters.get_mut(name) {
            emitter.track.set_position(
                mint::Vector3 {
                    x: position[0],
                    y: position[1],
                    z: position[2],
                },
                instant(),
            );
            true
        } else {
            false
        }
    }

    /// An emitter's own volume, before distance falloff.
    pub fn set_emitter_volume(&mut self, name: &str, amplitude: f32) -> bool {
        if let Some(emitter) = self.emitters.get_mut(name) {
            emitter
                .track
                .set_volume(amplitude_to_db(amplitude), instant());
            true
        } else {
            false
        }
    }

    /// Stop and forget an emitter.
    pub fn remove_emitter(&mut self, name: &str) -> bool {
        self.emitters.remove(name).is_some()
    }

    /// The emitters currently sounding.
    pub fn emitter_names(&self) -> Vec<&str> {
        self.emitters.keys().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplitude_converts_to_decibels_with_silence_at_the_bottom() {
        // Unity is 0 dB, half amplitude is about -6 dB, and nothing at or below
        // zero becomes SILENCE rather than -inf, which would poison the mixer.
        assert!((amplitude_to_db(1.0).0 - 0.0).abs() < 1e-6);
        assert!((amplitude_to_db(0.5).0 + 6.0206).abs() < 1e-3);
        assert_eq!(amplitude_to_db(0.0), Decibels::SILENCE);
        assert_eq!(amplitude_to_db(-1.0), Decibels::SILENCE);
    }

    // Anything past this needs an output device, which a CI runner has not got.
    // `Engine::new` returning None there is the contract, not a failure, so the
    // device-dependent behaviour is exercised by Gen's and Verse's own runs.
    #[test]
    fn engine_either_opens_a_device_or_declines() {
        match Engine::new() {
            Some(mut engine) => {
                engine.set_master_volume(0.8);
                engine.set_ambience(&[(
                    "wind".to_string(),
                    wt::AudioSource::Wind {
                        speed: 0.4,
                        gustiness: 0.3,
                    },
                    0.5,
                )]);
                assert_eq!(engine.ambience_names(), ["wind"]);
                assert!(engine.set_ambience_volume("wind", 0.2));
                assert!(!engine.set_ambience_volume("nope", 0.2));

                assert!(engine.add_emitter(
                    "campfire",
                    &wt::AudioSource::Fire {
                        intensity: 0.6,
                        crackle: 0.7
                    },
                    0.7,
                    [1.0, 0.0, 2.0],
                    8.0,
                ));
                assert_eq!(engine.emitter_names(), ["campfire"]);
                assert!(engine.set_emitter_position("campfire", [2.0, 0.0, 2.0]));
                assert!(engine.set_emitter_volume("campfire", 0.4));
                assert!(engine.remove_emitter("campfire"));
                assert!(!engine.remove_emitter("campfire"));

                // Silence is the absence of a voice, so it adds no layer.
                engine.set_ambience(&[("quiet".to_string(), wt::AudioSource::Silence, 1.0)]);
                assert!(engine.ambience_names().is_empty());
            }
            None => {
                // No device: the contract is that callers go silent, not that
                // they fail.
            }
        }
    }
}
