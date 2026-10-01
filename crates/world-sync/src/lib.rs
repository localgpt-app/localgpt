//! # localgpt-world-sync
//!
//! Collaborative sessions over the LocalGPT world format. The format's
//! own surface — the document fold, the op log, the session op
//! vocabulary — is re-exported from the `openworldformat` crate (the
//! Rust reference this code was extracted into). What stays here is the
//! room around the fold: the authority that orders ops, the wire
//! protocol, scene diffing, and undo.
//!
//! See `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`.
//!
//! No Bevy, no async runtime, no sockets: the transport (Gen's session
//! HTTP server today; a relay, a headless server or a SpacetimeDB
//! module later) feeds messages in and delivers what comes out.

pub mod authority;
pub mod diff;
pub mod protocol;
pub mod undo;

// The format's session surface — modules re-exported so `crate::doc`
// and friends keep resolving for the room machinery above.
pub use openworldformat::doc;
pub use openworldformat::oplog;
pub use openworldformat::session;

pub use authority::{
    Authority, AuthorityEvent, Limits, Outbound, Recipients, sanitize_name, undo_key_for_local,
    undo_key_for_peer,
};
pub use diff::{diff_entities, diff_scene};
pub use openworldformat::doc::{ApplyError, WorldDoc};
pub use openworldformat::oplog::{OpLogEntry, decode_line, encode_line};
pub use openworldformat::session::{
    ClockRecord, ClockState, ExtensionRecord, InputRecord, InputSample, MergeRecord,
    SESSION_FORMAT_VERSION, SessionMeta, SessionOp, StateRecord, ToolRecord, fold_log, fold_path,
};
pub use protocol::{
    ChatKind, ClientKind, ClientMsg, JobInfo, JobState, PROTOCOL_VERSION, PeerId, PeerInfo,
    Presence, Role, ServerMsg, SessionInfo,
};
pub use undo::compute_inverse;

// The protocol speaks the same Author as the log: one type, from the
// crate, re-exported through the module that used to define it.
pub use openworldformat::author::Author;
