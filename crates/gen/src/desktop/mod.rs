//! What Gen's agent loop shares with a window that has no terminal: the
//! channels it reports its work on, the models this machine can use, and
//! the login shell's `PATH`. The window's own interface is the app's web
//! page (`crates/app/ui`); Gen draws no panels.
//!
//! - [`chat`]: channels between the agent thread and whatever shows its
//!   work, and the events the agent loop reports (streamed text, tool
//!   calls, errors).
//! - [`models`]: which models this machine can switch to (installed CLI
//!   backends, local Ollama models).
//! - [`shell_env`]: recovers the login shell's `PATH`, which apps launched
//!   from Finder don't inherit, so CLI backends like `claude` are found.

pub mod chat;
pub mod hardware;
pub mod models;
pub mod shell_env;

pub use chat::{AgentChannels, ChatEvent, ChatSink, PanelChannels, create_chat_channels};
