//! The shell's side of the page's protocol: one more client of it, in the
//! same process (`crate::shell`, `docs/rfcs/authoring/all-rust.md`).
//!
//! The window's web view and a paired device are sent [`ToPage`] as JSON; the
//! shell is sent the same messages as owned values ([`ShellEvent`]), made from
//! the typed message rather than parsed back from its JSON, and it asks with
//! the same [`FromPage`] values the page's JSON reads as. Every message the
//! page is sent becomes a shell event through one exhaustive match, so a new
//! message does not build until the shell has it too.

use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::prelude::*;
use localgpt_gen::gen3d::live::HistoryRow;

use super::{FromPage, Host, ToPage, remote::RemoteView, term::Tab};
use crate::document::DocumentView;
use crate::scene::SceneView;
use crate::timeline::TimelineView;

/// What the app tells the shell: [`ToPage`], owned.
#[derive(Debug, Clone)]
pub enum ShellEvent {
    Mode {
        history: bool,
        document: bool,
    },
    History {
        rows: Vec<HistoryRow>,
        current: Option<String>,
        status: Option<String>,
    },
    Playback(bool),
    Title(Option<String>),
    Opener {
        busy: bool,
        status: Option<String>,
    },
    Document(Box<DocumentView>),
    DocText {
        text: String,
        rev: u64,
    },
    DocReveal(usize),
    Timeline(Option<Box<TimelineView>>),
    Scene(Option<Box<SceneView>>),
    Selection(Vec<u64>),
    Clock {
        at: f32,
        playing: bool,
        looping: Option<[f32; 2]>,
    },
    /// A shot's newest frame, a `data:` URL of a JPEG.
    Thumb {
        id: String,
        url: String,
    },
    TermData {
        id: String,
        bytes: Vec<u8>,
    },
    TermExit {
        id: String,
        code: Option<u32>,
        error: Option<String>,
    },
    TermList(Vec<Tab>),
    TermSize {
        id: String,
        cols: u16,
        rows: u16,
    },
    TermState {
        id: String,
        label: Option<String>,
    },
    Remote(Box<RemoteView>),
}

impl ShellEvent {
    pub(super) fn of(message: &ToPage) -> Self {
        match message {
            ToPage::Mode { history, document } => Self::Mode {
                history: *history,
                document: *document,
            },
            ToPage::History {
                rows,
                current,
                status,
            } => Self::History {
                rows: rows
                    .iter()
                    .map(|row| HistoryRow {
                        id: row.id.to_string(),
                        parent: row.parent.map(str::to_string),
                        revision: row.revision,
                        author: row.author.to_string(),
                        summary: row.summary.to_string(),
                        message: row.message.map(str::to_string),
                        timestamp_ms: row.timestamp_ms,
                        tip: row.tip,
                        head: row.head,
                    })
                    .collect(),
                current: current.map(str::to_string),
                status: status.map(str::to_string),
            },
            ToPage::Playback { playing } => Self::Playback(*playing),
            ToPage::Title { text } => Self::Title(text.map(str::to_string)),
            ToPage::Opener { busy, status } => Self::Opener {
                busy: *busy,
                status: status.map(str::to_string),
            },
            ToPage::Document { doc } => Self::Document(Box::new((*doc).clone())),
            ToPage::DocText { text, rev } => Self::DocText {
                text: text.to_string(),
                rev: *rev,
            },
            ToPage::DocReveal { line } => Self::DocReveal(*line),
            ToPage::Timeline { view } => Self::Timeline(view.map(|v| Box::new(v.clone()))),
            ToPage::Scene { view } => Self::Scene(view.map(|v| Box::new(v.clone()))),
            ToPage::Selection { ids } => Self::Selection(ids.to_vec()),
            ToPage::Clock {
                at,
                playing,
                looping,
            } => Self::Clock {
                at: *at,
                playing: *playing,
                looping: *looping,
            },
            ToPage::Thumb { id, url } => Self::Thumb {
                id: id.clone(),
                url: url.to_string(),
            },
            ToPage::TermData { id, bytes } => Self::TermData {
                id: id.to_string(),
                bytes: bytes.to_vec(),
            },
            ToPage::TermExit { id, code, error } => Self::TermExit {
                id: id.to_string(),
                code: *code,
                error: error.clone(),
            },
            ToPage::TermList { tabs } => Self::TermList(tabs.clone()),
            ToPage::TermSize { id, cols, rows } => Self::TermSize {
                id: id.to_string(),
                cols: *cols,
                rows: *rows,
            },
            ToPage::TermState { id, label } => Self::TermState {
                id: id.to_string(),
                label: label.map(str::to_string),
            },
            ToPage::Remote { view } => Self::Remote(Box::new((*view).clone())),
        }
    }
}

