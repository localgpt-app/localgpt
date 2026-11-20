//! Sharing the world in the window, joining someone's, the sessions kept on
//! this computer, and Gen's tools for an agent: what the bar's Share and MCP
//! panels drive (`docs/rfcs/authoring/one-app-and-cli.md`, step 3).
//!
//! - **Share** hosts Gen's room (`localgpt_gen::net::host`) over the world
//!   in the window: people on this network join from a browser or another
//!   LocalGPT, and people elsewhere through a relay. Guests watch, walk and
//!   talk; none of them edits. An edit of theirs would change the scene
//!   outside the one authority a live canvas has, so editing guests wait for
//!   the room `rooms-and-the-channel.md` designs, which commits through it.
//! - **Join** opens someone's session in a window of its own
//!   (`localgpt-app --join`), after its host has been asked here, so that a
//!   session that is gone, or wants a PIN nobody typed, is said here and not
//!   in a window that never comes up.
//! - **Sessions** are the ones hosted on this computer, a package each in
//!   Gen's workspace: one plays back as a time-lapse in a window of its own
//!   (`localgpt-app --replay`), or goes out as one `.world` file.
//! - **MCP** serves Gen's tools over the world in this window through Gen's
//!   relay, for an agent whose MCP server is `localgpt gen mcp-server
//!   --connect`. Not on a live canvas, whose world changes only through its
//!   API (`localgpt world mcp`), nor in a replay or a guest's window, whose
//!   scenes a log and a room drive.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use localgpt_gen::gen3d::GenBridge;
use localgpt_gen::gen3d::mcp_relay::{McpRelay, start_mcp_relay};
use localgpt_gen::net::host::{HostControl, HostSessionInfo, HostStartRequest};
use localgpt_gen::net::join::SessionInfo;
use localgpt_gen::net::ops_client::{ClientEntry, ClientPanelLog};
use localgpt_gen::net::web::WebRoom;
use localgpt_world_agent::session;
use serde::Serialize;

/// Saved sessions the panel lists, the newest first.
const SESSIONS_SHOWN: usize = 8;

/// What a window opened from here must not take from this one: the dev
/// hooks that drive this window, and the phone's listener, which is this
/// window's (a second one would find its port taken).
const NOT_INHERITED: &[&str] = &[
    crate::shell::LOCALGPT_APP_SCREENSHOT,
    crate::shell::LOCALGPT_APP_SCREENSHOT_AFTER,
    crate::shell::LOCALGPT_APP_SHELL_DRIVE,
    crate::open::LOCALGPT_APP_OPEN,
    crate::open::LOCALGPT_APP_OPEN_AFTER,
];

/// What the window shows, which decides what it may share.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The app's own scene: a document's, a song's, a world's.
    #[default]
    Scene,
    /// A package's live canvas: its authority owns the scene.
    Canvas,
    /// A session played back.
    Replay,
    /// Someone's session, as their guest.
    Guest,
}

impl Mode {
    /// A guest's window shows someone else's session: it is theirs to share.
    pub fn can_host(self) -> bool {
        self != Mode::Guest
    }

    /// Why Gen's tools may not drive this window's scene, when they may not.
    pub fn mcp_refused(self) -> Option<&'static str> {
        match self {
            Mode::Scene => None,
            Mode::Canvas => Some(
                "A live world changes only through its API: give an agent \
                 `localgpt world mcp` as its MCP server instead.",
            ),
            Mode::Replay => Some("A replay plays a session's log; tools would race it."),
            Mode::Guest => Some("A guest's world is the host's: the room drives it."),
        }
    }
}

pub struct SharePlugin {
    pub mode: Mode,
    /// Gen's tool channel into this window's scene, which the relay serves.
    pub bridge: Arc<GenBridge>,
    /// Gen's workspace, where sessions are kept.
    pub workspace: PathBuf,
}

impl Plugin for SharePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Share::new(
            self.mode,
            self.bridge.clone(),
            self.workspace.clone(),
        ))
        .add_message::<ShareAsk>()
        .add_systems(Update, (asks, track).chain());
    }
}

