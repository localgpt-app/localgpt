//! Agent scene-construction types. The **data half** — the command
//! protocol, cmd structs, `SceneBuild`, `AgentResponse`, and the serde
//! defaults — lives in the shared `localgpt-world-agent` crate (M5) and is
//! re-exported here, so MD, Verse, and any future app speak one protocol
//! and one sidecar shape. What stays in this file is Verse's alone: the
//! Bevy markers its live executor uses for track scoping, section reveal,
//! and the environment override.
//!
//! Keeping the markers ungated means ungated code (`world.rs`) can exclude
//! agent-owned entities from its queries even in builds that never spawn
//! them. The feature-gated runtime (mistral.rs tool-calling loop, the Bevy
//! executor) lives in [`crate::agent`].

// The re-exported surface is only fully exercised under `feature = "llm"`;
// dead-code/unused-import warnings in other configurations are expected and
// allowed.
#![allow(dead_code)]
#![allow(unused_imports)]

use bevy::prelude::{Color, Component, Resource};

pub use localgpt_world_agent::interpreter::{
    AgentCommand, AgentResponse, BuildOutput, EnvironmentCmd, ModifyEntityCmd, PlaceAssetCmd,
    PrimitiveShape, ScatterFieldCmd, SceneBuild, SectionRole, SetLightCmd, SpawnPrimitiveCmd,
    parse_tool_call,
};

// ---------------------------------------------------------------------------
// Track scoping — agent entities belong to the track they were authored for
// ---------------------------------------------------------------------------

/// Marker for every agent-spawned entity, carrying its stable name and the
/// content-hash id of the track whose session spawned it. Entities spawned
/// while a *lookahead* track is being analyzed start hidden and are revealed
/// only when their track becomes current (`sync_agent_scene_scope`), so an
/// ahead-of-playback agent session never pops into the world mid-song.
#[derive(Component)]
pub struct AgentEntity {
    pub name: String,
    pub track: String,
}

/// An agent entity that belongs to a *section* of its track (the agent's
/// `at_role` placement timing): revealed when the transport is in that
/// section, hidden otherwise — structures that rise on the drop or appear
/// only for the bridge.
#[derive(Component)]
pub struct SectionScoped {
    pub role: SectionRole,
    pub track: String,
}

/// Marker for an agent-owned light, with its authored intensity so it can be
/// zeroed while its track is not current and restored when it is.
#[derive(Component)]
pub struct AgentLight {
    pub name: String,
    pub track: String,
    /// The pre-Comfort, pre-scope intensity (`illuminance` for directional).
    pub base_intensity: f32,
}

/// Marker for the agent's single ambient-light entity (spawned once by
/// `set_environment`, updated by later calls). Excluded from `world.rs`'s
/// palette wash, which owns the world's own ambient.
#[derive(Component)]
pub struct AgentAmbient {
    pub track: String,
    /// The authored brightness, zeroed while the track is not current.
    pub base_brightness: f32,
}

/// The agent's background-colour override, if its track is current. Written
/// by the agent executor (via `set_environment`), read by `world.rs`'s
/// palette wash, which falls back to the mood's sky when `None`. A resource
/// rather than a direct `ClearColor` write so the two writers never fight.
#[derive(Resource, Default)]
pub struct EnvOverride {
    pub background: Option<Color>,
}
