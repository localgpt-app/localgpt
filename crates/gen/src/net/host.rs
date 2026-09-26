//! Host-side session lifecycle: brings a hosted session up (secrets, session
//! HTTP with the ops room, mDNS) and runs the prompt job queue whose worker
//! is the agent loop.
//!
//! The room itself (document, peers, ops fan-out) lives in
//! [`super::web::WebRoom`]; the scene projection there turns the host's
//! world into ops. This module owns only what the room doesn't: session
//! setup, the job queue, job scaffolds in the host's own window, and the
//! agent loop's chat channel.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use bevy::prelude::*;
use tokio::sync::mpsc;

use super::jobs::{JobId, JobQueue};
use super::mdns::SessionAnnouncer;
use super::web::{self, WebRoom};

/// A client prompt handed to the agent loop as a job.
#[derive(Debug, Clone)]
pub struct RemoteJob {
    pub job_id: JobId,
    /// Full text for the agent (prompt + spatial context).
    pub agent_prompt: String,
    /// What the user typed (for console/chat echo).
    pub display: String,
}

/// Worker → host progress reports for a [`RemoteJob`].
#[derive(Debug, Clone)]
pub enum JobEvent {
    Started(JobId),
    Finished {
        job_id: JobId,
        error: Option<String>,
    },
}

/// A chat line from the agent loop, shown to every guest.
#[derive(Debug, Clone)]
pub struct HostChat {
    pub speaker: String,
    pub text: String,
}

/// Host-side options, built by the CLI when `--host` is given.
pub struct NetHostOptions {
    /// Session name shown in mDNS discovery and to joining clients.
    pub session_name: String,
    /// Port to listen on.
    pub port: u16,
    /// Sender half of the job channel into the agent loop.
    pub job_tx: mpsc::UnboundedSender<RemoteJob>,
    /// Receiver half of the chat channel out of the agent loop.
    pub chat_rx: mpsc::UnboundedReceiver<HostChat>,
    /// Receiver half of worker progress events.
    pub job_events_rx: mpsc::UnboundedReceiver<JobEvent>,
    /// Skip the PIN: anyone on the LAN can join (trusted networks only).
    pub open: bool,
    /// Run remote prompts on the host's own agent with all of its tools
    /// (`--remote-tools full`). Default: a scene-only agent
    /// (see [`super::remote_scope`]).
    pub full_access: bool,
    /// Start hosting as soon as the app runs (CLI `--host`). When false the
    /// plugin stays dormant until the prompt panel requests a session via
    /// [`HostControl`].
    pub autostart: bool,
    /// Let browsers join as guests: serve a join page and the WebSocket ops
    /// endpoint on the session port (CLI `--web`).
    pub web: bool,
    /// Browser guests join as editors who may move/rotate/scale/delete
    /// entities directly (CLI `--web-edit`). Default: prompt-only guests.
    pub web_edit: bool,
    /// Replay a previous session's op log before guests join (CLI `--resume`).
    pub resume: Option<String>,
    /// Sender half of the control channel into the agent loop (hosting
    /// started notifications).
    pub control_tx: mpsc::UnboundedSender<HostControlEvent>,
}

/// What joiners need to know about the running session, for in-window
/// display (the desktop prompt panel).
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
/// the app runs) or from the prompt panel (at any time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostStartRequest {
    pub session_name: String,
    pub port: u16,
    /// Open session: no PIN (trusted LANs only).
    pub open: bool,
    /// Remote prompts run on the host's own agent with full tool access.
    pub full_access: bool,
    /// Browsers may join as guests (join page + WebSocket endpoint).
    pub web: bool,
    /// Browser guests get the editor role (direct edits).
    pub web_edit: bool,
    /// Op log to replay at start (`--resume`; panel sessions start fresh).
    pub resume: Option<String>,
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
    /// (e.g. mDNS failed) for the panel to show.
    Active { warning: Option<String> },
    /// The start failed (e.g. the port is taken); the panel shows the
    /// reason and hosting can be requested again.
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

/// Agent-loop side of the job/chat bridge.
pub struct AgentNetHooks {
    /// Jobs dispatched from the host's prompt queue (one at a time).
    pub job_rx: mpsc::UnboundedReceiver<RemoteJob>,
    /// Outbound chat events shown to connected clients.
    pub chat_tx: mpsc::UnboundedSender<HostChat>,
    /// Progress reports for dispatched jobs.
    pub job_events_tx: mpsc::UnboundedSender<JobEvent>,
    /// Lifecycle notifications (hosting started) from the net systems.
    pub control_rx: mpsc::UnboundedReceiver<HostControlEvent>,
}