/// What the bar's Share and MCP panels ask.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum ShareAsk {
    /// Share the world in the window: with browsers on this network too
    /// (`web`), and with anyone through a relay.
    Host {
        name: String,
        web: bool,
        relay: Option<String>,
    },
    Stop,
    /// Gen's tools over this window's world, through the relay.
    Mcp(bool),
    /// The saved sessions, again.
    Sessions,
    /// Browse this network for sessions.
    Find,
    /// Someone's session, in a window of its own.
    Join {
        addr: String,
        pin: Option<String>,
    },
    /// A saved session played back, in a window of its own.
    Replay(PathBuf),
    /// A saved session, as one `.world` file.
    Export(PathBuf),
}

/// What the panels show.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct ShareView {
    pub mode: Mode,
    /// The name a session takes when no other is typed.
    pub name: String,
    pub hosting: Option<HostingView>,
    /// A share was asked for and is starting.
    pub starting: bool,
    /// Why the last share did not start.
    pub error: Option<String>,
    pub mcp: McpView,
    pub sessions: Vec<SessionView>,
    /// The sessions found on this network, once browsed.
    pub found: Option<Vec<FoundView>>,
    pub finding: bool,
    /// A host is being asked about its session.
    pub joining: bool,
    /// What was last done or refused: a window opened, an export, a PIN
    /// missing.
    pub note: Option<String>,
    /// In a guest's window: how the connection stands.
    pub guest: Option<String>,
}

/// A share that is live.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct HostingView {
    pub name: String,
    /// What a guest types to join, unless the session is open.
    pub pin: Option<String>,
    /// The link for a browser on this network.
    pub browser: Option<String>,
    /// What another LocalGPT joins at: this computer and the session port.
    pub address: Option<String>,
    /// A relay was asked for.
    pub relay: bool,
    /// The link for anyone, once the relay has given a room.
    pub internet: Option<String>,
    pub relay_error: Option<String>,
    pub guests: usize,
    /// Trouble that did not stop the share, such as no LAN announcement.
    pub warning: Option<String>,
}

/// Gen's relay over this window's world.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct McpView {
    pub on: bool,
    pub port: Option<u16>,
    pub error: Option<String>,
    /// Why it cannot be on in this window, when it cannot.
    pub refused: Option<&'static str>,
}

/// A session kept on this computer.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct SessionView {
    pub dir: PathBuf,
    pub name: String,
    /// The newest revision its log reaches: how many edits made it.
    pub revisions: u64,
    pub updated_ms: u64,
}

/// A session announced on this network.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct FoundView {
    pub name: String,
    pub addr: String,
}

/// What threads and dialogs answer.
enum Done {
    Found(Result<Vec<FoundView>, String>),
    Asked {
        session: Result<SessionInfo, String>,
        pin: Option<String>,
    },
    Saved {
        from: PathBuf,
        to: Option<PathBuf>,
    },
    Exported(Result<PathBuf, String>),
}

#[derive(Resource)]
pub struct Share {
    mode: Mode,
    bridge: Arc<GenBridge>,
    workspace: PathBuf,
    view: ShareView,
    rev: u64,
    /// The relay's runtime, made the first time it is switched on.
    runtime: Option<tokio::runtime::Runtime>,
    relay: Option<McpRelay>,
    mcp_error: Option<String>,
    sessions: Vec<SessionView>,
    found: Option<Vec<FoundView>>,
    finding: bool,
    joining: bool,
    note: Option<String>,
    /// This computer's address and the session port, while sharing.
    address: Option<String>,
    tx: Sender<Done>,
    rx: Mutex<Receiver<Done>>,
}

impl Share {
    fn new(mode: Mode, bridge: Arc<GenBridge>, workspace: PathBuf) -> Self {
        let (tx, rx) = channel();
        let mut share = Self {
            mode,
            bridge,
            workspace,
            view: ShareView::default(),
            rev: 0,
            runtime: None,
            relay: None,
            mcp_error: None,
            sessions: Vec::new(),
            found: None,
            finding: false,
            joining: false,
            note: None,
            address: None,
            tx,
            rx: Mutex::new(rx),
        };
        share.list_sessions();
        share
    }

