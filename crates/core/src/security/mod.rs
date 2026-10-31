//! Centralized security module for LocalGPT.
//!
//! See [`localgpt`] for the module overview, architecture diagram,
//! and public API documentation.

mod localgpt;
mod policy;
mod suffix;

// The localgpt.rs facade controls the entire public API surface.
pub use self::localgpt::*;
