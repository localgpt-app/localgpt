//! The timeline: the world's clock and its tracks across the window
//! (`crates/app/ui/src/timeline/` in the page). The shots with their frames,
//! the tour's stops, the song's waveform and stems, what the song moves,
//! the behaviors and the ambience, on one clock, over the bars and sections.
//!
//! What it shows comes from the app (`TimelineView`, built from the world's
//! document); what it keeps is its own: the part of the clock in view, the
//! headers' toggles, the selected shot, a drag in progress. It asks with the
//! page's messages: a seek, play, a loop, a look through a shot, the muted
//! and hidden tracks, a review's answer, and on a live canvas a shot's new
//! in and out points, which land as one entry in the history.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use base64::Engine;
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, pos2, vec2,
};

use super::state::State;
use super::theme::{self, alpha};
use crate::timeline::{CameraKey, Clip, ClockView, ShotTime, Signal, TimelineView, TrackView};
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

const HEADER: f32 = 176.0;
/// The sections band, the bars' numbers and the seconds, one under another.
const RULER: f32 = 44.0;
/// A row of overlapping shots — coverage under its master.
const SHOT_ROW: f32 = 40.0;
/// A shot's edges, where a drag trims it, points wide.
const GRIP: f32 = 6.0;
/// How near a line a dragged edge comes before it snaps there, points.
const PULL: f32 = 8.0;
/// The shortest a trim leaves a shot, seconds.
const SHORTEST: f32 = 0.1;

/// What a drag of a shot holds: its in point, its out point, or all of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    In,
    Out,
    Body,
}

/// What a drag snaps to, as the page's menu has it. The playhead always.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Snap {
    Beats,
    Sections,
    Shots,
    Off,
}

impl Snap {
    const ALL: [Snap; 4] = [Snap::Beats, Snap::Sections, Snap::Shots, Snap::Off];

    fn label(self) -> &'static str {
        match self {
            Snap::Beats => "Beats",
            Snap::Sections => "Sections",
            Snap::Shots => "Shots",
            Snap::Off => "Off",
        }
    }
}

/// A time a drag can snap to, and what it is.
#[derive(Clone, Debug, PartialEq)]
struct Line {
    at: f32,
    label: String,
}

/// A shot held by the pointer: what was pressed, the neighbour whose cut
/// rolls with it, and where it would go if let go now.
#[derive(Clone, Debug)]
struct Held {
    edge: Edge,
    shot: Clip,
    partner: Option<Clip>,
    from: f32,
    to: f32,
    snapped: Option<Line>,
    moved: bool,
}

impl Held {
    /// Where `id` is drawn while this is held: the shot where it would go,
    /// its partner with the rolled cut.
    fn times_of(&self, id: &str) -> Option<(f32, f32)> {
        if !self.moved {
            return None;
        }
        if self.shot.id == id {
            return Some((self.from, self.to));
        }
        let partner = self.partner.as_ref().filter(|p| p.id == id)?;
        Some(match self.edge {
            Edge::Out => (self.to, partner.to),
            _ => (partner.from, self.from),
        })
    }

    /// The edit letting go writes: the shot, and its partner's rolled cut.
    fn edits(&self) -> Vec<ShotTime> {
        let id = |clip: &Clip| clip.id.parse().unwrap_or_default();
        let mut edits = vec![ShotTime {
            id: id(&self.shot),
            in_s: self.from,
            out_s: self.to,
        }];
        if let Some(partner) = &self.partner {
            let (in_s, out_s) = match self.edge {
                Edge::Out => (self.to, partner.to),
                _ => (partner.from, self.from),
            };
            edits.push(ShotTime {
                id: id(partner),
                in_s,
                out_s,
            });
        }
        edits
    }

    /// The drag in words, as the page's toolbar says it.
    fn describe(&self) -> (String, String) {
        let name = match (&self.partner, self.edge) {
            (Some(p), Edge::In) => format!("{} | {}", p.name, self.shot.name),
            (Some(p), _) => format!("{} | {}", self.shot.name, p.name),
            (None, _) => self.shot.name.clone(),
        };
        let detail = if self.edge == Edge::Out {
            format!(
                "out {} → {} · {}",
                timecode(self.shot.to),
                timecode(self.to),
                signed(self.to - self.shot.to)
            )
        } else {
            format!(
                "in {} → {} · {}",
                timecode(self.shot.from),
                timecode(self.from),
                signed(self.from - self.shot.from)
            )
        };
        (name, detail)
    }
}

/// The shortest a recut leaves a section, seconds.
const MIN_SECTION: f32 = 1.0;
/// How near its neighbour a section's start comes before the two join,
/// points.
const JOIN: f32 = 6.0;

/// A section's start held on the band: where it would go, and whether it is
/// onto a neighbour, to join it.
#[derive(Clone, Debug)]
struct Recutting {
    /// The start held: section `section` (from 0) begins there.
    section: usize,
    to: f32,
    snapped: Option<Line>,
    join: bool,
    moved: bool,
}

impl Recutting {
    /// The sections' starts if it were let go now.
    fn apply(&self, starts: &[f32]) -> Vec<f32> {
        let mut next = starts.to_vec();
        if self.section < next.len() {
            if self.join {
                next.remove(self.section);
            } else {
                next[self.section] = self.to;
            }
        }
        next
    }

    /// The recut in words, as the page's toolbar says it.
    fn describe(&self) -> String {
        if self.join {
            format!("§{} and §{} join", self.section, self.section + 1)
        } else {
            match &self.snapped {
                None => timecode(self.to),
                Some(line) if line.label.starts_with("playhead") => line.label.clone(),
                Some(line) => format!("{} · {}", line.label, timecode(self.to)),
            }
        }
    }
}

/// A tour stop's hold, held by its end.
#[derive(Clone, Debug)]
struct Holding {
    track: String,
    tour: String,
    stop: usize,
    /// When the camera reaches the stop, and how long it held there.
    at: f32,
    was: f32,
    hold: f32,
    snapped: Option<Line>,
    moved: bool,
}

/// A tour's stops with one stop's hold changed: the stops after it come
/// later by as much, and the travel between them stays.
fn held_keys(keys: &[CameraKey], stop: usize, hold: f32) -> Vec<CameraKey> {
    let by = hold - keys.get(stop).and_then(|k| k.hold).unwrap_or(0.0);
    keys.iter()
        .enumerate()
        .map(|(i, key)| match i.cmp(&stop) {
            std::cmp::Ordering::Equal => CameraKey {
                hold: (hold > 0.0).then_some(hold),
                ..key.clone()
            },
            std::cmp::Ordering::Greater => CameraKey {
                t: key.t + by,
                ..key.clone()
            },
            std::cmp::Ordering::Less => key.clone(),
        })
        .collect()
}

/// What a drag needs to know of the rest of the timeline.
struct Editing<'a> {
    /// A live canvas: its authority takes the edits.
    can: bool,
    clock: &'a ClockView,
    now: f32,
    view: [f32; 2],
    /// A song is the clock's length; a shot list's grows with its last shot.
    has_song: bool,
    snap: Snap,
    /// The shots, for the cuts a drag snaps to.
    shots: &'a [Clip],
}

/// Seconds across the lanes: `from` at `x0`, `to` at `x0 + width`.
#[derive(Clone, Copy, Debug)]
struct Scale {
    from: f32,
    to: f32,
    x0: f32,
    width: f32,
}

impl Scale {
    fn x(&self, t: f32) -> f32 {
        self.x0 + (t - self.from) / (self.to - self.from) * self.width
    }

    fn t(&self, x: f32) -> f32 {
        self.from + (x - self.x0) / self.width * (self.to - self.from)
    }

