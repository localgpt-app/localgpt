//! `localgpt-world-audio` — the one mapping from the world format's audio to
//! sound.
//!
//! Audio was the last part of `localgpt-world-types` with no shared renderer.
//! The format carries it throughout — [`wt::AudioDef`] per entity,
//! `AmbienceLayerDef` and `AudioEmitterSpec` in a library, `SoundtrackDef` and
//! `ModulationDef` on a world — while `localgpt-world-bevy` carries none of it.
//! So Gen grew a FunDSP synthesis engine over its own cpal stream (ambience and
//! spatial emitters) and Verse grew a kira one (soundtrack playback and
//! modulation), and neither could play the other's half. Worlds has to play
//! both: a document world has ambience and emitters, a song world has a
//! soundtrack.
//!
//! **kira owns the device; FunDSP is a source.** They occupy different layers —
//! kira is a playback engine (mixer, clocks, tweens, streaming decode, spatial
//! tracks), FunDSP is a synthesis DSL — so the unification is one
//! `AudioManager` with the graphs feeding it through [`FundspSound`], not a
//! choice between the two libraries. What that retires is Gen's hand-rolled
//! three-thread cpal engine and its own distance attenuation, not its graphs.
//!
//! ```text
//!   wt::AudioSource ──graphs::build──> Box<dyn AudioUnit> ──FundspSound──> kira
//!   wt::AudioSource::File ─────────────────────────────────────────────> kira
//! ```
//!
//! [`graphs`] is pure synthesis and needs no device, so it is testable without
//! one; that is where the conformance of a sound to the format lives.

pub mod engine;
pub mod graphs;
pub mod source;

pub use engine::Engine;
pub use graphs::{build, infer_emitter_from_name};
pub use source::FundspSound;

/// Re-exported so callers need not depend on `localgpt-world-types` by name
/// just to spell an audio type.
pub use localgpt_world_types as wt;
