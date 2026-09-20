//! Phase-1 collaborative multiplayer (listen server prototype).
//!
//! Implements §1 of
//! `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`:
//! one desktop `localgpt-gen` instance acts as authoritative host and
//! rendering client (`--host`), secondary clients discover the session over
//! mDNS and join as read-only viewers (`--join`), and connected clients can
//! send natural-language prompts to the host's agent.
//!
//! §2 compatibility: the wire vocabulary is `localgpt-world-types` (the same
//! model the SpacetimeDB tier uses), entities are identified by stable
//! host-assigned ids, and replication targets are expressed as
//! `NetworkTarget`s — flat broadcast today, interest-managed scopes later.
//!
//! See `docs/gen/multiplayer.md` for the full design and limitations.

pub mod client;
pub mod host;
pub mod mdns;
pub mod protocol;

/// Default UDP port for hosted sessions.
///
/// (9877 is the inspector WebSocket, 9878 the MCP relay.)
pub const DEFAULT_PORT: u16 = 9879;

/// Lightyear netcode protocol id — hosts and clients must match.
pub const PROTOCOL_ID: u64 = 1;

/// Shared netcode private key.
///
/// Phase-1 trust model: LAN-only sessions with no per-session key exchange.
/// Both sides derive connect tokens from this constant. Not a security
/// boundary — anyone on the LAN with the binary can connect. Per-session
/// keys and a pin/pairing step land with the auth work in a later phase.
pub const PRIVATE_KEY: [u8; 32] = [0u8; 32];
