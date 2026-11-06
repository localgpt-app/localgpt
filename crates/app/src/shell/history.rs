//! The history: a live canvas's entries as `git log --graph` draws them
//! (`crates/app/ui/src/HistoryPanel.tsx` and `PlaybackBar.tsx` in the
//! page): newest at the top, a lane per branch, the base at the bottom. A
//! click shows the world as it was at that entry; the bar under it steps
//! through the entries in the order they were written, plays them, or
//! scrubs. Every move is a seek: nothing is written.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};
use localgpt_gen::gen3d::live::HistoryRow;

use super::state::State;
use super::theme;
use crate::webui::link::ShellEnd;
use crate::webui::{FromPage, Step};

const ROW: f32 = 30.0;
const LANE: f32 = 14.0;
const PAD: f32 = 12.0;
const NODE: f32 = 4.0;
/// The playback bar under the graph.
const BAR: f32 = 40.0;
/// A scrub seeks as it goes, at most this often.
const SCRUB: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct History {
    scrubbed: Option<Instant>,
}

impl History {
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
        let Some(history) = &state.history else {
            return;
        };
        let rows = &history.rows;
        // The status is the bar's, as the page has it.
        ui.weak(count(rows.len()));
        egui::ScrollArea::vertical()
            .id_salt("history")
            .max_height((ui.available_height() - BAR).max(ROW))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(
                        RichText::new(
                            "Nothing written yet. Each edit an agent makes lands here, and a \
                             click shows the world as it was.",
                        )
                        .color(theme::MUTED),
                    );
                } else {
                    graph(ui, rows, history.current.as_deref(), end);
                }
            });
        ui.separator();
        self.bar(ui, state, rows, history.current.as_deref(), end);
    }

    /// Base, back, play, on, head, and a scrubber over the entries in the
    /// order they were written.
    fn bar(
        &mut self,
        ui: &mut egui::Ui,
        state: &State,
        rows: &[HistoryRow],
        current: Option<&str>,
        end: &ShellEnd,
    ) {
        let n = rows.len();
        // Position 0 is the base; position i is rows[i - 1].
        let at = current
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .map_or(0, |i| i + 1);
        ui.horizontal(|ui| {
            let step = |ui: &mut egui::Ui, glyph: &str, label: &str, to: Step| {
                if ui.button(glyph).on_hover_text(label).clicked() {
                    end.ask(FromPage::Step { to });
                }
            };
            step(ui, "⏮", "Base", Step::First);
            step(ui, "⏴", "Previous entry", Step::Prev);
            let (play, label) = if state.playing {
                ("⏸", "Pause the history")
            } else {
                ("▶", "Play the history")
            };
            if ui.button(play).on_hover_text(label).clicked() {
                end.ask(FromPage::Play {
                    playing: !state.playing,
                });
            }
            step(ui, "⏵", "Next entry", Step::Next);
            step(ui, "⏭", "Head", Step::Last);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{at} / {n}")).color(theme::MUTED));
                let mut position = at;
                ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(40.0);
                let slider = ui.add_enabled(
                    n > 0,
                    egui::Slider::new(&mut position, 0..=n.max(1)).show_value(false),
                );
                let due = self.scrubbed.is_none_or(|at| at.elapsed() >= SCRUB);
                if slider.changed() && position != at && due {
                    self.scrubbed = Some(Instant::now());
                    end.ask(FromPage::Seek {
                        id: position
                            .checked_sub(1)
                            .and_then(|i| rows.get(i))
                            .map(|row| row.id.clone()),
                    });
                }
            });
        });
    }
}

