//! The app's interface protocol, and the page that speaks it.
//!
//! The interface asks with [`FromPage`] and is answered with [`ToPage`].
//! Each side keeps its own state: the interface asks with the app's
//! messages ([`SeekTo`], [`DocumentAsk`], [`OpenFile`], [`TimelineAsk`]) and
//! the app sends it snapshots of what it keeps — the history, the document,
//! the timeline, the scene and the clock.
//!
//! Its clients: the shell, the window's interface (`crate::shell`), over an
//! in-process channel (`link.rs`), its asks and answers typed; and a paired
//! phone or tablet, over the local network (`remote.rs`), as JSON. The
//! device gets the page — React, Tailwind and xterm.js, built from
//! `crates/app/ui` and served from the binary (`ui/dist`, read from disk in
//! a debug build) — until the shell's panels are built for its browser
//! (`docs/rfcs/authoring/all-rust.md`, step F). What the window is sent,
//! the device is sent, and what the device asks is asked as the window's
//! would be, bar the asks that belong to this computer (Open…, a link for
//! its browser, the listener). The terminals are every client's
//! (`term.rs`).
//!
//! `LOCALGPT_APP_UI_OPEN=terminal` starts with a terminal open (`outliner`
//! starts the left column on its Outliner tab).

mod dist;
pub mod link;
mod remote;
mod term;

pub use remote::RemoteView;
#[cfg(test)]
pub use remote::{DeviceView, PairingView};
pub use term::Tab;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use base64::Engine;
use bevy::ecs::message::Messages;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use localgpt_gen::gen3d::live::{LiveHistory, SeekTo};
use serde::{Deserialize, Serialize};

use crate::document::{Document, DocumentAsk, DocumentView};
use crate::open::{OpenFile, Opener, Showing};
use crate::scene::{Scene, SceneAsk, SceneEdit, SceneView};
use crate::timeline::{ShotTime, Thumbs, Timeline, TimelineAsk, TimelineView, WorldClock};
use remote::{Frames, Inbound, Mirror, Remote};
use term::{TermOut, Terms};

pub const LOCALGPT_APP_UI_OPEN: &str = "LOCALGPT_APP_UI_OPEN";

/// History entries per second while playing.
const PLAY_RATE: f32 = 2.0;

pub struct WebUiPlugin {
    /// Where the agents' terminals start: the open world's folder.
    pub cwd: PathBuf,
}

impl Plugin for WebUiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Settings {
            cwd: self.cwd.clone(),
        })
        .init_resource::<Play>()
        .insert_resource(Remote::from_env())
        .init_resource::<Frames>()
        .insert_non_send(Terms::default())
        .add_systems(
            Update,
            (
                open_page,
                read_page,
                play_history,
                push_page,
                pump_terms,
                remote::show,
                remote::tick,
            )
                .chain(),
        );
    }
}

/// Who a message came from or goes to: the window's interface, or a paired
/// device's page, by its connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Client {
    Window,
    Device(u64),
}

#[derive(Resource)]
struct Settings {
    cwd: PathBuf,
}

/// What the window's interface is sent through: the shell's link
/// (`link::ShellHost`), which takes each message typed.
trait Host {
    fn send(&self, message: &ToPage);
}

/// The window's side of the protocol, once it is open. The terminals' PTYs
/// it pumps belong to the thread that made them, so this is a non-send
/// resource, and the systems that touch it run there. Without a host (a
/// test) it is still made, ready from the start, so a paired device's page
/// has everything to draw.
struct Page {
    host: Option<Box<dyn Host>>,
    /// The shell's asks, typed: they need no JSON (`link`).
    shell: Option<Receiver<FromPage>>,
    /// Everything sent goes to the paired devices too.
    mirror: Mirror,
    /// The page said it is listening; until then nothing is sent.
    ready: bool,
    /// Send everything on the next push, not just what changed.
    fresh: bool,
    /// What was sent last, so only what changed goes again.
    sent: Sent,
}

#[derive(Default)]
struct Sent {
    playing: Option<bool>,
    mode: Option<(bool, bool)>,
    opener: Option<(bool, Option<String>)>,
    document: Option<String>,
    text_rev: u64,
    timeline_rev: Option<u64>,
    scene_rev: Option<u64>,
    selection: Option<Vec<u64>>,
    clock: Option<ClockSent>,
    thumbs_rev: u64,
    remote_rev: Option<u64>,
}

/// The clock as the page was last told it, and when: while it plays, the
/// page runs it on from there.
struct ClockSent {
    at: f32,
    playing: bool,
    looping: Option<[f32; 2]>,
    when: f64,
}

