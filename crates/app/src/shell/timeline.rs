//! The timeline: the world's clock and its tracks across the window
//! (`crates/app/ui/src/timeline/` in the page). The shots with their frames,
//! the tour's stops, the song's waveform and stems, what the song moves,
//! the behaviors and the ambience, on one clock, over the bars and sections.
//!
//! What it shows comes from the app (`TimelineView`, built from the world's
//! document); what it keeps is its own: the part of the clock in view, the
//! headers' toggles, the selected shot. It asks with the page's messages:
//! a seek, play, a loop, a look through a shot, the muted and hidden tracks,
//! a review's answer.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use base64::Engine;
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, pos2, vec2,
};

use super::state::State;
use super::theme::{self, alpha};
use crate::timeline::{CameraKey, Clip, ClockView, Signal, TimelineView, TrackView};
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

const HEADER: f32 = 176.0;
/// The sections band, the bars' numbers and the seconds, one under another.
const RULER: f32 = 44.0;
/// A row of overlapping shots — coverage under its master.
const SHOT_ROW: f32 = 40.0;

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
}

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
        self.ruler(ui, ruler, s, clock, now, state.clock.looping, end);

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
                    self.lane(ui, lane, s, clock, &timeline.signals, track, end);
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
                .on_hover_text("Loop what is in view")
                .clicked()
            {
                end.ask(FromPage::ClockLoop {
                    span: (!looping).then_some(*view),
                });
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
        clock: &ClockView,
        now: f32,
        looping: Option<[f32; 2]>,
        end: &ShellEnd,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::RAISED);
        let lanes = Rect::from_min_max(pos2(s.x0, rect.top()), rect.max);
        // The sections band: a block per section, numbered.
        let band = Rect::from_min_max(lanes.min, pos2(lanes.right(), lanes.top() + 14.0));
        let starts: Vec<f32> = clock.sections.iter().map(|f| f * clock.duration).collect();
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
        clock: &ClockView,
        signals: &BTreeMap<String, Signal>,
        track: &TrackView,
        end: &ShellEnd,
    ) {
        // A click where nothing takes it seeks there. Registered first, so
        // what the lane draws that takes a click (a shot) is above it.
        let response = ui.interact(
            lane,
            ui.scope_id().with(("lane", describe(track).0)),
            Sense::click(),
        );
        if response.clicked()
            && !matches!(track, TrackView::Shots { .. })
            && let Some(p) = response.interact_pointer_pos()
        {
            end.ask(FromPage::ClockSeek {
                at: s.t(p.x).clamp(0.0, clock.duration),
            });
        }
        let painter = ui.painter_at(lane);
        let h = lane.height();
        match track {
            TrackView::Shots { shots, .. } => self.shots(ui, &painter, lane, s, shots, end),
            TrackView::Camera { keys, .. } => camera(&painter, lane, s, keys),
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

    fn shots(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        lane: Rect,
        s: Scale,
        shots: &[Clip],
        end: &ShellEnd,
    ) {
        let (placed, rows, count) = pack(shots);
        let row_h = if count == 1 {
            lane.height() - 8.0
        } else {
            SHOT_ROW
        };
        for (shot, row) in placed.iter().zip(rows) {
            let top = lane.top() + 4.0 + row as f32 * (SHOT_ROW + 4.0);
            let rect = Rect::from_min_max(
                pos2(s.x(shot.from) + 1.0, top),
                pos2(s.x(shot.to) - 1.0, top + row_h),
            );
            if rect.right() < lane.left() || rect.left() > lane.right() {
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
            let response = ui
                .interact(
                    rect.intersect(lane),
                    ui.scope_id().with(("shot", &shot.id)),
                    Sense::click(),
                )
                .on_hover_text("A click selects it; a double click looks through it");
            if response.clicked() {
                self.selected = Some(shot.id.clone());
            }
            if response.double_clicked() {
                end.ask(FromPage::ShotLook {
                    id: shot.id.parse().ok(),
                });
            }
        }
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
}
