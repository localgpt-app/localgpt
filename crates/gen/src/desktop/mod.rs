//! What a windowed launch needs that a terminal launch has already: the
//! login shell's `PATH`.
//!
//! - [`shell_env`]: recovers the login shell's `PATH`, which apps launched
//!   from Finder don't inherit, so programs installed by Homebrew or npm
//!   are found.

pub mod shell_env;
