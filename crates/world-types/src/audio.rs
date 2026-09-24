//! Unified audio definitions — merges ambient and emitter sound systems.
//!
//! Current code has two disconnected enums (`AmbientSound` 6+1 variants,
//! `EmitterSound` 5 variants).  This module merges all sound types into
//! one [`AudioSource`] enum.  Spatial vs global behavior is determined by
//! [`AudioKind`] + `radius`.

use serde::{Deserialize, Serialize};

/// Audio component attached to an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AudioDef {
    /// High-level audio category.
    #[serde(default)]
    pub kind: AudioKind,
    /// The sound source — what to play.
    pub source: AudioSource,
    /// Volume multiplier (0.0–1.0).
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Spatial radius.  `None` = global (fills the scene), `Some(r)` = spatial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    /// Distance rolloff model for spatial audio.
    #[serde(default)]
    pub rolloff: Rolloff,
}

/// High-level audio category — determines mixing bus and behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AudioKind {
    /// Environmental background (wind, rain, etc.)
    #[default]
    Ambient,
    /// Sound effects (spatial, one-shot or looping).
    Sfx,
    /// Music track.
    Music,
}

/// What sound to produce — unified taxonomy of all sound types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AudioSource {
    // --- Ambient sounds (from current AmbientSound) ---
    Wind {
        #[serde(default = "default_half")]
        speed: f32,
        #[serde(default = "default_half")]
        gustiness: f32,
    },
    Rain {
        #[serde(default = "default_half")]
        intensity: f32,
    },
    Forest {
        #[serde(default = "default_half")]
        bird_density: f32,
        #[serde(default = "default_half")]
        wind: f32,
    },
    Ocean {
        #[serde(default = "default_half")]
        wave_size: f32,
    },
    Cave {
        #[serde(default = "default_half")]
        drip_rate: f32,
        #[serde(default = "default_half")]
        resonance: f32,
    },
    Stream {
        #[serde(default = "default_half")]
        flow_rate: f32,
    },

    // --- Emitter sounds (from current EmitterSound) ---
    Water {
        #[serde(default = "default_half")]
        turbulence: f32,
    },
    Fire {
        #[serde(default = "default_half")]
        intensity: f32,
        #[serde(default = "default_half")]
        crackle: f32,
    },
    Hum {
        #[serde(default = "default_hum_freq")]
        frequency: f32,
        #[serde(default = "default_half")]
        warmth: f32,
    },
    WindEmitter {
        #[serde(default = "default_one")]
        pitch: f32,
    },
    Custom {
        #[serde(default)]
        waveform: WaveformType,
        #[serde(default = "default_filter_cutoff")]
        filter_cutoff: f32,
        #[serde(default)]
        filter_type: FilterType,
    },

    // --- New types from RFC ---
    /// ABC notation music score.
    Abc { notation: String },
    /// External audio file reference.
    File {
        path: String,
        #[serde(default = "default_true")]
        looping: bool,
    },
    /// Silence (placeholder / mute).
    Silence,
}

/// Waveform type for custom audio synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WaveformType {
    #[default]
    Sine,
    Saw,
    Square,
    WhiteNoise,
    PinkNoise,
    BrownNoise,
}

/// Audio filter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
}

/// Distance rolloff model for spatial audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Rolloff {
    #[default]
    InverseSquare,
    Linear,
    Exponential,
}

/// Reverb parameters for ambient audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReverbParams {
    #[serde(default = "default_half")]
    pub room_size: f32,
    #[serde(default = "default_half")]
    pub damping: f32,
    #[serde(default = "default_third")]
    pub wet: f32,
}

// ---- Default helpers ----

fn default_volume() -> f32 {
    1.0
}
fn default_half() -> f32 {
    0.5
}
fn default_one() -> f32 {
    1.0
}
fn default_hum_freq() -> f32 {
    220.0
}
fn default_filter_cutoff() -> f32 {
    1000.0
}
fn default_true() -> bool {
    true
}
fn default_third() -> f32 {
    0.33
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_source_roundtrip_all_variants() {
        let sources = vec![
            AudioSource::Wind {
                speed: 0.7,
                gustiness: 0.3,
            },
            AudioSource::Rain { intensity: 0.8 },
            AudioSource::Forest {
                bird_density: 0.5,
                wind: 0.3,
            },
            AudioSource::Ocean { wave_size: 1.0 },
            AudioSource::Cave {
                drip_rate: 0.4,
                resonance: 0.6,
            },
            AudioSource::Stream { flow_rate: 0.5 },
            AudioSource::Water { turbulence: 0.6 },
            AudioSource::Fire {
                intensity: 0.8,
                crackle: 0.5,
            },
            AudioSource::Hum {
                frequency: 440.0,
                warmth: 0.7,
            },
            AudioSource::WindEmitter { pitch: 1.5 },
            AudioSource::Custom {
                waveform: WaveformType::PinkNoise,
                filter_cutoff: 800.0,
                filter_type: FilterType::Lowpass,
            },
            AudioSource::Abc {
                notation: "X:1\nT:Test\nK:C\nCDEF|".to_string(),
            },
            AudioSource::File {
                path: "music.ogg".to_string(),
                looping: true,
            },
            AudioSource::Silence,
        ];

        for src in &sources {
            let json = serde_json::to_string(src).unwrap();
            let back: AudioSource = serde_json::from_str(&json).unwrap();
            assert_eq!(*src, back);
        }
    }

    #[test]
    fn audio_def_spatial_vs_global() {
        let global = AudioDef {
            kind: AudioKind::Ambient,
            source: AudioSource::Wind {
                speed: 0.5,
                gustiness: 0.3,
            },
            volume: 0.8,
            radius: None,
            rolloff: Rolloff::InverseSquare,
        };
        assert!(global.radius.is_none());

        let spatial = AudioDef {
            radius: Some(20.0),
            kind: AudioKind::Sfx,
            ..global.clone()
        };
        assert_eq!(spatial.radius, Some(20.0));

        // Both roundtrip
        for def in [&global, &spatial] {
            let json = serde_json::to_string(def).unwrap();
            let back: AudioDef = serde_json::from_str(&json).unwrap();
            assert_eq!(*def, back);
        }
    }
}
