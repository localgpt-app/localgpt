//! Behavior and audio libraries — shared definitions referenced by name.
//!
//! Multi-file worlds can store behavior definitions in separate files
//! and reference them by key from entity data.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::audio::AudioDef;
use crate::behavior::BehaviorDef;
use crate::history::AmbienceLayerDef;

/// A named collection of reusable behavior definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BehaviorLibrary {
    /// Behaviors keyed by name (e.g., "gentle_bob", "slow_orbit").
    pub behaviors: HashMap<String, BehaviorDef>,
}

/// Audio specification for a world — ambience layers and spatial emitters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AudioSpec {
    /// Global ambient sound layers.
    #[serde(default)]
    pub ambience: Vec<AmbienceLayerDef>,
    /// Positioned audio emitters.
    #[serde(default)]
    pub emitters: Vec<AudioEmitterSpec>,
}

/// A named, positioned audio emitter in the world.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AudioEmitterSpec {
    /// Emitter name (e.g., "campfire_crackle").
    pub name: String,
    /// World-space position [x, y, z].
    pub position: [f32; 3],
    /// Audio definition for this emitter.
    pub audio: AudioDef,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioKind, AudioSource, Rolloff};

    #[test]
    fn behavior_library_roundtrip_json() {
        let mut behaviors = HashMap::new();
        behaviors.insert(
            "gentle_bob".to_string(),
            BehaviorDef::Bob {
                axis: [0.0, 1.0, 0.0],
                amplitude: 0.3,
                frequency: 0.25,
                phase: 0.0,
            },
        );
        let lib = BehaviorLibrary { behaviors };
        let json = serde_json::to_string_pretty(&lib).unwrap();
        let back: BehaviorLibrary = serde_json::from_str(&json).unwrap();
        assert!(back.behaviors.contains_key("gentle_bob"));
    }

    #[test]
    fn behavior_library_roundtrip_ron() {
        let lib = BehaviorLibrary {
            behaviors: HashMap::new(),
        };
        let ron_str = ron::to_string(&lib).unwrap();
        let back: BehaviorLibrary = ron::from_str(&ron_str).unwrap();
        assert!(back.behaviors.is_empty());
    }

    #[test]
    fn audio_spec_roundtrip_json() {
        let spec = AudioSpec {
            ambience: vec![AmbienceLayerDef {
                name: "wind".to_string(),
                source: AudioSource::Wind {
                    speed: 0.6,
                    gustiness: 0.4,
                },
                volume: 0.5,
            }],
            emitters: vec![AudioEmitterSpec {
                name: "campfire".to_string(),
                position: [3.0, 0.0, -2.0],
                audio: AudioDef {
                    kind: AudioKind::Sfx,
                    source: AudioSource::Fire {
                        intensity: 0.8,
                        crackle: 0.5,
                    },
                    volume: 0.7,
                    radius: Some(15.0),
                    rolloff: Rolloff::InverseSquare,
                },
            }],
        };
        let json = serde_json::to_string_pretty(&spec).unwrap();
        let back: AudioSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ambience.len(), 1);
        assert_eq!(back.emitters.len(), 1);
        assert_eq!(back.emitters[0].name, "campfire");
    }

    #[test]
    fn audio_spec_roundtrip_ron() {
        let spec = AudioSpec {
            ambience: Vec::new(),
            emitters: Vec::new(),
        };
        let ron_str = ron::to_string(&spec).unwrap();
        let back: AudioSpec = ron::from_str(&ron_str).unwrap();
        assert!(back.ambience.is_empty());
        assert!(back.emitters.is_empty());
    }
}
