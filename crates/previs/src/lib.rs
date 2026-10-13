//! `localgpt_previs` — script to board: a screenplay in (Fountain, or
//! Markdown script pages), per scene a `.world` package with
//! set/cast/coverage, a printable board and a shot-list CSV out.
//!
//! Deterministic only: no model, no network, no renderer — the draft is
//! a pure function of the screenplay text (same text, same bytes).
//!
//! Pipeline: [`markdown`] (a `.md` file's convention → Fountain, when the
//! pages are Markdown) → [`fountain`] (screenplay → scenes and elements) →
//! [`stage`] (a scene → set, cast and coverage as a `WorldManifest`,
//! cameras and shots riding `WorldEntity::extra` under
//! `ext-cinematography`) → [`shots`] (the shot list with the derived
//! lens data, and `shotlist.csv`) → [`package`] (a head-first `.world`
//! per scene) and [`board`] (`board.html`, the vendored Open World
//! Format viewer stepping through each shot's camera). [`draft`] runs
//! the whole pipeline in memory — what the CLI writes to disk and what
//! the previs page hands to the browser. [`cinema`] is the extension's
//! normative math — `openworldformat`'s module, through
//! `localgpt-world-types` — so every renderer of a camera entity
//! derives the same numbers.

pub mod board;
pub mod draft;
pub mod fountain;
pub mod markdown;
pub mod package;
pub mod shots;
pub mod stage;

pub use localgpt_world_types::cinematography as cinema;

pub use draft::Format;
pub use fountain::{Element, Script};
pub use shots::ShotRow;
pub use stage::{Scene, SceneKind, Speaker, TimeOfDay};
