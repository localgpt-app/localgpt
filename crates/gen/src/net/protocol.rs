//! Shared network protocol for the Phase-1 collaborative session.
//!
//! The wire model is `localgpt-world-types` data — the same serde vocabulary
//! used by world save/load and the SpacetimeDB tier (§2 of the umbrella spec)
//! — wrapped in Bevy components that lightyear replicates. Hosts decompose
//! live ECS state into these components; clients compose them back into
//! visuals. Nothing on the wire references Bevy entity ids: entities are
//! identified by the host-assigned stable [`NetWorldId`].

use bevy::math::curve::{Ease, FunctionCurve, Interval};
use bevy::prelude::*;
use bevy_replicon::prelude::RuleFns;
use bevy_replicon::shared::replication::registry::ctx::{SerializeCtx, WriteCtx};
use bytes::{Buf as _, Bytes};
use lightyear::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use localgpt_world_types as wt;

use super::interest::ChunkSummary;
use super::jobs::{JobId, JobState};

/// Stable host-assigned entity id (mirrors `wt::EntityId`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetWorldId(pub u64);

/// Human-readable entity name (unique within a session).
#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetName(pub String);

/// Which gen entity kind this is — tells clients how to render it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetEntityKind(pub NetKind);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetKind {
    Primitive,
    Light,
    Mesh,
    Group,
    AudioEmitter,
}

impl NetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Primitive => "primitive",
            Self::Light => "light",
            Self::Mesh => "mesh",
            Self::Group => "group",
            Self::AudioEmitter => "audio_emitter",
        }
    }
}

/// Entity transform (local/parent-relative, matching `wt::WorldTransform`).
///
/// Interpolated on clients between replicated states.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetTransform(pub wt::WorldTransform);

impl Ease for NetTransform {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| {
            NetTransform(lerp_transform(&start.0, &end.0, t))
        })
    }
}

/// Parametric shape (dimensions survive the wire exactly).
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetShape(pub wt::Shape);

/// PBR material properties.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetMaterial(pub wt::MaterialDef);

/// Light source definition (may coexist with a shape: glowing orb).
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetLight(pub wt::LightDef);

/// Behavior stack — declarative, so clients can replay them locally later.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetBehaviors(pub Vec<wt::BehaviorDef>);

/// Procedural audio definition (clients render nothing for it in Phase 1).
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetAudio(pub wt::AudioDef);

/// Imported mesh reference (Phase-1 clients show a placeholder).
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetMeshRef(pub wt::MeshAssetRef);

/// Parent entity, referenced by stable world id (resolved client-side after
/// both halves of the relationship have arrived).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetParentId(pub u64);

/// Authoritative placeholder for a prompt that is queued or being built
/// (§2 scaffold-then-replace). Rides its own replicated entity together with
/// a [`NetTransform`] at the prompt's anchor; despawned when the job ends.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetScaffold {
    pub job_id: JobId,
    pub request_id: u64,
    pub prompt: String,
    /// `false` while queued, `true` once a worker picked it up.
    pub running: bool,
}

/// Coarse per-chunk summary (HLOD impostor), replicated to every client so
/// chunks outside a client's view window still read as mass on the horizon.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetChunkSummary(pub ChunkSummary);

/// Content address of a custom mesh's geometry (§2 asset streaming): the
/// client fetches the blob from the host's asset server on demand.
#[derive(Component, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetMeshAsset {
    /// Hex SHA-256 of the `LGM1` blob.
    pub digest: String,
    /// Blob size in bytes (for progress / budgeting).
    pub bytes: u32,
}

/// Session-level metadata carried by a dedicated singleton entity.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetWorldMeta {
    pub name: String,
    pub environment: wt::EnvironmentDef,
    /// TCP port of the host's content-addressed asset server, if running.
    #[serde(default)]
    pub asset_port: Option<u16>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Natural-language prompt sent by a client to the host's agent.
///
/// (lightyear blanket-implements `Message` for serde types.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientPrompt {
    pub text: String,
    /// Client-chosen correlation id, echoed in [`JobStatus`] and
    /// [`NetScaffold`] so the requester can hand its local scaffold over to
    /// the replicated one.
    pub request_id: u64,
    /// World-space point the client was looking at when it prompted.
    pub anchor: Option<[f32; 3]>,
}

/// Client → host: where the client's camera is (drives spatial interest
/// management, §2 AoI). Sent periodically on a sequenced channel — only the
/// newest view matters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ClientView {
    pub position: [f32; 3],
    /// Requested view radius in chunks (host clamps it).
    pub radius: u8,
}

/// Host → requester: lifecycle updates for a queued prompt (§2 async
/// inference queue).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    pub request_id: u64,
    pub job_id: JobId,
    pub state: JobState,
}

/// Chat transcript entry broadcast by the host: user prompts, agent replies,
/// and session notices (joins/leaves).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostChat {
    pub speaker: String,
    pub text: String,
}

