//! herdr as the pane host: tier 0 of the agents RFC, with a light on.
//!
//! Gen never owns an agent's terminal. When herdr — a terminal multiplexer
//! that detects what the agent in each pane is doing — is running, the
//! bottom bar can open a pane in the world folder (a shell, or one of the
//! coding agents found on the path), and the canvas subscribes to that
//! pane's agent status over herdr's socket: `blocked` lights the bar
//! ("claude is waiting for you"), and a turn that ends (`working` → `done`
//! or `idle`) marks the history rail, so the entries between two marks are
//! one turn. Gen reads a state and shows it; it composes no prompt and
//! interprets no reply. Without herdr nothing here shows, and tier 0 is
//! what it was: the agent in the person's own terminal.
//!
//! The socket speaks newline-delimited JSON (herdr 0.9, protocol 22):
//! `{"id", "method", "params"}` per line and one reply per request, and an
//! `events.subscribe` connection that stays open and pushes
//! `{"event": "pane.agent_status_changed", "data": {...}}` lines. All of it
//! happens on one thread; the canvas drains a channel each frame.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde_json::{Value, json};

use super::live::{Live, TurnMark};

/// How often the socket is asked whether herdr is there.
const PROBE: Duration = Duration::from_secs(3);
/// How long one request may take before it counts as no answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The agents herdr detects that the bar offers when they are on the path.
const AGENTS: &[&str] = &["claude", "codex", "gemini", "opencode"];
/// The environment a pane opened from the canvas gets: where the live API
/// is, which the agent would otherwise read from the same file by hand.
pub const ENDPOINT_ENV: &str = "LOCALGPT_LIVE_ENDPOINT";

/// Where herdr's socket is, the way its CLI resolves it: `HERDR_SOCKET_PATH`,
/// else the named session `HERDR_SESSION`, else the default session under
/// the config directory (`XDG_CONFIG_HOME`, or `~/.config`).
pub fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let herdr = config.join("herdr");
    Some(
        match std::env::var("HERDR_SESSION")
            .ok()
            .filter(|s| !s.is_empty())
        {
            Some(name) => herdr.join("sessions").join(name).join("herdr.sock"),
            None => herdr.join("herdr.sock"),
        },
    )
}

/// What herdr says the agent in a pane is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl AgentStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }
}

/// The pane the canvas opened and watches.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentPane {
    pub id: String,
    /// The agent herdr detected in it (`claude`, `codex`, …), once it has.
    pub agent: Option<String>,
    pub status: AgentStatus,
    /// The canvas clock when the status last changed.
    pub since: f64,
    /// The command sent into the pane when it opened, if any.
    pub command: Option<String>,
}

/// What the person asked for, handed to the thread.
enum Cmd {
    Open { command: Option<String> },
    Focus,
}

/// What the thread saw.
enum Event {
    /// herdr's version when its socket answered, or `None`.
    Probe(Option<String>),
    Opened(AgentPane),
    Status {
        pane: String,
        agent: Option<String>,
        status: AgentStatus,
    },
    Closed(String),
    Failed(String),
}

/// herdr as the canvas sees it, and the pane it watches.
#[derive(Resource)]
pub struct AgentPresence {
    pub socket: Option<PathBuf>,
    /// herdr's version when its socket last answered; `None` when it is
    /// not running (or not installed), and then nothing is offered.
    pub server: Option<String>,
    pub pane: Option<AgentPane>,
    /// The agents found on the path, offered as "<name> here".
    pub commands: Vec<String>,
    /// The turns that ended, in order.
    pub turns: Vec<TurnMark>,
    /// The newest thing that went wrong, for the bar.
    pub error: Option<String>,
    cmds: Sender<Cmd>,
    events: Mutex<Receiver<Event>>,
}

impl AgentPresence {
    /// herdr answered the last probe.
    pub fn available(&self) -> bool {
        self.server.is_some()
    }