    fn pps(&self) -> f32 {
        self.width / (self.to - self.from)
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Flags {
    hidden: bool,
    muted: bool,
    solo: bool,
    locked: bool,
}

pub struct Timeline {
    /// The part of the clock in view, seconds.
    view: Option<[f32; 2]>,
    /// The clock's length the view was last fitted to.
    fitted: Option<f32>,
    flags: HashMap<String, Flags>,
    stems_open: bool,
    selected: Option<String>,
    thumbs: HashMap<String, (u64, egui::TextureHandle)>,
    /// What was last asked to be silent and hidden, so each change is asked
    /// once.
    sent_mute: Option<Vec<String>>,
    sent_hide: Option<Vec<String>>,
    /// The last seek a scrub asked for.
    scrubbed: Option<f32>,
    /// What a drag snaps to; until chosen, beats where there is a beat grid,
    /// else the other shots' cuts.
    snap: Option<Snap>,
    /// A shot under the pointer, being moved or trimmed.
    held: Option<Held>,
    /// Shots' new times sent and not yet in a view the app sent: drawn where
    /// they were let go until the app's word on them arrives.
    pending: HashMap<String, (f32, f32)>,
    /// A section's start under the pointer, and the sections sent.
    recutting: Option<Recutting>,
    pending_sections: Option<Vec<f32>>,
    /// A tour stop's hold under the pointer, and the hold sent: the track,
    /// the stop, the seconds.
    holding: Option<Holding>,
    pending_hold: Option<(String, usize, f32)>,
    /// The timeline view last drawn, by its arrival.
    seen: u64,
    /// A lane asked to be shown (the inspector's link): brought into view
    /// once, and ringed for a moment from when it was asked.
    scroll_to: Option<String>,
    flash: Option<(String, std::time::Instant)>,
    /// The keys are the timeline's: the last press was in it, as the
    /// page's keys go where the last click went.
    focused: bool,
}

/// How long a lane asked for stays ringed.
const FLASH: std::time::Duration = std::time::Duration::from_millis(1600);

impl Timeline {
    pub fn new() -> Self {
        Self {
            view: None,
            fitted: None,
            flags: HashMap::new(),
            stems_open: false,
            selected: None,
            thumbs: HashMap::new(),
            sent_mute: None,
            sent_hide: None,
            scrubbed: None,
            snap: None,
            held: None,
            pending: HashMap::new(),
            recutting: None,
            pending_sections: None,
            holding: None,
            pending_hold: None,
            seen: 0,
            scroll_to: None,
            flash: None,
            focused: false,
        }
    }

    /// Bring the lane of `track` into view and ring it for a moment, as the
    /// page does when the inspector asks for a modulation's track.
    pub fn reveal(&mut self, track: String) {
        self.scroll_to = Some(track.clone());
        self.flash = Some((track, std::time::Instant::now()));
    }

    /// The timeline's keys, as the page's are: Space plays and pauses; ←
    /// and → move a beat (a second with no beat grid), and with Shift go to
    /// the cut before or after; Home and End go to the start and the end;
    /// Enter and Esc accept and reject a waiting proposal, and Esc else
    /// lets go of the selected shot.
    fn keys(
        &mut self,
        ui: &egui::Ui,
        timeline: &TimelineView,
        clock: &ClockView,
        state: &State,
        end: &ShellEnd,
    ) {
        use egui::Key;
        let duration = clock.duration.max(0.01);
        let now = state.clock.now().min(duration);
        let beat = if clock.bpm > 0.0 {
            60.0 / clock.bpm
        } else {
            1.0
        };
        let seek = |at: f32| {
            end.ask(FromPage::ClockSeek {
                at: at.clamp(0.0, duration),
            })
        };
        let pressed = |key| ui.input(|i| i.key_pressed(key));
        if pressed(Key::Space) {
            end.ask(FromPage::ClockPlay {
                playing: !state.clock.playing,
            });
        }
        if pressed(Key::Enter) && timeline.proposal.is_some() {
            end.ask(FromPage::Review { accept: true });
        }
        if pressed(Key::Escape) {
            if timeline.proposal.is_some() {
                end.ask(FromPage::Review { accept: false });
            } else {
                self.selected = None;
            }
        }
        let (left, right) = (pressed(Key::ArrowLeft), pressed(Key::ArrowRight));
        if left || right {
            let forward = right;
            if ui.input(|i| i.modifiers.shift) {
                let cuts = cuts(timeline, clock);
                let to = if forward {
                    cuts.iter().find(|t| **t > now + 0.02)
                } else {
                    cuts.iter().rev().find(|t| **t < now - 0.02)
                };
                if let Some(to) = to {
                    seek(*to);
                }
            } else {
                seek(now + if forward { beat } else { -beat });
            }
        }
        if pressed(Key::Home) {
            seek(0.0);
        }
        if pressed(Key::End) {
            seek(duration);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
        let Some(timeline) = state.timeline.as_deref() else {
            ui.centered_and_justified(|ui| {
                ui.weak(
                    "No clock yet: a song, a screenplay's previs or a world with a tour has one.",
                )
            });
            return;
        };
        let Some(clock) = timeline.clock.as_ref() else {
            // One quiet line, as the page has it, and the world gets the
            // height back.
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("This world keeps no time yet.").color(theme::MUTED));
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "A soundtrack, a shot list or a tour gives it a clock; its \
                             behaviors loop without one.",
                        )
                        .color(theme::FAINT),
                    )
                    .truncate(),
                );
            });
            return;
        };
        self.thumbnails(ui.ctx(), state);
        // A new view is the app's word on the edits sent: what was sent is
        // in it, or was refused.
        if state.timeline_serial != self.seen {
            self.seen = state.timeline_serial;
            self.pending.clear();
            self.pending_sections = None;
            self.pending_hold = None;
        }
        let snap = *self.snap.get_or_insert(if clock.bpm > 0.0 {
            Snap::Beats
        } else {
            Snap::Shots
        });
        // Esc lets go of a drag without writing anything.
        let dragging = self.held.is_some() || self.recutting.is_some() || self.holding.is_some();
        let let_go = dragging && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if let_go {
            self.held = None;
            self.recutting = None;
            self.holding = None;
        }
        // The keys go where the last press went, as the page's do: into
        // the timeline when it was pressed in, and nowhere else then.
        if let Some(at) = ui.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        }) {
            self.focused = ui.max_rect().contains(at);
        }
        if self.focused && !let_go && !ui.ctx().egui_wants_keyboard_input() {
            self.keys(ui, timeline, clock, state, end);
        }
        let duration = clock.duration.max(0.01);
        // The whole clock in view to begin with, and again when the world
        // changes under a view that showed all of it, or ran past its end.
        if self.fitted != Some(duration) {
            if let (Some([from, to]), Some(was)) = (self.view, self.fitted)
                && ((from <= 1e-3 && (to - was).abs() < 1e-2) || to > duration)
            {
                self.view = None;
            }
            self.fitted = Some(duration);
        }
        let mut view = self.view.unwrap_or([0.0, duration]);
        view[1] = view[1].min(duration).max(view[0] + 0.05);
        view[0] = view[0].clamp(0.0, (duration - 0.05).max(0.0));
        let now = state.clock.now().min(duration);
        // Playing past the view pages it on.
        if state.clock.playing && (now > view[1] || now < view[0]) {
            let span = view[1] - view[0];
            view = [
                now.min(duration - span).max(0.0),
                (now + span).min(duration).max(span),
            ];
            view[0] = view[1] - span;
        }

        self.toolbar(ui, state, timeline, clock, &mut view, now, end);
        ui.separator();

        let tracks = self.tracks(timeline);
        let editing = Editing {
            can: timeline.editable,
            clock,
            now,
            view,
            has_song: timeline
                .tracks
                .iter()
                .any(|t| matches!(t, TrackView::Soundtrack { .. })),
            snap,
            shots: timeline
                .tracks
                .iter()
                .find_map(|t| match t {
                    TrackView::Shots { shots, .. } => Some(shots.as_slice()),
                    _ => None,
                })
                .unwrap_or(&[]),
        };
        let full = ui.available_rect_before_wrap();
        let lanes_x = full.left() + HEADER;
        let s = Scale {
            from: view[0],
            to: view[1],
            x0: lanes_x,
            width: (full.width() - HEADER).max(40.0),
        };

        // The ruler, which stays put while the lanes scroll.
        let ruler = Rect::from_min_size(full.min, vec2(full.width(), RULER));
        ui.allocate_rect(ruler, Sense::hover());
        self.ruler(ui, ruler, s, &editing, state.clock.looping, end);

        // The wheel pans; with ⌘ or Ctrl it zooms about the pointer.
        let hovered = ui.rect_contains_pointer(full);
        if hovered {
            let (scroll, zoom, pointer) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.modifiers.command || i.modifiers.ctrl,
                    i.pointer.hover_pos(),
                )
            });
            if zoom && scroll.y != 0.0 {
                let at = pointer.map_or((view[0] + view[1]) / 2.0, |p| s.t(p.x));
                let factor = (1.0 - scroll.y * 0.003).clamp(0.5, 2.0);
                let span = ((view[1] - view[0]) * factor).clamp(0.5, duration);
                let left = ((at - view[0]) / (view[1] - view[0])).clamp(0.0, 1.0);
                view[0] = (at - span * left).clamp(0.0, duration - span);
                view[1] = view[0] + span;
            } else if !zoom && scroll.x != 0.0 {
                let dt = -scroll.x / s.pps();
                let span = view[1] - view[0];
                view[0] = (view[0] + dt).clamp(0.0, duration - span);
                view[1] = view[0] + span;
            }
        }
        self.view = Some(view);

        let lanes_top = ruler.bottom();
        let rest = Rect::from_min_max(pos2(full.left(), lanes_top), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rest));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let total: f32 = tracks.iter().map(height).sum::<f32>().max(rest.height());
                let (rect, _) = ui.allocate_exact_size(vec2(full.width(), total), Sense::hover());
                let painter = ui.painter_at(rect);
                self.grid(&painter, rect, s, clock, state.clock.looping);
                let mut y = rect.top();
                for track in &tracks {
                    let h = height(track);
                    let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), h));
                    self.header(ui, row, track);
                    let lane = Rect::from_min_max(pos2(lanes_x, row.top()), row.max);
                    self.lane(ui, lane, s, &editing, &timeline.signals, track, end);
                    // A lane asked for: into view once, ringed a moment.
                    let id = describe(track).0;
                    if self.scroll_to.as_deref() == Some(id) {
                        self.scroll_to = None;
                        ui.scroll_to_rect(row, Some(egui::Align::Center));
                    }
                    if let Some((_, since)) = self.flash.as_ref().filter(|(t, _)| t == id) {
                        let left = 1.0 - since.elapsed().as_secs_f32() / FLASH.as_secs_f32();
                        if left > 0.0 {
                            painter.rect_stroke(
                                row.shrink(1.0),
                                3.0,
                                Stroke::new(1.5, alpha(theme::HEAD, left)),
                                egui::StrokeKind::Inside,
                            );
                            ui.ctx().request_repaint();
                        } else {
                            self.flash = None;
                        }
                    }
                    painter.line_segment(
                        [
                            pos2(rect.left(), row.bottom()),
                            pos2(rect.right(), row.bottom()),
                        ],
                        Stroke::new(1.0, theme::LINE),
                    );
                    y += h;
                }
                // The header column's edge, and the playhead over the lanes.
                painter.line_segment(
                    [pos2(lanes_x, rect.top()), pos2(lanes_x, rect.bottom())],
                    Stroke::new(1.0, theme::LINE),
                );
                if (view[0]..=view[1]).contains(&now) {
                    let x = s.x(now);
                    painter.line_segment(
                        [pos2(x, rect.top()), pos2(x, rect.bottom())],
                        Stroke::new(1.0, theme::DANGER),
                    );
                }
            });
        // The lanes were drawn in a child: the panel keeps the size it was
        // given only if its parent hears the space was used.
        ui.advance_cursor_after_rect(rest);

        self.ask_silence_and_hiding(timeline, end);
    }

    #[allow(clippy::too_many_arguments)]
    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        state: &State,
        timeline: &TimelineView,
        clock: &ClockView,
        view: &mut [f32; 2],
        now: f32,
        end: &ShellEnd,
    ) {
        let duration = clock.duration.max(0.01);
        ui.horizontal(|ui| {
            ui.strong("Timeline");
            let play = if state.clock.playing { "⏸" } else { "▶" };
            if ui
                .button(play)
                .on_hover_text("Play the world's clock")
                .clicked()
            {
                end.ask(FromPage::ClockPlay {
                    playing: !state.clock.playing,
                });
            }
            if ui.button("⏮").on_hover_text("Back to the start").clicked() {
                end.ask(FromPage::ClockSeek { at: 0.0 });
            }
            ui.monospace(timecode(now));
            if let Some((bar, beat)) = bar_beat(clock, now) {
                ui.weak(format!("bar {bar}.{beat}"));
            }
            ui.separator();
            let looping = state.clock.looping.is_some();
            let label = if looping { "⟲ Loop on" } else { "⟲ Loop" };
            if ui
                .selectable_label(looping, label)
                .on_hover_text("Loop the selected shot, else the section under the playhead, else what is in view")
                .clicked()
            {
                let span = (!looping)
                    .then(|| loop_span(timeline, clock, self.selected.as_deref(), now, *view));
                end.ask(FromPage::ClockLoop { span });
            }
            if ui
                .button("Fit")
                .on_hover_text("The whole clock in view")
                .clicked()
            {
                *view = [0.0, duration];
            }
            if ui
                .button("−")
                .on_hover_text("Zoom out (⌘ and the wheel)")
                .clicked()
            {
                let mid = (view[0] + view[1]) / 2.0;
                let span = ((view[1] - view[0]) * 1.6).min(duration);
                view[0] = (mid - span / 2.0).clamp(0.0, duration - span);
                view[1] = view[0] + span;
            }
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                let mid = if (view[0]..=view[1]).contains(&now) {
                    now
                } else {
                    (view[0] + view[1]) / 2.0
                };
                let span = ((view[1] - view[0]) / 1.6).max(0.5);
                view[0] = (mid - span / 2.0).clamp(0.0, (duration - span).max(0.0));
                view[1] = view[0] + span;
            }
            if timeline
                .tracks
                .iter()
                .any(|t| matches!(t, TrackView::Stem { .. }))
            {
                let label = if self.stems_open {
                    "Stems ▾"
                } else {
                    "Stems ▸"
                };
                if ui.selectable_label(self.stems_open, label).clicked() {
                    self.stems_open = !self.stems_open;
                }
            }
            if let Some(selected) = &self.selected {
                if ui
                    .button("👁 Look through")
                    .on_hover_text("See the world through this shot's camera")
                    .clicked()
                {
                    end.ask(FromPage::ShotLook {
                        id: selected.parse().ok(),
                    });
                }
                if ui.button("Free camera").clicked() {
                    end.ask(FromPage::ShotLook { id: None });
                }
            }
            // On a live canvas, what a drag snaps to.
            if timeline.editable {
                ui.separator();
                let mut snap = self.snap.unwrap_or(Snap::Shots);
                egui::ComboBox::from_id_salt("snap")
                    .selected_text(format!("Snap: {}", snap.label()))
                    .show_ui(ui, |ui| {
                        for option in Snap::ALL {
                            ui.selectable_value(&mut snap, option, option.label());
                        }
                    });
                self.snap = Some(snap);
            }
            // A drag in words while it is held.
            if let Some(held) = self.held.as_ref().filter(|h| h.moved) {
                let (name, detail) = held.describe();
                ui.separator();
                ui.strong(name);
                ui.label(RichText::new(detail).color(theme::MUTED));
            }
            if let Some(recut) = self.recutting.as_ref().filter(|r| r.moved) {
                ui.separator();
                ui.strong(format!("§{}", recut.section + 1));
                ui.label(RichText::new(recut.describe()).color(theme::MUTED));
            }
            if let Some(held) = self.holding.as_ref().filter(|h| h.moved) {
                ui.separator();
                ui.strong("Camera");
                ui.label(
                    RichText::new(format!(
                        "holds {:.1} s · the stops after {}",
                        held.hold,
                        signed(held.hold - held.was)
                    ))
                    .color(theme::MUTED),
                );
            }
            if let Some(proposal) = &timeline.proposal {
                ui.separator();
                ui.colored_label(
                    theme::ACCENT,
                    format!("{} proposes: {}", proposal.author, proposal.what),
                );
                if ui.button("Accept").clicked() {
                    end.ask(FromPage::Review { accept: true });
                }
                if ui.button("Reject").clicked() {
                    end.ask(FromPage::Review { accept: false });
                }
            }
        });
    }

    /// The tracks to draw: the stems only when the song's are open, and a
    /// waiting proposal's shots among the shots.
    fn tracks(&self, timeline: &TimelineView) -> Vec<TrackView> {
        timeline
            .tracks
            .iter()
            .filter(|track| self.stems_open || !matches!(track, TrackView::Stem { .. }))
            .cloned()
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn ruler(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        s: Scale,
        editing: &Editing,
        looping: Option<[f32; 2]>,
        end: &ShellEnd,
    ) {
        let (clock, now) = (editing.clock, editing.now);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::RAISED);
        let lanes = Rect::from_min_max(pos2(s.x0, rect.top()), rect.max);
        // The sections band: a block per section, numbered; as a recut
        // holds them, or as they were sent until the app's view has them.
        let band = Rect::from_min_max(lanes.min, pos2(lanes.right(), lanes.top() + 14.0));
        let fractions = self.pending_sections.as_ref().unwrap_or(&clock.sections);
        let base: Vec<f32> = fractions.iter().map(|f| f * clock.duration).collect();
        let starts = match self.recutting.as_ref().filter(|r| r.moved) {
            Some(recut) => recut.apply(&base),
            None => base.clone(),
        };
        for (i, start) in starts.iter().enumerate() {
            let next = starts.get(i + 1).copied().unwrap_or(clock.duration);
            let (a, b) = (s.x(*start).max(band.left()), s.x(next).min(band.right()));
            if b <= a {
                continue;
            }
            let fill = if i % 2 == 0 {
                alpha(theme::MODULATION, 0.18)
            } else {
                alpha(theme::MODULATION, 0.10)
            };
            painter.rect_filled(
                Rect::from_min_max(pos2(a, band.top()), pos2(b, band.bottom())),
                0.0,
                fill,
            );
            painter.text(
                pos2(a + 4.0, band.center().y),
                Align2::LEFT_CENTER,
                format!("§{}", i + 1),
                FontId::proportional(10.0),
                theme::MUTED,
            );
        }
        // Bars, numbered where there is room, and the time.
        let bars = beat_grid(clock, s);
        for (t, n, label) in &bars {
            let x = s.x(*t);
            painter.line_segment(
                [pos2(x, band.bottom()), pos2(x, band.bottom() + 5.0)],
                Stroke::new(1.0, theme::FAINT),
            );
            if *label {
                painter.text(
                    pos2(x + 2.0, band.bottom() + 1.0),
                    Align2::LEFT_TOP,
                    n.to_string(),
                    FontId::monospace(9.0),
                    theme::FAINT,
                );
            }
        }
        let (major, minor) = ticks(s, 64.0);
        for t in minor {
            let x = s.x(t);
            painter.line_segment(
                [pos2(x, rect.bottom() - 4.0), pos2(x, rect.bottom())],
                Stroke::new(1.0, theme::FAINT),
            );
        }
        // Below the bars' numbers: the seconds, labelled beside their ticks.
        for t in major {
            let x = s.x(t);
            painter.line_segment(
                [pos2(x, rect.bottom() - 12.0), pos2(x, rect.bottom())],
                Stroke::new(1.0, theme::MUTED),
            );
            painter.text(
                pos2(x + 3.0, rect.bottom() - 1.0),
                Align2::LEFT_BOTTOM,
                timecode_short(t),
                FontId::monospace(10.0),
                theme::MUTED,
            );
        }
        if let Some([a, b]) = looping {
            painter.rect_filled(
                Rect::from_min_max(pos2(s.x(a), band.bottom()), pos2(s.x(b), rect.bottom())),
                0.0,
                alpha(theme::ACCENT, 0.18),
            );
        }
        if (s.from..=s.to).contains(&now) {
            let x = s.x(now);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.5, theme::DANGER),
            );
        }
        // A press on the ruler seeks there, and a drag scrubs.
        let response = ui.interact(lanes, ui.scope_id().with("ruler"), Sense::click_and_drag());
        if (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
        {
            let at = s.t(p.x).clamp(0.0, clock.duration);
            if self
                .scrubbed
                .is_none_or(|last| (last - at).abs() > 1.0 / 60.0)
            {
                self.scrubbed = Some(at);
                end.ask(FromPage::ClockSeek { at });
            }
        }
        if response.drag_stopped() || response.clicked() {
            self.scrubbed = None;
        }
        // A song's sections recut on a live canvas: each start but the
        // first a grip on the band, over the ruler; a double click on the
        // band starts a new section there.
        if !(editing.can && editing.has_song) {
            return;
        }
        let duration = clock.duration;
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && band.contains(p)
        {
            let free = ui.input(|i| i.modifiers.alt);
            let at = s.t(p.x);
            let lines = if free {
                Vec::new()
            } else {
                recut_lines(editing, s)
            };
            let t = nearest(&lines, at, PULL / s.pps()).map_or(at, |l| l.at);
            let room = t >= MIN_SECTION
                && t <= duration - MIN_SECTION
                && base.iter().all(|start| (start - t).abs() >= MIN_SECTION);
            if room {
                let mut next = base.clone();
                next.push(t);
                next.sort_by(f32::total_cmp);
                self.send_sections(&next, duration, end);
            }
        }
        for k in 1..base.len() {
            let x = s.x(starts.get(k).copied().unwrap_or(base[k]));
            let grip = Rect::from_min_max(
                pos2(x - GRIP / 2.0, band.top()),
                pos2(x + GRIP / 2.0, band.bottom()),
            );
            let response = ui
                .interact(grip, ui.scope_id().with(("section", k)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeColumn)
                .on_hover_text(format!(
                    "§{} starts here — drag it along the band; onto its neighbour to join them",
                    k + 1
                ));
            if response.drag_started() {
                self.recutting = Some(Recutting {
                    section: k,
                    to: base[k],
                    snapped: None,
                    join: false,
                    moved: false,
                });
            }
            if response.dragged()
                && let Some(recut) = self.recutting.as_mut()
                && let Some((origin, pointer, free)) = ui.input(|i| {
                    Some((
                        i.pointer.press_origin()?,
                        i.pointer.latest_pos()?,
                        i.modifiers.alt,
                    ))
                })
            {
                let dx = pointer.x - origin.x;
                if recut.moved || dx.abs() >= 3.0 {
                    recut.moved = true;
                    let (before, after) =
                        (base[k - 1], base.get(k + 1).copied().unwrap_or(duration));
                    let t = base[k] + dx / s.pps();
                    let reach = JOIN / s.pps();
                    if t <= before + reach || t >= after - reach {
                        // Onto a neighbour: the two sections join.
                        recut.to = if t <= before + reach { before } else { after };
                        recut.snapped = None;
                        recut.join = true;
                    } else {
                        let lines = if free {
                            Vec::new()
                        } else {
                            recut_lines(editing, s)
                        };
                        let line = nearest(&lines, t, PULL / s.pps());
                        let to = clamp(
                            line.map_or(t, |l| l.at),
                            before + MIN_SECTION,
                            after - MIN_SECTION,
                        );
                        recut.snapped = line.filter(|l| l.at == to).cloned();
                        recut.to = to;
                        recut.join = false;
                    }
                }
            }
            if response.drag_stopped()
                && let Some(recut) = self.recutting.take()
                && recut.moved
            {
                self.send_sections(&recut.apply(&base), duration, end);
            }
        }
    }

    /// The sections' starts, sent as fractions of the clock, and drawn so
    /// until the app's view has them.
    fn send_sections(&mut self, starts: &[f32], duration: f32, end: &ShellEnd) {
        let sections: Vec<f32> = starts.iter().map(|t| t / duration.max(0.01)).collect();
        self.pending_sections = Some(sections.clone());
        end.ask(FromPage::SongSections { sections });
    }

    /// Bar lines, section starts and the loop, under the lanes.
    fn grid(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        s: Scale,
        clock: &ClockView,
        looping: Option<[f32; 2]>,
    ) {
        for (t, _, _) in beat_grid(clock, s) {
            let x = s.x(t);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, alpha(theme::LINE, 0.6)),
            );
        }
        for start in clock.sections.iter().map(|f| f * clock.duration) {
            if (s.from..=s.to).contains(&start) {
                let x = s.x(start);
                painter.line_segment(
                    [pos2(x, rect.top()), pos2(x, rect.bottom())],
                    Stroke::new(1.0, alpha(theme::MODULATION, 0.35)),
                );
            }
        }
        if let Some([a, b]) = looping {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(s.x(a).max(s.x0), rect.top()),
                    pos2(s.x(b), rect.bottom()),
                ),
                0.0,
                alpha(theme::ACCENT, 0.06),
            );
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, row: Rect, track: &TrackView) {
        let rect = Rect::from_min_size(row.min, vec2(HEADER, row.height()));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::PANEL);
        let (id, name, kind, color, toggles) = describe(track);
        let flags = self.flags.get(id).copied().unwrap_or_default();
        let quiet = flags.hidden || flags.muted;
        // The name in what the toggles leave, cut short with an ellipsis.
        let room = HEADER - 16.0 - 20.0 * toggles.len() as f32;
        let font = FontId::proportional(12.0);
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.to_string(),
            font,
            if quiet { theme::FAINT } else { theme::INK },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(40.0));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(
            pos2(rect.left() + 8.0, rect.top() + 6.0),
            galley,
            theme::INK,
        );
        // The kind under the name, where the lane has room for two lines.
        if row.height() >= 30.0 {
            painter.text(
                pos2(rect.left() + 8.0, rect.top() + 20.0),
                Align2::LEFT_TOP,
                kind,
                FontId::proportional(10.0),
                color,
            );
        }
        // The toggles the app names for this track, right to left.
        let mut x = rect.right() - 6.0;
        for toggle in ["lock", "hide", "mute", "solo"] {
            if !toggles.contains(&toggle) {
                continue;
            }
            let set = match toggle {
                "hide" => flags.hidden,
                "mute" => flags.muted,
                "solo" => flags.solo,
                _ => flags.locked,
            };
            let glyph = match toggle {
                "hide" if set => "◌",
                "hide" => "◉",
                "mute" if set => "🔇",
                "mute" => "🔈",
                "solo" => "S",
                _ => "🔒",
            };
            let at = Rect::from_center_size(pos2(x - 9.0, rect.top() + 12.0), vec2(18.0, 18.0));
            x -= 20.0;
            let response = ui
                .interact(at, ui.scope_id().with((id, toggle)), Sense::click())
                .on_hover_text(match toggle {
                    "hide" if set => "Show",
                    "hide" => "Hide what it moves",
                    "mute" if set => "Unmute",
                    "mute" => "Mute",
                    "solo" => "Solo",
                    _ => "Lock",
                });
            let tint = if set { theme::ACCENT } else { theme::FAINT };
            if response.hovered() {
                painter.rect_filled(at, 3.0, theme::RAISED);
            }
            painter.text(
                at.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(11.0),
                tint,
            );
            if response.clicked() {
                let entry = self.flags.entry(id.to_string()).or_default();
                match toggle {
                    "hide" => entry.hidden = !entry.hidden,
                    "mute" => entry.muted = !entry.muted,
                    "solo" => entry.solo = !entry.solo,
                    _ => entry.locked = !entry.locked,
                }
            }
        }
    }

    /// A track's lane. A modulation's samples are the view's `signals`,
    /// named by the track's source: the app sends each signal once.
    #[allow(clippy::too_many_arguments)]
    fn lane(
        &mut self,
        ui: &mut egui::Ui,
        lane: Rect,
        s: Scale,
        editing: &Editing,
        signals: &BTreeMap<String, Signal>,
        track: &TrackView,
        end: &ShellEnd,
    ) {
        let clock = editing.clock;
        // A click where nothing takes it seeks there. Registered first, so
        // what the lane draws that takes a click (a shot) is above it.
        let response = ui.interact(
            lane,
            ui.scope_id().with(("lane", describe(track).0)),
            Sense::click(),
        );
        // In the shots' lane, a click between shots lets the selected go.
        if response.clicked() && matches!(track, TrackView::Shots { .. }) {
            self.selected = None;
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            end.ask(FromPage::ClockSeek {
                at: s.t(p.x).clamp(0.0, clock.duration),
            });
        }
        let painter = ui.painter_at(lane);
        let h = lane.height();
        match track {
            TrackView::Shots { id, shots, .. } => {
                let locked = self.flags.get(id).is_some_and(|f| f.locked);
                self.shots(ui, &painter, lane, s, shots, editing, !locked, end)
            }
            TrackView::Camera { id, tour, keys, .. } => {
                let locked = self.flags.get(id).is_some_and(|f| f.locked);
                self.camera_lane(
                    ui,
                    &painter,
                    lane,
                    s,
                    (id, tour),
                    keys,
                    editing,
                    !locked,
                    end,
                )
            }
            TrackView::Soundtrack {
                title,
                artist,
                licence,
                peaks,
                ..
            } => {
                let song = Rect::from_min_max(
                    pos2(s.x(0.0) + 1.0, lane.top() + 4.0),
                    pos2(s.x(clock.duration) - 1.0, lane.bottom() - 4.0),
                );
                painter.rect(
                    song,
                    3.0,
                    alpha(theme::SOUND, 0.07),
                    Stroke::new(1.0, alpha(theme::SOUND, 0.3)),
                    egui::StrokeKind::Inside,
                );
                painter.add(area(peaks, s, lane, true, alpha(theme::SOUND, 0.5)));
                let label = [title.as_str(), artist.as_str(), licence.as_str()]
                    .into_iter()
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                painter.text(
                    pos2(song.left().max(lane.left()) + 6.0, lane.top() + 6.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(10.0),
                    theme::INK,
                );
            }
            TrackView::Stem { envelope, .. } => {
                painter.add(area(envelope, s, lane, false, alpha(theme::SOUND, 0.3)));
            }
            TrackView::Modulation { source, .. } => {
                if let Some(signal) = signals.get(source) {
                    painter.add(area(signal, s, lane, false, alpha(theme::MODULATION, 0.12)));
                    let line = points(signal, s)
                        .into_iter()
                        .map(|(x, v)| pos2(x, lane.bottom() - 3.0 - v * (h - 6.0)))
                        .collect::<Vec<_>>();
                    painter.add(Shape::line(line, Stroke::new(1.25, theme::MODULATION)));
                }
            }
            TrackView::Behavior {
                what,
                period,
                from,
                to,
                ..
            } => {
                let bar = Rect::from_min_max(
                    pos2(s.x(*from) + 1.0, lane.top() + 4.0),
                    pos2(s.x(*to) - 1.0, lane.bottom() - 4.0),
                );
                painter.rect(
                    bar,
                    3.0,
                    alpha(theme::BEHAVIOR, 0.1),
                    Stroke::new(1.0, alpha(theme::BEHAVIOR, 0.35)),
                    egui::StrokeKind::Inside,
                );
                if *period > 0.0 && period * s.pps() >= 5.0 {
                    let mut t = from + period;
                    while t < *to && t <= s.to {
                        let x = s.x(t);
                        if x >= lane.left() {
                            painter.line_segment(
                                [pos2(x, bar.top() + 3.0), pos2(x, bar.bottom() - 3.0)],
                                Stroke::new(1.0, alpha(theme::BEHAVIOR, 0.35)),
                            );
                        }
                        t += period;
                    }
                }
                painter.text(
                    pos2(bar.left().max(lane.left()) + 6.0, bar.center().y),
                    Align2::LEFT_CENTER,
                    format!("{what} · every {period} s"),
                    FontId::proportional(10.0),
                    theme::BEHAVIOR,
                );
            }
            TrackView::Ambience {
                name,
                from,
                to,
                fade,
                ..
            } => {
                let (a, b) = (s.x(*from), s.x(*to));
                let (fi, fo) = (s.x(from + fade[0]) - a, b - s.x(to - fade[1]));
                let shape = vec![
                    pos2(a, lane.bottom() - 3.0),
                    pos2(a + fi, lane.top() + 3.0),
                    pos2(b - fo, lane.top() + 3.0),
                    pos2(b, lane.bottom() - 3.0),
                ];
                painter.add(Shape::convex_polygon(
                    shape,
                    alpha(theme::AMBIENCE, 0.16),
                    Stroke::new(1.0, alpha(theme::AMBIENCE, 0.45)),
                ));
                painter.text(
                    pos2((a + fi + 4.0).max(lane.left() + 4.0), lane.center().y),
                    Align2::LEFT_CENTER,
                    format!("{name} bed"),
                    FontId::proportional(10.0),
                    theme::AMBIENCE,
                );
            }
        }
    }

    /// The shots in rows, each with its frame. A click selects one and a
    /// double click looks through it; on a live canvas a drag moves it and
    /// one on an edge trims it, rolling the cut with a shot that meets it
    /// there, and letting go writes one entry (`ShotTimes`).
    #[allow(clippy::too_many_arguments)]
    fn shots(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        lane: Rect,
        s: Scale,
        shots: &[Clip],
        editing: &Editing,
        unlocked: bool,
        end: &ShellEnd,
    ) {
        let can = editing.can && unlocked;
        // Each where a drag holds it, or where it was let go until the
        // app's view has it.
        let shown: Vec<Clip> = shots
            .iter()
            .map(|shot| {
                let (from, to) = self
                    .held
                    .as_ref()
                    .and_then(|held| held.times_of(&shot.id))
                    .or_else(|| self.pending.get(&shot.id).copied())
                    .unwrap_or((shot.from, shot.to));
                Clip {
                    from,
                    to,
                    ..shot.clone()
                }
            })
            .collect();
        // The rows are the view's, so a held shot keeps its row.
        let (placed, rows, count) = pack(shots);
        let row_h = if count == 1 {
            lane.height() - 8.0
        } else {
            SHOT_ROW
        };
        for (view_shot, row) in placed.iter().zip(rows) {
            let Some(shot) = shown.iter().find(|c| c.id == view_shot.id) else {
                continue;
            };
            let top = lane.top() + 4.0 + row as f32 * (SHOT_ROW + 4.0);
            let rect = Rect::from_min_max(
                pos2(s.x(shot.from) + 1.0, top),
                pos2(s.x(shot.to) - 1.0, top + row_h),
            );
            let held_here = self.held.as_ref().is_some_and(|h| h.shot.id == shot.id);
            if !held_here && (rect.right() < lane.left() || rect.left() > lane.right()) {
                continue;
            }
            let selected = self.selected.as_deref() == Some(shot.id.as_str());
            let stroke = if selected {
                Stroke::new(1.5, theme::INK)
            } else {
                Stroke::new(1.0, alpha(theme::SHOT, 0.5))
            };
            painter.rect(
                rect,
                3.0,
                alpha(theme::SHOT, 0.14),
                stroke,
                egui::StrokeKind::Inside,
            );
            // The frame, where it fits.
            let mut text_left = rect.left() + 6.0;
            if let Some((_, texture)) = self.thumbs.get(&shot.id) {
                let frame_h = rect.height() - 6.0;
                let frame_w = frame_h * 16.0 / 9.0;
                if rect.width() > frame_w + 12.0 {
                    let frame = Rect::from_min_size(
                        pos2(rect.left() + 3.0, rect.top() + 3.0),
                        vec2(frame_w, frame_h),
                    );
                    painter.image(
                        texture.id(),
                        frame,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    text_left = frame.right() + 6.0;
                }
            }
            painter.text(
                pos2(text_left, rect.top() + 4.0),
                Align2::LEFT_TOP,
                &shot.name,
                FontId::proportional(11.0),
                theme::INK,
            );
            if rect.height() > 30.0 {
                painter.text(
                    pos2(text_left, rect.top() + 19.0),
                    Align2::LEFT_TOP,
                    format!(
                        "{} · {:.0} mm · {}",
                        shot.size,
                        shot.lens,
                        seconds(shot.to - shot.from)
                    ),
                    FontId::proportional(10.0),
                    theme::MUTED,
                );
            }
            let id = ui.scope_id().with(("shot", &shot.id));
            let body = if can {
                ui.interact(rect.intersect(lane), id, Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text(
                        "A click selects it; a double click looks through it; a drag moves it, \
                         and one on an edge trims it (Alt: no snapping, Esc: let go)",
                    )
            } else {
                ui.interact(rect.intersect(lane), id, Sense::click())
                    .on_hover_text("A click selects it; a double click looks through it")
            };
            if body.clicked() {
                self.selected = Some(shot.id.clone());
            }
            // A double click looks through it from its first frame.
            if body.double_clicked() {
                end.ask(FromPage::ClockSeek { at: shot.from });
                end.ask(FromPage::ShotLook {
                    id: shot.id.parse().ok(),
                });
            }
            let mut handles = vec![(Edge::Body, body)];
            if can {
                // The edges over the body, so a press there trims.
                let left = Rect::from_min_max(rect.min, pos2(rect.left() + GRIP, rect.bottom()));
                let right = Rect::from_min_max(pos2(rect.right() - GRIP, rect.top()), rect.max);
                for (edge, grip, salt) in [(Edge::In, left, "in"), (Edge::Out, right, "out")] {
                    let response = ui
                        .interact(grip.intersect(lane), id.with(salt), Sense::drag())
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                    handles.push((edge, response));
                }
            }
            for (edge, response) in handles {
                if response.drag_started() && can {
                    self.selected = Some(shot.id.clone());
                    self.held = Some(Held {
                        edge,
                        shot: shot.clone(),
                        partner: partner_of(&shown, shot, edge),
                        from: shot.from,
                        to: shot.to,
                        snapped: None,
                        moved: false,
                    });
                }
                if response.dragged() {
                    self.drag(ui, s, editing, &shown);
                }
                if response.drag_stopped()
                    && let Some(held) = self.held.take()
                    && held.moved
                {
                    let edits = held.edits();
                    for edit in &edits {
                        self.pending
                            .insert(edit.id.to_string(), (edit.in_s, edit.out_s));
                    }
                    end.ask(FromPage::ShotTimes { edits });
                }
            }
        }
        // What the held edge snapped to, across the lane.
        if let Some(line) = self
            .held
            .as_ref()
            .filter(|h| h.moved)
            .and_then(|h| h.snapped.as_ref())
        {
            snapped_line(painter, lane, s, line);
        }
    }

    /// A tour's stops on the clock. On a live canvas the end of each stop's
    /// hold is a grip (a stop that holds nothing has it on the stop): a drag
    /// changes how long the camera waits there, the stops after it coming
    /// later by as much, and letting go writes it (`tour.hold`).
    #[allow(clippy::too_many_arguments)]
    fn camera_lane(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        lane: Rect,
        s: Scale,
        (id, tour): (&str, &str),
        keys: &[CameraKey],
        editing: &Editing,
        unlocked: bool,
        end: &ShellEnd,
    ) {
        let shown = match (&self.holding, &self.pending_hold) {
            (Some(h), _) if h.moved && h.track == id => held_keys(keys, h.stop, h.hold),
            (_, Some((track, stop, hold))) if track == id => held_keys(keys, *stop, *hold),
            _ => keys.to_vec(),
        };
        camera(painter, lane, s, &shown);
        if !(editing.can && unlocked) {
            return;
        }
        for (i, key) in shown.iter().enumerate() {
            let hold = key.hold.unwrap_or(0.0);
            let left = s.x(key.t + hold) - if hold > 0.0 { GRIP / 2.0 } else { 0.0 };
            let grip = Rect::from_min_max(pos2(left, lane.top()), pos2(left + GRIP, lane.bottom()));
            let tip = if hold > 0.0 {
                format!("The camera waits {hold} s here — drag to change it")
            } else {
                "Drag to keep the camera here a while".to_string()
            };
            let response = ui
                .interact(
                    grip.intersect(lane),
                    ui.scope_id().with(("hold", id, i)),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text(tip);
            if response.drag_started() {
                self.holding = Some(Holding {
                    track: id.to_string(),
                    tour: tour.to_string(),
                    stop: i,
                    at: key.t,
                    was: hold,
                    hold,
                    snapped: None,
                    moved: false,
                });
            }
            if response.dragged()
                && let Some(held) = self.holding.as_mut()
                && let Some((origin, pointer, free)) = ui.input(|i| {
                    Some((
                        i.pointer.press_origin()?,
                        i.pointer.latest_pos()?,
                        i.modifiers.alt,
                    ))
                })
            {
                let dx = pointer.x - origin.x;
                if held.moved || dx.abs() >= 3.0 {
                    held.moved = true;
                    let t = held.at + held.was + dx / s.pps();
                    let lines = if free || editing.snap == Snap::Off {
                        Vec::new()
                    } else {
                        snap_lines(
                            editing.snap,
                            editing.clock,
                            editing.shots,
                            &[],
                            editing.now,
                            [s.from, s.to],
                        )
                    };
                    let line = nearest(&lines, t, PULL / s.pps());
                    held.hold = (line.map_or(t, |l| l.at) - held.at).max(0.0);
                    held.snapped = line
                        .filter(|l| (l.at - held.at - held.hold).abs() < 1e-5)
                        .cloned();
                }
            }
            if response.drag_stopped()
                && let Some(held) = self.holding.take()
                && held.moved
                && (held.hold - held.was).abs() >= 0.005
            {
                let hold = (held.hold * 100.0).round() / 100.0;
                self.pending_hold = Some((held.track, held.stop, hold));
                end.ask(FromPage::TourHold {
                    tour: held.tour,
                    stop: held.stop,
                    hold,
                });
            }
        }
        if let Some(line) = self
            .holding
            .as_ref()
            .filter(|h| h.moved && h.track == id)
            .and_then(|h| h.snapped.as_ref())
        {
            snapped_line(painter, lane, s, line);
        }
    }

    /// The held shot follows the pointer, past 3 points of travel: snapped to
    /// the lines the menu chose and the playhead, unless Alt is down.
    fn drag(&mut self, ui: &egui::Ui, s: Scale, editing: &Editing, shown: &[Clip]) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        let Some((origin, pointer, free)) = ui.input(|i| {
            Some((
                i.pointer.press_origin()?,
                i.pointer.latest_pos()?,
                i.modifiers.alt,
            ))
        }) else {
            return;
        };
        let dx = pointer.x - origin.x;
        if !held.moved && dx.abs() < 3.0 {
            return;
        }
        held.moved = true;
        let ids: Vec<String> = std::iter::once(held.shot.id.clone())
            .chain(held.partner.as_ref().map(|p| p.id.clone()))
            .collect();
        let except: Vec<&str> = ids.iter().map(String::as_str).collect();
        let lines = if free || editing.snap == Snap::Off {
            Vec::new()
        } else {
            snap_lines(
                editing.snap,
                editing.clock,
                shown,
                &except,
                editing.now,
                editing.view,
            )
        };
        let limit = if editing.has_song {
            editing.clock.duration
        } else {
            f32::INFINITY
        };
        let (from, to, snapped) = place(held, dx / s.pps(), &lines, PULL / s.pps(), limit);
        held.from = from;
        held.to = to;
        held.snapped = snapped;
    }

    /// Each shot's newest frame, decoded once for each version the app sends.
    fn thumbnails(&mut self, ctx: &egui::Context, state: &State) {
        for (id, url) in &state.thumbs {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            url.hash(&mut hasher);
            let version = hasher.finish();
            if self.thumbs.get(id).is_some_and(|(v, _)| *v == version) {
                continue;
            }
            let Some(image) = decode_data_url(url) else {
                continue;
            };
            let texture =
                ctx.load_texture(format!("shot {id}"), image, egui::TextureOptions::LINEAR);
            self.thumbs.insert(id.clone(), (version, texture));
        }
    }

    /// The muted and hidden tracks, asked of the app when they change: the
    /// muted, or with a solo anywhere, every heard track but the soloed.
    fn ask_silence_and_hiding(&mut self, timeline: &TimelineView, end: &ShellEnd) {
        let heard: Vec<&str> = timeline
            .tracks
            .iter()
            .map(describe)
            .filter(|(_, _, _, _, toggles)| toggles.contains(&"mute"))
            .map(|(id, ..)| id)
            .collect();
        let flag = |id: &str| self.flags.get(id).copied().unwrap_or_default();
        let soloed: Vec<&str> = heard.iter().copied().filter(|id| flag(id).solo).collect();
        let mut muted: Vec<String> = heard
            .iter()
            .filter(|id| flag(id).muted || (!soloed.is_empty() && !soloed.contains(id)))
            .map(|id| id.to_string())
            .collect();
        muted.sort();
        let mut hidden: Vec<String> = timeline
            .tracks
            .iter()
            .map(describe)
            .filter(|(id, _, _, _, toggles)| toggles.contains(&"hide") && flag(id).hidden)
            .map(|(id, ..)| id.to_string())
            .collect();
        hidden.sort();
        if self.sent_mute.as_ref() != Some(&muted) {
            end.ask(FromPage::Mute { ids: muted.clone() });
            self.sent_mute = Some(muted);
        }
        if self.sent_hide.as_ref() != Some(&hidden) {
            end.ask(FromPage::Hide {
                ids: hidden.clone(),
            });
            self.sent_hide = Some(hidden);
        }
    }
}

/// A track's id, name, kind, colour and the toggles the app names for it.
fn describe(track: &TrackView) -> (&str, &str, &'static str, Color32, &[&'static str]) {
    match track {
        TrackView::Shots {
            id, name, toggles, ..
        } => (id, name, "shots", theme::SHOT, toggles),
        TrackView::Camera {
            id, name, toggles, ..
        } => (id, name, "camera", theme::CAMERA, toggles),
        TrackView::Soundtrack {
            id, name, toggles, ..
        } => (id, name, "soundtrack", theme::SOUND, toggles),
        TrackView::Stem { id, name, .. } => (id, name, "stem", theme::SOUND, &[]),
        TrackView::Modulation {
            id, name, toggles, ..
        } => (id, name, "modulation", theme::MODULATION, toggles),
        TrackView::Behavior {
            id, name, toggles, ..
        } => (id, name, "behavior", theme::BEHAVIOR, toggles),
        TrackView::Ambience {
            id, name, toggles, ..
        } => (id, name, "ambience", theme::AMBIENCE, toggles),
    }
}

/// The timeline's height, as the page sizes its own: the toolbar, the ruler
/// and every lane, the lanes up to 30% of the window's height (between 110
/// and 340) and scrolling past it; one line for a world with no clock. It
/// follows what the timeline holds, so a world opened in place of another
/// gets its own.
pub fn wanted_height(timeline: Option<&TimelineView>, window: f32) -> f32 {
    const TOOLBAR: f32 = 34.0;
    match timeline {
        Some(timeline) if timeline.clock.is_some() => {
            let lanes: f32 = timeline.tracks.iter().map(height).sum();
            let most = (window * 0.3).clamp(110.0, 340.0);
            TOOLBAR + RULER + lanes.min(most) + 12.0
        }
        _ => 40.0,
    }
}

fn height(track: &TrackView) -> f32 {
    match track {
        TrackView::Shots { shots, .. } => {
            let (_, _, rows) = pack(shots);
            if rows == 1 {
                56.0
            } else {
                4.0 + rows as f32 * (SHOT_ROW + 4.0)
            }
        }
        TrackView::Camera { .. } => 26.0,
        TrackView::Soundtrack { .. } => 46.0,
        TrackView::Stem { .. } => 18.0,
        TrackView::Modulation { .. } => 30.0,
        TrackView::Behavior { .. } | TrackView::Ambience { .. } => 22.0,
    }
}

/// Shots in rows: each where the row it goes in is free, so coverage stacks
/// under its master. Returns the shots in start order, each one's row, and
/// the rows.
fn pack(shots: &[Clip]) -> (Vec<&Clip>, Vec<usize>, usize) {
    let mut placed: Vec<&Clip> = shots.iter().collect();
    placed.sort_by(|a, b| a.from.total_cmp(&b.from).then(a.to.total_cmp(&b.to)));
    let mut ends: Vec<f32> = Vec::new();
    let mut rows = Vec::with_capacity(placed.len());
    for shot in &placed {
        let row = match ends.iter().position(|end| *end <= shot.from + 1e-6) {
            Some(r) => {
                ends[r] = shot.to;
                r
            }
            None => {
                ends.push(shot.to);
                ends.len() - 1
            }
        };
        rows.push(row);
    }
    (placed, rows, ends.len().max(1))
}

fn camera(painter: &egui::Painter, lane: Rect, s: Scale, keys: &[CameraKey]) {
    let y = lane.center().y;
    for pair in keys.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        painter.line_segment(
            [pos2(s.x(a.t + a.hold.unwrap_or(0.0)), y), pos2(s.x(b.t), y)],
            Stroke::new(1.0, alpha(theme::CAMERA, 0.45)),
        );
    }
    for key in keys {
        if let Some(hold) = key.hold {
            painter.rect_filled(
                Rect::from_min_max(pos2(s.x(key.t), y - 2.5), pos2(s.x(key.t + hold), y + 2.5)),
                2.5,
                alpha(theme::CAMERA, 0.55),
            );
        }
        let c = pos2(s.x(key.t), y);
        painter.add(Shape::convex_polygon(
            vec![
                c + vec2(0.0, -5.0),
                c + vec2(5.0, 0.0),
                c + vec2(0.0, 5.0),
                c + vec2(-5.0, 0.0),
            ],
            theme::CAMERA,
            Stroke::NONE,
        ));
    }
}

