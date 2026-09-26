//! The JSON wire protocol for collaborative sessions (spec: wire protocol v1).
//!
//! Every message is a JSON object with a `type` field (serde's internally
//! tagged form). Ops are world-types [`EditOp`]s in their usual serde form,
//! so the same payload loads from a saved `world.ron` and travels on the
//! wire. Coordinates are world units, Y up.

use serde::{Deserialize, Serialize};

use localgpt_world_types as wt;
use wt::EditOp;

/// Wire protocol version. Bumped on any incompatible change.
pub const PROTOCOL_VERSION: u32 = 1;

/// A peer's id within one session. Assigned by the transport, unique per
/// connection; reused never (a rejoin gets a new id).
pub type PeerId = u64;

/// What a peer may do. Assigned by the transport at join (the host decides
/// who may edit); the authority only enforces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Watch, walk, chat and prompt the room's AI.
    Guest,
    /// Guest plus submitting ops directly.
    Editor,
    /// Full control.
    Host,
}

impl Role {
    /// May this role submit ops?
    pub fn can_edit(self) -> bool {
        matches!(self, Self::Editor | Self::Host)
    }
}

/// The kind of client a peer connected with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    /// A browser running the world-export viewer.
    Web,
    /// A `localgpt-gen` process.
    Gen,
}

/// High-rate, lossy, never stored: where a peer is and what it's doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    /// Camera/avatar position.
    pub position: [f32; 3],
    /// Where the peer is looking.
    pub look_at: [f32; 3],
    /// Entity the peer has selected, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<u64>,
}

/// A connected peer as told to other peers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub name: String,
    pub role: Role,
    pub client: ClientKind,
    /// Last reported presence, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
}

/// Static facts about the room, sent in `welcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session name (also the world's name).
    pub name: String,
}

/// Who authored an `ops` message or a chat line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    /// The peer, when the author is connected (None for the host app itself).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<PeerId>,
    pub name: String,
}

/// A prompt job as clients see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: u64,
    /// The peer who asked (None for host-local prompts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<PeerId>,
    pub prompt: String,
    /// Where the build lands, if the requester aimed somewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<[f32; 3]>,
    pub state: JobState,
}

/// Lifecycle of a prompt job. Serializes as `{ "state": "queued", ... }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobState {
    /// Waiting for a worker; `position` 0 means next up.
    Queued {
        position: u32,
    },
    Running,
    Done,
    Failed {
        reason: String,
    },
    Rejected {
        reason: String,
    },
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed { .. } | Self::Rejected { .. }
        )
    }
}

/// What kind of a chat line this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatKind {
    Human,
    Agent,
    System,
}

/// Client → server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message on a connection. `token` is the invite-link bearer
    /// token; PIN sessions require it, open sessions ignore it.
    Hello {
        protocol: u32,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        client: ClientKind,
    },
    /// Update this peer's presence.
    Presence(Presence),
    /// Ask the room's AI to build. `anchor` is where the prompter is
    /// looking (a ground point), so the build lands there.
    Prompt {
        request_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<[f32; 3]>,
    },
    Chat {
        text: String,
    },
    /// Submit ops directly (editors and up). `expected_revision`, when set,
    /// rejects the submission if the document has since moved on.
    Submit {
        client_seq: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_revision: Option<u64>,
        ops: Vec<EditOp>,
    },
    /// Ask for a fresh snapshot after seeing a revision gap.
    Resync,
    Ping {
        t: f64,
    },
}