/// Reliable ordered channel for client → host prompts.
pub struct PromptChannel;

/// Sequenced unreliable channel for client → host view updates.
pub struct ViewChannel;

/// Reliable ordered channel for host → client chat.
pub struct ChatChannel;

/// Register every replicated component, message, and channel.
///
/// Must be added to both host and client apps, after the lightyear
/// `ServerPlugins`/`ClientPlugins` groups and before spawning link entities.
pub struct NetProtocolPlugin;

impl Plugin for NetProtocolPlugin {
    fn build(&self, app: &mut App) {
        // Replicated components. Option-heavy world-types payloads go
        // through JSON rules (see `json_rule_fns`); the rest keep the
        // compact postcard default.
        app.component::<NetWorldId>().replicate();
        app.component::<NetName>().replicate();
        app.component::<NetEntityKind>().replicate();
        app.component::<NetTransform>()
            .replicate()
            .add_linear_interpolation();
        app.component::<NetShape>().replicate();
        app.component::<NetMaterial>()
            .replicate_with(json_rule_fns::<NetMaterial>());
        app.component::<NetLight>()
            .replicate_with(json_rule_fns::<NetLight>());
        app.component::<NetBehaviors>()
            .replicate_with(json_rule_fns::<NetBehaviors>());
        app.component::<NetAudio>()
            .replicate_with(json_rule_fns::<NetAudio>());
        app.component::<NetMeshRef>()
            .replicate_with(json_rule_fns::<NetMeshRef>());
        app.component::<NetParentId>().replicate();
        app.component::<NetWorldMeta>()
            .replicate_with(json_rule_fns::<NetWorldMeta>());
        app.component::<NetScaffold>().replicate();
        app.component::<NetMeshAsset>().replicate();
        app.component::<NetChunkSummary>().replicate();

        // Messages + channels
        app.register_message::<ClientPrompt>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<HostChat>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<ClientView>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<JobStatus>()
            .add_direction(NetworkDirection::ServerToClient);
        app.add_channel::<ViewChannel>(ChannelSettings {
            mode: ChannelMode::SequencedUnreliable,
            ..default()
        })
        .add_direction(NetworkDirection::ClientToServer);
        app.add_channel::<PromptChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::ClientToServer);
        app.add_channel::<ChatChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::ServerToClient);
    }
}

// ---------------------------------------------------------------------------
// JSON replication rules for world-types payloads
// ---------------------------------------------------------------------------

/// World-types structs lean on `#[serde(default, skip_serializing_if = …)]`.
/// That is symmetric in self-describing formats (JSON/RON) but corrupts
/// compact field-order formats like postcard — replicon's default — because
/// a skipped `Option` writes nothing while the reader expects a presence
/// tag, misaligning every following field. Components wrapping those types
/// replicate through JSON rules instead; the remaining components (plain
/// numbers, strings, enums, `WorldTransform`, `Shape`) have no skips and
/// keep the compact default.
pub(crate) fn json_rule_fns<C: Component + Serialize + DeserializeOwned>() -> RuleFns<C> {
    RuleFns::new(serialize_json::<C>, deserialize_json::<C>)
}

fn serialize_json<C: Serialize>(
    _ctx: &mut SerializeCtx,
    component: &C,
    message: &mut Vec<u8>,
) -> bevy::ecs::error::Result<()> {
    serde_json::to_writer(&mut *message, component)?;
    Ok(())
}