/// A signal's points in view, `(x, value)`: where samples crowd several to
/// a pixel — a song's waveform at fit — each pixel keeps its loudest.
fn points(signal: &Signal, s: Scale) -> Vec<(f32, f32)> {
    let n = signal.values.len();
    if n == 0 || signal.rate <= 0.0 {
        return Vec::new();
    }
    let first = ((s.from * signal.rate).floor() as isize - 1).max(0) as usize;
    let last = (((s.to * signal.rate).ceil() as isize + 1).max(0) as usize).min(n - 1);
    let mut out = Vec::new();
    if signal.rate / s.pps() <= 4.0 {
        for i in first..=last {
            out.push((s.x(i as f32 / signal.rate), signal.values[i]));
        }
        return out;
    }
    let mut px = f32::NAN;
    let mut hi = 0.0f32;
    for i in first..=last {
        let x = s.x(i as f32 / signal.rate).round();
        let v = signal.values[i];
        if x != px {
            if !px.is_nan() {
                out.push((px, hi));
            }
            px = x;
            hi = v;
        } else if v > hi {
            hi = v;
        }
    }
    if !px.is_nan() {
        out.push((px, hi));
    }
    out
}

/// A signal as a filled band over a lane: mirrored about the middle (a
/// waveform), or standing on the bottom (an envelope).
fn area(signal: &Signal, s: Scale, lane: Rect, mirror: bool, color: Color32) -> Shape {
    let pts = points(signal, s);
    let mut mesh = egui::Mesh::default();
    let h = lane.height();
    for (i, (x, v)) in pts.iter().enumerate() {
        let (top, bottom) = if mirror {
            let mid = lane.top() + h / 2.0;
            (mid - v * h / 2.0 * 0.9, mid + v * h / 2.0 * 0.9)
        } else {
            (lane.bottom() - v * (h - 3.0), lane.bottom())
        };
        mesh.colored_vertex(pos2(*x, top), color);
        mesh.colored_vertex(pos2(*x, bottom), color);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    Shape::mesh(mesh)
}

/// The beat grid in view: each bar's start, its number from 1, and whether
/// it is far enough from the last numbered one to number.
fn beat_grid(clock: &ClockView, s: Scale) -> Vec<(f32, u32, bool)> {
    if clock.bpm <= 0.0 {
        return Vec::new();
    }
    let beat = 60.0 / clock.bpm;
    let bar = beat * 4.0;
    let every = [1u32, 2, 4, 8, 16, 32]
        .into_iter()
        .find(|k| *k as f32 * bar * s.pps() >= 30.0)
        .unwrap_or(64);
    let start = ((s.from - clock.beat_offset) / bar).floor().max(0.0) as u32;
    let mut out = Vec::new();
    let mut i = start;
    loop {
        let t = clock.beat_offset + i as f32 * bar;
        if t > s.to.min(clock.duration) {
            break;
        }
        if t >= s.from {
            let n = i + 1;
            out.push((t, n, (n - 1).is_multiple_of(every)));
        }
        i += 1;
    }
    out
}

/// The ruler's labelled ticks, at least `gap` px apart, and the minor ticks
/// between.
fn ticks(s: Scale, gap: f32) -> (Vec<f32>, Vec<f32>) {
    const STEPS: [(f32, f32); 9] = [
        (1.0, 0.5),
        (2.0, 0.5),
        (5.0, 1.0),
        (10.0, 2.0),
        (15.0, 5.0),
        (30.0, 10.0),
        (60.0, 10.0),
        (120.0, 30.0),
        (300.0, 60.0),
    ];
    let pps = s.pps();
    let (step, minor_step) = STEPS
        .into_iter()
        .find(|(k, _)| k * pps >= gap)
        .unwrap_or(STEPS[STEPS.len() - 1]);
    let (mut major, mut minor) = (Vec::new(), Vec::new());
    let mut k = (s.from / minor_step).ceil() as i64;
    loop {
        let t = k as f32 * minor_step;
        if t > s.to + 1e-6 {
            break;
        }
        if ((t / step) - (t / step).round()).abs() < 1e-4 {
            major.push(t);
        } else {
            minor.push(t);
        }
        k += 1;
    }
    (major, minor)
}

/// Bar and beat, both from 1, or none without a grid.
fn bar_beat(clock: &ClockView, t: f32) -> Option<(u32, u32)> {
    if clock.bpm <= 0.0 {
        return None;
    }
    let beats = ((t - clock.beat_offset) * clock.bpm / 60.0).max(0.0);
    Some(((beats / 4.0) as u32 + 1, (beats % 4.0) as u32 + 1))
}

fn timecode(t: f32) -> String {
    let t = t.max(0.0);
    let minutes = (t / 60.0) as u32;
    let rest = t - minutes as f32 * 60.0;
    format!("{minutes}:{rest:05.2}")
}

fn timecode_short(t: f32) -> String {
    let t = t.max(0.0).round() as u32;
    format!("{}:{:02}", t / 60, t % 60)
}

/// Where the cuts are, for Shift and an arrow: the shots' ins and outs, or
/// with no shots the sections' starts; and the clock's two ends.
fn cuts(timeline: &TimelineView, clock: &ClockView) -> Vec<f32> {
    let shots: Vec<f32> = timeline
        .tracks
        .iter()
        .filter_map(|t| match t {
            TrackView::Shots { shots, .. } => Some(shots),
            _ => None,
        })
        .flatten()
        .flat_map(|s| [s.from, s.to])
        .collect();
    let mut all = if shots.is_empty() {
        clock.sections.iter().map(|f| f * clock.duration).collect()
    } else {
        shots
    };
    all.extend([0.0, clock.duration]);
    all.sort_by(f32::total_cmp);
    all.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    all
}

/// What Loop loops, as the page chooses: the selected shot, else the
/// section under the playhead, else what is in view.
fn loop_span(
    timeline: &TimelineView,
    clock: &ClockView,
    selected: Option<&str>,
    now: f32,
    view: [f32; 2],
) -> [f32; 2] {
    let shot = selected.and_then(|id| {
        timeline.tracks.iter().find_map(|t| match t {
            TrackView::Shots { shots, .. } => shots.iter().find(|s| s.id == id),
            _ => None,
        })
    });
    if let Some(shot) = shot {
        return [shot.from, shot.to];
    }
    let mut starts: Vec<f32> = clock.sections.iter().map(|f| f * clock.duration).collect();
    starts.push(clock.duration);
    if starts.len() > 2
        && let Some(w) = starts.windows(2).find(|w| now >= w[0] && now < w[1])
    {
        return [w[0], w[1]];
    }
    view
}

/// "+0.50 s", "−1.25 s": how far a drag moved an edge.
fn signed(d: f32) -> String {
    format!("{}{:.2} s", if d < 0.0 { "−" } else { "+" }, d.abs())
}

/// The page's clamp: `lo` wins when the range is empty.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(lo.max(hi))
}