/// Net systems → agent loop: hosting is live, build the remote worker now.
///
/// Sent by [`host_lifecycle`] both for CLI-started sessions (`--host`) and
/// panel-started ones, so the agent loop learns about remote prompts
/// exactly when they become possible.
#[derive(Debug, Clone)]
pub enum HostControlEvent {
    HostingStarted {
        /// Run remote prompts with the host agent's full tool access.
        full_access: bool,
    },
}

/// Create a matched (options, hooks) pair for the host.
pub fn create_host_channels() -> (NetHostOptions, AgentNetHooks) {
    let (job_tx, job_rx) = mpsc::unbounded_channel();
    let (chat_tx, chat_rx) = mpsc::unbounded_channel();
    let (job_events_tx, job_events_rx) = mpsc::unbounded_channel();
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    (
        NetHostOptions {
            session_name: String::new(),
            port: super::DEFAULT_PORT,
            job_tx,
            chat_rx,
            job_events_rx,
            open: false,
            full_access: false,
            autostart: false,
            web: false,
            web_edit: false,
            resume: None,
            control_tx,
        },
        AgentNetHooks {
            job_rx,
            chat_tx,
            job_events_tx,
            control_rx,
        },
    )
}

/// Resource: the prompt queue plus the channels to/from its worker, and the
/// scaffold entities marking running builds in the host's own window.
#[derive(Resource)]
pub(crate) struct HostJobs {
    queue: JobQueue,
    /// Translucent marker per job (its anchor), shown while it runs.
    scaffolds: HashMap<JobId, (Entity, [f32; 3])>,
    /// A job has been handed to the worker and not yet finished.
    dispatched: Option<JobId>,
    job_tx: mpsc::UnboundedSender<RemoteJob>,
    events_rx: Mutex<mpsc::UnboundedReceiver<JobEvent>>,
}

/// Resource: chat events published by the agent loop, drained and broadcast
/// each frame.
#[derive(Resource)]
struct HostChatOutbox {
    rx: Mutex<mpsc::UnboundedReceiver<HostChat>>,
}