/// The page's host when the shell is the interface: what the page would be
/// sent goes to the shell as an event, and the hole is the shell's business
/// (the world is a panel there, drawn into a texture of its own).
pub(super) struct ShellHost {
    events: Sender<ShellEvent>,
    /// To wake the shell's window when something arrives.
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Host for ShellHost {
    fn send(&self, message: &ToPage) {
        if self.events.send(ShellEvent::of(message)).is_ok() {
            (self.wake)();
        }
    }
}

/// The app's end of the link: inserted before the app starts, and taken
/// when the window's side of the protocol opens (`open_page`).
#[derive(Resource)]
pub struct ShellLink {
    pub(super) host: std::sync::Mutex<Option<ShellHost>>,
    pub(super) asks: std::sync::Mutex<Option<Receiver<FromPage>>>,
}

/// The shell's end: what it is told, and where it asks.
pub struct ShellEnd {
    pub events: Receiver<ShellEvent>,
    pub asks: Sender<FromPage>,
}

impl ShellEnd {
    pub fn ask(&self, message: FromPage) {
        let _ = self.asks.send(message);
    }
}

/// A link: the app's end to insert, the shell's to keep. `wake` is called
/// whenever an event is sent, so a window idling between frames repaints.
pub fn pair(wake: impl Fn() + Send + Sync + 'static) -> (ShellLink, ShellEnd) {
    let (events_tx, events) = channel();
    let (asks, asks_rx) = channel();
    (
        ShellLink {
            host: std::sync::Mutex::new(Some(ShellHost {
                events: events_tx,
                wake: Box::new(wake),
            })),
            asks: std::sync::Mutex::new(Some(asks_rx)),
        },
        ShellEnd { events, asks },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_is_told_what_the_page_is_told() {
        let (link, end) = pair(|| {});
        let host = link.host.lock().unwrap().take().unwrap();
        let message = ToPage::TermState {
            id: "t1",
            label: Some("Fix the jetty"),
        };
        host.send(&message);
        let bytes = [27u8, b'[', b'1', b'm', b'h', b'i'];
        let data = ToPage::TermData {
            id: "t1",
            bytes: &bytes,
        };
        host.send(&data);
        match end.events.try_recv().unwrap() {
            ShellEvent::TermState { id, label } => {
                assert_eq!(
                    (id.as_str(), label.as_deref()),
                    ("t1", Some("Fix the jetty"))
                )
            }
            other => panic!("{other:?}"),
        }
        match end.events.try_recv().unwrap() {
            ShellEvent::TermData { id, bytes: got } => {
                assert_eq!((id.as_str(), got.as_slice()), ("t1", &bytes[..]))
            }
            other => panic!("{other:?}"),
        }
    }

    /// The page still reads a tab's bytes as base64, under `b64`.
    #[test]
    fn a_tabs_bytes_go_to_the_page_as_base64() {
        let json = serde_json::to_value(ToPage::TermData {
            id: "t1",
            bytes: b"hi",
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"t": "term.data", "id": "t1", "b64": "aGk="})
        );
    }
}
