//! # localgpt-world-sync
//!
//! Collaborative sessions over the LocalGPT world format. A world is a shared
//! document; every change to it is a world-types [`EditOp`]; one
//! [`Authority`] per room checks, orders and fans out those ops. See
//! `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`.
//!
//! - [`doc::WorldDoc`] — the document: entities by id plus the scene-wide
//!   settings, with atomic, validated op application.
//! - [`diff::diff_scene`] — the ops that turn a document into a fresh
//!   projection of a live scene (how Gen's host captures edits from ~100
//!   tools without instrumenting them).
//! - [`protocol`] — the JSON wire messages ([`ClientMsg`], [`ServerMsg`]).
//! - [`authority::Authority`] — peers, roles, revisions, presence, chat and
//!   the prompt queue: the room's bookkeeping, returning the messages to
//!   send and to whom.
//!
//! No Bevy, no async runtime, no sockets: the transport (Gen's session HTTP
//! server today; a relay, a headless server or a SpacetimeDB module later)
//! feeds messages in and delivers what comes out.
//!
//! [`EditOp`]: localgpt_world_types::EditOp

pub mod authority;
pub mod diff;
pub mod doc;
pub mod oplog;
pub mod protocol;
pub mod undo;

pub use authority::{
    Authority, AuthorityEvent, Limits, Outbound, Recipients, sanitize_name, undo_key_for_local,
    undo_key_for_peer,
};
pub use diff::{diff_entities, diff_scene};
pub use doc::{ApplyError, WorldDoc};
pub use oplog::{OpLogEntry, decode_line, encode_line};
pub use protocol::{
    Author, ChatKind, ClientKind, ClientMsg, JobInfo, JobState, PROTOCOL_VERSION, PeerId, PeerInfo,
    Presence, Role, ServerMsg, SessionInfo,
};
pub use undo::compute_inverse;
