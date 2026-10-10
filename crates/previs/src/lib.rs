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
//! the extension's normative math, implemented locally until
//! `openworldformat` ships a typed cinematography module.

pub mod board;
pub mod cinema;
pub mod fountain;
pub mod package;
pub mod shots;
pub mod stage;

pub use fountain::{Element, Script};
pub use shots::ShotRow;
pub use stage::{Scene, SceneKind, Speaker, TimeOfDay};
