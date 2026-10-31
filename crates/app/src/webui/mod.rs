//! The app's interface: a web page in one web view over the window.
//!
//! Bevy owns the window and draws the world; the page — React, Tailwind,
//! CodeMirror and xterm.js, built from `crates/app/ui` — draws everything
//! else: the history graph and its playback, the document and its outline,
//! the song's transport, Open…, and the agents' terminals. It leaves a hole
//! where the world shows through and reports where it is: clicks inside it
//! fall through to Bevy (`mac::HoleView`), and the world camera draws into
//! it. Nothing else draws chrome; Gen has no panels of its own.
//!
//! The page posts [`FromPage`] as JSON through `window.ipc`; the app answers
//! with [`ToPage`] through `window.__lg.recv(…)`. Each side keeps its own
//! state: the page asks with the app's messages ([`SeekTo`], [`DocumentAsk`],
//! [`OpenFile`], [`ToggleSong`]) and the app sends it snapshots, so the page
//! also opens in a plain browser, with nothing behind it, to be designed.
//!
//! The window's side is a [`Host`]: on macOS a WKWebView over Bevy's view
//! (`mac.rs`), serving the built page from the binary (`ui/dist`, read from
//! disk in a debug build). Windows and Linux have no host yet: the window
//! shows the world alone, and the rest of this module — the protocol, the
//! terminals — is built and tested there all the same, waiting for one.
//!
//! `LOCALGPT_APP_UI_URL` loads a dev server instead of the built page (`npm
//! run dev` in `crates/app/ui`, which reloads as the page is edited);
//! `LOCALGPT_APP_UI_OPEN=claude,shell` starts with those tabs open; and
//! `LOCALGPT_APP_UI=none` leaves the page out, the world alone in the window.

#[cfg(target_os = "macos")]
mod mac;
mod term;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use base64::Engine;
use bevy::camera::Viewport;
use bevy::diagnostic::FrameCount;
use bevy::ecs::message::Messages;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use localgpt_gen::gen3d::audio::AudioEngine;
use localgpt_gen::gen3d::live::{LiveHistory, SeekTo};
use localgpt_gen::gen3d::plugin::FlyCam;
use localgpt_world_bevy::modulation::Soundtrack;
use serde::{Deserialize, Serialize};

use crate::document::{Document, DocumentAsk, DocumentView};
use crate::now_playing::{NowPlaying, ToggleSong, Transport};
use crate::open::{OpenFile, Opener, Showing};
use term::{TermOut, Terms};

pub const LOCALGPT_APP_UI: &str = "LOCALGPT_APP_UI";
pub const LOCALGPT_APP_UI_URL: &str = "LOCALGPT_APP_UI_URL";
pub const LOCALGPT_APP_UI_OPEN: &str = "LOCALGPT_APP_UI_OPEN";

/// History entries per second while playing.
const PLAY_RATE: f32 = 2.0;

pub struct WebUiPlugin {
    /// Where the agents' terminals start: the open world's folder.
    pub cwd: PathBuf,
    url: Option<String>,
}

impl WebUiPlugin {
    /// The interface, unless `LOCALGPT_APP_UI=none` asks for the world alone.
    pub fn from_env(cwd: PathBuf) -> Option<Self> {
        let off = std::env::var(LOCALGPT_APP_UI).is_ok_and(|v| v == "none");
        (!off).then(|| Self {
            cwd,
            url: std::env::var(LOCALGPT_APP_UI_URL).ok(),
        })
    }
}

impl Plugin for WebUiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Settings {
            cwd: self.cwd.clone(),
            url: self.url.clone(),
        })
        .init_resource::<Hole>()
        .init_resource::<Play>()
        .insert_non_send(Terms::default())
        .add_systems(
            Update,
            (
                open_page,
                read_page,
                play_history,
                push_page,
                pump_terms,
                fit_camera,
            )
                .chain(),
        );
    }
}

#[derive(Resource)]
struct Settings {
    cwd: PathBuf,
    url: Option<String>,
}

/// What a platform gives the page: somewhere to run script in it, and a way
/// to cut the hole the world shows through. macOS's is `mac::MacHost`;
/// Windows and Linux have none yet, and there the window shows the world
/// alone. Everything else here is the same on every platform.
trait Host {
    fn run(&self, script: &str);
    fn set_hole(&self, hole: [f64; 4]);
}

/// The page, once it is open. Web views live on the main thread, so this is
/// a non-send resource, and the systems that touch it run there.
struct Page {
    host: Box<dyn Host>,
    inbox: Receiver<String>,
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
    song: Option<String>,
}

impl Page {
    fn new(host: Box<dyn Host>, inbox: Receiver<String>) -> Self {
        Self {
            host,
            inbox,
            ready: false,
            fresh: true,
            sent: Sent::default(),
        }
    }

    fn send(&self, message: &ToPage) {
        if let Ok(json) = serde_json::to_string(message) {
            self.host
                .run(&format!("window.__lg&&window.__lg.recv({json})"));
        }
    }
}

