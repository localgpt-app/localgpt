//! The song a world performs to.
//!
//! A [`SoundtrackDef`] carries the *analysis* the modulation signals are
//! derived from — tempo, beat offset, section boundaries, a per-second
//! loudness envelope, optional stem envelopes — and, optionally, the audio
//! file itself. The analysis is a few kilobytes of numbers, so a world can
//! perform in time without shipping the recording: a renderer drives
//! [`ModulationDef`](crate::ModulationDef)s from the curves whether or not
//! `path` is set, and plays the file only when it is.
//!
//! `path` is meant for audio the world may distribute (its own or CC0).
//! LocalGPT Verse exports a listener's personal library with `path: None`.

use serde::{Deserialize, Serialize};

/// The soundtrack and the analysis it was performed from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SoundtrackDef {
    /// Audio file relative to the world's `assets/` directory. `None`: no
    /// audio ships with the world; the curves below still drive modulations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Track title, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Artist, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// License of the audio (an SPDX id such as `CC0-1.0`, or free text).
    /// Set it whenever `path` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Track length in seconds.
    #[serde(default)]
    pub duration: f32,
    /// Beats per minute; `0` means no reliable beat grid.
    #[serde(default)]
    pub bpm: f32,
    /// Time of the first beat in seconds.
    #[serde(default)]
    pub beat_offset: f32,
    /// Section boundaries as fractions of `duration` in `0..=1`, ascending,
    /// starting with `0`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<f32>,
    /// Loudness envelope, one value per second, normalized to `0..=1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub energy: Vec<f32>,
    /// Per-stem envelopes aligned to `energy`, when the track was separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stems: Option<StemCurves>,
}

/// Per-second stem envelopes (`0..=1`), aligned to
/// [`SoundtrackDef::energy`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StemCurves {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drums: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bass: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vocals: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<f32>,
}

impl Default for SoundtrackDef {
    fn default() -> Self {
        Self {
            path: None,
            title: None,
            artist: None,
            license: None,
            duration: 0.0,
            bpm: 0.0,
            beat_offset: 0.0,
            sections: Vec::new(),
            energy: Vec::new(),
            stems: None,
        }
    }
}

/// Sample a per-second curve at `t` seconds with linear interpolation;
/// `0` when the curve is empty, held at the ends outside its span.
pub fn curve_at(curve: &[f32], t: f32) -> f32 {
    match curve.len() {
        0 => 0.0,
        1 => curve[0].clamp(0.0, 1.0),
        n => {
            let t = t.clamp(0.0, (n - 1) as f32);
            let i = t.floor() as usize;
            let f = t - i as f32;
            if i + 1 >= n {
                curve[n - 1].clamp(0.0, 1.0)
            } else {
                (curve[i] + (curve[i + 1] - curve[i]) * f).clamp(0.0, 1.0)
            }
        }
    }
}

impl SoundtrackDef {
    /// Loudness at `t` seconds.
    pub fn energy_at(&self, t: f32) -> f32 {
        curve_at(&self.energy, t)
    }

    /// Beat pulse at `t` seconds: `1` on a beat, decaying linearly to `0` at
    /// the next; `0` without a beat grid.
    pub fn beat_at(&self, t: f32) -> f32 {
        if self.bpm <= 0.0 || !self.bpm.is_finite() {
            return 0.0;
        }
        let period = 60.0 / self.bpm;
        let since = (t - self.beat_offset).rem_euclid(period);
        1.0 - since / period
    }

    /// Index of the section containing `t` seconds (`0` when there are no
    /// sections or no duration).
    pub fn section_at(&self, t: f32) -> usize {
        if self.duration <= 0.0 || self.sections.is_empty() {
            return 0;
        }
        let frac = (t / self.duration).clamp(0.0, 1.0);
        self.sections
            .iter()
            .rposition(|&start| start <= frac)
            .unwrap_or(0)
    }