    /// Bumped whenever the view changes, so it is sent only then.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn view(&self) -> &ShareView {
        &self.view
    }

    fn host(
        &mut self,
        control: Option<&mut HostControl>,
        name: &str,
        web: bool,
        relay: Option<&str>,
    ) {
        let Some(control) = control.filter(|_| self.mode.can_host()) else {
            self.note = Some("A guest's window shares nothing: the session is the host's.".into());
            return;
        };
        if control.is_active() || matches!(control, HostControl::StartRequested(_)) {
            return;
        }
        let relay = relay.map(str::trim).filter(|url| !url.is_empty());
        if let Some(url) = relay
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            self.note = Some(format!(
                "A relay is an http:// or https:// address, not {url}"
            ));
            return;
        }
        let name = match name.trim() {
            "" => localgpt_gen::net::default_session_name(),
            name => name.to_string(),
        };
        *control = HostControl::StartRequested(HostStartRequest {
            session_name: name,
            port: localgpt_gen::net::DEFAULT_PORT,
            open: false,
            web,
            // Guests watch: an edit of theirs would go round the authority.
            web_edit: false,
            resume: None,
            relay: relay.map(str::to_string),
        });
        self.note = None;
    }

    fn set_mcp(&mut self, on: bool) {
        self.mcp_error = None;
        if !on {
            // Dropping the handle stops the relay and its clients.
            self.relay = None;
            return;
        }
        if self.relay.is_some() {
            return;
        }
        if let Some(why) = self.mode.mcp_refused() {
            self.mcp_error = Some(why.to_string());
            return;
        }
        if self.runtime.is_none() {
            match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("mcp-relay")
                .enable_all()
                .build()
            {
                Ok(runtime) => self.runtime = Some(runtime),
                Err(e) => {
                    self.mcp_error = Some(format!("no runtime for the relay: {e}"));
                    return;
                }
            }
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        match runtime.block_on(start_mcp_relay(self.bridge.clone())) {
            Ok(relay) => self.relay = Some(relay),
            Err(e) => self.mcp_error = Some(format!("the relay couldn't listen: {e:#}")),
        }
    }

    fn list_sessions(&mut self) {
        self.sessions = session::list(&self.workspace)
            .into_iter()
            .take(SESSIONS_SHOWN)
            .map(|s| SessionView {
                dir: s.dir,
                name: s.name,
                revisions: s.head_revision,
                updated_ms: s.updated_ms,
            })
            .collect();
    }

    fn find(&mut self) {
        if self.finding {
            return;
        }
        self.finding = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let found = localgpt_gen::net::join::browse()
                .map(|sessions| {
                    sessions
                        .into_iter()
                        .map(|s| FoundView {
                            name: s.session_name,
                            addr: s.addr.to_string(),
                        })
                        .collect()
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Done::Found(found));
        });
    }

    fn join(&mut self, addr: &str, pin: Option<&str>) {
        if self.joining {
            return;
        }
        let addr = match localgpt_gen::net::parse_peer_addr(addr.trim()) {
            Ok(addr) => addr,
            Err(e) => {
                self.note = Some(e.to_string());
                return;
            }
        };
        let pin = pin
            .map(localgpt_gen::net::normalize_pin)
            .filter(|pin| !pin.is_empty());
        self.joining = true;
        self.note = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let session = localgpt_gen::net::join::ask(addr).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Done::Asked { session, pin });
        });
    }

    fn export(&mut self, dir: PathBuf) {
        let tx = self.tx.clone();
        let name = session::export_name(&dir).to_string_lossy().into_owned();
        let task = async move {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Export the session as one .world file")
                .set_file_name(name)
                .add_filter("World", &["world"])
                .save_file()
                .await;
            let _ = tx.send(Done::Saved {
                from: dir,
                to: picked.map(|file| file.path().to_path_buf()),
            });
        };
        AsyncComputeTaskPool::get().spawn(task).detach();
    }

    /// What the threads and dialogs answered.
    fn receive(&mut self) {
        let done: Vec<Done> = match self.rx.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => return,
        };
        for done in done {
            match done {
                Done::Found(Ok(found)) => {
                    self.finding = false;
                    self.note = found
                        .is_empty()
                        .then(|| "No session is being shared on this network.".into());
                    self.found = Some(found);
                }
                Done::Found(Err(e)) => {
                    self.finding = false;
                    self.note = Some(format!("Couldn't browse this network: {e}"));
                }
                Done::Asked { session, pin } => {
                    self.joining = false;
                    self.note = Some(match session {
                        Err(e) => e,
                        Ok(session) if session.needs_pin && pin.is_none() => {
                            format!("'{}' asks for its PIN: type it, then Join.", session.name)
                        }
                        Ok(session) => {
                            let addr = OsString::from(session.addr.to_string());
                            match open_window(&[OsString::from("--join"), addr], pin.as_deref()) {
                                Ok(()) => {
                                    format!("'{}' opens in a window of its own.", session.name)
                                }
                                Err(e) => e,
                            }
                        }
                    });
                }
                Done::Saved { from, to: Some(to) } => {
                    self.note = Some(format!("Exporting to {} …", to.display()));
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let exported = session::export_zip(&from, &to)
                            .map(|()| to)
                            .map_err(|e| format!("Couldn't export {}: {e}", from.display()));
                        let _ = tx.send(Done::Exported(exported));
                    });
                }
                Done::Saved { to: None, .. } => {}
                Done::Exported(Ok(to)) => {
                    self.note = Some(format!("Exported {}", to.display()));
                }
                Done::Exported(Err(e)) => self.note = Some(e),
            }
        }
    }

    /// The view, from what Gen's room and the client say and what is kept
    /// here.
    fn view_of(
        &self,
        control: Option<&HostControl>,
        info: Option<&HostSessionInfo>,
        room: Option<&WebRoom>,
        guest: Option<&ClientPanelLog>,
    ) -> ShareView {
        let (starting, error) = match control {
            Some(HostControl::StartRequested(_)) => (true, None),
            Some(HostControl::Failed(why)) => (false, Some(why.clone())),
            _ => (false, None),
        };
        let hosting = match (control, info) {
            (Some(HostControl::Active { warning }), Some(info)) => {
                let relay = info.relay();
                Some(HostingView {
                    name: info.session_name.clone(),
                    pin: info.pin(),
                    browser: info.browser_link.clone(),
                    address: self.address.clone(),
                    relay: relay.is_some(),
                    internet: relay.as_ref().and_then(|r| r.link.clone()),
                    relay_error: relay.and_then(|r| r.error),
                    guests: room.map_or(0, |room| room.authority.peer_count()),
                    warning: warning.clone(),
                })
            }
            _ => None,
        };
        ShareView {
            mode: self.mode,
            name: localgpt_gen::net::default_session_name(),
            hosting,
            starting,
            error,
            mcp: McpView {
                on: self.relay.is_some(),
                port: self.relay.as_ref().map(McpRelay::port),
                error: self.mcp_error.clone(),
                refused: self.mode.mcp_refused(),
            },
            sessions: self.sessions.clone(),
            found: self.found.clone(),
            finding: self.finding,
            joining: self.joining,
            note: self.note.clone(),
            guest: guest.and_then(guest_line),
        }
    }

    fn show(&mut self, view: ShareView) {
        if view != self.view {
            self.view = view;
            self.rev += 1;
        }
    }
}

