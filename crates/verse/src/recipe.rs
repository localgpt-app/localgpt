//! LLM-authored scene recipe — PLAN.md M7 (`llm` feature).
//!
//! Where [`crate::theme::Theme`] picks *which* of the four built-in worlds a
//! track lives in, a `WorldRecipe` describes how to dress that world up: how
//! dense, how fast, how glowy, what landmarks to raise, how the detected
//! sections should choreograph, and what particles drift through it. It is the
//! top rung of the signal-ownership ladder
//!
//! ```text
//! rule mapper (map_mood) → CLAP zero-shot mood (ml) → WorldRecipe (llm)
//! ```
//!
//! and like every rung above it, it degrades gracefully: when no recipe is
//! present (no `llm` feature, no model fetched, or generation failed) the
//! renderer keeps today's mood-only behaviour verbatim — see [`WorldRecipe::default`].
//!
//! The recipe is filled by an LLM via constrained generation
//! (`mistralrs::Model::generate_structured`), so the [`schemars::JsonSchema`]
//! derive (compiled only under `feature = "llm"`) *is* the grammar: the model
//! literally cannot emit a field with a wrong type or out-of-range value.
//! Doc comments on each field become the schema descriptions the model reads,
//! so they are written for it.
//!
//! Design rule (idea.md / PLAN §0.3): the recipe **modulates within a mood** —
//! it never replaces the base palette, which stays owned by [`crate::theme`].
//! That keeps the Comfort gates (2 Hz flash cap, gentler-motion), the
//! equal-power crossfade, and the pinned-world logic untouched.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

/// The currently playing track's recipe, as a live resource the renderer reads.
/// Mirrors [`crate::theme::Theme`]'s role for mood: set by `sync_analysis` when
/// a track's analysis lands, cleared when the track has no recipe (or the `llm`
/// feature is off). `None` everywhere is the rule-derived path — today's
/// behaviour — so the renderer never has to special-case the recipe's absence.
#[derive(Resource, Default)]
pub struct ActiveRecipe {
    /// The recipe for the track currently playing, already
    /// [`WorldRecipe::clamped`]. `None` = no recipe (use the rule-derived path).
    pub recipe: Option<WorldRecipe>,
    /// The recipe's [`SectionMoment`]s resolved to measured section indices —
    /// `(segment index, moment)` pairs, computed by `sync_analysis` (which sees
    /// the full analysis, not just the recipe). The world's
    /// `sync_section_moment` system keys off these; empty = no choreography.
    pub moments: Vec<(usize, SectionMoment)>,
}

impl ActiveRecipe {
    /// Borrow the active recipe, if any.
    pub fn get(&self) -> Option<&WorldRecipe> {
        self.recipe.as_ref()
    }
} // `schemars::JsonSchema` is only needed to constrain LLM generation, so it is
// derived solely under the `llm` feature — the default (rules-only) build stays
// free of the proc-macro dependency. analysis.rs / world.rs see only
// Serialize + Deserialize.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldRecipe {
    /// A free-form name for this world (e.g. "Velvet Megacity Ruins"). Shown in
    /// the HUD title slot (capped to 40 chars there); empty = the mood's name.
    pub world_name: String,

    /// One or more zones that compose the world. The first biome's `mood`
    /// must match the track's detected mood (0=Ember, 1=Velvet, 2=Tide,
    /// 3=Glass); further biomes are contrasting accents the renderer blends.
    pub biomes: Vec<Biome>,

    /// Hero structures raised from the CC0 asset set (see `world_assets`).
    /// Placed at notable positions (centre / cardinal points / horizon rim).
    /// Empty = no landmarks; the drifters carry the scene as today.
    pub landmarks: Vec<Landmark>,

    /// Atmosphere: fog, ambient tint, and the bloom/emissive ceiling. All
    /// values are Comfort-gated by the renderer (never exceeding the flash cap).
    pub atmosphere: Atmosphere,

    /// How each detected song section should shift the world (energy,
    /// palette wash, motion). Entries snap to the nearest measured boundary;
    /// the renderer maps `at_role` to sections by position. Empty = the world
    /// holds steady through the whole track (current behaviour).
    pub section_choreography: Vec<SectionMoment>,

    /// Ambient drifting particles (embers, dust, snow, sparks). `None`/Dust
    /// with rate 0 = no particle layer.
    pub particles: Particles,

    /// Overall motion speed multiplier for drifters and particles (1.0 =
    /// today's default). Clamped to 0.25..2.5 by the renderer; high energy
    /// tracks nudge it up, calm tracks pull it down.
    pub motion_speed: f32,

    /// Density multiplier for prop/scatter placement (1.0 = today's count).
    /// Clamped to 0.3..2.0; very dense worlds are capped by the perf budget
    /// (PLAN.md stress notes: ~70–90 props is the 60 fps sweet spot).
    pub density: f32,

    /// Layout RNG seed. Same recipe + seed → same placement; "Build a different
    /// world" re-rolls it. `0` = derive per-track (see `sync_analysis`); a pin
    /// always wins over the recipe's seed.
    pub seed: u64,
}