impl ClockSent {
    /// The page's clock has drifted from the app's, or is told too seldom.
    fn stale(&self, clock: &WorldClock, now: f64) -> bool {
        if self.playing != clock.playing || self.looping != clock.looping {
            return true;
        }
        let since = (now - self.when) as f32;
        let expected = if self.playing {
            self.at + since
        } else {
            self.at
        };
        (clock.at - expected).abs() > 0.05 || (self.playing && since > 1.0)
    }
}

impl Page {
    fn new(host: Option<Box<dyn Host>>, shell: Option<Receiver<FromPage>>, mirror: Mirror) -> Self {
        Self {
            ready: host.is_none(),
            host,
            shell,
            mirror,
            fresh: true,
            sent: Sent::default(),
        }
    }

    /// To every client: the window's, and the devices', as JSON.
    fn send(&self, message: &ToPage) {
        self.send_window(message);
        if let Ok(json) = serde_json::to_string(message) {
            self.mirror.broadcast(&json);
        }
    }

    /// To the window's interface alone, typed.
    fn send_window(&self, message: &ToPage) {
        if let Some(host) = &self.host {
            host.send(message);
        }
    }

    fn send_to(&self, client: Client, message: &ToPage) {
        match client {
            Client::Window => self.send_window(message),
            Client::Device(conn) => {
                if let Ok(json) = serde_json::to_string(message) {
                    self.mirror.to(conn, json);
                }
            }
        }
    }
}

/// Playing the history: one entry after another, in the order written.
#[derive(Resource, Default)]
struct Play {
    playing: bool,
    since: f32,
    /// The seek asked for and not yet shown, so one lands before the next.
    pending: Option<Option<String>>,
    waited: f32,
}

/// What a page asks: the window's web view and a device's as JSON, the
/// shell's as these values (`link`).
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t")]
pub enum FromPage {
    #[serde(rename = "ready")]
    Ready,
    /// `None` is the base.
    #[serde(rename = "seek")]
    Seek { id: Option<String> },
    #[serde(rename = "step")]
    Step { to: Step },
    #[serde(rename = "play")]
    Play { playing: bool },
    /// A link in a terminal, for the default browser.
    #[serde(rename = "open")]
    Open { url: String },
    /// The top bar's Open…: the native picker.
    #[serde(rename = "open.file")]
    OpenFile,
    #[serde(rename = "clock.seek")]
    ClockSeek { at: f32 },
    #[serde(rename = "clock.play")]
    ClockPlay { playing: bool },
    #[serde(rename = "clock.loop")]
    ClockLoop { span: Option<[f32; 2]> },
    /// Look through a shot's camera; `None`, back to the free camera.
    #[serde(rename = "shot.look")]
    ShotLook { id: Option<u64> },
    /// A drag let go: the shots' new in and out points, one edit.
    #[serde(rename = "shot.time")]
    ShotTimes { edits: Vec<ShotTime> },
    #[serde(rename = "review")]
    Review { accept: bool },
    /// The heard tracks to silence (a solo is the others muted).
    #[serde(rename = "mute")]
    Mute { ids: Vec<String> },
    /// The seen tracks to hide.
    #[serde(rename = "hide")]
    Hide { ids: Vec<String> },
    /// The song's sections as a recut lets go of them, fractions from 0.
    #[serde(rename = "song.sections")]
    SongSections { sections: Vec<f32> },
    /// How long the camera waits at a tour's stop, in seconds.
    #[serde(rename = "tour.hold")]
    TourHold {
        tour: String,
        stop: usize,
        hold: f32,
    },
    /// Select these in the world, the primary first.
    #[serde(rename = "select")]
    Select { ids: Vec<u64> },
    /// The camera flies to it.
    #[serde(rename = "focus")]
    Focus { id: u64 },
    /// A field let go of: one edit, for each thing it changes.
    #[serde(rename = "entity.patch")]
    EntityPatch { edits: Vec<SceneEdit>, what: String },
    /// A field held: shown on the canvas, not committed.
    #[serde(rename = "entity.preview")]
    EntityPreview { edits: Vec<SceneEdit> },
    /// What the outliner's eye hides in the window.
    #[serde(rename = "entity.hide")]
    EntityHide { ids: Vec<u64> },
    /// A drag let go in the outliner: hang these under `parent` (none: at
    /// the top), where they stand, or carried at their offset (`keep`).
    #[serde(rename = "entity.parent")]
    EntityParent {
        ids: Vec<u64>,
        parent: Option<u64>,
        keep: bool,
    },
    #[serde(rename = "doc.edit")]
    DocEdit { text: String },
    #[serde(rename = "doc.caret")]
    DocCaret { line: usize },
    #[serde(rename = "doc.go")]
    DocGo { section: usize, line: Option<usize> },
    #[serde(rename = "doc.pick")]
    DocPick { name: String },
    #[serde(rename = "doc.follow")]
    DocFollow { on: bool },
    #[serde(rename = "term.open")]
    TermOpen { id: String, cols: u16, rows: u16 },
    #[serde(rename = "term.input")]
    TermInput { id: String, data: String },
    #[serde(rename = "term.resize")]
    TermResize { id: String, cols: u16, rows: u16 },
    #[serde(rename = "term.close")]
    TermClose { id: String },
    /// A tab another page opened, to show here: what fits here, and the
    /// screen comes back.
    #[serde(rename = "term.attach")]
    TermAttach { id: String, cols: u16, rows: u16 },
    /// Clicked into a tab: this page's size, as typing would.
    #[serde(rename = "term.focus")]
    TermFocus { id: String },
    /// A device's page asks for the world as the window shows it.
    #[serde(rename = "frame")]
    Frame,
    /// The window's page turns the phone's listener on or off.
    #[serde(rename = "remote.on")]
    RemoteOn { on: bool },
    /// A fresh pairing code.
    #[serde(rename = "remote.pair")]
    RemotePair,
    #[serde(rename = "remote.forget")]
    RemoteForget { id: String },
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    First,
    Prev,
    Next,
    Last,
}