/// How a guest's connection stands: the client's last word on it.
fn guest_line(log: &ClientPanelLog) -> Option<String> {
    log.entries.iter().rev().find_map(|entry| match entry {
        ClientEntry::Status(status) => Some(status.clone()),
        ClientEntry::Error(error) => Some(format!("error: {error}")),
        ClientEntry::You(_) | ClientEntry::Chat { .. } => None,
    })
}

fn asks(
    mut asks: MessageReader<ShareAsk>,
    mut share: ResMut<Share>,
    mut control: Option<ResMut<HostControl>>,
) {
    for ask in asks.read() {
        match ask {
            ShareAsk::Host { name, web, relay } => {
                share.host(control.as_deref_mut(), name, *web, relay.as_deref())
            }
            ShareAsk::Stop => {
                if let Some(control) = control.as_deref_mut()
                    && control.is_active()
                {
                    *control = HostControl::StopRequested;
                }
            }
            ShareAsk::Mcp(on) => share.set_mcp(*on),
            ShareAsk::Sessions => share.list_sessions(),
            ShareAsk::Find => share.find(),
            ShareAsk::Join { addr, pin } => share.join(addr, pin.as_deref()),
            ShareAsk::Replay(dir) => {
                share.note = Some(
                    match open_window(&[OsString::from("--replay"), dir.into()], None) {
                        Ok(()) => format!("{} plays in a window of its own.", file_name(dir)),
                        Err(e) => e,
                    },
                );
            }
            ShareAsk::Export(dir) => share.export(dir.clone()),
        }
    }
}