/// A single zone within the world.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Biome {
    /// Index into [`crate::theme::moods()`] (0=Ember Flats, 1=Velvet Circuit,
    /// 2=Tide Gardens, 3=Glass Expanse). The primary biome must equal the
    /// detected mood; secondaries may differ for contrast.
    pub mood: usize,

    /// Placement rule for this biome's props.
    pub layout: LayoutStyle,

    /// 0..1 share of the world's prop budget this biome claims. The renderer
    /// normalizes so all biomes sum to 1.0.
    pub density: f32,

    /// RGB tint (0..1 each) blended onto the biome's base palette. Subtle
    /// shifts read better than strong ones; the renderer mixes at <=40%.
    pub tint: [f32; 3],
}

/// How props are arranged on the ground.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LayoutStyle {
    /// Organic golden-angle spiral (the default; clears the centre). Best for
    /// natural / ambient moods.
    Spiral,
    /// Jittered city grid — regular blocks with small random offset. Best for
    /// driving / electronic moods (Velvet Circuit).
    Grid,
    /// Concentric rings around the centre. Best for bright / crystalline
    /// moods (Glass Expanse).
    Rings,
}

impl LayoutStyle {
    /// The renderer-side arrangement this style maps onto. The recipe's
    /// primary biome overrides the mood's default arrangement with this.
    pub fn arrangement(self) -> crate::theme::Arrangement {
        match self {
            LayoutStyle::Spiral => crate::theme::Arrangement::Spiral,
            LayoutStyle::Grid => crate::theme::Arrangement::Grid,
            LayoutStyle::Rings => crate::theme::Arrangement::Rings,
        }
    }
}

/// A hero landmark raised from the asset set.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Landmark {
    /// Semantic kind; the renderer maps this to the closest matching CC0 asset
    /// of tier Hero in the landmark's biome mood.
    pub kind: LandmarkKind,

    /// Where to place it.
    pub at: Anchor,

    /// Scale multiplier vs the asset's normalized size (1.0 = default hero
    /// span). Clamped to 0.5..3.0.
    pub scale: f32,

    /// Emissive intensity 0..1 — how much it glows. Comfort-gated.
    pub emissive: f32,
}

/// What sort of hero structure to raise.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LandmarkKind {
    /// A tall vertical structure (tower, spire, monolith).
    Spire,
    /// A frame or portal the camera can pass through.
    Gateway,
    /// A broad solid mass (rock, ruin, mesa).
    Mass,
    /// An abstract centrepiece (crystal cluster, sculpture).
    Monument,
}

/// Where a landmark sits.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    /// Dead centre — the camera's focus. Use sparingly (one per world).
    Center,
    /// One of the four cardinal points on the rim.
    Cardinal,
    /// On the far horizon, defining the skyline.
    Rim,
}

/// Atmosphere settings.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Atmosphere {
    /// Fog thickness 0..1 (0 = today's default, 1 = dense wall). Adjusts the
    /// DistanceFog start/end.
    pub fog_density: f32,

    /// RGB ambient light tint (0..1 each). Mixed onto the mood's ambient.
    pub ambient_tint: [f32; 3],

    /// Bloom / emissive ceiling 0..1 — caps how bright glow can get. Lower =
    /// subdued; higher = luminous. Comfort-gated.
    pub bloom_ceiling: f32,
}