    /// A stem's level at `t` seconds, falling back to `energy` when the
    /// track carries no such stem.
    pub fn stem_at(&self, stem: crate::StemKind, t: f32) -> f32 {
        let curve = self.stems.as_ref().map(|s| match stem {
            crate::StemKind::Drums => &s.drums,
            crate::StemKind::Bass => &s.bass,
            crate::StemKind::Vocals => &s.vocals,
            crate::StemKind::Other => &s.other,
        });
        match curve {
            Some(c) if !c.is_empty() => curve_at(c, t),
            _ => self.energy_at(t),
        }
    }

    /// Whether the numbers are finite and in range: `duration`, `bpm` and
    /// `beat_offset` non-negative, curves in `0..=1`, sections ascending
    /// within `0..=1`.
    pub fn is_valid(&self) -> bool {
        let non_negative = |v: f32| v.is_finite() && v >= 0.0;
        let unit = |c: &[f32]| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v));
        non_negative(self.duration)
            && non_negative(self.bpm)
            && non_negative(self.beat_offset)
            && unit(&self.energy)
            && unit(&self.sections)
            && self.sections.windows(2).all(|w| w[0] <= w[1])
            && self.stems.as_ref().is_none_or(|s| {
                unit(&s.drums) && unit(&s.bass) && unit(&s.vocals) && unit(&s.other)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> SoundtrackDef {
        SoundtrackDef {
            duration: 4.0,
            bpm: 120.0,
            beat_offset: 0.25,
            sections: vec![0.0, 0.5],
            energy: vec![0.0, 1.0, 0.5, 0.5],
            ..Default::default()
        }
    }

    #[test]
    fn curve_sampling() {
        assert_eq!(curve_at(&[], 3.0), 0.0);
        assert_eq!(curve_at(&[0.7], 3.0), 0.7);
        let c = [0.0, 1.0, 0.5];
        assert_eq!(curve_at(&c, 0.5), 0.5);
        assert_eq!(curve_at(&c, 1.5), 0.75);
        assert_eq!(curve_at(&c, 9.0), 0.5);
        assert_eq!(curve_at(&c, -1.0), 0.0);
        assert_eq!(curve_at(&[2.0, 2.0], 0.5), 1.0, "clamped to unit range");
    }

    #[test]
    fn beat_pulse() {
        let t = track();
        assert!((t.beat_at(0.25) - 1.0).abs() < 1e-6);
        assert!((t.beat_at(0.5) - 0.5).abs() < 1e-6);
        assert!((t.beat_at(0.75) - 1.0).abs() < 1e-6);
        assert!(
            t.beat_at(0.0) > 0.0 && t.beat_at(0.0) < 1.0,
            "before the offset wraps"
        );
        assert_eq!(SoundtrackDef::default().beat_at(1.0), 0.0);
    }

    #[test]
    fn sections_and_stems() {
        let mut t = track();
        assert_eq!(t.section_at(0.0), 0);
        assert_eq!(t.section_at(1.9), 0);
        assert_eq!(t.section_at(2.0), 1);
        assert_eq!(t.section_at(99.0), 1);
        assert_eq!(
            t.stem_at(crate::StemKind::Drums, 1.0),
            1.0,
            "falls back to energy"
        );
        t.stems = Some(StemCurves {
            drums: vec![0.2, 0.2, 0.2, 0.2],
            ..Default::default()
        });
        assert!((t.stem_at(crate::StemKind::Drums, 1.0) - 0.2).abs() < 1e-6);
        assert_eq!(
            t.stem_at(crate::StemKind::Vocals, 1.0),
            1.0,
            "empty stem falls back"
        );
    }

    #[test]
    fn validity() {
        assert!(track().is_valid());
        let mut bad = track();
        bad.energy[1] = 1.5;
        assert!(!bad.is_valid());
        let mut bad = track();
        bad.sections = vec![0.5, 0.2];
        assert!(!bad.is_valid());
        let mut bad = track();
        bad.bpm = f32::NAN;
        assert!(!bad.is_valid());
    }

    #[test]
    fn serde_omits_empties() {
        let t = SoundtrackDef {
            title: Some("Amber Drift".into()),
            license: Some("CC0-1.0".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(
            json,
            r#"{"title":"Amber Drift","license":"CC0-1.0","duration":0.0,"bpm":0.0,"beat_offset":0.0}"#
        );
        let back: SoundtrackDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