#[derive(Serialize)]
#[serde(tag = "t")]
enum ToPage<'a> {
    /// Which panels have something to show: the history (a live canvas)
    /// and the document (document mode).
    #[serde(rename = "mode")]
    Mode { history: bool, document: bool },
    #[serde(rename = "history")]
    History {
        rows: Vec<Row<'a>>,
        current: Option<&'a str>,
        status: Option<&'a str>,
    },
    #[serde(rename = "playback")]
    Playback { playing: bool },
    #[serde(rename = "title")]
    Title { text: Option<&'a str> },
    /// Open…'s progress: a song's analysis takes seconds.
    #[serde(rename = "opener")]
    Opener { busy: bool, status: Option<&'a str> },
    #[serde(rename = "document")]
    Document { doc: &'a DocumentView },
    /// The editor's text, replaced from here: opened, or reloaded from disk.
    #[serde(rename = "doc.text")]
    DocText { text: &'a str, rev: u64 },
    /// Bring a line of the text into view.
    #[serde(rename = "doc.reveal")]
    DocReveal { line: usize },
    /// The world's clock and its tracks, or none before a world is open.
    #[serde(rename = "timeline")]
    Timeline { view: Option<&'a TimelineView> },
    /// The world's things as its document has them, or none.
    #[serde(rename = "scene")]
    Scene { view: Option<&'a SceneView> },
    /// What is selected, by world id, the primary first.
    #[serde(rename = "selection")]
    Selection { ids: &'a [u64] },
    /// Where the clock is. While it plays, the page runs it on between these.
    #[serde(rename = "clock")]
    Clock {
        at: f32,
        playing: bool,
        #[serde(rename = "loop")]
        looping: Option<[f32; 2]>,
    },
    /// A shot's newest frame, by its camera entity's id.
    #[serde(rename = "thumb")]
    Thumb { id: String, url: &'a str },
    /// A tab's bytes: base64 in the page's JSON, as they are in the shell's.
    #[serde(rename = "term.data")]
    TermData {
        id: &'a str,
        #[serde(rename = "b64", serialize_with = "base64_bytes")]
        bytes: &'a [u8],
    },
    #[serde(rename = "term.exit")]
    TermExit {
        id: &'a str,
        code: Option<u32>,
        error: Option<String>,
    },
    /// Every tab, whichever page opened it: its size and its label.
    #[serde(rename = "term.list")]
    TermList { tabs: Vec<term::Tab> },
    /// A tab's PTY took another size: another page drives it.
    #[serde(rename = "term.size")]
    TermSize { id: &'a str, cols: u16, rows: u16 },
    /// A tab's program set its title: the tab's label.
    #[serde(rename = "term.state")]
    TermState { id: &'a str, label: Option<&'a str> },
    /// The phone's listener and its devices: the window's page alone.
    #[serde(rename = "remote")]
    Remote { view: &'a remote::RemoteView },
}

fn base64_bytes<S: serde::Serializer>(bytes: &&[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[derive(Serialize)]
struct Row<'a> {
    id: &'a str,
    parent: Option<&'a str>,
    revision: u64,
    author: &'a str,
    summary: &'a str,
    message: Option<&'a str>,
    timestamp_ms: u64,
    tip: bool,
    head: bool,
}

/// Opens the window's side of the protocol, once (an exclusive system, so
/// it runs on the thread that owns the terminals): the shell's link, typed
/// both ways, which says when it is ready. Without a link (a test), the
/// side opens with no host, for paired devices alone.
fn open_page(world: &mut World, mut opened: Local<bool>) {
    if std::mem::replace(&mut *opened, true) {
        return;
    }
    let (host, asks) = match world.get_resource::<link::ShellLink>() {
        Some(link) => (
            link.host.lock().ok().and_then(|mut h| h.take()),
            link.asks.lock().ok().and_then(|mut a| a.take()),
        ),
        None => (None, None),
    };
    let mirror = world.resource::<Remote>().mirror();
    let host = host.map(|host| Box::new(host) as Box<dyn Host>);
    world.insert_non_send(Page::new(host, asks, mirror));
}

/// Where the page's asks go: each is one of the app's own messages, there
/// when the mode that answers it is.
#[derive(SystemParam)]
struct Asks<'w> {
    seeks: Option<ResMut<'w, Messages<SeekTo>>>,
    document: Option<ResMut<'w, Messages<DocumentAsk>>>,
    open: Option<ResMut<'w, Messages<OpenFile>>>,
    timeline: Option<ResMut<'w, Messages<TimelineAsk>>>,
    scene: Option<ResMut<'w, Messages<SceneAsk>>>,
}

fn ask<T: Message>(messages: &mut Option<ResMut<Messages<T>>>, message: T) {
    if let Some(messages) = messages.as_mut() {
        messages.write(message);
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn read_page(
    page: Option<NonSendMut<Page>>,
    mut terms: NonSendMut<Terms>,
    settings: Res<Settings>,
    history: Option<Res<LiveHistory>>,
    mut asks: Asks,
    mut play: ResMut<Play>,
    mut remote: ResMut<Remote>,
    mut frames: ResMut<Frames>,
) {
    let Some(mut page) = page else {
        return;
    };
    // The window's asks and the devices', in the order each sent them: the
    // shell's typed, a device's as JSON; a device that went is `Gone`.
    enum Incoming {
        Json(String),
        Ask(FromPage),
        Gone,
    }
    let mut inbox: Vec<(Client, Incoming)> = Vec::new();
    if let Some(shell) = &page.shell {
        inbox.extend(
            shell
                .try_iter()
                .map(|ask| (Client::Window, Incoming::Ask(ask))),
        );
    }
    inbox.extend(remote.take_inbox().into_iter().map(|came| match came {
        Inbound::Message(conn, raw) => (Client::Device(conn), Incoming::Json(raw)),
        Inbound::Gone(conn) => (Client::Device(conn), Incoming::Gone),
    }));
    for (client, incoming) in inbox {
        let message = match incoming {
            Incoming::Ask(message) => message,
            Incoming::Json(raw) => match serde_json::from_str::<FromPage>(&raw) {
                Ok(message) => message,
                Err(e) => {
                    tracing::warn!("interface: {e}: {raw}");
                    continue;
                }
            },
            Incoming::Gone => {
                // The tabs it drove go back to the window.
                terms.leave(client);
                if let Client::Device(conn) = client {
                    frames.forget(conn);
                }
                continue;
            }
        };
        // What belongs to this computer is the window's to ask: Open…, a
        // link for its browser, the listener.
        if client != Client::Window
            && matches!(
                message,
                FromPage::Open { .. }
                    | FromPage::OpenFile
                    | FromPage::RemoteOn { .. }
                    | FromPage::RemotePair
                    | FromPage::RemoteForget { .. }
            )
        {
            continue;
        }
        match message {
            FromPage::Ready => match client {
                // A page that reloaded finds its tabs still running.
                Client::Window => {
                    page.ready = true;
                    page.fresh = true;
                    page.send_window(&ToPage::TermList { tabs: terms.tabs() });
                }
                // What the window has, then the tabs; a tab's screen comes
                // when the device shows it.
                Client::Device(conn) => {
                    let tabs = serde_json::to_string(&ToPage::TermList { tabs: terms.tabs() });
                    remote.ready(conn, tabs.into_iter().collect());
                }
            },
            FromPage::Seek { id } => {
                play.playing = false;
                ask(&mut asks.seeks, SeekTo(id));
            }
            FromPage::Step { to } => {
                play.playing = false;
                if let Some(target) = history.as_deref().and_then(|h| step_target(h, to)) {
                    ask(&mut asks.seeks, SeekTo(target));
                }
            }
            FromPage::Play { playing } => {
                play.playing = playing;
                play.since = 0.0;
                play.pending = None;
                // Play at the end starts again from the base.
                let at_end = history
                    .as_deref()
                    .is_some_and(|h| step_target(h, Step::Next).is_none());
                if playing && at_end {
                    ask(&mut asks.seeks, SeekTo(None));
                    play.pending = Some(None);
                }
            }
            FromPage::Open { url } => {
                if url.starts_with("https://") || url.starts_with("http://") {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
            }
            FromPage::OpenFile => ask(&mut asks.open, OpenFile),
            FromPage::ClockSeek { at } => ask(&mut asks.timeline, TimelineAsk::Seek(at)),
            FromPage::ClockPlay { playing } => ask(&mut asks.timeline, TimelineAsk::Play(playing)),
            FromPage::ClockLoop { span } => ask(&mut asks.timeline, TimelineAsk::Loop(span)),
            FromPage::ShotLook { id } => ask(&mut asks.timeline, TimelineAsk::Look(id)),
            FromPage::ShotTimes { edits } => ask(&mut asks.timeline, TimelineAsk::Shots(edits)),
            FromPage::Review { accept } => ask(&mut asks.timeline, TimelineAsk::Review(accept)),
            FromPage::Mute { ids } => ask(&mut asks.timeline, TimelineAsk::Mute(ids)),
            FromPage::Hide { ids } => ask(&mut asks.timeline, TimelineAsk::Hide(ids)),
            FromPage::SongSections { sections } => {
                ask(&mut asks.timeline, TimelineAsk::Sections(sections))
            }
            FromPage::TourHold { tour, stop, hold } => {
                ask(&mut asks.timeline, TimelineAsk::Hold { tour, stop, hold })
            }
            FromPage::Select { ids } => ask(&mut asks.scene, SceneAsk::Select(ids)),
            FromPage::Focus { id } => ask(&mut asks.scene, SceneAsk::Focus(id)),
            FromPage::EntityPatch { edits, what } => {
                ask(&mut asks.scene, SceneAsk::Patch { edits, what })
            }
            FromPage::EntityPreview { edits } => ask(&mut asks.scene, SceneAsk::Preview(edits)),
            FromPage::EntityHide { ids } => ask(&mut asks.scene, SceneAsk::Hide(ids)),
            FromPage::EntityParent { ids, parent, keep } => ask(
                &mut asks.scene,
                SceneAsk::Reparent {
                    ids,
                    parent,
                    keep_offset: keep,
                },
            ),
            FromPage::DocEdit { text } => ask(&mut asks.document, DocumentAsk::Edit(text)),
            FromPage::DocCaret { line } => ask(&mut asks.document, DocumentAsk::Caret(line)),
            FromPage::DocGo { section, line } => {
                ask(&mut asks.document, DocumentAsk::Go { section, line })
            }
            FromPage::DocPick { name } => ask(&mut asks.document, DocumentAsk::Pick(name)),
            FromPage::DocFollow { on } => ask(&mut asks.document, DocumentAsk::Follow(on)),
            FromPage::TermOpen { id, cols, rows } => {
                if let Err(e) = terms.open(&id, (cols, rows), &settings.cwd, client) {
                    page.send_to(
                        client,
                        &ToPage::TermExit {
                            id: &id,
                            code: None,
                            error: Some(format!("{e:#}")),
                        },
                    );
                }
            }
            FromPage::TermInput { id, data } => {
                if let Err(e) = terms.input(&id, client, data.as_bytes()) {
                    tracing::debug!("terminal {id}: {e}");
                }
            }
            FromPage::TermResize { id, cols, rows } => {
                if let Err(e) = terms.resize(&id, client, (cols, rows)) {
                    tracing::debug!("terminal {id}: {e}");
                }
            }
            FromPage::TermClose { id } => terms.close(&id),
            FromPage::TermAttach { id, cols, rows } => {
                match terms.attach(&id, client, (cols, rows)) {
                    // The size first, so the screen lands on a terminal of
                    // the size it was drawn at.
                    Some((screen, (cols, rows))) => {
                        page.send_to(
                            client,
                            &ToPage::TermSize {
                                id: &id,
                                cols,
                                rows,
                            },
                        );
                        page.send_to(
                            client,
                            &ToPage::TermData {
                                id: &id,
                                bytes: &screen,
                            },
                        );
                    }
                    None => page.send_to(
                        client,
                        &ToPage::TermExit {
                            id: &id,
                            code: None,
                            error: Some("it has ended".into()),
                        },
                    ),
                }
            }
            FromPage::TermFocus { id } => terms.focus(&id, client),
            FromPage::Frame => {
                if let Client::Device(conn) = client {
                    frames.want(conn);
                }
            }
            FromPage::RemoteOn { on } => {
                remote.set_on(on);
                // A device arrives to what the window was sent since the
                // listener started: send it everything again.
                page.fresh = true;
            }
            FromPage::RemotePair => {
                remote.pair();
            }
            FromPage::RemoteForget { id } => remote.forget(&id),
        }
    }
}

/// Where a step lands, in the order entries were written: `Some(None)` is
/// the base, `None` is nowhere (already there).
fn step_target(history: &LiveHistory, to: Step) -> Option<Option<String>> {
    let rows = &history.rows;
    let at = history
        .current
        .as_deref()
        .and_then(|id| rows.iter().position(|row| row.id == id));
    let row = match (to, at) {
        (Step::First, _) => return at.map(|_| None),
        (Step::Prev, None) => return None,
        (Step::Prev, Some(0)) => return Some(None),
        (Step::Prev, Some(i)) => rows.get(i - 1),
        (Step::Next, None) => rows.first(),
        (Step::Next, Some(i)) => rows.get(i + 1),
        (Step::Last, _) => rows.iter().rfind(|row| row.head).or(rows.last()),
    }?;
    (Some(row.id.as_str()) != history.current.as_deref()).then(|| Some(row.id.clone()))
}

fn play_history(
    time: Res<Time>,
    mut play: ResMut<Play>,
    history: Option<Res<LiveHistory>>,
    mut seeks: Option<ResMut<Messages<SeekTo>>>,
) {
    let (Some(history), Some(seeks)) = (history, seeks.as_mut()) else {
        return;
    };
    if !play.playing {
        return;
    }
    let dt = time.delta_secs();
    if let Some(pending) = &play.pending {
        if history.current != *pending && play.waited < 2.0 {
            play.waited += dt;
            return;
        }
        play.pending = None;
    }
    play.since += dt;
    if play.since < 1.0 / PLAY_RATE {
        return;
    }
    play.since = 0.0;
    match step_target(&history, Step::Next) {
        Some(next) => {
            seeks.write(SeekTo(next.clone()));
            play.pending = Some(next);
            play.waited = 0.0;
        }
        None => play.playing = false,
    }
}

/// What the page shows, as the app has it.
#[derive(SystemParam)]
struct Shown<'w> {
    history: Option<Res<'w, LiveHistory>>,
    showing: Option<Res<'w, Showing>>,
    opener: Option<Res<'w, Opener>>,
    document: Option<ResMut<'w, Document>>,
    timeline: Option<Res<'w, Timeline>>,
    scene: Option<Res<'w, Scene>>,
    clock: Option<Res<'w, WorldClock>>,
    thumbs: Option<Res<'w, Thumbs>>,
}

fn push_page(page: Option<NonSendMut<Page>>, mut shown: Shown, play: Res<Play>, time: Res<Time>) {
    let Some(mut page) = page else {
        return;
    };
    if !page.ready {
        return;
    }
    if std::mem::take(&mut page.fresh) {
        page.sent = Sent::default();
    }
    let fresh = page.sent.mode.is_none();

    let mode = (shown.history.is_some(), shown.document.is_some());
    if page.sent.mode != Some(mode) {
        page.sent.mode = Some(mode);
        page.send(&ToPage::Mode {
            history: mode.0,
            document: mode.1,
        });
    }
    if let Some(history) = &shown.history
        && (fresh || history.is_changed())
    {
        page.send(&history_message(history));
    }
    if let Some(showing) = &shown.showing
        && (fresh || showing.is_changed())
    {
        page.send(&ToPage::Title {
            text: showing.0.as_deref(),
        });
    }
    if page.sent.playing != Some(play.playing) {
        page.sent.playing = Some(play.playing);
        page.send(&ToPage::Playback {
            playing: play.playing,
        });
    }
    if let Some(opener) = &shown.opener {
        let now = (opener.busy(), opener.status().map(str::to_string));
        if page.sent.opener.as_ref() != Some(&now) {
            page.send(&ToPage::Opener {
                busy: now.0,
                status: now.1.as_deref(),
            });
            page.sent.opener = Some(now);
        }
    }
    if let Some(doc) = shown.document.as_mut() {
        // The text first, so the outline below lands on the text it is of.
        let (text, rev) = doc.text();
        if page.sent.text_rev != rev {
            page.send(&ToPage::DocText { text, rev });
            page.sent.text_rev = rev;
        }
        let view = doc.view();
        let json = serde_json::to_string(&view).unwrap_or_default();
        if page.sent.document.as_deref() != Some(json.as_str()) {
            page.send(&ToPage::Document { doc: &view });
            page.sent.document = Some(json);
        }
        if let Some(line) = doc.take_scroll() {
            page.send(&ToPage::DocReveal { line });
        }
    }
    if let Some(timeline) = &shown.timeline
        && page.sent.timeline_rev != Some(timeline.rev)
    {
        page.sent.timeline_rev = Some(timeline.rev);
        page.send(&ToPage::Timeline {
            view: timeline.view.as_ref(),
        });
    }
    if let Some(scene) = &shown.scene {
        if page.sent.scene_rev != Some(scene.rev) {
            page.sent.scene_rev = Some(scene.rev);
            page.send(&ToPage::Scene {
                view: scene.view.as_ref(),
            });
        }
        if page.sent.selection.as_deref() != Some(scene.selected.as_slice()) {
            page.send(&ToPage::Selection {
                ids: &scene.selected,
            });
            page.sent.selection = Some(scene.selected.clone());
        }
    }
    if let Some(clock) = &shown.clock {
        let now = time.elapsed_secs_f64();
        if page
            .sent
            .clock
            .as_ref()
            .is_none_or(|sent| sent.stale(clock, now))
        {
            page.send(&ToPage::Clock {
                at: (clock.at * 100.0).round() / 100.0,
                playing: clock.playing,
                looping: clock.looping,
            });
            page.sent.clock = Some(ClockSent {
                at: clock.at,
                playing: clock.playing,
                looping: clock.looping,
                when: now,
            });
        }
    }
    if let Some(thumbs) = &shown.thumbs
        && thumbs.rev > page.sent.thumbs_rev
    {
        for (id, (rev, url)) in &thumbs.urls {
            if *rev > page.sent.thumbs_rev {
                page.send(&ToPage::Thumb {
                    id: id.to_string(),
                    url,
                });
            }
        }
        page.sent.thumbs_rev = thumbs.rev;
    }
}

fn history_message(history: &LiveHistory) -> ToPage<'_> {
    ToPage::History {
        rows: history
            .rows
            .iter()
            .map(|row| Row {
                id: &row.id,
                parent: row.parent.as_deref(),
                revision: row.revision,
                author: &row.author,
                summary: &row.summary,
                message: row.message.as_deref(),
                timestamp_ms: row.timestamp_ms,
                tip: row.tip,
                head: row.head,
            })
            .collect(),
        current: history.current.as_deref(),
        status: history.status.as_deref(),
    }
}

/// The PTYs' bytes since the last frame, one message per tab, to every page;
/// and what the tabs said besides — a new size, a title, the tabs
/// themselves.
fn pump_terms(page: Option<NonSend<Page>>, mut terms: NonSendMut<Terms>) {
    let Some(page) = page else {
        return;
    };
    let mut data: Vec<(String, Vec<u8>)> = Vec::new();
    let mut tabs = false;
    for out in terms.drain() {
        match out {
            TermOut::Data(id, bytes) => match data.iter_mut().find(|(tab, _)| *tab == id) {
                Some((_, buf)) => buf.extend_from_slice(&bytes),
                None => data.push((id, bytes)),
            },
            // The tab's bytes so far go before what follows them.
            TermOut::Exit(id, code) => {
                flush(&page, &mut data, &id);
                page.send(&ToPage::TermExit {
                    id: &id,
                    code,
                    error: None,
                });
            }
            TermOut::Size(id, cols, rows) => {
                flush(&page, &mut data, &id);
                page.send(&ToPage::TermSize {
                    id: &id,
                    cols,
                    rows,
                });
            }
            TermOut::State(id, label) => {
                flush(&page, &mut data, &id);
                page.send(&ToPage::TermState {
                    id: &id,
                    label: label.as_deref(),
                });
            }
            TermOut::Tabs => tabs = true,
        }
    }
    for (id, bytes) in data {
        send_data(&page, &id, &bytes);
    }
    if tabs {
        page.send(&ToPage::TermList { tabs: terms.tabs() });
    }
}

fn flush(page: &Page, data: &mut Vec<(String, Vec<u8>)>, id: &str) {
    if let Some(at) = data.iter().position(|(tab, _)| tab == id) {
        let (id, bytes) = data.remove(at);
        send_data(page, &id, &bytes);
    }
}

fn send_data(page: &Page, id: &str, bytes: &[u8]) {
    page.send(&ToPage::TermData { id, bytes });
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_gen::gen3d::live::HistoryRow;

    fn row(id: &str, parent: Option<&str>, head: bool) -> HistoryRow {
        HistoryRow {
            id: id.into(),
            parent: parent.map(str::to_string),
            revision: 1,
            author: "claude".into(),
            summary: id.into(),
            message: None,
            timestamp_ms: 0,
            tip: head,
            head,
        }
    }

    /// The spike's harbour: main up to the boat, a fork off the tower.
    fn history(current: Option<&str>) -> LiveHistory {
        LiveHistory {
            rows: vec![
                row("ground", None, false),
                row("tower", Some("ground"), false),
                row("dome", Some("tower"), false),
                row("boat", Some("tower"), true),
            ],
            current: current.map(str::to_string),
            status: None,
        }
    }

    /// The timeline's asks as `ui/src/bridge.ts` writes them.
    #[test]
    fn the_timeline_asks_read_as_the_page_writes_them() {
        let read = |json: &str| serde_json::from_str::<FromPage>(json).expect(json);
        assert!(
            matches!(read(r#"{"t":"clock.seek","at":12.5}"#), FromPage::ClockSeek { at } if at == 12.5)
        );
        assert!(matches!(
            read(r#"{"t":"clock.play","playing":true}"#),
            FromPage::ClockPlay { playing: true }
        ));
        assert!(matches!(
            read(r#"{"t":"clock.loop","span":null}"#),
            FromPage::ClockLoop { span: None }
        ));
        assert!(matches!(
            read(r#"{"t":"clock.loop","span":[60,90.5]}"#),
            FromPage::ClockLoop {
                span: Some([60.0, 90.5])
            }
        ));
        assert!(matches!(
            read(r#"{"t":"shot.look","id":7}"#),
            FromPage::ShotLook { id: Some(7) }
        ));
        let FromPage::ShotTimes { edits } =
            read(r#"{"t":"shot.time","edits":[{"id":2,"in_s":87.78,"out_s":105.22}]}"#)
        else {
            panic!("a shot edit");
        };
        assert_eq!(
            (edits[0].id, edits[0].in_s, edits[0].out_s),
            (2, 87.78, 105.22)
        );
        assert!(matches!(
            read(r#"{"t":"review","accept":false}"#),
            FromPage::Review { accept: false }
        ));
        assert!(
            matches!(read(r#"{"t":"mute","ids":["soundtrack"]}"#), FromPage::Mute { ids } if ids == ["soundtrack"])
        );
        let FromPage::SongSections { sections } =
            read(r#"{"t":"song.sections","sections":[0,0.25,0.55]}"#)
        else {
            panic!("a recut");
        };
        assert_eq!(sections, [0.0, 0.25, 0.55]);
        assert!(matches!(
            read(r#"{"t":"tour.hold","tour":"dusk","stop":1,"hold":4.5}"#),
            FromPage::TourHold { tour, stop: 1, hold } if tour == "dusk" && hold == 4.5
        ));
    }

    #[test]
    fn steps_follow_the_order_entries_were_written() {
        let at_tower = history(Some("tower"));
        assert_eq!(
            step_target(&at_tower, Step::Next),
            Some(Some("dome".into()))
        );
        assert_eq!(
            step_target(&at_tower, Step::Prev),
            Some(Some("ground".into()))
        );
        assert_eq!(step_target(&at_tower, Step::First), Some(None), "the base");
        assert_eq!(
            step_target(&at_tower, Step::Last),
            Some(Some("boat".into())),
            "main's tip"
        );
        // From the base, next is the first entry, and there is no before.
        let at_base = history(None);
        assert_eq!(
            step_target(&at_base, Step::Next),
            Some(Some("ground".into()))
        );
        assert_eq!(step_target(&at_base, Step::Prev), None);
        assert_eq!(step_target(&at_base, Step::First), None, "already there");
        // At the end, nothing is next: playback stops there.
        assert_eq!(step_target(&history(Some("boat")), Step::Next), None);
        assert_eq!(step_target(&history(Some("boat")), Step::Last), None);
    }

    /// A device's asks and the phone panel's, as `ui/src/bridge.ts` writes
    /// them; and the tabs as the pages read them.
    #[test]
    fn the_remotes_messages_read_as_the_pages_write_them() {
        let read = |json: &str| serde_json::from_str::<FromPage>(json).expect(json);
        assert!(matches!(
            read(r#"{"t":"term.attach","id":"p1","cols":48,"rows":30}"#),
            FromPage::TermAttach { id, cols: 48, rows: 30 } if id == "p1"
        ));
        assert!(
            matches!(read(r#"{"t":"term.focus","id":"t1"}"#), FromPage::TermFocus { id } if id == "t1")
        );
        assert!(matches!(read(r#"{"t":"frame"}"#), FromPage::Frame));
        assert!(matches!(
            read(r#"{"t":"remote.on","on":true}"#),
            FromPage::RemoteOn { on: true }
        ));
        assert!(matches!(
            read(r#"{"t":"remote.pair"}"#),
            FromPage::RemotePair
        ));
        assert!(matches!(
            read(r#"{"t":"remote.forget","id":"ab12cd34"}"#),
            FromPage::RemoteForget { id } if id == "ab12cd34"
        ));
        let state = serde_json::to_value(ToPage::TermState {
            id: "t1",
            label: Some("Fix the jetty"),
        })
        .unwrap();
        assert_eq!(
            state,
            serde_json::json!({"t": "term.state", "id": "t1", "label": "Fix the jetty"})
        );
    }
}