/// The shot whose cut an edge's trim rolls: the one that meets `shot` at
/// that edge.
fn partner_of(shown: &[Clip], shot: &Clip, edge: Edge) -> Option<Clip> {
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    shown
        .iter()
        .find(|other| {
            other.id != shot.id
                && match edge {
                    Edge::Out => near(other.from, shot.to),
                    Edge::In => near(other.to, shot.from),
                    Edge::Body => false,
                }
        })
        .cloned()
}

/// What a drag can snap to under `snap`, and the playhead always: the beats
/// in view (bars named), the sections' starts and the end, or the other
/// shots' cuts.
fn snap_lines(
    snap: Snap,
    clock: &ClockView,
    shots: &[Clip],
    except: &[&str],
    now: f32,
    view: [f32; 2],
) -> Vec<Line> {
    let mut lines = vec![Line {
        at: now,
        label: format!("playhead · {}", timecode(now)),
    }];
    if snap == Snap::Beats && clock.bpm > 0.0 {
        let beat = 60.0 / clock.bpm;
        let mut i = ((view[0] - clock.beat_offset) / beat).floor().max(0.0) as u32;
        loop {
            let t = clock.beat_offset + i as f32 * beat;
            if t > view[1].min(clock.duration) {
                break;
            }
            let bar = i / 4 + 1;
            let label = if i.is_multiple_of(4) {
                format!("bar {bar}")
            } else {
                format!("bar {bar} · {}", i % 4 + 1)
            };
            lines.push(Line { at: t, label });
            i += 1;
        }
    }
    if matches!(snap, Snap::Beats | Snap::Sections) {
        for (i, t) in clock
            .sections
            .iter()
            .map(|f| f * clock.duration)
            .enumerate()
        {
            lines.push(Line {
                at: t,
                label: format!("§{} · {}", i + 1, timecode(t)),
            });
        }
        lines.push(Line {
            at: clock.duration,
            label: format!("end · {}", timecode(clock.duration)),
        });
    }
    if snap == Snap::Shots {
        for shot in shots.iter().filter(|s| !except.contains(&s.id.as_str())) {
            lines.push(Line {
                at: shot.from,
                label: format!("{} in", shot.name),
            });
            lines.push(Line {
                at: shot.to,
                label: format!("{} out", shot.name),
            });
        }
    }
    lines
}

