//! Host-side session lifecycle: brings a hosted session up (secrets, session
//! HTTP with the ops room, mDNS) and keeps it running.
//!
//! The room itself (document, peers, ops fan-out) lives in
//! [`super::web::WebRoom`]; the scene projection there turns the host's
//! world into ops. This module owns only what the room doesn't: session
//! setup and the announcement. No model runs for the room: a guest's prompt
//! is refused at the authority, and a guest edits the world through ops, as
//! an agent does through the world's API.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bevy::prelude::*;

use super::mdns::SessionAnnouncer;
use super::web::{self, WebRoom};

/// Host-side options, built by the CLI when `--host` is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetHostOptions {
    /// Session name shown in mDNS discovery and to joining clients.
    pub session_name: String,
    /// Port to listen on.
    pub port: u16,
    /// Skip the PIN: anyone on the LAN can join (trusted networks only).
    pub open: bool,
    /// Start hosting as soon as the app runs (CLI `--host`). When false the
    /// plugin stays dormant until a session is requested through
    /// [`HostControl`].
    pub autostart: bool,
    /// Let browsers join as guests: serve a join page and the WebSocket ops
    /// endpoint on the session port (CLI `--web`).
    pub web: bool,
    /// Browser guests join as editors who may move/rotate/scale/delete
    /// entities directly (CLI `--web-edit`). Default: watching guests.
    pub web_edit: bool,
    /// Replay a previous session's op log before guests join (CLI `--resume`).
    pub resume: Option<String>,
    /// Reach guests beyond the LAN through this relay URL (CLI `--relay`).
    pub relay: Option<String>,
}

impl Default for NetHostOptions {
    fn default() -> Self {
        Self {
            session_name: String::new(),
            port: super::DEFAULT_PORT,
            open: false,
            autostart: false,
            web: false,
            web_edit: false,
            resume: None,
            relay: None,
        }
    }
}

/// What joiners need to know about the running session, for display.
#[derive(Resource, Clone)]
pub struct HostSessionInfo {
    pub session_name: String,
    pub port: u16,
    /// Currently connected guests (kept in step by the room).
    pub clients: usize,
    pin: Option<String>,
}

impl HostSessionInfo {
    /// The current session PIN, or `None` for an open session.
    pub fn pin(&self) -> Option<String> {
        self.pin.clone()
    }

    /// One line for a status display.
    pub fn summary(&self) -> String {
        let mut line = match &self.pin {
            Some(pin) => format!("Hosting '{}' · PIN {pin}", self.session_name),
            None => format!("Hosting '{}' · open session (no PIN)", self.session_name),
        };
        if self.clients > 0 {
            line.push_str(&format!(
                " · {} guest{}",
                self.clients,
                plural(self.clients)
            ));
        }
        line
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// A request to start hosting a session — from the CLI (`--host`, before
/// the app runs) or from the window at any time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostStartRequest {
    pub session_name: String,
    pub port: u16,
    /// Open session: no PIN (trusted LANs only).
    pub open: bool,
    /// Browsers may join as guests (join page + WebSocket endpoint).
    pub web: bool,
    /// Browser guests get the editor role (direct edits).
    pub web_edit: bool,
    /// Op log to replay at start (`--resume`).
    pub resume: Option<String>,
    /// Relay URL for internet guests (`--relay`).
    pub relay: Option<String>,
}

/// Lifecycle of a hosted session. The plugin is always installed (so
/// hosting can start from the window at any time), but nothing listens,
/// announces, or replicates until a request arrives.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub enum HostControl {
    /// Not hosting and nothing requested.
    #[default]
    NotHosting,
    /// A start was requested; [`host_lifecycle`] runs it this frame.
    StartRequested(HostStartRequest),
    /// The session is live. `warning` carries a non-fatal startup problem
    /// (e.g. mDNS failed).
    Active { warning: Option<String> },
    /// The start failed (e.g. the port is taken); the reason, and hosting
    /// can be requested again.
    Failed(String),
}