fn track(
    mut share: ResMut<Share>,
    control: Option<Res<HostControl>>,
    info: Option<Res<HostSessionInfo>>,
    room: Option<Res<WebRoom>>,
    guest: Option<Res<ClientPanelLog>>,
    mut was_hosting: Local<bool>,
    mut had_room: Local<bool>,
) {
    share.receive();
    let hosting = control.as_deref().is_some_and(HostControl::is_active);
    if hosting && !*was_hosting {
        share.address = info.as_deref().map(|info| {
            let host = localgpt_gen::net::web::primary_lan_ip()
                .map_or_else(|| "<this-computer>".to_string(), |ip| ip.to_string());
            format!("{host}:{}", info.port)
        });
    }
    if *was_hosting && !hosting {
        share.address = None;
    }
    *was_hosting = hosting;
    // The session that ended is a saved one now. Listed once its room has
    // gone, a frame after the stop was asked for: the room writes how far
    // its log reached as it goes.
    if *had_room && room.is_none() {
        share.list_sessions();
    }
    *had_room = room.is_some();
    let view = share.view_of(
        control.as_deref(),
        info.as_deref(),
        room.as_deref(),
        guest.as_deref(),
    );
    share.show(view);
}

/// This binary, again, opening `args` in a window of its own: without this
/// window's dev hooks or its phone listener, and with a guest's PIN in the
/// environment rather than on a command line anyone can list.
fn window_command(exe: &Path, args: &[OsString], pin: Option<&str>) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(args).stdin(std::process::Stdio::null());
    for var in NOT_INHERITED {
        command.env_remove(var);
    }
    command.env(crate::webui::LOCALGPT_APP_REMOTE, "off");
    match pin {
        Some(pin) => command.env(localgpt_gen::net::JOIN_PIN_ENV, pin),
        None => command.env_remove(localgpt_gen::net::JOIN_PIN_ENV),
    };
    command
}

