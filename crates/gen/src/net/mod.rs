//! Collaborative multiplayer for gen.
//!
//! Implements `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`:
//!
//! - **§1 listen server:** one desktop `localgpt-gen` instance acts as
//!   authoritative host and rendering client (`--host`); secondary clients
//!   discover the session over mDNS and join as viewers (`--join`) that can
//!   send natural-language prompts to the host's agent.
//! - **§2 scaling mechanisms**, applied to the same session:
//!   - spatial interest management — per-client chunk windows
//!     ([`interest`], host-side lightyear visibility);
//!   - an asynchronous inference queue with scaffold-then-replace
//!     ([`jobs`], replicated `NetScaffold`s);
//!   - HLOD chunk impostors and static mesh baking ([`client_lod`],
//!     [`bake`]);
//!   - content-addressed on-demand asset streaming ([`assets`]).
//!
//! The wire vocabulary is `localgpt-world-types` (the same model the
//! SpacetimeDB tier uses) and entities are identified by stable
//! host-assigned ids, so the cloud tier can reuse the protocol.
//!
//! See `docs/gen/multiplayer.md` for the full design and limitations.

pub mod assets;
pub mod bake;
pub mod client;
pub mod client_lod;
pub mod host;
pub mod interest;
pub mod jobs;
pub mod mdns;
pub mod pairing;
pub mod protocol;
pub mod remote_scope;

/// Default UDP port for hosted sessions.
///
/// (9877 is the inspector WebSocket, 9878 the MCP relay.)
pub const DEFAULT_PORT: u16 = 9879;

/// Lightyear netcode protocol id — hosts and clients must match.
///
/// 2: §2 additions (view reports, prompt jobs/scaffolds, chunk summaries).
/// 3: per-session keys + PIN pairing (host-minted connect tokens).
pub const PROTOCOL_ID: u64 = 3;

/// Netcode private key for `--open` sessions only.
///
/// Open sessions skip pairing: both sides derive connect tokens from this
/// public constant, so anyone on the LAN with the binary can connect. Normal
/// sessions use a random per-session key that never leaves the host and
/// hand out tokens only after PIN pairing (see [`pairing`]).
pub const OPEN_SESSION_KEY: [u8; 32] = [0u8; 32];