    /// Open a pane in the world folder; `command` is sent into it with
    /// Enter (`None`: just the shell).
    pub fn open(&self, command: Option<&str>) {
        let _ = self.cmds.send(Cmd::Open {
            command: command.map(str::to_string),
        });
    }

    /// Bring the pane to the front in herdr.
    pub fn focus(&self) {
        let _ = self.cmds.send(Cmd::Focus);
    }

    /// The bar's words, and whether they ask for the person: the agent is
    /// blocked on them.
    pub fn line(&self, now: f64) -> Option<(String, bool)> {
        let pane = self.pane.as_ref()?;
        let who = pane
            .agent
            .clone()
            .or_else(|| pane.command.clone())
            .unwrap_or_else(|| "the pane".to_string());
        let turns = match self.turns.len() {
            0 => String::new(),
            1 => " · 1 turn".to_string(),
            n => format!(" · {n} turns"),
        };
        Some(match pane.status {
            AgentStatus::Blocked => (
                format!("{who} is waiting for you ({})", ago(now - pane.since)),
                true,
            ),
            AgentStatus::Working => (format!("{who} · working{turns}"), false),
            AgentStatus::Done | AgentStatus::Idle => {
                (format!("{who} · {}{turns}", pane.status.as_str()), false)
            }
            AgentStatus::Unknown => (format!("{who} · no agent yet"), false),
        })
    }

    pub fn to_json(&self, now: f64) -> Value {
        json!({
            "herdr": {
                "socket": self.socket,
                "server": self.server,
            },
            "pane": self.pane.as_ref().map(|p| json!({
                "id": p.id,
                "agent": p.agent,
                "status": p.status.as_str(),
                "since_seconds": (now - p.since).max(0.0),
                "command": p.command,
            })),
            "commands": self.commands,
            "turns": self.turns.iter().map(|t| json!({
                "n": t.n, "agent": t.agent, "rows": t.rows, "revision": t.revision,
            })).collect::<Vec<_>>(),
            "error": self.error,
        })
    }
}

/// "12 s", "3 m", "1 h".
fn ago(seconds: f64) -> String {
    let s = seconds.max(0.0);
    if s < 60.0 {
        format!("{} s", s as u64)
    } else if s < 3600.0 {
        format!("{} m", (s / 60.0) as u64)
    } else {
        format!("{} h", (s / 3600.0) as u64)
    }
}

/// Adds [`AgentPresence`] and the thread that keeps it true.
pub struct HerdrPlugin {
    /// The world folder: the pane's working directory.
    pub dir: PathBuf,
}

impl Plugin for HerdrPlugin {
    fn build(&self, app: &mut App) {
        let (cmd_tx, cmd_rx) = channel();
        let (event_tx, event_rx) = channel();
        let socket = socket_path();
        if let Some(socket) = socket.clone() {
            let dir = self.dir.clone();
            let endpoint = dir
                .join(localgpt_world_agent::live::LIVE)
                .join("endpoint.json");
            if let Err(e) = std::thread::Builder::new()
                .name("herdr-watch".into())
                .spawn(move || watch(&socket, &dir, &endpoint, cmd_rx, event_tx))
            {
                warn!("herdr: the watch thread didn't start: {e}");
            }
        }
        app.insert_resource(AgentPresence {
            socket,
            server: None,
            pane: None,
            commands: agents_on_path(),
            turns: Vec::new(),
            error: None,
            cmds: cmd_tx,
            events: Mutex::new(event_rx),
        })
        .add_systems(Update, follow_agent);
    }
}

/// The coding agents on `PATH`, in [`AGENTS`] order.
fn agents_on_path() -> Vec<String> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    AGENTS
        .iter()
        .filter(|name| dirs.iter().any(|d| d.join(name).is_file()))
        .map(|name| name.to_string())
        .collect()
}

