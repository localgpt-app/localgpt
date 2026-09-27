//! The agent tier, now shared: the tool-call protocol and the pure
//! interpreter live in the `localgpt-world-agent` crate (extracted here in
//! M5). This module re-exports the two entry points the app's other modules
//! use: [`clamp_local`] (`draft.rs` applies it to cached and overridden
//! entities) and the `llm`-gated [`run_session`] (`generation.rs`).

pub use localgpt_world_agent::clamp_local;

#[cfg(feature = "llm")]
pub use localgpt_world_agent::run_session;