/// A lane per branch, as Gen's history panel draws them: an entry continues
/// the lane whose tip is its parent; one whose parent has moved on (a fork)
/// opens a new lane. Lane 0 starts at the base.
fn lanes(rows: &[HistoryRow]) -> (Vec<usize>, usize) {
    let mut tips: Vec<Option<&str>> = vec![None];
    let mut lane = Vec::with_capacity(rows.len());
    for row in rows {
        let at = match tips.iter().position(|tip| *tip == row.parent.as_deref()) {
            Some(at) => at,
            None => {
                tips.push(None);
                tips.len() - 1
            }
        };
        tips[at] = Some(row.id.as_str());
        lane.push(at);
    }
    (lane, tips.len())
}

/// The rows and the graph beside them: an edge from each entry down to the
/// one it builds on, a node for each, main's tip ringed, the base at the
/// bottom.
fn graph(ui: &mut egui::Ui, rows: &[HistoryRow], current: Option<&str>, end: &ShellEnd) {
    let n = rows.len();
    let (lane, count) = lanes(rows);
    let index: HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| (row.id.as_str(), i))
        .collect();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, (n + 1) as f32 * ROW), Sense::hover());
    let x = |l: usize| rect.left() + PAD + l as f32 * LANE;
    // Row i (write order) sits at position n-1-i from the top; the base at n.
    let y = |position: usize| rect.top() + position as f32 * ROW + ROW / 2.0;
    let left = PAD * 2.0 + (count - 1) as f32 * LANE;
    let color = |l: usize| theme::LANES[l % theme::LANES.len()];
    let painter = ui.painter_at(rect);

    // The rows first, so a highlight never covers a node.
    let row_at = |position: usize| {
        Rect::from_min_size(
            pos2(rect.left(), rect.top() + position as f32 * ROW),
            vec2(width, ROW),
        )
    };
    for (i, row) in rows.iter().enumerate() {
        let area = row_at(n - 1 - i);
        let response = ui
            .interact(area, ui.scope_id().with(("entry", &row.id)), Sense::click())
            .on_hover_text(format!(
                "{}\n{} · {}",
                row.message.as_deref().unwrap_or(&row.summary),
                short(&row.id),
                row.id
            ));
        let here = current == Some(row.id.as_str());
        if here || response.hovered() {
            painter.rect_filled(area, 0.0, theme::RAISED);
        }
        if response.clicked() {
            end.ask(FromPage::Seek {
                id: Some(row.id.clone()),
            });
        }
        text_row(&painter, area, left, row, here);
    }
    let base = row_at(n);
    let response = ui.interact(base, ui.scope_id().with("base"), Sense::click());
    if current.is_none() || response.hovered() {
        painter.rect_filled(base, 0.0, theme::RAISED);
    }
    if response.clicked() {
        end.ask(FromPage::Seek { id: None });
    }
    painter.text(
        pos2(base.left() + left, base.center().y),
        Align2::LEFT_CENTER,
        "base",
        FontId::proportional(12.0),
        theme::MUTED,
    );

    // The edges, then the nodes over them.
    for (i, row) in rows.iter().enumerate() {
        let from = pos2(x(lane[i]), y(n - 1 - i));
        let to = match row.parent.as_deref().and_then(|p| index.get(p)) {
            Some(&j) => pos2(x(lane[j]), y(n - 1 - j)),
            None => pos2(x(0), y(n)),
        };
        let stroke = Stroke::new(1.75, color(lane[i]));
        if from.x == to.x {
            painter.line_segment([from, to], stroke);
        } else {
            // Down its own lane, then a curve into the parent's.
            let bend = pos2(from.x, to.y - ROW * 0.6);
            painter.line_segment([from, bend], stroke);
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [
                    bend,
                    pos2(from.x, to.y - ROW * 0.2),
                    pos2(to.x, to.y - ROW * 0.35),
                    to,
                ],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
    }
    for (i, row) in rows.iter().enumerate() {
        let at = pos2(x(lane[i]), y(n - 1 - i));
        let c = color(lane[i]);
        if row.head {
            painter.circle_stroke(at, NODE + 3.0, Stroke::new(1.5, theme::HEAD));
        }
        let fill = if current == Some(row.id.as_str()) {
            c
        } else {
            theme::PANEL
        };
        painter.circle(at, NODE, fill, Stroke::new(1.75, c));
    }
    let fill = if current.is_none() {
        color(0)
    } else {
        theme::PANEL
    };
    painter.circle(pos2(x(0), y(n)), NODE, fill, Stroke::new(1.75, color(0)));
}

