//! Collaborative multiplayer for gen (the v2 op-based architecture).
//!
//! Implements `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`:
//! a world is a shared document, every change is a world-types `EditOp`, and
//! one authority per room checks, orders and fans out those ops.
//!
//! - [`web`] — the host side: the room authority, the join page, the
//!   WebSocket endpoint, and the scene projection (scene → ops).
//! - [`ops_client`] — the native client (`--join`): the full gen scene
//!   driven by the room.
//! - [`guest_avatars`] — peer capsules, rendered on both sides.
//! - [`jobs`] — the prompt job queue shared by every client kind.
//! - [`mdns`] — LAN session discovery.
//! - [`remote_scope`] — the scene-only agent room prompts run on.
//!
//! The wire vocabulary is `localgpt-world-types` (via
//! `localgpt-world-sync`), so any renderer that reads the world format can
//! join — the browser client uses the same protocol.
//!
//! See `docs/gen/multiplayer.md` for the full design and limitations.

pub mod guest_avatars;
pub mod host;
pub mod jobs;
pub mod mdns;
pub mod ops_client;
pub mod relay_client;
pub mod remote_scope;
pub mod web;

/// Default port for hosted sessions.
///
/// (9877 is the inspector WebSocket, 9878 the MCP relay.)
pub const DEFAULT_PORT: u16 = 9879;

/// Session discovery id — hosts announce and clients browse this mDNS
/// service instance marker. Bumped with the ops protocol (v1).
pub const PROTOCOL_ID: u64 = 4;

/// Env var carrying a session PIN to a `--join` child process. Set by the
/// desktop panel when it spawns a viewer window, so the child (which has no
/// terminal to type a PIN into) can join immediately.
pub const JOIN_PIN_ENV: &str = "LOCALGPT_GEN_JOIN_PIN";

/// A fresh 6-digit session PIN.
pub fn generate_pin() -> String {
    let n: u32 = rand::random_range(0..1_000_000);
    format!("{n:06}")
}

/// Format a PIN for display (grouped digits).
pub fn format_pin(pin: &str) -> String {
    pin.to_string()
}

/// Strip formatting from a PIN the user typed (spaces, dashes).
pub fn normalize_pin(input: &str) -> String {
    input.chars().filter(|c| c.is_ascii_digit()).collect()
}

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

    #[test]
    fn pins_normalize() {
        assert_eq!(normalize_pin("482 913"), "482913");
        assert_eq!(normalize_pin("482-913"), "482913");
        assert_eq!(generate_pin().len(), 6);
    }
}