/// The page's hole, in CSS pixels from the window's top left.
#[derive(Resource, Default)]
struct Hole(Option<[f64; 4]>);

/// Playing the history: one entry after another, in the order written.
#[derive(Resource, Default)]
struct Play {
    playing: bool,
    since: f32,
    /// The seek asked for and not yet shown, so one lands before the next.
    pending: Option<Option<String>>,
    waited: f32,
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum FromPage {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "viewport")]
    Viewport { x: f64, y: f64, w: f64, h: f64 },
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
    #[serde(rename = "song.toggle")]
    SongToggle,
    #[serde(rename = "doc.edit")]
    DocEdit { text: String },
    #[serde(rename = "doc.caret")]
    DocCaret { line: usize },
    #[serde(rename = "doc.go")]
    DocGo { section: usize, line: Option<usize> },
    #[serde(rename = "doc.pick")]
    DocPick { name: String },
    #[serde(rename = "doc.reauthor")]
    DocReauthor { section: usize },
    #[serde(rename = "doc.author")]
    DocAuthor { on: bool },
    #[serde(rename = "doc.follow")]
    DocFollow { on: bool },
    #[serde(rename = "term.open")]
    TermOpen {
        id: String,
        kind: String,
        cols: u16,
        rows: u16,
    },
    #[serde(rename = "term.input")]
    TermInput { id: String, data: String },
    #[serde(rename = "term.resize")]
    TermResize { id: String, cols: u16, rows: u16 },
    #[serde(rename = "term.close")]
    TermClose { id: String },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Step {
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
        turns: Vec<Turn<'a>>,
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
    /// The song the world performs, or none.
    #[serde(rename = "song")]
    Song { song: Option<&'a NowPlaying> },
    #[serde(rename = "term.data")]
    TermData { id: &'a str, b64: String },
    #[serde(rename = "term.exit")]
    TermExit {
        id: &'a str,
        code: Option<u32>,
        error: Option<String>,
    },
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

#[derive(Serialize)]
struct Turn<'a> {
    n: u32,
    agent: &'a str,
    rows: usize,
    revision: u64,
}

/// Opens the page once Bevy's window and surface exist (an exclusive
/// system, so it runs on the main thread). A failure is logged once.
fn open_page(world: &mut World, mut tried: Local<bool>) {
    // Bevy makes its surface on the first frames; the page goes above it.
    if *tried || world.resource::<FrameCount>().0 < 3 {
        return;
    }
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return;
    };
    let dev = world.resource::<Settings>().url.clone();
    let query = std::env::var(LOCALGPT_APP_UI_OPEN)
        .map(|open| format!("?open={open}"))
        .unwrap_or_default();
    let (tx, inbox) = channel::<String>();
    #[cfg(target_os = "macos")]
    let host: anyhow::Result<Box<dyn Host>> = {
        let Some(ns_view) = mac::content_view(window) else {
            return;
        };
        mac::open(ns_view, dev.as_deref(), &query, tx).map(|host| Box::new(host) as Box<dyn Host>)
    };
    #[cfg(not(target_os = "macos"))]
    let host: anyhow::Result<Box<dyn Host>> = {
        // Nothing to open them with yet.
        drop((window, dev, query, tx));
        Err(anyhow::anyhow!(
            "it opens on macOS only so far, and this window shows the world alone"
        ))
    };
    *tried = true;
    match host {
        Ok(host) => world.insert_non_send(Page::new(host, inbox)),
        Err(e) => tracing::warn!("the interface did not open: {e:#}"),
    }
}

/// Where the page's asks go: each is one of the app's own messages, there
/// when the mode that answers it is.
#[derive(SystemParam)]
struct Asks<'w> {
    seeks: Option<ResMut<'w, Messages<SeekTo>>>,
    document: Option<ResMut<'w, Messages<DocumentAsk>>>,
    open: Option<ResMut<'w, Messages<OpenFile>>>,
    song: Option<ResMut<'w, Messages<ToggleSong>>>,
}

fn ask<T: Message>(messages: &mut Option<ResMut<Messages<T>>>, message: T) {
    if let Some(messages) = messages.as_mut() {
        messages.write(message);
    }
}