/// An entry's line: its summary, "main" on main's tip, its author and how
/// long ago.
fn text_row(painter: &egui::Painter, area: Rect, left: f32, row: &HistoryRow, here: bool) {
    let mid = area.center().y;
    let right = area.right() - 12.0;
    painter.text(
        pos2(right, mid),
        Align2::RIGHT_CENTER,
        ago(row.timestamp_ms),
        FontId::proportional(11.0),
        theme::FAINT,
    );
    let author_right = right - 34.0;
    let author = painter.layout_no_wrap(
        truncate(&row.author, 9),
        FontId::proportional(12.0),
        theme::MUTED,
    );
    painter.galley(
        pos2(author_right - author.size().x, mid - author.size().y / 2.0),
        author,
        theme::MUTED,
    );
    let mut end = author_right - 64.0;
    if row.head {
        let badge =
            painter.layout_no_wrap("main".to_string(), FontId::proportional(10.0), theme::HEAD);
        let size = badge.size() + vec2(8.0, 2.0);
        let at = Rect::from_min_size(pos2(end - size.x, mid - size.y / 2.0), size);
        painter.rect_filled(at, 3.0, theme::alpha(theme::HEAD, 0.15));
        painter.galley(at.min + vec2(4.0, 1.0), badge, theme::HEAD);
        end = at.left() - 6.0;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(
        row.summary.clone(),
        FontId::proportional(12.0),
        if here {
            theme::INK
        } else {
            theme::alpha(theme::INK, 0.85)
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((end - area.left() - left).max(24.0));
    let summary = painter.layout_job(job);
    painter.galley(
        pos2(area.left() + left, mid - summary.size().y / 2.0),
        summary,
        theme::INK,
    );
}

fn count(n: usize) -> String {
    match n {
        1 => "1 entry".to_string(),
        n => format!("{n} entries"),
    }
}

/// An entry's id as a person reads it: a content hash's first seven.
fn short(id: &str) -> &str {
    match id.strip_prefix("sha256:") {
        Some(hash) => hash.get(..7).unwrap_or(hash),
        None => id,
    }
}

fn truncate(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        text.to_string()
    } else {
        let cut: String = text.chars().take(chars - 1).collect();
        format!("{cut}…")
    }
}

/// "now", "5m", "3h", "2d", as the page's history says it.
fn ago(ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let s = now.saturating_sub(ms) / 1000;
    match s {
        0..60 => "now".to_string(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, parent: Option<&str>) -> HistoryRow {
        HistoryRow {
            id: id.into(),
            parent: parent.map(Into::into),
            revision: 0,
            author: "agent".into(),
            summary: id.into(),
            message: None,
            timestamp_ms: 0,
            tip: false,
            head: false,
        }
    }

    #[test]
    fn a_fork_opens_a_lane_and_its_line_keeps_it() {
        // a ← b ← c on main; d forks from a; e continues d; f from the base.
        let rows = [
            row("a", None),
            row("b", Some("a")),
            row("c", Some("b")),
            row("d", Some("a")),
            row("e", Some("d")),
            row("f", None),
        ];
        let (lane, count) = lanes(&rows);
        assert_eq!(lane, vec![0, 0, 0, 1, 1, 2]);
        assert_eq!(count, 3);
    }

    #[test]
    fn an_id_reads_short_and_a_name_is_cut_with_an_ellipsis() {
        assert_eq!(short("sha256:0123456789abcdef"), "0123456");
        assert_eq!(short("entry-7"), "entry-7");
        assert_eq!(truncate("an agent", 9), "an agent");
        assert_eq!(truncate("a long author name", 9), "a long a…");
    }
}
