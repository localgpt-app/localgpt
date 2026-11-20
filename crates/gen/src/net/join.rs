//! Joining a hosted session, as Gen's `--join` and the desktop app's
//! `--join` both do: find it, ask its host what it is, and the client
//! plugin that shows it.
//!
//! The steps are apart so a window can take them where they belong: the
//! desktop app's Share panel finds and asks before it opens a guest window,
//! so a session that is not there, or wants a PIN nobody typed, is said in
//! the panel instead of in a window that never comes up.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::mpsc;

use super::ops_client::{OpsClientOptions, OpsClientPlugin};

/// What a host says about its session before anyone joins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub addr: SocketAddr,
    /// The session's name, as its host gave it.
    pub name: String,
    /// It asks for its PIN (an open session does not).
    pub needs_pin: bool,
}

/// What `/session-info` returns.
#[derive(serde::Deserialize)]
struct SessionInfoWire {
    protocol: u32,
    name: String,
    secret_required: bool,
}

/// How long the LAN is browsed for sessions.
pub const BROWSE: Duration = Duration::from_secs(3);

/// The sessions announced on this network, found in [`BROWSE`].
pub fn browse() -> anyhow::Result<Vec<super::mdns::DiscoveredSession>> {
    super::mdns::browse_sessions(BROWSE, super::PROTOCOL_ID)
}

/// Where the session is: `spec` is `host[:port]`; without one, the LAN is
/// browsed and the first session found is taken.
pub fn locate(spec: Option<&str>) -> anyhow::Result<SocketAddr> {
    if let Some(spec) = spec {
        return super::parse_peer_addr(spec);
    }
    eprintln!("Browsing the LAN for collaborative sessions…");
    let sessions = browse()?;
    for session in &sessions {
        eprintln!("  found: {} — {}", session.session_name, session.addr);
    }
    let Some(chosen) = sessions.first() else {
        anyhow::bail!(
            "No sessions found. Host one with `localgpt gen --host` or Share in the \
             desktop app, then join it,\n\
             or connect directly: localgpt gen --join 192.168.1.5:9879"
        );
    };
    eprintln!("Joining '{}' at {}", chosen.session_name, chosen.addr);
    Ok(chosen.addr)
}

/// Ask the host at `addr` what its session is, and refuse a host that
/// speaks another protocol than this build.
pub fn ask(addr: SocketAddr) -> anyhow::Result<SessionInfo> {
    let url = format!("http://{addr}/session-info");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let wire: SessionInfoWire = rt.block_on(async {
        let resp = reqwest::Client::new()
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Couldn't reach the host's session endpoint at {url} ({e}). Is the host \
                     running, and is TCP port {} open?",
                    addr.port()
                )
            })?;
        if !resp.status().is_success() {
            anyhow::bail!("session endpoint answered HTTP {}", resp.status());
        }
        Ok(resp.json::<SessionInfoWire>().await?)
    })?;
    if wire.protocol != localgpt_world_sync::PROTOCOL_VERSION {
        anyhow::bail!(
            "Host speaks session protocol {} but this client speaks {} — use matching \
             LocalGPT versions",
            wire.protocol,
            localgpt_world_sync::PROTOCOL_VERSION
        );
    }
    Ok(SessionInfo {
        addr,
        name: wire.name,
        needs_pin: wire.secret_required,
    })
}

/// The guest's display name: the user's, else "guest".
pub fn guest_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "guest".to_string())
}

/// The PIN a parent process handed over in [`super::JOIN_PIN_ENV`], taken
/// out of the environment so that nothing this process starts inherits it.
///
/// # Safety
///
/// It edits the environment: call it at program start, before any other
/// thread exists.
pub unsafe fn take_env_pin() -> Option<String> {
    let pin = std::env::var(super::JOIN_PIN_ENV).ok()?;
    // SAFETY: the caller's promise — program start, before other threads.
    unsafe { std::env::remove_var(super::JOIN_PIN_ENV) };
    Some(super::normalize_pin(&pin)).filter(|pin| !pin.is_empty())
}

/// The client plugin that joins `addr` (with the PIN, when the session asks
/// for one) and drives the scene from the room; with it, the sender of the
/// lines the guest says in the room.
pub fn client(
    addr: SocketAddr,
    pin: Option<String>,
) -> (OpsClientPlugin, mpsc::UnboundedSender<String>) {
    let (say, said) = mpsc::unbounded_channel();
    let plugin = OpsClientPlugin {
        options: std::sync::Mutex::new(Some(OpsClientOptions {
            server_addr: addr,
            pin,
            name: guest_name(),
            prompt_rx: said,
        })),
    };
    (plugin, say)
}