/// The line a drag snapped to, dashed across the lane, with what it is.
fn snapped_line(painter: &egui::Painter, lane: Rect, s: Scale, line: &Line) {
    let x = s.x(line.at);
    painter.add(Shape::dashed_line(
        &[pos2(x, lane.top()), pos2(x, lane.bottom())],
        Stroke::new(1.0, theme::HEAD),
        4.0,
        3.0,
    ));
    painter.text(
        pos2(x + 4.0, lane.top() + 2.0),
        Align2::LEFT_TOP,
        &line.label,
        FontId::proportional(10.0),
        theme::HEAD,
    );
}

/// What a section's start snaps to, as the page has it: the bars, or the
/// shots' cuts, and the playhead.
fn recut_lines(editing: &Editing, s: Scale) -> Vec<Line> {
    let view = [s.from, s.to];
    match editing.snap {
        Snap::Off => Vec::new(),
        Snap::Shots => snap_lines(
            Snap::Shots,
            editing.clock,
            editing.shots,
            &[],
            editing.now,
            view,
        ),
        Snap::Beats | Snap::Sections => {
            snap_lines(Snap::Beats, editing.clock, &[], &[], editing.now, view)
                .into_iter()
                .filter(|l| {
                    l.label.starts_with("playhead")
                        || (l.label.starts_with("bar ") && !l.label.contains('·'))
                })
                .collect()
        }
    }
}

