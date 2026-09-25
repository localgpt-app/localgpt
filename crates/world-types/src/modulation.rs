//! Signal-driven modulation — the reactive layer of a world.
//!
//! A [`ModulationDef`] binds one parameter of an entity to a named signal
//! that the runtime produces while the world plays: the soundtrack's energy
//! envelope, its beat, a stem, or a free-running oscillator. It is what lets
//! a LocalGPT Verse world *perform* a song, and it is declarative like
//! [`BehaviorDef`](crate::BehaviorDef) so every renderer (Bevy, the web
//! viewer) evaluates it the same way.
//!
//! Evaluation, per frame, with `s` the smoothed signal in `0..=1`:
//!
//! ```text
//! factor = range[0] + (range[1] - range[0]) * s
//! ```
//!
//! Multiplicative targets (`emissive`, `scale`, `light_intensity`,
//! `opacity`) apply `factor` to the entity's authored value; `offset_y` adds
//! `factor` (world units) to the authored position. When the runtime has no
//! source for the signal (no soundtrack, no stems) the modulation is
//! **inactive**: multiplicative targets use `1.0` and `offset_y` uses `0.0`,
//! so a world without music renders exactly as authored.
//!
//! `smoothing` is the time constant, in seconds, of a one-pole low-pass on
//! the raw signal (`s += (raw - s) * min(1, dt / smoothing)`); `0` uses the
//! raw signal.

use serde::{Deserialize, Serialize};

/// One parameter of an entity bound to a runtime signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModulationDef {
    /// Which parameter of the entity is driven.
    pub target: ModulationTarget,
    /// Which runtime signal drives it.
    pub signal: SignalSource,
    /// `[value at signal 0, value at signal 1]` — a multiplier for
    /// multiplicative targets, world units for `offset_y`.
    #[serde(default = "default_range")]
    pub range: [f32; 2],
    /// Low-pass time constant in seconds (`0` = raw signal).
    #[serde(default)]
    pub smoothing: f32,
}

/// The entity parameter a modulation drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModulationTarget {
    /// Multiplies the material's emissive color.
    Emissive,
    /// Multiplies the entity's scale uniformly.
    Scale,
    /// Multiplies the light's intensity.
    LightIntensity,
    /// Multiplies the material's alpha.
    Opacity,
    /// Adds an offset (world units) to the entity's Y position.
    OffsetY,
}

/// A runtime signal in `0..=1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    /// The soundtrack's loudness envelope
    /// ([`SoundtrackDef::energy`](crate::SoundtrackDef::energy)).
    Energy,
    /// `1` on each beat, decaying linearly to `0` at the next
    /// ([`SoundtrackDef::beat_at`](crate::SoundtrackDef::beat_at)).
    Beat,
    /// Low band level. Live audio when playing; else the `bass` stem, else
    /// `energy`.
    Bass,
    /// High band level. Live audio when playing; else the `other` stem, else
    /// `energy`.
    Highs,
    /// A separated stem's envelope; falls back to `energy` when the
    /// soundtrack carries no stems.
    Stem(StemKind),
    /// A free-running sine, `0..=1`, at `frequency` Hz. Needs no soundtrack.
    Oscillator {
        /// Cycles per second.
        frequency: f32,
    },
    /// A fixed value — for authoring and previews.
    Constant(f32),
}

/// A source-separated stem (Demucs naming).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    Drums,
    Bass,
    Vocals,
    Other,
}

fn default_range() -> [f32; 2] {
    [1.0, 1.0]
}

impl ModulationDef {
    /// Bind `target` to `signal` over `range`.
    pub fn new(target: ModulationTarget, signal: SignalSource, range: [f32; 2]) -> Self {
        Self {
            target,
            signal,
            range,
            smoothing: 0.0,
        }
    }

    /// Builder: set the low-pass time constant.
    pub fn with_smoothing(mut self, seconds: f32) -> Self {
        self.smoothing = seconds;
        self
    }

    /// The factor for a signal value (clamped to `0..=1`).
    pub fn factor(&self, signal: f32) -> f32 {
        let s = signal.clamp(0.0, 1.0);
        self.range[0] + (self.range[1] - self.range[0]) * s
    }

    /// The factor a renderer applies when the signal has no source.
    pub fn inactive_factor(&self) -> f32 {
        match self.target {
            ModulationTarget::OffsetY => 0.0,
            _ => 1.0,
        }
    }

    /// Advance a smoothed signal by `dt` seconds toward `raw`.
    pub fn smooth(&self, current: f32, raw: f32, dt: f32) -> f32 {
        if self.smoothing <= 0.0 {
            return raw;
        }
        let k = (dt / self.smoothing).clamp(0.0, 1.0);
        current + (raw - current) * k
    }

