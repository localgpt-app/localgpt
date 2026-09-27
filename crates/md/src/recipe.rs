//! LLM-authored region recipes (PLAN.md M1): how one section's region should
//! look — palette, landmark, props — as authored by the local model
//! ([`crate::llm`]) and cached per section hash ([`crate::sidecar`]).
//!
//! Pure (no Bevy, no model), and always compiled: the draft applies whatever
//! recipe it is handed, so the default (no-`llm`) build can still render a
//! cached sidecar exactly as the `llm` build would. This mirrors Verse's
//! split, where the recipe type is unconditionally compiled and only the
//! *authoring* is feature-gated.
//!
//! Safety never depends on the model: every field is optional (`None` = the
//! rule-derived value stands), [`RegionRecipe::clamped`] bounds the free
//! values, and a malformed reply fails [`RegionRecipe::from_llm_text`] so the
//! caller keeps the draft. That is why generation is plain instructed-JSON
//! with a lenient parse rather than mistral.rs's `generate_structured` — the
//! grammar-constrained path hangs on GGUF in mistral.rs 0.8 (verified in
//! Verse; see its ARCHITECTURE.md §10).

use serde::{Deserialize, Serialize};

/// How one section's region should look. Authored by the local LLM; applied
/// by [`crate::draft`] on top of the rule-derived draft.
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

    /// True when nothing is set — not worth caching.
    // Called by the `llm` tier; kept in the default build with the type.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Parse a recipe out of an LLM reply: the first balanced JSON object,
    /// tolerating code fences and surrounding prose. `None` when the reply
    /// holds no complete object (e.g. truncated) or it doesn't parse — the
    /// caller keeps the rule-derived draft. Ported from Verse's
    /// `extract_json_object` (Apache-2.0).
    // Called by the `llm` tier; compiled in the default build so its tests
    // (and any cached-sidecar tooling) run without the feature.
    #[allow(dead_code)]
    pub fn from_llm_text(text: &str) -> Option<Self> {
        let start = text.find('{')?;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (i, c) in text[start..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let json = &text[start..start + i + c.len_utf8()];
                        return serde_json::from_str(json).ok();
                    }
                }
                _ => {}
            }
        }
        None // unbalanced (truncated) or no object at all
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
    fn parses_plain_object() {
        let recipe = RegionRecipe::from_llm_text(
            r#"{"accent":[0.2,0.4,0.9],"props":{"kind":"spheres","count":9}}"#,
        )
        .unwrap();
        assert_eq!(recipe.accent, Some([0.2, 0.4, 0.9]));
        assert_eq!(
            recipe.props.map(|p| (p.kind, p.count)),
            Some((PropKind::Spheres, 9))
        );
        assert_eq!(recipe.landmark, None);
    }

    #[test]
    fn parses_through_prose_and_fences() {
        let reply = "Sure! Here is the JSON:\n```json\n{\"landmark\":{\"kind\":\"orb\",\"scale\":1.4,\"emissive\":0.2}}\n```\nEnjoy.";
        let recipe = RegionRecipe::from_llm_text(reply).unwrap();
        assert_eq!(
            recipe.landmark,
            Some(LandmarkSpec {
                kind: LandmarkKind::Orb,
                scale: 1.4,
                emissive: 0.2,
            })
        );
    }

    #[test]
    fn missing_and_partial_fields_default() {
        let recipe = RegionRecipe::from_llm_text("{\"landmark\":{\"scale\":2.0}}").unwrap();
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
    fn truncated_object_is_rejected() {
        assert!(RegionRecipe::from_llm_text("{\"accent\":[0.1,0.2").is_none());
        assert!(RegionRecipe::from_llm_text("no json here").is_none());
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
