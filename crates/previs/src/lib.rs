//! `localgpt_previs` — script to board: a Fountain screenplay in, per
//! scene a `.world` package with set/cast/coverage, a printable board
//! and a shot-list CSV out.
//!
//! Deterministic only: no model, no network, no renderer — the draft is
//! a pure function of the screenplay text (same text, same bytes).
//!
//! Pipeline: [`fountain`] (screenplay → scenes and elements) →
//! [`stage`] (a scene → set, cast and coverage as a `WorldManifest`,
//! cameras and shots riding `WorldEntity::extra` under
//! `ext-cinematography`) → [`shots`] (the shot list with the derived
//! lens data, and `shotlist.csv`) → [`package`] (a head-first `.world`
//! per scene) and [`board`] (`board.html`, the vendored Open World
//! Format viewer stepping through each shot's camera). [`cinema`] is
//! the extension's normative math, re-exported from
//! `localgpt-world-types` (where it lives until `openworldformat`
//! ships a typed cinematography module) so every renderer of a camera
//! entity derives the same numbers.

pub mod board;
pub mod fountain;
pub mod package;
pub mod shots;
pub mod stage;

pub use localgpt_world_types::cinematography as cinema;

pub use fountain::{Element, Script};
pub use shots::ShotRow;
pub use stage::{Scene, SceneKind, Speaker, TimeOfDay};
