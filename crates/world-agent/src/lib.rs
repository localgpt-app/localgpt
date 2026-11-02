//! `localgpt-world-agent` — a `.world`'s authority, and what agents reach
//! it through.
//!
//! - [`live`]: the live authority an app holds over an open package, which
//!   agents change from outside through batches of ops.
//! - [`headless`]: the same authority for scripts, CI and the terminal —
//!   the logic behind `localgpt world`.
//! - [`session`]: `.world` packages on disk and in their transport form.
//! - [`starter`]: the worlds a fresh install opens.
//! - [`agent_env`]: the environment variables that tell an agent in a shell
//!   which world is open and how to reach it.
//! - [`paths`]: the app directories and the shared asset pack, resolved by
//!   one XDG rule for every app.
//!
//! The agent-facing tool surface — the [`mcp::Tool`] trait and the MCP
//! servers that serve a tool set to any MCP client — is behind **`mcp`**
//! (**`mcp-http`** adds the HTTP transport). It is what the shim
//! (`localgpt world mcp`) and Gen's tool server stand on.
//!
//! This crate is **Bevy-free** (like `localgpt-world-types`), and it runs
//! no model: the agents come from their makers, and reach a world over MCP
//! or its API.

pub mod agent_env;
pub mod headless;
pub mod live;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod paths;
pub mod session;
pub mod starter;

pub use paths::world_pack_dir;