/// A choreographed moment synced to a song section.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionMoment {
    /// Which section role this moment targets. The renderer maps roles to the
    /// measured sections by position (intro ≈ first, chorus/drop ≈ the
    /// highest-energy middle section, etc.).
    pub at_role: SectionRole,

    /// Energy shift applied for this section, -1..1 (negative = calm it,
    /// positive = intensify). Added to the live energy envelope.
    pub energy_shift: f32,

    /// If true, run a palette wash (a brief colour transition) on entering
    /// this section — mirrors the crossfade palette wash.
    pub palette_wash: bool,

    /// Motion intent for this section.
    pub motion: Motion,
}

/// A song section's musical role (heuristic — the renderer infers from
/// position). One definition with the other LocalGPT apps: re-exported from
/// the shared `localgpt-world-agent` crate (M5), where the agent protocol's
/// `at_role` fields use it.
pub use localgpt_world_agent::SectionRole;

/// Motion character for a section.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    /// Still / barely moving.
    Calm,
    /// Normal continuous drift (today's default).
    Drift,
    /// Energised, faster motion.
    Active,
}

/// Ambient particle layer.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Particles {
    /// What kind of particle to drift.
    pub kind: ParticleKind,
    /// Emission rate 0..1 (0 = none). Clamped; high values are capped by perf.
    pub rate: f32,
    /// Upward drift speed multiplier (embers rise; snow falls = negative feel
    /// via the kind). 1.0 = default.
    pub drift: f32,
}

/// Particle visual.
#[cfg_attr(feature = "llm", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParticleKind {
    /// Neutral fine dust (the default — reads as air).
    Dust,
    /// Warm rising embers (Ember Flats).
    Ember,
    /// Cool falling motes / snow (Glass Expanse).
    Snow,
    /// Bright sparks (Velvet Circuit).
    Spark,
    /// Soft floating spores / pollen (Tide Gardens).
    Spore,
}

// ---------------------------------------------------------------------------
// Defaults — every field defaults to "today's behaviour" so a missing or
// partial recipe is a no-op. This is the graceful-fallback contract.
// ---------------------------------------------------------------------------

impl Default for WorldRecipe {
    fn default() -> Self {
        Self {
            world_name: String::new(),
            biomes: Vec::new(),
            landmarks: Vec::new(),
            atmosphere: Atmosphere::default(),
            section_choreography: Vec::new(),
            particles: Particles::default(),
            motion_speed: 1.0,
            density: 1.0,
            seed: 0,
        }
    }
}

impl Default for Biome {
    fn default() -> Self {
        Self {
            mood: 0,
            layout: LayoutStyle::Spiral,
            density: 1.0,
            tint: [0.5, 0.5, 0.5],
        }
    }
}

impl Default for Landmark {
    fn default() -> Self {
        Self {
            kind: LandmarkKind::Monument,
            at: Anchor::Cardinal,
            scale: 1.0,
            emissive: 0.0,
        }
    }
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            fog_density: 0.0,
            ambient_tint: [0.5, 0.5, 0.5],
            bloom_ceiling: 1.0,
        }
    }
}

impl Default for SectionMoment {
    fn default() -> Self {
        Self {
            at_role: SectionRole::Verse,
            energy_shift: 0.0,
            palette_wash: false,
            motion: Motion::Drift,
        }
    }
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            kind: ParticleKind::Dust,
            rate: 0.0,
            drift: 1.0,
        }
    }
}