impl HostControl {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

/// Run condition for the host systems that touch the world or the network:
/// they only do work while a session is actually live.
pub fn hosting(control: Res<HostControl>) -> bool {
    control.is_active()
}

/// Resource owning the mDNS announcer (unregisters on drop).
#[derive(Resource)]
struct HostAnnouncer {
    _announcer: SessionAnnouncer,
}

/// The host plugin. Always installed; dormant until a session starts.
pub struct NetHostPlugin {
    /// Consumed on build.
    pub options: std::sync::Mutex<Option<NetHostOptions>>,
}

impl Plugin for NetHostPlugin {
    fn build(&self, app: &mut App) {
        let NetHostOptions {
            session_name,
            port,
            open,
            autostart,
            web,
            web_edit,
            resume,
            relay,
        } = self
            .options
            .lock()
            .expect("NetHostPlugin options lock poisoned")
            .take()
            .expect("NetHostPlugin options consumed twice");

        // Dormant until a session is requested through `HostControl` — at
        // startup (`--host`) or later, from the window.
        let control = if autostart {
            HostControl::StartRequested(HostStartRequest {
                session_name,
                port,
                open,
                web,
                web_edit,
                resume,
                relay,
            })
        } else {
            HostControl::NotHosting
        };
        app.insert_resource(control)
            .init_resource::<super::guest_avatars::GuestAvatars>()
            .init_resource::<web::PendingSceneOps>()
            .add_systems(PreUpdate, host_lifecycle)
            // The ops room (browser page, native ops guests): the room
            // resource exists only while a session is live.
            .add_systems(
                Update,
                (
                    web::web_drain_inbound,
                    web::web_apply_scene_ops,
                    web::web_projection_sync,
                    web::web_session_history,
                )
                    .chain()
                    .run_if(|room: Option<Res<web::WebRoom>>| room.is_some()),
            )
            // Guest avatars in the host's own window.
            .add_systems(Update, super::guest_avatars::web_guest_avatars);
    }
}

/// Bring a requested session up: the PIN, session HTTP (join page +
/// WebSocket ops endpoint), the ops room, and mDNS. Runs in `PreUpdate` so
/// the gated host systems see [`HostControl::Active`] from the same frame on.
///
/// This is the single start path: `--host` requests it before the first
/// frame; the window may request it at any later one.
fn host_lifecycle(
    mut commands: Commands,
    mut control: ResMut<HostControl>,
    mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    workspace: Res<crate::gen3d::plugin::GenWorkspace>,
) {
    let request = match std::mem::take(&mut *control) {
        HostControl::StartRequested(request) => request,
        HostControl::NotHosting => return,
        active @ HostControl::Active { .. } | active @ HostControl::Failed(_) => {
            *control = active;
            return;
        }
    };

    // Session secrets: the PIN native guests join with (open sessions skip)
    // and the invite token browser links carry.
    let session_pin = (!request.open).then(super::generate_pin);
    let token = (!request.open).then(web::generate_web_token);

    // Session HTTP (TCP, session port): the ops room's join page + WebSocket
    // endpoint — served for every hosted session.
    let http_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), request.port);
    let (bridge, inbound_rx) = web::WebBridge::new(token.clone(), &request.session_name);
    let router = web::web_router(bridge.clone());
    if let Err(e) = web::spawn_session_http(router, http_addr) {
        eprintln!(
            "Collaborative session failed to start: session HTTP couldn't listen on \
             tcp://{http_addr} ({e}) — free the port or pass --port"
        );
        *control = HostControl::Failed(format!(
            "couldn't listen on port {port} ({e}) — is another session running?",
            port = request.port
        ));
        return;
    }
    if let Some(relay_url) = &request.relay {
        super::relay_client::start_relay_client(bridge.clone(), relay_url, token.clone());
    }
    commands.insert_resource(WebRoom::new(
        &request.session_name,
        bridge,
        inbound_rx,
        &workspace.path,
        request.resume.as_deref(),
        request.web_edit,
        session_pin.clone(),
    ));

    match &session_pin {
        Some(pin) => eprintln!(
            "\n  Session PIN: {pin}   (joiners: localgpt-gen --join <this-host> --pin <PIN>)\n"
        ),
        None => {
            eprintln!("\n  OPEN session: no PIN — anyone on the LAN can join\n")
        }
    }
    if request.web {
        match (web::primary_lan_ip(), &token) {
            (Some(ip), Some(token)) => eprintln!(
                "\n  Browser guests: http://{ip}:{port}/#t={token}\n",
                port = request.port
            ),
            (Some(ip), None) => eprintln!(
                "\n  Browser guests: http://{ip}:{port}/  (open session)\n",
                port = request.port
            ),
            (None, _) => {
                eprintln!("\n  Browser guests: http://<this-host>:{}/\n", request.port)
            }
        }
    }
    eprintln!("Collaborative session listening on {http_addr}");

    let mut warning = None;
    match SessionAnnouncer::start(&request.session_name, request.port, super::PROTOCOL_ID) {
        Ok(announcer) => {
            eprintln!(
                "mDNS: session '{}' discoverable on the LAN ({}), port {port}",
                request.session_name,
                super::mdns::SERVICE_TYPE,
                port = request.port
            );
            commands.insert_resource(HostAnnouncer {
                _announcer: announcer,
            });
        }
        Err(e) => {
            eprintln!("mDNS announcement failed ({e}) — clients must connect by address");
            warning = Some(format!(
                "mDNS failed ({e}) — guests must type this computer's address to join"
            ));
        }
    }

    commands.insert_resource(HostSessionInfo {
        session_name: request.session_name.clone(),
        port: request.port,
        clients: 0,
        pin: session_pin,
    });
    if let Ok(mut window) = window.single_mut() {
        window.title = format!("LocalGPT Gen — Hosting '{}'", request.session_name);
    }

    *control = HostControl::Active { warning };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_info(pin: Option<String>, clients: usize) -> HostSessionInfo {
        HostSessionInfo {
            session_name: "test world".into(),
            port: 9879,
            clients,
            pin,
        }
    }

    #[test]
    fn session_summary_mentions_pin_open_and_guests() {
        assert_eq!(
            session_info(Some("123456".into()), 0).summary(),
            "Hosting 'test world' · PIN 123456"
        );
        assert_eq!(
            session_info(Some("123456".into()), 1).summary(),
            "Hosting 'test world' · PIN 123456 · 1 guest"
        );
        assert_eq!(
            session_info(None, 3).summary(),
            "Hosting 'test world' · open session (no PIN) · 3 guests"
        );
    }

    #[test]
    fn host_control_tracks_lifecycle() {
        assert!(!HostControl::NotHosting.is_active());
        assert!(
            !HostControl::StartRequested(HostStartRequest {
                session_name: "w".into(),
                port: 9879,
                open: false,
                web: false,
                web_edit: false,
                resume: None,
                relay: None,
            })
            .is_active()
        );
        assert!(HostControl::Active { warning: None }.is_active());
        assert!(!HostControl::Failed("x".into()).is_active());
    }
}