/// The line nearest `t`, within `pull` seconds.
fn nearest(lines: &[Line], t: f32, pull: f32) -> Option<&Line> {
    lines
        .iter()
        .filter(|line| (line.at - t).abs() <= pull)
        .min_by(|a, b| (a.at - t).abs().total_cmp(&(b.at - t).abs()))
}

/// Where a held shot goes for the pointer `dt` seconds on, as the page
/// places it: a trim leaves the shot `SHORTEST` long and stops short of its
/// partner's far end; a move snaps whichever edge comes nearer a line; and
/// nothing goes before 0 or past `limit` (a song's end).
fn place(held: &Held, dt: f32, lines: &[Line], pull: f32, limit: f32) -> (f32, f32, Option<Line>) {
    let shot = &held.shot;
    let snapped = |line: Option<&Line>, at: f32| line.filter(|l| l.at == at).cloned();
    match held.edge {
        Edge::In => {
            let line = nearest(lines, shot.from + dt, pull);
            let lo = held.partner.as_ref().map_or(0.0, |p| p.from + SHORTEST);
            let from = clamp(
                line.map_or(shot.from + dt, |l| l.at),
                lo,
                shot.to - SHORTEST,
            );
            (from, shot.to, snapped(line, from))
        }
        Edge::Out => {
            let line = nearest(lines, shot.to + dt, pull);
            let hi = held.partner.as_ref().map_or(limit, |p| p.to - SHORTEST);
            let to = clamp(
                line.map_or(shot.to + dt, |l| l.at),
                shot.from + SHORTEST,
                hi,
            );
            (shot.from, to, snapped(line, to))
        }
        Edge::Body => {
            let length = shot.to - shot.from;
            let start = shot.from + dt;
            let a = nearest(lines, start, pull);
            let b = nearest(lines, start + length, pull);
            let (from, line) = match (a, b) {
                (Some(a), b)
                    if b.is_none_or(|b| (a.at - start).abs() <= (b.at - start - length).abs()) =>
                {
                    (a.at, Some(a))
                }
                (_, Some(b)) => (b.at - length, Some(b)),
                _ => (start, None),
            };
            let placed = clamp(from, 0.0, limit - length);
            let line = if placed == from { line.cloned() } else { None };
            (placed, placed + length, line)
        }
    }
}