/// Drains the thread's events into the resource; a turn that ends marks
/// the history.
fn follow_agent(mut presence: ResMut<AgentPresence>, mut live: ResMut<Live>, time: Res<Time>) {
    let now = time.elapsed_secs_f64();
    let events: Vec<Event> = match presence.events.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for event in events {
        match event {
            Event::Probe(version) => presence.server = version,
            Event::Opened(mut pane) => {
                pane.since = now;
                presence.pane = Some(pane);
                presence.error = None;
            }
            Event::Status {
                pane,
                agent,
                status,
            } => {
                let n = presence.turns.len() as u32 + 1;
                let Some(open) = presence.pane.as_mut().filter(|p| p.id == pane) else {
                    continue;
                };
                let was = open.status;
                if agent.is_some() {
                    open.agent = agent;
                }
                if was == status {
                    continue;
                }
                open.status = status;
                open.since = now;
                // herdr says `done` for a detected agent, `idle` for one
                // reported by hand: either ends the turn that was working.
                if was == AgentStatus::Working
                    && matches!(status, AgentStatus::Done | AgentStatus::Idle)
                {
                    let mark = TurnMark {
                        n,
                        agent: open.agent.clone().unwrap_or_else(|| "agent".to_string()),
                        rows: live.history_len(),
                        revision: live.revision(),
                    };
                    presence.turns.push(mark.clone());
                    live.note_turn(mark);
                }
            }
            Event::Closed(id) => {
                if presence.pane.as_ref().is_some_and(|p| p.id == id) {
                    presence.pane = None;
                }
            }
            Event::Failed(why) => {
                warn!("herdr: {why}");
                presence.error = Some(why);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The thread
// ---------------------------------------------------------------------------

fn watch(socket: &Path, dir: &Path, endpoint: &Path, cmds: Receiver<Cmd>, events: Sender<Event>) {
    let mut pane: Option<String> = None;
    let mut sub: Option<LineStream> = None;
    let mut last_probe: Option<Instant> = None;
    loop {
        // What the person asked for.
        while let Ok(cmd) = cmds.try_recv() {
            match cmd {
                Cmd::Open { command } => match open_pane(socket, dir, endpoint, command.as_deref())
                {
                    Ok(opened) => {
                        pane = Some(opened.id.clone());
                        sub = match subscribe(socket, &opened.id) {
                            Ok(stream) => Some(stream),
                            Err(e) => {
                                let _ = events.send(Event::Failed(format!(
                                    "the pane opened but its status can't be followed: {e}"
                                )));
                                None
                            }
                        };
                        if events.send(Event::Opened(opened)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        if events.send(Event::Failed(e)).is_err() {
                            return;
                        }
                    }
                },
                Cmd::Focus => {
                    if let Some(id) = &pane {
                        let _ = request(socket, "pane.focus", json!({"pane_id": id}));
                    }
                }
            }
        }

        // The subscription: one line at a time, with a timeout so the
        // commands above are served between lines.
        match (sub.as_mut(), pane.as_deref()) {
            (Some(stream), Some(id)) => match stream.next_line() {
                Ok(Some(line)) => {
                    if let Some(event) = parse_event(&line, id)
                        && events.send(event).is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {}
                Err(_) => sub = None,
            },
            _ => std::thread::sleep(Duration::from_millis(200)),
        }
        // The connection went: ask where the pane stands and follow it again,
        // or let it go when herdr says it is gone.
        if sub.is_none()
            && let Some(id) = pane.clone()
        {
            match request(socket, "pane.get", json!({"pane_id": id})) {
                Ok(reply) => {
                    let info = &reply["pane"];
                    let _ = events.send(Event::Status {
                        pane: id.clone(),
                        agent: info["agent"].as_str().map(str::to_string),
                        status: AgentStatus::parse(info["agent_status"].as_str().unwrap_or("")),
                    });
                    sub = subscribe(socket, &id).ok();
                }
                Err(_) => {
                    pane = None;
                    if events.send(Event::Closed(id)).is_err() {
                        return;
                    }
                }
            }
            if sub.is_none() {
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        if last_probe.is_none_or(|t| t.elapsed() >= PROBE) {
            last_probe = Some(Instant::now());
            if events.send(Event::Probe(ping(socket))).is_err() {
                return;
            }
        }
    }
}

/// A pushed line → what it means for our pane, if anything.
fn parse_event(line: &str, pane: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line).ok()?;
    let data = &v["data"];
    if data["pane_id"].as_str() != Some(pane) {
        return None;
    }
    match v["event"].as_str()? {
        "pane.agent_status_changed" => Some(Event::Status {
            pane: pane.to_string(),
            agent: data["agent"].as_str().map(str::to_string),
            status: AgentStatus::parse(data["agent_status"].as_str().unwrap_or("")),
        }),
        // Lifecycle events arrive with an underscore on the wire.
        "pane.closed" | "pane_closed" | "pane.exited" | "pane_exited" => {
            Some(Event::Closed(pane.to_string()))
        }
        _ => None,
    }
}

/// A pane in the world folder: beside Gen's own pane when Gen runs inside
/// herdr, else beside the focused pane, else in a workspace of its own.
fn open_pane(
    socket: &Path,
    dir: &Path,
    endpoint: &Path,
    command: Option<&str>,
) -> Result<AgentPane, String> {
    let env = json!({ ENDPOINT_ENV: endpoint });
    let mut params = json!({"direction": "right", "cwd": dir, "focus": true, "env": env});
    if let Ok(own) = std::env::var("HERDR_PANE_ID")
        && !own.is_empty()
    {
        params["target_pane_id"] = json!(own);
    }
    let info = match request(socket, "pane.split", params) {
        Ok(reply) => reply["pane"].clone(),
        Err(_) => {
            let label = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "world".to_string());
            request(
                socket,
                "workspace.create",
                json!({"cwd": dir, "label": label, "focus": true, "env": env}),
            )?["root_pane"]
                .clone()
        }
    };
    let id = info["pane_id"]
        .as_str()
        .ok_or_else(|| "herdr's reply named no pane".to_string())?
        .to_string();
    if let Some(command) = command {
        start_in(socket, &id, command)?;
    }
    Ok(AgentPane {
        id,
        agent: info["agent"].as_str().map(str::to_string),
        status: AgentStatus::parse(info["agent_status"].as_str().unwrap_or("")),
        since: 0.0,
        command: command.map(str::to_string),
    })
}

/// Run `command` in the pane. For an agent herdr knows (`claude`, `codex`,
/// …) that is herdr's own starter, `agent.start`, which waits until the
/// agent is ready and labels the pane; herdr can refuse it while the shell
/// is still coming up, and then — as for any other command — the text goes
/// in with Enter, what `herdr pane run` does.
fn start_in(socket: &Path, pane: &str, command: &str) -> Result<(), String> {
    if AGENTS.contains(&command) {
        let started = request_with(
            socket,
            "agent.start",
            json!({"name": command, "kind": command, "pane_id": pane, "timeout_ms": 20_000}),
            Duration::from_secs(25),
        );
        if started.is_ok() {
            return Ok(());
        }
    }
    request(
        socket,
        "pane.send_input",
        json!({"pane_id": pane, "text": command, "keys": ["enter"]}),
    )
    .map(|_| ())
}

/// A connection that stays open for the pane's status changes and its end.
fn subscribe(socket: &Path, pane: &str) -> Result<LineStream, String> {
    let mut stream = connect(socket)?;
    let request = json!({
        "id": "gen-subscribe",
        "method": "events.subscribe",
        "params": {"subscriptions": [
            {"type": "pane.agent_status_changed", "pane_id": pane},
            {"type": "pane.closed"},
            {"type": "pane.exited"},
        ]},
    });
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let mut lines = LineStream::new(stream);
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        match lines.next_line() {
            Ok(Some(line)) => {
                let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                if let Some(err) = v.get("error") {
                    return Err(error_text(err));
                }
                if v["result"]["type"] == "subscription_started" {
                    return Ok(lines);
                }
            }
            Ok(None) if Instant::now() < deadline => {}
            Ok(None) => return Err("no acknowledgement of the subscription".into()),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// herdr's version, when the socket answers a ping.
fn ping(socket: &Path) -> Option<String> {
    request(socket, "ping", json!({}))
        .ok()
        .and_then(|r| r["version"].as_str().map(str::to_string))
}

/// One request, one reply: the `result`, or the error's message.
fn request(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    request_with(socket, method, params, REQUEST_TIMEOUT)
}

/// [`request`], allowed `wait` for its reply (a start that waits for an
/// agent to be ready takes longer than a lookup).
fn request_with(
    socket: &Path,
    method: &str,
    params: Value,
    wait: Duration,
) -> Result<Value, String> {
    let mut stream = connect(socket)?;
    let _ = stream.set_read_timeout(Some(wait));
    let line = json!({"id": format!("gen-{method}"), "method": method, "params": params});
    writeln!(stream, "{line}").map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    let mut v: Value = serde_json::from_str(&reply).map_err(|e| format!("not JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(error_text(err));
    }
    Ok(v["result"].take())
}

fn error_text(err: &Value) -> String {
    match (err["code"].as_str(), err["message"].as_str()) {
        (Some(code), Some(message)) => format!("{message} ({code})"),
        (_, Some(message)) => message.to_string(),
        (Some(code), _) => code.to_string(),
        _ => err.to_string(),
    }
}

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

#[cfg(unix)]
fn connect(socket: &Path) -> Result<Stream, String> {
    let stream = Stream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
    Ok(stream)
}

#[cfg(not(unix))]
type Stream = std::net::TcpStream;

#[cfg(not(unix))]
fn connect(socket: &Path) -> Result<Stream, String> {
    Err(format!(
        "herdr's socket ({}) is a Unix socket; its named pipe isn't spoken here yet",
        socket.display()
    ))
}

/// Lines off a stream read with a short timeout, so a read that returns
/// nothing is not the end: the bytes of a line that arrived in parts are
/// kept until the newline does.
struct LineStream {
    stream: Stream,
    buf: Vec<u8>,
}

impl LineStream {
    fn new(stream: Stream) -> Self {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
        Self {
            stream,
            buf: Vec::new(),
        }
    }

    /// `Ok(Some(line))` for a complete line, `Ok(None)` when none arrived
    /// within the timeout, `Err` when the stream ended or broke.
    fn next_line(&mut self) -> io::Result<Option<String>> {
        loop {
            if let Some(at) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=at).collect();
                return Ok(Some(
                    String::from_utf8_lossy(&line[..line.len() - 1]).into_owned(),
                ));
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed")),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_and_unknown_words_are_unknown() {
        for s in ["idle", "working", "blocked", "done"] {
            assert_eq!(AgentStatus::parse(s).as_str(), s);
        }
        assert_eq!(AgentStatus::parse("asleep"), AgentStatus::Unknown);
    }

    #[test]
    fn pushed_lines_are_read_for_our_pane_only() {
        let ours = r#"{"data":{"agent":"claude","agent_status":"blocked","pane_id":"w1:p1","workspace_id":"w1"},"event":"pane.agent_status_changed"}"#;
        match parse_event(ours, "w1:p1") {
            Some(Event::Status {
                pane,
                agent,
                status,
            }) => {
                assert_eq!(pane, "w1:p1");
                assert_eq!(agent.as_deref(), Some("claude"));
                assert_eq!(status, AgentStatus::Blocked);
            }
            _ => panic!("a status event for our pane"),
        }
        assert!(
            parse_event(ours, "w1:p2").is_none(),
            "another pane's status is not ours"
        );
        let closed = r#"{"data":{"pane_id":"w1:p1","type":"pane_closed","workspace_id":"w1"},"event":"pane_closed"}"#;
        assert!(matches!(parse_event(closed, "w1:p1"), Some(Event::Closed(id)) if id == "w1:p1"));
        assert!(parse_event("not json", "w1:p1").is_none());
    }

    #[test]
    fn the_socket_path_follows_herdr_s_own_order() {
        // Read under a lock: the environment is process-wide.
        let path = PathBuf::from("/tmp/x/herdr.sock");
        // SAFETY: tests in this module are the only writers of these
        // variables, and the test runner's threads outside it never read
        // them.
        unsafe {
            std::env::set_var("HERDR_SOCKET_PATH", &path);
        }
        assert_eq!(socket_path(), Some(path));
        unsafe {
            std::env::remove_var("HERDR_SOCKET_PATH");
            std::env::set_var("XDG_CONFIG_HOME", "/tmp/cfg");
            std::env::set_var("HERDR_SESSION", "work");
        }
        assert_eq!(
            socket_path(),
            Some(PathBuf::from("/tmp/cfg/herdr/sessions/work/herdr.sock"))
        );
        unsafe {
            std::env::remove_var("HERDR_SESSION");
        }
        assert_eq!(
            socket_path(),
            Some(PathBuf::from("/tmp/cfg/herdr/herdr.sock"))
        );
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn durations_read_as_a_person_would() {
        assert_eq!(ago(12.4), "12 s");
        assert_eq!(ago(200.0), "3 m");
        assert_eq!(ago(7300.0), "2 h");
    }

    /// A stand-in for herdr: one request per connection answered from a
    /// table, and a subscription that pushes what the test says.
    #[cfg(unix)]
    #[test]
    fn requests_and_subscriptions_speak_herdr_s_lines() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        // Unix socket paths are short-lived and short: keep it under the limit.
        let sock = dir.path().join("h.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                let mut out = stream;
                match req["method"].as_str().unwrap() {
                    "ping" => writeln!(out, r#"{{"id":"{}","result":{{"type":"pong","version":"0.9.3","protocol":22}}}}"#, req["id"].as_str().unwrap()).unwrap(),
                    "pane.get" => writeln!(out, r#"{{"id":"x","error":{{"code":"pane_not_found","message":"pane w1:p9 not found"}}}}"#).unwrap(),
                    "events.subscribe" => {
                        assert_eq!(req["params"]["subscriptions"][0]["pane_id"], "w1:p1");
                        writeln!(out, r#"{{"id":"gen-subscribe","result":{{"type":"subscription_started"}}}}"#).unwrap();
                        // A line in two parts, then a whole one.
                        let event = r#"{"data":{"agent":"claude","agent_status":"working","pane_id":"w1:p1","workspace_id":"w1"},"event":"pane.agent_status_changed"}"#;
                        out.write_all(&event.as_bytes()[..40]).unwrap();
                        out.flush().unwrap();
                        std::thread::sleep(Duration::from_millis(300));
                        out.write_all(&event.as_bytes()[40..]).unwrap();
                        out.write_all(b"\n").unwrap();
                        writeln!(out, r#"{{"data":{{"pane_id":"w1:p1","type":"pane_closed","workspace_id":"w1"}},"event":"pane_closed"}}"#).unwrap();
                    }
                    other => panic!("unexpected method {other}"),
                }
            }
        });
        assert_eq!(ping(&sock).as_deref(), Some("0.9.3"));
        let err = request(&sock, "pane.get", json!({"pane_id": "w1:p9"})).unwrap_err();
        assert!(
            err.contains("pane w1:p9 not found") && err.contains("pane_not_found"),
            "{err}"
        );
        let mut lines = subscribe(&sock, "w1:p1").unwrap();
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.len() < 2 && Instant::now() < deadline {
            match lines.next_line() {
                Ok(Some(line)) => seen.push(line),
                Ok(None) => {}
                Err(_) => break,
            }
        }
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(matches!(
            parse_event(&seen[0], "w1:p1"),
            Some(Event::Status {
                status: AgentStatus::Working,
                ..
            })
        ));
        assert!(matches!(
            parse_event(&seen[1], "w1:p1"),
            Some(Event::Closed(_))
        ));
        server.join().unwrap();
    }
}