impl WorldRecipe {
    /// `true` when the recipe carries no actual modulation — i.e. it is the
    /// default/empty recipe and the renderer should take today's path. Used by
    /// the upcoming richer renderer tiers; the current path keys off
    /// `ActiveRecipe::get()` returning `None` instead.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.world_name.is_empty()
            && self.biomes.is_empty()
            && self.landmarks.is_empty()
            && self.section_choreography.is_empty()
            && self.particles.rate == 0.0
            && self.atmosphere == Atmosphere::default()
            && (self.motion_speed - 1.0).abs() < 1e-4
            && (self.density - 1.0).abs() < 1e-4
    }

    /// Clamp every free field into its renderer-safe range. Called on any
    /// recipe before use (whether from the LLM or a sidecar) so a malformed
    /// value can never blow up the Comfort gates or the perf budget. Only the
    /// `llm` tier calls it today (the schema's defaults are already in-range),
    /// so it is dead code without `feature = "llm"`.
    #[allow(dead_code)]
    pub fn clamped(mut self) -> Self {
        self.motion_speed = self.motion_speed.clamp(0.25, 2.5);
        self.density = self.density.clamp(0.3, 2.0);
        for b in &mut self.biomes {
            b.mood %= 4;
            b.density = b.density.clamp(0.0, 1.0);
            for c in &mut b.tint {
                *c = (*c).clamp(0.0, 1.0);
            }
        }
        for l in &mut self.landmarks {
            l.scale = l.scale.clamp(0.5, 3.0);
            l.emissive = l.emissive.clamp(0.0, 1.0);
        }
        self.atmosphere.fog_density = self.atmosphere.fog_density.clamp(0.0, 1.0);
        self.atmosphere.bloom_ceiling = self.atmosphere.bloom_ceiling.clamp(0.0, 1.0);
        for c in &mut self.atmosphere.ambient_tint {
            *c = (*c).clamp(0.0, 1.0);
        }
        for m in &mut self.section_choreography {
            m.energy_shift = m.energy_shift.clamp(-1.0, 1.0);
        }
        self.particles.rate = self.particles.rate.clamp(0.0, 1.0);
        self
    }
}

impl PartialEq for Atmosphere {
    fn eq(&self, other: &Self) -> bool {
        self.fog_density == other.fog_density
            && self.bloom_ceiling == other.bloom_ceiling
            && self.ambient_tint == other.ambient_tint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_recipe_is_empty() {
        assert!(WorldRecipe::default().is_empty());
    }

    #[test]
    fn clamped_recipe_keeps_valid_values() {
        let r = WorldRecipe {
            motion_speed: 5.0,
            density: -1.0,
            ..Default::default()
        }
        .clamped();
        assert_eq!(r.motion_speed, 2.5);
        assert_eq!(r.density, 0.3);
    }

    #[test]
    fn clamped_recipe_fixes_biome_mood_and_tints() {
        let r = WorldRecipe {
            biomes: vec![Biome {
                mood: 9,
                tint: [-0.5, 2.0, 0.3],
                ..Default::default()
            }],
            ..Default::default()
        }
        .clamped();
        assert_eq!(r.biomes[0].mood, 1); // 9 % 4
        assert_eq!(r.biomes[0].tint, [0.0, 1.0, 0.3]);
    }

    #[test]
    fn sidecar_roundtrip_preserves_recipe() {
        let r = WorldRecipe {
            world_name: "Test World".into(),
            motion_speed: 1.5,
            landmarks: vec![Landmark {
                kind: LandmarkKind::Spire,
                at: Anchor::Center,
                scale: 2.0,
                emissive: 0.6,
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: WorldRecipe = serde_json::from_str(&json).unwrap();
        assert_eq!(back.world_name, "Test World");
        assert_eq!(back.landmarks.len(), 1);
        assert_eq!(back.landmarks[0].kind, LandmarkKind::Spire);
    }

    /// A sidecar written *before* recipes existed (no recipe field) must still
    /// deserialize — the recipe simply reads as `None` (PLAN §41 contract).
    /// This test lives here to keep the contract visible at the schema.
    #[test]
    fn missing_recipe_field_is_none_at_caller() {
        // Simulates an old sidecar: the TrackAnalysis struct carries
        // `recipe: Option<WorldRecipe>` with #[serde(default)]; a JSON object
        // lacking the key deserializes to None.
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            recipe: Option<WorldRecipe>,
        }
        let old = serde_json::json!({ "version": 2, "bpm": 120.0 });
        let w: Wrap = serde_json::from_value(old).unwrap();
        assert!(w.recipe.is_none());
    }
}