fn seconds(span: f32) -> String {
    if span < 60.0 {
        format!("{span:.1} s")
    } else {
        timecode(span)
    }
}

/// A `data:` URL of an image, as egui draws it.
fn decode_data_url(url: &str) -> Option<egui::ColorImage> {
    let (_, data) = url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, from: f32, to: f32) -> Clip {
        Clip {
            id: id.into(),
            name: id.into(),
            size: "MS".into(),
            lens: 35.0,
            from,
            to,
        }
    }

    #[test]
    fn a_longer_hold_moves_the_stops_after_it_and_keeps_the_travel() {
        let key = |t: f32, hold: Option<f32>| CameraKey { t, hold };
        let keys = [key(0.0, Some(1.0)), key(3.0, None), key(6.0, Some(2.0))];
        let held = held_keys(&keys, 1, 1.5);
        assert_eq!(held[0], keys[0], "before the stop: as it was");
        assert_eq!(held[1], key(3.0, Some(1.5)));
        assert_eq!(held[2], key(7.5, Some(2.0)), "after it: later by the hold");
        assert_eq!(held_keys(&keys, 0, 0.0)[0].hold, None, "no hold is none");
    }

    #[test]
    fn a_recut_moves_a_start_or_joins_two_sections() {
        let starts = [0.0, 15.0, 45.0];
        let mut recut = Recutting {
            section: 1,
            to: 18.1,
            snapped: Some(line(18.1)),
            join: false,
            moved: true,
        };
        assert_eq!(recut.apply(&starts), [0.0, 18.1, 45.0]);
        assert_eq!(recut.describe(), "at 18.1 · 0:18.10");
        recut.join = true;
        assert_eq!(recut.apply(&starts), [0.0, 45.0]);
        assert_eq!(recut.describe(), "§1 and §2 join");
        let clock = ClockView {
            duration: 8.0,
            bpm: 120.0,
            beat_offset: 0.0,
            sections: vec![0.0, 0.5],
        };
        let editing = Editing {
            can: true,
            clock: &clock,
            now: 1.0,
            view: [0.0, 8.0],
            has_song: true,
            snap: Snap::Beats,
            shots: &[],
        };
        let s = Scale {
            from: 0.0,
            to: 4.0,
            x0: 0.0,
            width: 400.0,
        };
        // A section starts on a bar, not on a beat inside one.
        let at: Vec<f32> = recut_lines(&editing, s).iter().map(|l| l.at).collect();
        assert_eq!(at, [1.0, 0.0, 2.0, 4.0]);
    }

    #[test]
    fn coverage_stacks_under_its_master() {
        let shots = [
            clip("a", 0.0, 10.0),
            clip("b", 2.0, 4.0),
            clip("c", 10.0, 12.0),
        ];
        let (placed, rows, count) = pack(&shots);
        let names: Vec<&str> = placed.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(rows, [0, 1, 0]);
        assert_eq!(count, 2);
    }

    #[test]
    fn the_rulers_ticks_are_far_enough_apart_to_read() {
        let s = Scale {
            from: 0.0,
            to: 120.0,
            x0: 0.0,
            width: 1200.0,
        };
        let (major, minor) = ticks(s, 64.0);
        // 10 px a second: a label every 10 s, a tick every 2.
        assert_eq!(&major[..3], &[0.0, 10.0, 20.0]);
        assert_eq!(&minor[..2], &[2.0, 4.0]);
    }

    #[test]
    fn bars_and_beats_count_from_one() {
        let clock = ClockView {
            duration: 60.0,
            bpm: 120.0,
            beat_offset: 0.0,
            sections: vec![0.0],
        };
        assert_eq!(bar_beat(&clock, 0.0), Some((1, 1)));
        assert_eq!(bar_beat(&clock, 2.5), Some((2, 2)));
        assert_eq!(timecode(75.5), "1:15.50");
    }

    fn held(edge: Edge, shot: Clip, partner: Option<Clip>) -> Held {
        Held {
            edge,
            from: shot.from,
            to: shot.to,
            shot,
            partner,
            snapped: None,
            moved: true,
        }
    }

    fn line(at: f32) -> Line {
        Line {
            at,
            label: format!("at {at}"),
        }
    }

    #[test]
    fn a_trimmed_edge_snaps_within_the_pull_and_rolls_its_partner() {
        // 1A 0–4 meets 1B 4–9; its out edge dragged 0.9 s on, a line at 5.
        let a = clip("1", 0.0, 4.0);
        let b = clip("2", 4.0, 9.0);
        let shots = [a.clone(), b.clone()];
        let mut h = held(Edge::Out, a.clone(), partner_of(&shots, &a, Edge::Out));
        assert_eq!(h.partner.as_ref().map(|p| p.id.as_str()), Some("2"));
        let (from, to, snapped) = place(&h, 0.9, &[line(5.0)], 0.2, 60.0);
        assert_eq!((from, to), (0.0, 5.0), "pulled onto the line");
        assert_eq!(snapped, Some(line(5.0)));
        (h.from, h.to) = (from, to);
        let edits = h.edits();
        assert_eq!(edits.len(), 2, "the cut rolls: one edit, both shots");
        assert_eq!((edits[1].id, edits[1].in_s, edits[1].out_s), (2, 5.0, 9.0));
        assert_eq!(h.times_of("2"), Some((5.0, 9.0)));
        // Past the partner's end, the cut stops short of it.
        let (_, to, _) = place(&h, 20.0, &[], 0.2, 60.0);
        assert!((to - (9.0 - SHORTEST)).abs() < 1e-5);
    }

    #[test]
    fn a_move_snaps_its_nearer_edge_and_stays_on_the_clock() {
        let shot = clip("1", 2.0, 5.0);
        let h = held(Edge::Body, shot, None);
        // Moved 1.1 s on: its in at 3.1 is 0.1 from a line at 3; its out at
        // 6.1 is 0.05 from a line at 6.05, nearer, so the out snaps.
        let (from, to, snapped) = place(&h, 1.1, &[line(3.0), line(6.05)], 0.2, 60.0);
        assert!((from - 3.05).abs() < 1e-5 && (to - 6.05).abs() < 1e-5);
        assert_eq!(snapped, Some(line(6.05)));
        // Not before the start, nor past a song's end.
        assert_eq!(place(&h, -10.0, &[], 0.2, 60.0).0, 0.0);
        assert_eq!(place(&h, 100.0, &[], 0.2, 8.0).1, 8.0);
        // A trim leaves the shot its shortest.
        let h = held(Edge::In, clip("1", 2.0, 5.0), None);
        assert!((place(&h, 10.0, &[], 0.2, 60.0).0 - (5.0 - SHORTEST)).abs() < 1e-5);
    }

    #[test]
    fn the_snap_lines_are_the_menus_choice_and_the_playhead() {
        let clock = ClockView {
            duration: 8.0,
            bpm: 120.0,
            beat_offset: 0.0,
            sections: vec![0.0, 0.5],
        };
        let shots = [clip("1", 0.0, 3.0), clip("2", 3.0, 8.0)];
        let beats = snap_lines(Snap::Beats, &clock, &shots, &[], 1.25, [0.0, 2.0]);
        let labels: Vec<&str> = beats.iter().map(|l| l.label.as_str()).collect();
        assert!(labels[0].starts_with("playhead"));
        // Half a second a beat, 0 to 2 in view: five beats, bar 2 at 2 s.
        assert_eq!(
            &labels[1..6],
            ["bar 1", "bar 1 · 2", "bar 1 · 3", "bar 1 · 4", "bar 2"]
        );
        assert!(labels.contains(&"§2 · 0:04.00") && labels.contains(&"end · 0:08.00"));
        let cuts = snap_lines(Snap::Shots, &clock, &shots, &["2"], 0.0, [0.0, 8.0]);
        let at: Vec<f32> = cuts.iter().map(|l| l.at).collect();
        assert_eq!(
            at,
            [0.0, 0.0, 3.0],
            "the playhead, then the other shot's in and out"
        );
        assert_eq!(
            snap_lines(Snap::Off, &clock, &shots, &[], 2.0, [0.0, 8.0]).len(),
            1
        );
        assert_eq!(signed(-1.25), "−1.25 s");
    }
}