    /// Whether every number is finite and `smoothing` is non-negative.
    pub fn is_valid(&self) -> bool {
        self.range.iter().all(|v| v.is_finite())
            && self.smoothing.is_finite()
            && self.smoothing >= 0.0
            && match &self.signal {
                SignalSource::Oscillator { frequency } => frequency.is_finite() && *frequency > 0.0,
                SignalSource::Constant(v) => v.is_finite(),
                _ => true,
            }
    }
}

impl SignalSource {
    /// Signals that exist without a soundtrack.
    pub fn needs_soundtrack(&self) -> bool {
        !matches!(
            self,
            SignalSource::Oscillator { .. } | SignalSource::Constant(_)
        )
    }

    /// The value of a soundtrack-free signal at time `t` (seconds); `None`
    /// for signals a soundtrack provides.
    pub fn free_running_at(&self, t: f32) -> Option<f32> {
        match self {
            SignalSource::Oscillator { frequency } => {
                Some(((t * frequency * std::f32::consts::TAU).sin() + 1.0) * 0.5)
            }
            SignalSource::Constant(v) => Some(v.clamp(0.0, 1.0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_interpolates_and_clamps() {
        let m = ModulationDef::new(ModulationTarget::Emissive, SignalSource::Beat, [0.5, 2.0]);
        assert_eq!(m.factor(0.0), 0.5);
        assert_eq!(m.factor(1.0), 2.0);
        assert_eq!(m.factor(2.0), 2.0);
        assert_eq!(m.factor(-1.0), 0.5);
        assert!((m.factor(0.5) - 1.25).abs() < 1e-6);
    }

    #[test]
    fn inactive_factor_is_identity() {
        let mul = ModulationDef::new(ModulationTarget::Scale, SignalSource::Energy, [0.0, 3.0]);
        let add = ModulationDef::new(ModulationTarget::OffsetY, SignalSource::Energy, [0.0, 3.0]);
        assert_eq!(mul.inactive_factor(), 1.0);
        assert_eq!(add.inactive_factor(), 0.0);
    }

    #[test]
    fn smoothing_is_one_pole() {
        let raw = ModulationDef::new(ModulationTarget::Scale, SignalSource::Energy, [1.0, 2.0]);
        assert_eq!(raw.smooth(0.0, 1.0, 0.016), 1.0);
        let slow = raw.clone().with_smoothing(1.0);
        let s = slow.smooth(0.0, 1.0, 0.25);
        assert!((s - 0.25).abs() < 1e-6);
        // A step longer than the time constant lands on the target.
        assert_eq!(slow.smooth(0.0, 1.0, 5.0), 1.0);
    }

    #[test]
    fn free_running_signals() {
        let osc = SignalSource::Oscillator { frequency: 1.0 };
        assert!((osc.free_running_at(0.0).unwrap() - 0.5).abs() < 1e-6);
        assert!((osc.free_running_at(0.25).unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(SignalSource::Constant(7.0).free_running_at(3.0), Some(1.0));
        assert_eq!(SignalSource::Energy.free_running_at(3.0), None);
        assert!(SignalSource::Energy.needs_soundtrack());
        assert!(!osc.needs_soundtrack());
    }

    #[test]
    fn validity() {
        let ok = ModulationDef::new(ModulationTarget::Scale, SignalSource::Energy, [1.0, 2.0]);
        assert!(ok.is_valid());
        let mut bad = ok.clone();
        bad.range = [f32::NAN, 1.0];
        assert!(!bad.is_valid());
        let mut bad = ok.clone();
        bad.smoothing = -1.0;
        assert!(!bad.is_valid());
        let bad = ModulationDef::new(
            ModulationTarget::Scale,
            SignalSource::Oscillator { frequency: 0.0 },
            [1.0, 2.0],
        );
        assert!(!bad.is_valid());
    }

    #[test]
    fn serde_shapes_are_stable() {
        let m = ModulationDef::new(
            ModulationTarget::LightIntensity,
            SignalSource::Stem(StemKind::Drums),
            [0.5, 1.5],
        )
        .with_smoothing(0.1);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(
            json,
            r#"{"target":"light_intensity","signal":{"stem":"drums"},"range":[0.5,1.5],"smoothing":0.1}"#
        );
        let back: ModulationDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);

        let osc: ModulationDef = serde_json::from_str(
            r#"{"target":"offset_y","signal":{"oscillator":{"frequency":0.5}}}"#,
        )
        .unwrap();
        assert_eq!(osc.range, [1.0, 1.0]);
        assert_eq!(osc.smoothing, 0.0);
        let plain: ModulationDef =
            serde_json::from_str(r#"{"target":"emissive","signal":"beat","range":[1,3]}"#).unwrap();
        assert_eq!(plain.signal, SignalSource::Beat);
    }
}