/// Server → client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Answer to `hello`: identity, the whole world, and who's here.
    Welcome {
        protocol: u32,
        peer_id: PeerId,
        role: Role,
        session: SessionInfo,
        revision: u64,
        world: Box<wt::WorldManifest>,
        peers: Vec<PeerInfo>,
        /// Non-terminal prompt jobs (so a joiner sees in-flight builds).
        jobs: Vec<JobInfo>,
        /// Base URL for streamed assets, when the session has one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asset_base: Option<String>,
    },
    /// Ops committed by the authority, in order. `revision` is the
    /// document's revision after applying them.
    Ops {
        revision: u64,
        author: Author,
        ops: Vec<EditOp>,
        /// Echoed only to the submitter's connection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_seq: Option<u64>,
    },
    /// A `submit` was refused; the document is unchanged.
    Reject {
        client_seq: u64,
        reason: String,
    },
    /// A fresh full snapshot (answer to `resync`).
    Snapshot {
        revision: u64,
        world: Box<wt::WorldManifest>,
    },
    PeerJoined {
        peer: PeerInfo,
    },
    PeerLeft {
        peer_id: PeerId,
    },
    /// A peer's latest presence.
    Presence {
        peer_id: PeerId,
        presence: Presence,
    },
    /// A prompt job changed state. `request_id` lets the requester match
    /// the update to its `prompt` message.
    Job {
        job: JobInfo,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Chat {
        from: Author,
        text: String,
        kind: ChatKind,
    },
    /// Fatal for the connection; the server closes the socket after it.
    Error {
        reason: String,
    },
    Pong {
        t: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use wt::{EntityId, EntityPatch, WorldEntity};

    #[test]
    fn client_hello_wire_form() {
        let json = serde_json::to_value(ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            name: "maya".into(),
            token: Some("abc".into()),
            client: ClientKind::Web,
        })
        .unwrap();
        assert_eq!(json["type"], "hello");
        assert_eq!(json["client"], "web");
        // Parses back.
        let back: ClientMsg = serde_json::from_value(json).unwrap();
        assert!(matches!(back, ClientMsg::Hello { name, .. } if name == "maya"));
    }

    #[test]
    fn submit_carries_edit_ops_verbatim() {
        let entity = WorldEntity::new(7, "lantern");
        let msg = ClientMsg::Submit {
            client_seq: 3,
            expected_revision: Some(12),
            ops: vec![
                EditOp::spawn(entity),
                EditOp::modify(
                    EntityId(4),
                    EntityPatch {
                        name: Some(wt::EntityName::new("oak")),
                        ..Default::default()
                    },
                ),
            ],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"submit\""));
        assert!(json.contains("\"SpawnEntity\""));
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        match back {
            ClientMsg::Submit {
                client_seq,
                expected_revision,
                ops,
            } => {
                assert_eq!(client_seq, 3);
                assert_eq!(expected_revision, Some(12));
                assert_eq!(ops.len(), 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn job_state_wire_form() {
        assert_eq!(
            serde_json::to_value(JobState::Queued { position: 2 }).unwrap(),
            serde_json::json!({"state": "queued", "position": 2})
        );
        assert_eq!(
            serde_json::to_value(JobState::Failed {
                reason: "no worker".into()
            })
            .unwrap(),
            serde_json::json!({"state": "failed", "reason": "no worker"})
        );
        assert!(JobState::Done.is_terminal());
        assert!(!JobState::Running.is_terminal());
    }

    #[test]
    fn server_msgs_roundtrip() {
        let msgs = vec![
            ServerMsg::PeerLeft { peer_id: 9 },
            ServerMsg::Presence {
                peer_id: 2,
                presence: Presence {
                    position: [1.0, 2.0, 3.0],
                    look_at: [0.0, 0.0, 0.0],
                    selected: Some(4),
                },
            },
            ServerMsg::Chat {
                from: Author {
                    peer: Some(2),
                    name: "maya".into(),
                },
                text: "hi".into(),
                kind: ChatKind::Human,
            },
            ServerMsg::Reject {
                client_seq: 1,
                reason: "stale".into(),
            },
            ServerMsg::Pong { t: 1.5 },
        ];
        for msg in msgs {
            let json = serde_json::to_string(&msg).unwrap();
            let back: ServerMsg = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_value(&msg).unwrap(),
                serde_json::to_value(&back).unwrap()
            );
        }
    }

    #[test]
    fn welcome_roundtrip_with_world() {
        let world = wt::WorldManifest::new("demo");
        let msg = ServerMsg::Welcome {
            protocol: PROTOCOL_VERSION,
            peer_id: 1,
            role: Role::Guest,
            session: SessionInfo {
                name: "demo".into(),
            },
            revision: 0,
            world: Box::new(world),
            peers: vec![PeerInfo {
                id: 1,
                name: "maya".into(),
                role: Role::Guest,
                client: ClientKind::Web,
                presence: None,
            }],
            jobs: vec![],
            asset_base: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        match back {
            ServerMsg::Welcome {
                revision, peers, ..
            } => {
                assert_eq!(revision, 0);
                assert_eq!(peers[0].name, "maya");
            }
            _ => panic!("wrong variant"),
        }
    }
}
