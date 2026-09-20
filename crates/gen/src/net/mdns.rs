//! mDNS session discovery for LAN collaborative sessions (§1).
//!
//! Hosts announce a `_localgpt-world._udp.local.` service carrying the
//! session name, UDP port, and protocol metadata as TXT records. Clients
//! browse for the same service type and connect without manual IP entry.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result};

/// mDNS service type for LocalGPT Gen collaborative sessions.
pub const SERVICE_TYPE: &str = "_localgpt-world._udp.local.";

/// TXT key carrying the lightyear protocol id (compatibility gate).
pub const TXT_PROTOCOL_ID: &str = "proto";

/// TXT key carrying the host's crate version.
pub const TXT_VERSION: &str = "ver";

/// TXT key carrying the session display name.
pub const TXT_SESSION_NAME: &str = "name";

/// A session found on the local network.
#[derive(Debug, Clone)]
pub struct DiscoveredSession {
    /// Instance name (defaults to the session name on the host).
    pub instance: String,
    /// Session display name from TXT records (falls back to `instance`).
    pub session_name: String,
    /// Host address + session port.
    pub addr: SocketAddr,
    /// TXT record properties.
    pub properties: HashMap<String, String>,
}

impl DiscoveredSession {
    /// True if this session speaks our netcode protocol.
    pub fn protocol_matches(&self, expected: u64) -> bool {
        self.properties
            .get(TXT_PROTOCOL_ID)
            .and_then(|p| p.parse::<u64>().ok())
            .map(|p| p == expected)
            .unwrap_or(false)
    }
}

/// Best-effort local LAN IPv4 address (no external traffic — the socket is
/// only routed, never sent on).
pub fn local_lan_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// Handle owning the mDNS daemon announcing a hosted session.
///
/// Unregisters + shuts the daemon down on drop so a closed host window
/// stops advertising immediately.
pub struct SessionAnnouncer {
    daemon: Option<mdns_sd::ServiceDaemon>,
    instance: String,
}

impl SessionAnnouncer {
    /// Start announcing a session in the background.
    pub fn start(session_name: &str, port: u16, protocol_id: u64) -> Result<Self> {
        let daemon = mdns_sd::ServiceDaemon::new().context("failed to start mDNS daemon")?;
        let ip = local_lan_ip().unwrap_or_else(|| std::net::Ipv4Addr::LOCALHOST.into());
        let properties: HashMap<String, String> = [
            (TXT_PROTOCOL_ID.to_string(), protocol_id.to_string()),
            (
                TXT_VERSION.to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ),
            (TXT_SESSION_NAME.to_string(), session_name.to_string()),
        ]
        .into_iter()
        .collect();

        let instance = sanitize_instance_name(session_name);
        let host_name = format!("{instance}.local.");
        let info =
            mdns_sd::ServiceInfo::new(SERVICE_TYPE, &instance, &host_name, ip, port, properties)
                .context("invalid mDNS service info")?;
        daemon
            .register(info)
            .context("failed to register mDNS service")?;

        tracing::info!(
            "mDNS: announcing session '{}' at {}:{} ({SERVICE_TYPE})",
            session_name,
            ip,
            port
        );
        Ok(Self {
            daemon: Some(daemon),
            instance,
        })
    }
}

impl Drop for SessionAnnouncer {
    fn drop(&mut self) {
        if let Some(daemon) = self.daemon.take() {
            if let Ok(rx) = daemon.unregister(&self.instance) {
                // Wait briefly for the unregister ack, then shut down.
                let _ = rx.recv_timeout(Duration::from_secs(1));
            }
            let _ = daemon.shutdown();
        }
    }
}

/// mDNS instance names must be valid DNS labels — keep it conservative.
fn sanitize_instance_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "localgpt-world".to_string()
    } else {
        trimmed.chars().take(63).collect()
    }
}

/// Browse the LAN for collaborative sessions for up to `duration`.
///
/// Returns every distinct resolved service whose protocol matches
/// `protocol_id`. Callers should use a window of 2–3 seconds.
pub fn browse_sessions(duration: Duration, protocol_id: u64) -> Result<Vec<DiscoveredSession>> {
    let daemon = mdns_sd::ServiceDaemon::new().context("failed to start mDNS daemon")?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .context("failed to start mDNS browse")?;

    let deadline = std::time::Instant::now() + duration;
    let mut sessions = Vec::new();
    while let Ok(event) =
        receiver.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
            let Some(scoped) = info
                .addresses
                .iter()
                .find(|a| a.is_ipv4())
                .or_else(|| info.addresses.iter().next())
            else {
                continue;
            };
            let properties = info
                .txt_properties
                .iter()
                .map(|p| (p.key().to_string(), p.val_str().to_string()))
                .collect::<HashMap<_, _>>();
            let session_name = properties
                .get(TXT_SESSION_NAME)
                .cloned()
                .unwrap_or_else(|| info.fullname.clone());
            let session = DiscoveredSession {
                instance: info
                    .fullname
                    .strip_suffix(&format!(".{SERVICE_TYPE}"))
                    .unwrap_or(&info.fullname)
                    .to_string(),
                session_name,
                addr: SocketAddr::new(scoped.to_ip_addr(), info.port),
                properties,
            };
            if session.protocol_matches(protocol_id)
                && !sessions
                    .iter()
                    .any(|s: &DiscoveredSession| s.addr == session.addr)
            {
                sessions.push(session);
            }
        }
        // Loop until the deadline passes; recv_timeout errors out then.
    }
    let _ = daemon.shutdown();
    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_instance_name_replaces_invalid_chars() {
        // Invalid chars become '-', then leading/trailing dashes are trimmed.
        assert_eq!(sanitize_instance_name("My World!"), "My-World");
        assert_eq!(sanitize_instance_name("  "), "localgpt-world");
        assert_eq!(sanitize_instance_name("ok"), "ok");
    }

    #[test]
    fn sanitize_instance_name_caps_length() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_instance_name(&long).len(), 63);
    }

    #[test]
    fn discovered_session_protocol_gate() {
        let mut s = DiscoveredSession {
            instance: "x".into(),
            session_name: "x".into(),
            addr: "127.0.0.1:9879".parse().unwrap(),
            properties: HashMap::new(),
        };
        assert!(!s.protocol_matches(1));
        s.properties.insert(TXT_PROTOCOL_ID.to_string(), "1".into());
        assert!(s.protocol_matches(1));
        assert!(!s.protocol_matches(2));
    }
}
