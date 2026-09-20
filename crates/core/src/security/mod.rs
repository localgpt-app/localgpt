//! Centralized security module for LocalGPT.
//!
//! See [`localgpt`] for the module overview, architecture diagram,
//! and public API documentation.

mod device_key;
mod localgpt;
mod policy;
mod protected_files;
mod suffix;

// The localgpt.rs facade controls the entire public API surface.
pub use self::localgpt::*;
