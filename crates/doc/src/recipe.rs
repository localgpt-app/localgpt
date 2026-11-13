//! Region recipes (PLAN.md M1): how one section's region should look —
//! palette, landmark, props — as an agent writes it into the sidecar
//! ([`crate::sidecar`]), keyed by the section's hash.
//!
//! Pure (no Bevy, no model): the draft applies whatever recipe it is handed.
//!
//! Safety never depends on the author: every field is optional (`None` =
//! the rule-derived value stands) and [`RegionRecipe::clamped`] bounds the
//! free values on sidecar load, so a malformed recipe can at worst be
//! ignored, never break the draft.

use serde::{Deserialize, Serialize};

/// How one section's region should look. Authored into the sidecar by an
/// agent; applied by [`crate::draft`] on top of the rule-derived draft.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionRecipe {
    /// Signature colour (sRGB, 0..1) for the landmark, lamp, and platform
    /// tint. `None` = derive from the section hash.
    pub accent: Option<[f32; 3]>,
    /// Ground strip tint (sRGB, 0..1). `None` = the shared ground colour.
    pub ground: Option<[f32; 3]>,
    pub landmark: Option<LandmarkSpec>,
    pub props: Option<PropSpec>,
}

/// The region's centrepiece.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandmarkSpec {
    pub kind: LandmarkKind,
    /// Multiplier on the rule-derived height (a function of prose length).
    pub scale: f32,
    /// Emissive glow strength.
    pub emissive: f32,
}

impl Default for LandmarkSpec {
    fn default() -> Self {
        Self {
            kind: LandmarkKind::Pyramid,
            scale: 1.0,
            emissive: 0.6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkKind {
    #[default]
    Pyramid,
    Cone,
    Column,
    Cube,
    Orb,
    Ring,
}

/// The ring of small shapes on the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PropSpec {
    pub kind: PropKind,
    pub count: u32,
}

impl Default for PropSpec {
    fn default() -> Self {
        Self {
            kind: PropKind::Blocks,
            count: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    #[default]
    Blocks,
    Spheres,
    Crystals,
}

impl RegionRecipe {
    /// Clamp model-authored values into the renderer's safe ranges. Called on
    /// everything that enters a [`crate::sidecar::RecipeStore`], so no
    /// out-of-range value can reach the world even from a hand-edited
    /// sidecar.
    pub fn clamped(mut self) -> Self {
        self.accent = self.accent.map(clamp_rgb);
        self.ground = self.ground.map(clamp_rgb);
        if let Some(landmark) = &mut self.landmark {
            landmark.scale = finite(landmark.scale).clamp(0.5, 2.5);
            landmark.emissive = finite(landmark.emissive).clamp(0.0, 1.0);
        }
        if let Some(props) = &mut self.props {
            props.count = props.count.clamp(3, 12);
        }
        self
    }

    /// True when nothing is set — not worth keeping in a sidecar.
    // Read by tests and by tooling that writes sidecars; nothing in Document
    // itself authors a recipe.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn clamp_rgb(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(|c| finite(c).clamp(0.0, 1.0))
}

/// NaN and infinities become a mid grey rather than poisoning the scene.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.5 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_object_with_defaults_for_what_is_missing() {
        let recipe: RegionRecipe = serde_json::from_str(
            r#"{"accent":[0.2,0.4,0.9],"props":{"kind":"spheres","count":9}}"#,
        )
        .unwrap();
        assert_eq!(recipe.accent, Some([0.2, 0.4, 0.9]));
        assert_eq!(
            recipe.props.map(|p| (p.kind, p.count)),
            Some((PropKind::Spheres, 9))
        );
        assert_eq!(recipe.landmark, None);

        let recipe: RegionRecipe = serde_json::from_str("{\"landmark\":{\"scale\":2.0}}").unwrap();
        assert_eq!(
            recipe.landmark,
            Some(LandmarkSpec {
                kind: LandmarkKind::Pyramid,
                scale: 2.0,
                emissive: 0.6,
            })
        );
    }

    #[test]
    fn clamped_bounds_everything() {
        let recipe = RegionRecipe {
            accent: Some([-3.0, 0.5, 9.0]),
            ground: Some([f32::NAN, 0.2, 0.2]),
            landmark: Some(LandmarkSpec {
                kind: LandmarkKind::Ring,
                scale: 50.0,
                emissive: -1.0,
            }),
            props: Some(PropSpec {
                kind: PropKind::Crystals,
                count: 99,
            }),
        }
        .clamped();
        assert_eq!(recipe.accent, Some([0.0, 0.5, 1.0]));
        assert_eq!(recipe.ground, Some([0.5, 0.2, 0.2]));
        assert_eq!(
            recipe.landmark.map(|l| (l.scale, l.emissive)),
            Some((2.5, 0.0))
        );
        assert_eq!(recipe.props.map(|p| p.count), Some(12));
    }

    #[test]
    fn is_empty_detects_the_default() {
        assert!(RegionRecipe::default().is_empty());
        assert!(
            !RegionRecipe {
                ground: Some([0.1; 3]),
                ..Default::default()
            }
            .is_empty()
        );
    }
}