impl HostJobs {
    /// Enqueue a web guest's prompt with its authority-assigned job id, so
    /// wire messages and the worker agree on the number. Web jobs share the
    /// queue's capacity and the single dispatch slot with native prompts.
    pub(crate) fn enqueue_web_prompt(
        &mut self,
        job_id: JobId,
        requester: u64,
        prompt: &str,
        anchor: Option<[f32; 3]>,
    ) -> Result<(), String> {
        self.queue
            .enqueue_with_id(job_id, requester, prompt, anchor)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Drop a departed web guest's queued prompts.
    pub(crate) fn cancel_web_requester(&mut self, requester: u64) {
        self.queue.cancel_requester(requester);
    }

    /// The web peer whose prompt the worker is currently building, if any
    /// (so the projection can attribute — and let them undo — the ops).
    pub(crate) fn running_web_requester(&self) -> Option<u64> {
        let id = self.dispatched?;
        let job = self.queue.get(id)?;
        job.requester.checked_sub(web::WEB_REQUESTER_OFFSET)
    }
}

/// Resource owning the mDNS announcer (unregisters on drop).
#[derive(Resource)]
struct HostAnnouncer {
    _announcer: SessionAnnouncer,
}

/// The host plugin. Always installed; dormant until a session starts.
pub struct NetHostPlugin {
    /// Consumed on build (contains non-clonable channel halves).
    pub options: std::sync::Mutex<Option<NetHostOptions>>,
}

impl Plugin for NetHostPlugin {
    fn build(&self, app: &mut App) {
        let NetHostOptions {
            session_name,
            port,
            job_tx,
            chat_rx,
            job_events_rx,
            open,
            full_access,
            autostart,
            web,
            web_edit,
            resume,
            control_tx,
        } = self
            .options
            .lock()
            .expect("NetHostPlugin options lock poisoned")
            .take()
            .expect("NetHostPlugin options consumed twice");

        // Dormant resources: channels exist from the start (the agent loop
        // always wires the hooks), but nothing listens, announces, or
        // replicates until a session is requested through `HostControl` —
        // at startup (`--host`) or later (the prompt panel's Collaborate
        // section).
        let control = if autostart {
            HostControl::StartRequested(HostStartRequest {
                session_name,
                port,
                open,
                full_access,
                web,
                web_edit,
                resume,
            })
        } else {
            HostControl::NotHosting
        };
        app.insert_resource(control)
            .insert_resource(ControlOutbox { tx: control_tx })
            .insert_resource(HostJobs {
                queue: JobQueue::default(),
                scaffolds: HashMap::new(),
                dispatched: None,
                job_tx,
                events_rx: Mutex::new(job_events_rx),
            })
            .insert_resource(HostChatOutbox {
                rx: Mutex::new(chat_rx),
            })
            .init_resource::<super::guest_avatars::GuestAvatars>()
            .init_resource::<web::PendingSceneOps>()
            .add_systems(PreUpdate, host_lifecycle)
            .add_systems(
                Update,
                (
                    net_job_events,
                    net_job_dispatch.after(net_job_events),
                    net_chat_broadcast,
                )
                    .run_if(hosting),
            )
            // The ops room (browser page, native ops guests): the room
            // resource exists only while a session is live.
            .add_systems(
                Update,
                (
                    web::web_drain_inbound,
                    web::web_apply_scene_ops,
                    web::web_projection_sync,
                )
                    .chain()
                    .run_if(|room: Option<Res<web::WebRoom>>| room.is_some()),
            )
            // Guest avatars in the host's own window.
            .add_systems(Update, super::guest_avatars::web_guest_avatars);
    }
}

/// Resource holding the agent-loop control sender.
#[derive(Resource)]
struct ControlOutbox {
    tx: mpsc::UnboundedSender<HostControlEvent>,
}

/// Bring a requested session up: the PIN, session HTTP (join page +
/// WebSocket ops endpoint), the ops room, and mDNS. Runs in `PreUpdate` so
/// the gated host systems see [`HostControl::Active`] from the same frame on.
///
/// This is the single start path: `--host` requests it before the first
/// frame, the panel's Collaborate section at any later one.
fn host_lifecycle(
    mut commands: Commands,
    mut control: ResMut<HostControl>,
    mut window: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    outbox: Res<ControlOutbox>,
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

    // Notify the agent loop that hosting is live so it can build the
    // remote-prompt worker.
    let _ = outbox.tx.send(HostControlEvent::HostingStarted {
        full_access: request.full_access,
    });

    *control = HostControl::Active { warning };
}

/// Hand the next queued job to the worker (one at a time) and put up its
/// scaffold marker.
fn net_job_dispatch(
    mut commands: Commands,
    mut jobs: ResMut<HostJobs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if jobs.dispatched.is_some() {
        return;
    }
    let Some(job) = jobs.queue.start_next() else {
        return;
    };
    let remote = RemoteJob {
        job_id: job.id,
        agent_prompt: job.agent_prompt(),
        display: job.prompt.clone(),
    };
    if jobs.job_tx.send(remote).is_err() {
        // Worker gone (agent loop exited) — drop the job.
        jobs.queue.finish(job.id);
        return;
    }
    jobs.dispatched = Some(job.id);

    // Scaffold: a translucent amber marker at the build anchor.
    let at = job.anchor.unwrap_or([0.0; 3]);
    let marker = commands
        .spawn((
            Name::new(format!("Scaffold #{}", job.id)),
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.6, 0.1, 0.35),
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::from_translation(Vec3::from_array(at)),
        ))
        .id();
    jobs.scaffolds.insert(job.id, (marker, at));
}

/// Apply worker progress: despawn scaffolds, retire finished jobs, and tee
/// everything to the ops room so guests see job states too.
fn net_job_events(
    mut commands: Commands,
    mut jobs: ResMut<HostJobs>,
    mut web_room: Option<ResMut<web::WebRoom>>,
) {
    let events: Vec<JobEvent> = {
        let Ok(mut rx) = jobs.events_rx.lock() else {
            return;
        };
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    for event in events {
        // Guests hear about every job through the authority.
        if let Some(room) = web_room.as_deref_mut() {
            web::tee_job_event(room, &event);
        }
        match event {
            JobEvent::Started(_) => {}
            JobEvent::Finished { job_id, .. } => {
                jobs.queue.finish(job_id);
                if jobs.dispatched == Some(job_id) {
                    jobs.dispatched = None;
                }
                if let Some((marker, _)) = jobs.scaffolds.remove(&job_id) {
                    commands.entity(marker).despawn();
                }
            }
        }
    }
}

/// Broadcast agent-loop chat events to every guest through the room.
fn net_chat_broadcast(outbox: Res<HostChatOutbox>, mut web_room: Option<ResMut<web::WebRoom>>) {
    let Ok(mut rx) = outbox.rx.lock() else {
        return;
    };
    while let Ok(chat) = rx.try_recv() {
        if let Some(room) = web_room.as_deref_mut() {
            let kind = if chat.speaker == "client" {
                localgpt_world_sync::ChatKind::Human
            } else {
                localgpt_world_sync::ChatKind::Agent
            };
            let out = room.authority.post_chat(&chat.speaker, kind, &chat.text);
            web::deliver(room, out);
        }
    }
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
                full_access: false,
                web: false,
                web_edit: false,
                resume: None,
            })
            .is_active()
        );
        assert!(HostControl::Active { warning: None }.is_active());
        assert!(!HostControl::Failed("x".into()).is_active());
    }
}