fn open_window(args: &[OsString], pin: Option<&str>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find this app: {e}"))?;
    let mut child = window_command(&exe, args, pin)
        .spawn()
        .map_err(|e| format!("Couldn't open a window: {e}"))?;
    // Reaped when it closes, so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_gen::gen3d::create_gen_channels;

    fn share_in(mode: Mode) -> Share {
        let (bridge, _channels) = create_gen_channels();
        Share::new(mode, bridge, PathBuf::from("/nonexistent/workspace"))
    }

    #[test]
    fn a_share_asks_gens_room_for_watching_guests_only() {
        let mut share = share_in(Mode::Canvas);
        let mut control = HostControl::NotHosting;
        share.host(
            Some(&mut control),
            "  Harbour  ",
            true,
            Some(" https://relay.example "),
        );
        let HostControl::StartRequested(request) = &control else {
            panic!("{control:?}");
        };
        assert_eq!(request.session_name, "Harbour");
        assert!(request.web);
        assert!(!request.web_edit, "guests never edit from here");
        assert!(!request.open, "a share has a PIN");
        assert_eq!(request.relay.as_deref(), Some("https://relay.example"));
        assert_eq!(request.port, localgpt_gen::net::DEFAULT_PORT);

        // No name: the default one. A relay that is not a URL: refused.
        let mut control = HostControl::NotHosting;
        share.host(Some(&mut control), "", false, Some("relay.example"));
        assert_eq!(control, HostControl::NotHosting);
        assert!(share.note.as_deref().unwrap().contains("http"));
        share.host(Some(&mut control), "", false, Some("  "));
        let HostControl::StartRequested(request) = &control else {
            panic!("{control:?}");
        };
        assert_eq!(
            request.session_name,
            localgpt_gen::net::default_session_name()
        );
        assert_eq!(request.relay, None);

        // A guest's window has nothing of its own to share.
        let mut guest = share_in(Mode::Guest);
        let mut control = HostControl::NotHosting;
        guest.host(Some(&mut control), "x", true, None);
        assert_eq!(control, HostControl::NotHosting);
    }

    #[test]
    fn gens_tools_drive_only_the_apps_own_scene() {
        assert_eq!(Mode::Scene.mcp_refused(), None);
        for mode in [Mode::Canvas, Mode::Replay, Mode::Guest] {
            let mut share = share_in(mode);
            share.set_mcp(true);
            assert!(share.relay.is_none(), "{mode:?}");
            assert_eq!(share.mcp_error.as_deref(), mode.mcp_refused());
            let view = share.view_of(None, None, None, None);
            assert!(!view.mcp.on);
            assert!(view.mcp.refused.is_some());
        }
        assert!(
            Mode::Canvas
                .mcp_refused()
                .unwrap()
                .contains("localgpt world mcp")
        );
    }

    #[test]
    fn a_window_opened_from_here_takes_no_hooks_and_no_listener() {
        let exe = Path::new("/Applications/LocalGPT.app/Contents/MacOS/localgpt-app");
        let command = window_command(
            exe,
            &[OsString::from("--join"), OsString::from("192.168.1.5:9879")],
            Some("482913"),
        );
        assert_eq!(command.get_program(), exe.as_os_str());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--join", "192.168.1.5:9879"]);
        let envs: std::collections::HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_owned())))
            .collect();
        assert_eq!(
            envs[localgpt_gen::net::JOIN_PIN_ENV],
            Some(OsString::from("482913")),
            "the PIN goes in the environment, never in the arguments"
        );
        assert_eq!(
            envs[crate::webui::LOCALGPT_APP_REMOTE],
            Some(OsString::from("off"))
        );
        for var in NOT_INHERITED {
            assert_eq!(envs[*var], None, "{var} is removed");
        }

        let replay = window_command(exe, &[OsString::from("--replay")], None);
        let envs: Vec<_> = replay.get_envs().collect();
        assert!(envs.contains(&(std::ffi::OsStr::new(localgpt_gen::net::JOIN_PIN_ENV), None)));
    }

    #[test]
    fn a_guests_window_says_how_its_connection_stands() {
        let mut log = ClientPanelLog::default();
        assert_eq!(guest_line(&log), None);
        log.entries
            .push_back(ClientEntry::Status("connected".into()));
        log.entries.push_back(ClientEntry::Chat {
            speaker: "maya".into(),
            text: "hi".into(),
        });
        assert_eq!(guest_line(&log).as_deref(), Some("connected"));
        log.entries
            .push_back(ClientEntry::Error("couldn't connect".into()));
        assert_eq!(guest_line(&log).as_deref(), Some("error: couldn't connect"));
    }

    #[test]
    fn a_stopped_session_is_listed_with_how_far_it_got() {
        use localgpt_world_types as wt;

        let workspace = std::env::temp_dir().join(format!(
            "lga-share-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&workspace);
        let (bridge, _channels) = create_gen_channels();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(SharePlugin {
            mode: Mode::Scene,
            bridge,
            workspace: workspace.clone(),
        });
        let (web, inbound) = localgpt_gen::net::web::WebBridge::new(None, "castle");
        app.insert_resource(WebRoom::new(
            "castle", web, inbound, &workspace, None, false, None,
        ));
        app.update();
        assert!(app.world().resource::<Share>().view().sessions.is_empty());

        // An edit, then the room goes, as a stop takes it: the list is read
        // after it has written how far its log reached.
        app.world_mut()
            .resource_mut::<WebRoom>()
            .authority
            .apply_replay(&[wt::EditOp::spawn(wt::WorldEntity::new(1, "tower"))])
            .unwrap();
        app.world_mut().remove_resource::<WebRoom>();
        app.update();
        let revisions: Vec<u64> = app
            .world()
            .resource::<Share>()
            .view()
            .sessions
            .iter()
            .map(|s| s.revisions)
            .collect();
        assert_eq!(revisions, [1]);
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn the_view_changes_only_when_something_does() {
        let mut share = share_in(Mode::Scene);
        let view = share.view_of(None, None, None, None);
        share.show(view.clone());
        let rev = share.rev();
        share.show(view);
        assert_eq!(share.rev(), rev);
        share.note = Some("Exported x.world".into());
        let view = share.view_of(
            Some(&HostControl::Failed("port taken".into())),
            None,
            None,
            None,
        );
        share.show(view);
        assert_eq!(share.rev(), rev + 1);
        assert_eq!(share.view().error.as_deref(), Some("port taken"));
        assert_eq!(share.view().note.as_deref(), Some("Exported x.world"));
    }
}
