//! Optional audio-analysis models for LocalGPT's agents.
//!
//! A coding agent reads text, not sound. These models turn a recording into
//! text it can read and files it can place: a song split into drums, bass,
//! vocals and other; the notes an instrument played; what a sound is, from a
//! fixed set of labels; how well audio matches a description. None of them
//! writes or chats: they analyse audio, on the CPU, in-process, through ONNX
//! Runtime.
//!
//! Nothing is installed by default. A person installs a model from the
//! desktop app or with `localgpt audio install <id>`; the files land under
//! the shared data directory ([`models_dir`]) and are kept only when their
//! SHA-256 matches the pinned one. An agent is offered a model's tool only
//! once the model is installed ([`tools::installed`]), and nothing touches the
//! network after the install.
//!
//! Without features the crate is the [`catalog`] and the [`install`]er, which
//! is all the desktop app needs. `run` adds the models themselves and `mcp`
//! their tools.

pub mod catalog;
pub mod install;

#[cfg(feature = "run")]
pub mod decode;
#[cfg(feature = "run")]
pub mod notes;
#[cfg(feature = "run")]
pub mod tags;

#[cfg(feature = "run")]
pub mod stems;

#[cfg(feature = "mcp")]
pub mod tools;

pub use catalog::{MODELS, Model, ModelFile, find};
pub use install::{Progress, install, installed, models_dir, remove};