fn deserialize_json<C: DeserializeOwned>(
    _ctx: &mut WriteCtx,
    bytes: &mut Bytes,
) -> bevy::ecs::error::Result<C> {
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<C>();
    match stream.next() {
        Some(Ok(value)) => {
            // Consume exactly the bytes this value occupied so the next
            // component in the buffer stays aligned.
            let consumed = stream.byte_offset();
            bytes.advance(consumed);
            Ok(value)
        }
        Some(Err(e)) => Err(e.into()),
        None => Err(std::io::Error::other("json rule: empty payload").into()),
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers (pure — unit-testable)
// ---------------------------------------------------------------------------

/// Insert the net components derived from a `wt::WorldEntity` onto an entity.
///
/// The host uses this after `snapshot_entity`. Slots that vanish from the
/// source (behaviors, parent) are removed so re-syncs converge; shape,
/// material, light, audio, and mesh never disappear from a live entity.
pub fn apply_net_components(entity: &mut EntityCommands, we: &wt::WorldEntity, kind: NetKind) {
    entity.insert((
        NetWorldId(we.id.0),
        NetName(we.name.as_str().to_string()),
        NetEntityKind(kind),
        NetTransform(we.transform.clone()),
    ));
    if let Some(shape) = &we.shape {
        entity.insert(NetShape(shape.clone()));
    }
    if let Some(material) = &we.material {
        entity.insert(NetMaterial(material.clone()));
    }
    if let Some(light) = &we.light {
        entity.insert(NetLight(light.clone()));
    }
    if let Some(audio) = &we.audio {
        entity.insert(NetAudio(audio.clone()));
    }
    if let Some(mesh) = &we.mesh_asset {
        entity.insert(NetMeshRef(mesh.clone()));
    }
    if we.behaviors.is_empty() {
        entity.remove::<NetBehaviors>();
    } else {
        entity.insert(NetBehaviors(we.behaviors.clone()));
    }
    match we.parent {
        Some(parent) => {
            entity.insert(NetParentId(parent.0));
        }
        None => {
            entity.remove::<NetParentId>();
        }
    }
}

/// Linear interpolation between two world transforms.
///
/// Euler angles are lerped component-wise (no wrap handling) — fine for the
/// prototype's mostly-cardinal rotations.
pub fn lerp_transform(
    a: &wt::WorldTransform,
    b: &wt::WorldTransform,
    t: f32,
) -> wt::WorldTransform {
    let lerp3 = |x: [f32; 3], y: [f32; 3]| {
        [
            x[0] + (y[0] - x[0]) * t,
            x[1] + (y[1] - x[1]) * t,
            x[2] + (y[2] - x[2]) * t,
        ]
    };
    wt::WorldTransform {
        position: lerp3(a.position, b.position),
        rotation_degrees: lerp3(a.rotation_degrees, b.rotation_degrees),
        scale: lerp3(a.scale, b.scale),
        visible: b.visible,
    }
}

/// Per-element epsilon comparison for transforms.
pub fn transforms_differ(a: &wt::WorldTransform, b: &wt::WorldTransform, eps: f32) -> bool {
    let ne = |x: [f32; 3], y: [f32; 3]| {
        (x[0] - y[0]).abs() > eps || (x[1] - y[1]).abs() > eps || (x[2] - y[2]).abs() > eps
    };
    ne(a.position, b.position) || ne(a.rotation_degrees, b.rotation_degrees) || ne(a.scale, b.scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entity() -> wt::WorldEntity {
        let mut we = wt::WorldEntity::new(7, "campfire");
        we.transform = wt::WorldTransform {
            position: [1.0, 2.0, 3.0],
            rotation_degrees: [10.0, 20.0, 30.0],
            scale: [2.0, 2.0, 2.0],
            visible: true,
        };
        we.shape = Some(wt::Shape::Cuboid {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        });
        we.material = Some(wt::MaterialDef {
            color: [1.0, 0.5, 0.2, 1.0],
            metallic: 0.1,
            roughness: 0.8,
            emissive: [0.0; 4],
            alpha_mode: None,
            unlit: None,
            double_sided: None,
            reflectance: None,
            ..Default::default()
        });
        we.behaviors = vec![wt::BehaviorDef::Bob {
            axis: [0.0, 1.0, 0.0],
            amplitude: 0.5,
            frequency: 1.0,
            phase: 0.0,
        }];
        we.parent = Some(wt::EntityId(3));
        we
    }

    #[test]
    fn net_kind_roundtrip_names() {
        assert_eq!(NetKind::Primitive.as_str(), "primitive");
        assert_eq!(NetKind::AudioEmitter.as_str(), "audio_emitter");
    }

    #[test]
    fn lerp_transform_midpoint() {
        let a = wt::WorldTransform {
            position: [0.0, 0.0, 0.0],
            rotation_degrees: [0.0; 3],
            scale: [1.0; 3],
            visible: true,
        };
        let b = wt::WorldTransform {
            position: [10.0, 20.0, 30.0],
            rotation_degrees: [0.0; 3],
            scale: [1.0; 3],
            visible: true,
        };
        let mid = lerp_transform(&a, &b, 0.5);
        assert_eq!(mid.position, [5.0, 10.0, 15.0]);
    }

    #[test]
    fn transforms_differ_epsilon() {
        let a = sample_entity().transform;
        let mut b = a.clone();
        assert!(!transforms_differ(&a, &b, 1e-6));
        b.position[0] += 0.01;
        assert!(transforms_differ(&a, &b, 1e-6));
        assert!(!transforms_differ(&a, &b, 0.1));
    }

    #[test]
    fn client_prompt_serde_roundtrip() {
        let msg = ClientPrompt {
            text: "build a castle".into(),
            request_id: 42,
            anchor: Some([1.0, 0.0, -2.0]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ClientPrompt = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, msg.text);
        assert_eq!(back.request_id, 42);
        assert_eq!(back.anchor, Some([1.0, 0.0, -2.0]));
    }

    #[test]
    fn host_chat_serde_roundtrip() {
        let msg = HostChat {
            speaker: "host".into(),
            text: "done".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: HostChat = serde_json::from_str(&json).unwrap();
        assert_eq!(back.speaker, msg.speaker);
    }
}
