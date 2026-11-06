//! What the app has told the shell, kept as the page keeps it
//! (`crates/app/ui/src/bridge.ts`): the latest of each message, and the
//! terminals' bytes queued for their panel.

use std::collections::HashMap;
use std::time::Instant;

use localgpt_gen::gen3d::live::HistoryRow;

use crate::document::DocumentView;
use crate::scene::SceneView;
use crate::timeline::TimelineView;
use crate::webui::link::ShellEvent;
use crate::webui::{RemoteView, Tab};

#[derive(Default)]
pub struct State {
    pub title: Option<String>,
    /// Which panels have something to show: a live canvas's history, a
    /// document.
    pub mode_history: bool,
    pub mode_document: bool,
    pub history: Option<History>,
    /// The history is playing, an entry after another.
    pub playing: bool,
    pub opener_busy: bool,
    pub opener_status: Option<String>,
    pub document: Option<Box<DocumentView>>,
    /// The editor's text, replaced from the app (opened, or reloaded from
    /// disk), and its revision.
    pub text: Option<(String, u64)>,
    /// A line of the text to bring into view.
    pub reveal: Option<usize>,
    pub timeline: Option<Box<TimelineView>>,
    pub scene: Option<Box<SceneView>>,
    pub selection: Vec<u64>,
    pub clock: Clock,
    /// Each shot's newest frame, a `data:` URL, by its camera's id.
    pub thumbs: HashMap<String, String>,
    pub tabs: Vec<Tab>,
    /// What the tabs said, for the terminals' panel to take.
    pub terms: Vec<TermEvent>,
    pub remote: Option<Box<RemoteView>>,
    /// When the remote's view came: its pairing code's seconds count down
    /// from there.
    pub remote_at: Option<Instant>,
}

impl State {
    /// The pairing code's seconds still to run, counted down here between
    /// the app's messages, as the page does.
    pub fn remote_left(&self) -> u64 {
        let from = self
            .remote
            .as_ref()
            .and_then(|r| r.pairing.as_ref())
            .map_or(0, |p| p.left_s);
        let gone = self.remote_at.map_or(0, |at| at.elapsed().as_secs());
        from.saturating_sub(gone)
    }
}

pub struct History {
    pub rows: Vec<HistoryRow>,
    pub current: Option<String>,
    pub status: Option<String>,
}

/// The world's clock as the app last said it, and when: while it plays, the
/// shell runs it on from there, as the page does.
pub struct Clock {
    pub at: f32,
    pub playing: bool,
    pub looping: Option<[f32; 2]>,
    pub told: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            at: 0.0,
            playing: false,
            looping: None,
            told: Instant::now(),
        }
    }
}

impl Clock {
    pub fn now(&self) -> f32 {
        if self.playing {
            self.at + self.told.elapsed().as_secs_f32()
        } else {
            self.at
        }
    }
}

pub enum TermEvent {
    Data(String, Vec<u8>),
    Exit(String, Option<u32>, Option<String>),
    Size(String, u16, u16),
    Label(String, Option<String>),
}

impl State {
    pub fn apply(&mut self, event: ShellEvent) {
        match event {
            ShellEvent::Mode { history, document } => {
                self.mode_history = history;
                self.mode_document = document;
                if !history {
                    self.history = None;
                }
                if !document {
                    self.document = None;
                    self.text = None;
                }
            }
            ShellEvent::History {
                rows,
                current,
                status,
            } => {
                self.history = Some(History {
                    rows,
                    current,
                    status,
                })
            }
            ShellEvent::Playback(playing) => self.playing = playing,
            ShellEvent::Title(title) => self.title = title,
            ShellEvent::Opener { busy, status } => {
                self.opener_busy = busy;
                self.opener_status = status;
            }
            ShellEvent::Document(doc) => self.document = Some(doc),
            ShellEvent::DocText { text, rev } => self.text = Some((text, rev)),
            ShellEvent::DocReveal(line) => self.reveal = Some(line),
            ShellEvent::Timeline(view) => self.timeline = view,
            ShellEvent::Scene(view) => self.scene = view,
            ShellEvent::Selection(ids) => self.selection = ids,
            ShellEvent::Clock {
                at,
                playing,
                looping,
            } => {
                self.clock = Clock {
                    at,
                    playing,
                    looping,
                    told: Instant::now(),
                }
            }
            ShellEvent::Thumb { id, url } => {
                self.thumbs.insert(id, url);
            }
            ShellEvent::TermData { id, bytes } => self.terms.push(TermEvent::Data(id, bytes)),
            ShellEvent::TermExit { id, code, error } => {
                self.terms.push(TermEvent::Exit(id, code, error))
            }
            ShellEvent::TermList(tabs) => self.tabs = tabs,
            ShellEvent::TermSize { id, cols, rows } => {
                self.terms.push(TermEvent::Size(id, cols, rows))
            }
            ShellEvent::TermState { id, label } => self.terms.push(TermEvent::Label(id, label)),
            ShellEvent::Remote(view) => {
                self.remote = Some(view);
                self.remote_at = Some(Instant::now());
            }
        }
    }
}
