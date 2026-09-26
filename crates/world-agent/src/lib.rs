//! `localgpt-world-agent` — the shared world agent runtime.
//!
//! The LLM-authored side of the LocalGPT world format: a tool-call
//! **protocol** ([`interpreter::AgentCommand`] and its `*Cmd` structs) and a
//! pure, in-process **interpreter** ([`interpreter::SceneInterpreter`]) that
//! applies those commands to `localgpt-world-types` entities — plus the
//! asset-pack manifest types and deterministic scatter math
//! ([`assets`]) the `place_asset`/`scatter_field` commands speak.
//!
//! This crate is **Bevy-free** (like `localgpt-world-types`): apps that
//! drive a live scene (Verse) map the same commands onto their own
//! executor, while apps that compile to a manifest (MD) feed the
//! interpreter's entities straight into a `WorldManifest`. The
//! [`interpreter`] and [`assets`] are always compiled so a cached agent
//! build is renderable without a model.
//!
//! The mistral.rs tool-calling loop ([`interpreter::run_session`]) is behind
//! the **`llm`** feature (model loading stays app-side); **`llm-metal`**
//! adds the Apple-Silicon GPU path. Grammar-constrained generation is never
//! used (it hangs on GGUF with mistral.rs 0.8).

pub mod assets;
pub mod interpreter;

pub use assets::{
    AssetEntry, AssetManifest, Tier, fold_seed, mesh_path, read_manifest_at, resolve_kind,
    scatter_offsets,
};
pub use interpreter::{
    AgentCommand, AgentResponse, BuildOutput, ModifyEntityCmd, PlaceAssetCmd, PrimitiveShape,
    ScatterFieldCmd, SceneInterpreter, SetLightCmd, SpawnPrimitiveCmd, clamp_local,
    parse_tool_call, resolve_agent_assets,
};

#[cfg(feature = "llm")]
pub use interpreter::run_session;