fn read_page(
    page: Option<NonSendMut<Page>>,
    mut terms: NonSendMut<Terms>,
    settings: Res<Settings>,
    history: Option<Res<LiveHistory>>,
    mut asks: Asks,
    mut play: ResMut<Play>,
    mut hole: ResMut<Hole>,
) {
    let Some(mut page) = page else {
        return;
    };
    let inbox: Vec<String> = page.inbox.try_iter().collect();
    for raw in inbox {
        let message = match serde_json::from_str::<FromPage>(&raw) {
            Ok(message) => message,
            Err(e) => {
                tracing::warn!("interface: {e}: {raw}");
                continue;
            }
        };
        match message {
            FromPage::Ready => {
                page.ready = true;
                page.fresh = true;
            }
            FromPage::Viewport { x, y, w, h } => {
                page.host.set_hole([x, y, w, h]);
                hole.0 = Some([x, y, w, h]);
            }
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
            FromPage::SongToggle => ask(&mut asks.song, ToggleSong),
            FromPage::DocEdit { text } => ask(&mut asks.document, DocumentAsk::Edit(text)),
            FromPage::DocCaret { line } => ask(&mut asks.document, DocumentAsk::Caret(line)),
            FromPage::DocGo { section, line } => {
                ask(&mut asks.document, DocumentAsk::Go { section, line })
            }
            FromPage::DocPick { name } => ask(&mut asks.document, DocumentAsk::Pick(name)),
            FromPage::DocReauthor { section } => {
                ask(&mut asks.document, DocumentAsk::Reauthor(section))
            }
            FromPage::DocAuthor { on } => ask(&mut asks.document, DocumentAsk::Author(on)),
            FromPage::DocFollow { on } => ask(&mut asks.document, DocumentAsk::Follow(on)),
            FromPage::TermOpen {
                id,
                kind,
                cols,
                rows,
            } => {
                if let Err(e) = terms.open(&id, &kind, cols, rows, &settings.cwd) {
                    page.send(&ToPage::TermExit {
                        id: &id,
                        code: None,
                        error: Some(format!("{e:#}")),
                    });
                }
            }
            FromPage::TermInput { id, data } => {
                if let Err(e) = terms.input(&id, data.as_bytes()) {
                    tracing::debug!("terminal {id}: {e}");
                }
            }
            FromPage::TermResize { id, cols, rows } => {
                if let Err(e) = terms.resize(&id, cols, rows) {
                    tracing::debug!("terminal {id}: {e}");
                }
            }
            FromPage::TermClose { id } => terms.close(&id),
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
    soundtrack: Option<Res<'w, Soundtrack>>,
    transport: Option<Res<'w, Transport>>,
    audio: Option<Res<'w, AudioEngine>>,
}

fn push_page(page: Option<NonSendMut<Page>>, mut shown: Shown, play: Res<Play>) {
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
    if let (Some(soundtrack), Some(transport)) = (&shown.soundtrack, &shown.transport) {
        let mut song =
            crate::now_playing::now_playing(soundtrack, transport, shown.audio.is_some());
        // A tenth of a second is as fine as the transport draws.
        if let Some(song) = song.as_mut() {
            song.position = (song.position * 10.0).round() / 10.0;
        }
        let json = serde_json::to_string(&song).unwrap_or_default();
        if page.sent.song.as_deref() != Some(json.as_str()) {
            page.send(&ToPage::Song {
                song: song.as_ref(),
            });
            page.sent.song = Some(json);
        }
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
        turns: history
            .turns
            .iter()
            .map(|turn| Turn {
                n: turn.n,
                agent: &turn.agent,
                rows: turn.rows,
                revision: turn.revision,
            })
            .collect(),
    }
}

/// The PTYs' bytes since the last frame, one message per tab.
fn pump_terms(page: Option<NonSend<Page>>, mut terms: NonSendMut<Terms>) {
    let Some(page) = page else {
        return;
    };
    let mut data: Vec<(String, Vec<u8>)> = Vec::new();
    for out in terms.drain() {
        match out {
            TermOut::Data(id, bytes) => match data.iter_mut().find(|(tab, _)| *tab == id) {
                Some((_, buf)) => buf.extend_from_slice(&bytes),
                None => data.push((id, bytes)),
            },
            TermOut::Exit(id, code) => {
                // The tab's last bytes go before its end.
                if let Some(at) = data.iter().position(|(tab, _)| *tab == id) {
                    let (id, bytes) = data.remove(at);
                    send_data(&page, &id, &bytes);
                }
                page.send(&ToPage::TermExit {
                    id: &id,
                    code,
                    error: None,
                });
            }
        }
    }
    for (id, bytes) in data {
        send_data(&page, &id, &bytes);
    }
}

fn send_data(page: &Page, id: &str, bytes: &[u8]) {
    page.send(&ToPage::TermData {
        id,
        b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    });
}

/// The world camera draws into the page's hole.
fn fit_camera(
    hole: Res<Hole>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<FlyCam>>,
) {
    let (Some([x, y, w, h]), Ok(window)) = (hole.0, windows.single()) else {
        return;
    };
    let scale = window.scale_factor() as f64;
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let at =
        UVec2::new((x * scale).round() as u32, (y * scale).round() as u32).min(size - UVec2::ONE);
    let extent = UVec2::new((w * scale).round() as u32, (h * scale).round() as u32)
        .min(size - at)
        .max(UVec2::ONE);
    for mut camera in &mut cameras {
        let same = camera
            .viewport
            .as_ref()
            .is_some_and(|v| v.physical_position == at && v.physical_size == extent);
        if !same {
            camera.viewport = Some(Viewport {
                physical_position: at,
                physical_size: extent,
                ..default()
            });
        }
    }
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
            turns: Vec::new(),
        }
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
}
