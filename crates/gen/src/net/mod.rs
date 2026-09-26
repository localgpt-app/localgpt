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
pub mod guest_avatars;
pub mod host;
pub mod interest;
pub mod jobs;
pub mod mdns;
pub mod pairing;
pub mod protocol;
pub mod remote_scope;
pub mod web;

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

/// Env var carrying an already-paired connect token (base64) to a
/// `--join` child process. Set by the desktop panel when it spawns a
/// viewer window after pairing on the user's behalf, so the child (which
/// has no terminal to type a PIN into) can connect immediately.
pub const JOIN_TOKEN_ENV: &str = "LOCALGPT_GEN_JOIN_TOKEN";

/// Parse a peer address: `host:port`, `ip:port`, or a bare host (port
/// defaults to the session port).
pub fn parse_peer_addr(spec: &str) -> anyhow::Result<std::net::SocketAddr> {
    use anyhow::Context as _;

    if let Ok(addr) = spec.parse() {
        return Ok(addr);
    }
    let with_port = format!("{spec}:{}", DEFAULT_PORT);
    with_port
        .parse()
        .with_context(|| format!("invalid host address '{spec}' (expected host:port)"))
}

/// Default hosted-session name: "{user}'s world".
pub fn default_session_name() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "gen".to_string());
    format!("{user}'s world")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_addresses_parse_with_and_without_port() {
        assert_eq!(parse_peer_addr("192.168.1.5:9879").unwrap().port(), 9879);
        assert_eq!(parse_peer_addr("192.168.1.5").unwrap().port(), DEFAULT_PORT);
        assert!(parse_peer_addr("not a host").is_err());
    }
}
