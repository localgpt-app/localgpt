//! The document: its outline, the selected section's place, and the text,
//! as the desktop page drew them until it went (tag `desktop-the-page`,
//! `crates/app/ui/src/DocumentPanel.tsx`). Each edit goes to
//! the app, which rebuilds the sections that changed in place; the caret's
//! line follows the world when Follow is on; an outline row or a thing on
//! the place card flies the camera there.
//!
//! The editor is egui's `TextEdit`, enough for a scene's pages
//! (`docs/rfcs/authoring/all-rust.md`, "The document"), with ⌘F's find as
//! CodeMirror has it: every match marked, Enter and Shift+Enter to the next
//! and the one before, the match shown selected in the text.

use std::ops::Range;

use eframe::egui::{self, RichText};

use super::state::State;
use super::theme;
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

pub struct Document {
    /// The text as edited here, and the app's revision it began from.
    text: String,
    rev: Option<u64>,
    caret_line: Option<usize>,
    /// The find bar, while it is open.
    find: Option<Find>,
}

/// What ⌘F looks for, and which match is shown.
#[derive(Default)]
struct Find {
    query: String,
    /// The match shown, by its order in the text.
    current: Option<usize>,
    /// The query takes the keys when the bar opens.
    focus: bool,
    /// The current match is to be selected and brought into view.
    reveal: bool,
}

impl Document {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            rev: None,
            caret_line: None,
            find: None,
        }
    }

    /// ⌘F's bar over the text, opened from the editor: what is looked for,
    /// how many there are and which is shown, and ↑ ↓ ×. Enter goes to the
    /// next match, Shift+Enter to the one before, and Esc closes the bar and
    /// gives the keys back to the text. The matches, as byte ranges.
    fn find_bar(&mut self, ui: &mut egui::Ui, editor: egui::Id) -> Vec<Range<usize>> {
        let field = ui.make_persistent_id("the document's find");
        let asked = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F))
            && ui.memory(|m| m.has_focus(editor) || m.has_focus(field));
        if asked {
            self.find.get_or_insert_with(Find::default).focus = true;
        }
        let Some(find) = self.find.as_mut() else {
            return Vec::new();
        };
        let mut hits = find_all(&self.text, &find.query);
        let mut close = false;
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut find.query)
                    .id(field)
                    .hint_text("Find")
                    .desired_width(180.0),
            );
            if std::mem::take(&mut find.focus) {
                response.request_focus();
            }
            if response.changed() {
                hits = find_all(&self.text, &find.query);
                // The first at or after the caret, as a search as you type
                // finds it.
                let caret = egui::text_edit::TextEditState::load(ui.ctx(), editor)
                    .and_then(|s| s.cursor.char_range())
                    .map_or(0, |r| r.primary.index.0);
                let caret = self
                    .text
                    .char_indices()
                    .nth(caret)
                    .map_or(self.text.len(), |(b, _)| b);
                find.current = (!hits.is_empty())
                    .then(|| hits.iter().position(|h| h.start >= caret).unwrap_or(0));
                find.reveal = find.current.is_some();
            }
            let (enter, shift, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::Escape),
                )
            });
            // A one-line field lets go of the keys on Enter: it keeps them.
            if response.lost_focus() && enter {
                step(find, hits.len(), !shift);
                response.request_focus();
            }
            let count = match (find.current, hits.len()) {
                (_, 0) if !find.query.is_empty() => "no matches".to_string(),
                (_, 0) => String::new(),
                (Some(i), n) => format!("{} of {n}", i + 1),
                (None, n) => format!("{n} found"),
            };
            ui.label(RichText::new(count).color(theme::MUTED));
            if ui
                .small_button("↑")
                .on_hover_text("The one before (Shift+Enter)")
                .clicked()
            {
                step(find, hits.len(), false);
            }
            if ui
                .small_button("↓")
                .on_hover_text("The next (Enter)")
                .clicked()
            {
                step(find, hits.len(), true);
            }
            let escaped = escape && (response.has_focus() || response.lost_focus());
            if ui.small_button("×").on_hover_text("Close (Esc)").clicked() || escaped {
                close = true;
            }
        });
        if close {
            self.find = None;
            ui.memory_mut(|m| m.request_focus(editor));
            return Vec::new();
        }
        hits
    }

    /// `reveal` is a line the app asked to bring into view: a section picked
    /// in the outline or in the world.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, reveal: Option<usize>, end: &ShellEnd) {
        let Some(doc) = state.document.as_deref() else {
            ui.weak("No document open.");
            return;
        };
        // The app's text replaces this one when it is newer: opened, or
        // reloaded from disk.
        if let Some((text, rev)) = &state.text
            && self.rev != Some(*rev)
        {
            self.text = text.clone();
            self.rev = Some(*rev);
        }
        ui.horizontal(|ui| {
            ui.strong(&doc.name);
            ui.weak(&doc.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut follow = doc.follow_cursor;
                if ui
                    .checkbox(&mut follow, "Follow")
                    .on_hover_text("The world follows the caret's section")
                    .changed()
                {
                    end.ask(FromPage::DocFollow { on: follow });
                }
            });
        });
        if doc.conflict {
            ui.colored_label(
                theme::DANGER,
                "The file changed on disk while it was edited here.",
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("outline")
            .max_height(160.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for row in &doc.outline {
                    let selected = row.section.is_some() && row.section == doc.selected;
                    let text = format!(
                        "{}{}",
                        "  ".repeat(row.level.saturating_sub(1) as usize),
                        row.text
                    );
                    let response = ui.selectable_label(selected, text);
                    if response.clicked()
                        && let Some(section) = row.section
                    {
                        end.ask(FromPage::DocGo {
                            section,
                            line: row.line,
                        });
                    }
                    if let Some(tier) = row.tier {
                        response.on_hover_text(format!("its place: {tier}"));
                    }
                }
            });
        if let Some(card) = &doc.card {
            ui.separator();
            ui.label(
                RichText::new(card.heading.as_deref().unwrap_or("The opening"))
                    .strong()
                    .color(theme::INK),
            );
            if let Some(description) = &card.description {
                ui.weak(description);
            }
            ui.horizontal_wrapped(|ui| {
                ui.weak(format!("{} things:", card.entities));
                for thing in &card.things {
                    let label = if thing.count > 1 {
                        format!("{} ×{}", thing.label, thing.count)
                    } else {
                        thing.label.clone()
                    };
                    if ui.small_button(label).on_hover_text("Fly to it").clicked() {
                        end.ask(FromPage::DocPick {
                            name: thing.first.clone(),
                        });
                    }
                }
                if card.more > 0 {
                    ui.weak(format!("and {} more", card.more));
                }
            });
        }
        ui.separator();
        let editor = ui.make_persistent_id("the document's text");
        let hits = self.find_bar(ui, editor);
        let current = self.find.as_ref().and_then(|f| f.current);
        egui::ScrollArea::vertical()
            .id_salt("text")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // With a find open, its matches marked, the one shown more.
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    let mut job = marked(buf.as_str(), &hits, current, &font);
                    job.wrap.max_width = wrap;
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let mut edit = egui::TextEdit::multiline(&mut self.text)
                    .id(editor)
                    .hint_text("Markdown")
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(24);
                if !hits.is_empty() {
                    edit = edit.layouter(&mut layouter);
                }
                let output = edit.show(ui);
                if output.response.changed() {
                    end.ask(FromPage::DocEdit {
                        text: self.text.clone(),
                    });
                }
                // The match shown: selected in the text, and in view.
                if let Some(find) = self.find.as_mut()
                    && std::mem::take(&mut find.reveal)
                    && let Some(range) = find.current.and_then(|i| hits.get(i))
                {
                    let from = self.text[..range.start].chars().count();
                    let to = from + self.text[range.clone()].chars().count();
                    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), editor)
                    {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(from),
                                egui::text::CCursor::new(to),
                            )));
                        state.store(ui.ctx(), editor);
                    }
                    let row = output
                        .galley
                        .pos_from_cursor(egui::text::CCursor::new(from))
                        .translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(row, Some(egui::Align::Center));
                }
                // The line at the top with a little room above it, as the
                // page does, and the keys left where they were: they may be
                // the world's.
                if let Some(line) = reveal {
                    let at = egui::text::CCursor::new(line_start(&self.text, line));
                    let row = output
                        .galley
                        .pos_from_cursor(at)
                        .translate(output.galley_pos.to_vec2());
                    let room = egui::Rect::from_min_max(row.min - egui::vec2(0.0, 12.0), row.max);
                    ui.scroll_to_rect(room, Some(egui::Align::TOP));
                }
                if let Some(range) = output.cursor_range {
                    let at = range.primary.index.0;
                    let line = self.text.chars().take(at).filter(|c| *c == '\n').count();
                    if self.caret_line != Some(line) {
                        self.caret_line = Some(line);
                        end.ask(FromPage::DocCaret { line });
                    }
                }
            });
    }
}

/// The next match, or the one before, round the end; the first or the last
/// when none was shown.
fn step(find: &mut Find, n: usize, forward: bool) {
    if n == 0 {
        return;
    }
    find.current = Some(match find.current {
        None if forward => 0,
        None => n - 1,
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
    });
    find.reveal = true;
}

/// Where `query` is in `text`, as byte ranges, without regard to case and
/// one after another, as CodeMirror's search finds them.
fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    let wanted: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (mut j, mut k) = (i, 0);
        while k < wanted.len() {
            let Some(&(_, c)) = chars.get(j) else {
                break;
            };
            let lower: Vec<char> = c.to_lowercase().collect();
            if wanted.get(k..k + lower.len()) != Some(&lower[..]) {
                break;
            }
            k += lower.len();
            j += 1;
        }
        if k == wanted.len() {
            let end = chars.get(j).map_or(text.len(), |(b, _)| *b);
            hits.push(chars[i].0..end);
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    hits
}

/// The text as the editor lays it out, the matches marked and the one
/// shown more. A match from before an edit this frame that no longer fits
/// the text is left unmarked.
fn marked(
    text: &str,
    hits: &[Range<usize>],
    current: Option<usize>,
    font: &egui::FontId,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color: theme::INK,
        ..Default::default()
    };
    let mut at = 0;
    for (n, hit) in hits.iter().enumerate() {
        let fits = hit.start >= at
            && hit.end <= text.len()
            && text.is_char_boundary(hit.start)
            && text.is_char_boundary(hit.end);
        if !fits {
            continue;
        }
        job.append(&text[at..hit.start], 0.0, plain.clone());
        let strength = if current == Some(n) { 0.6 } else { 0.25 };
        job.append(
            &text[hit.clone()],
            0.0,
            egui::TextFormat {
                background: theme::alpha(theme::HEAD, strength),
                ..plain.clone()
            },
        );
        at = hit.end;
    }
    job.append(&text[at..], 0.0, plain);
    job
}

/// Where line `line` (from 0) of `text` begins, in characters; past the
/// last line, the end.
fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut lines = 0;
    for (at, c) in text.chars().enumerate() {
        if c == '\n' {
            lines += 1;
            if lines == line {
                return at + 1;
            }
        }
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_without_case_one_after_another() {
        let text = "## The Library\nthe lamp, THE DESK; Thé";
        let hits = find_all(text, "the");
        let words: Vec<&str> = hits.iter().map(|h| &text[h.clone()]).collect();
        assert_eq!(words, ["The", "the", "THE"], "'Thé' is not 'the'");
        assert_eq!(find_all("aaaa", "aa"), [0..2, 2..4], "one after another");
        assert_eq!(find_all("café Café", "CAFÉ").len(), 2);
        assert!(find_all(text, "").is_empty());
        // Every byte of the text is laid out once, the matches with a mark.
        let job = marked(text, &hits, Some(1), &egui::FontId::monospace(12.0));
        assert_eq!(job.text, text);
        let marks = job
            .sections
            .iter()
            .filter(|s| s.format.background != egui::Color32::TRANSPARENT)
            .count();
        assert_eq!(marks, 3);
        let mut find = Find::default();
        step(&mut find, 3, false);
        assert_eq!(find.current, Some(2), "back from none is the last");
        step(&mut find, 3, true);
        assert_eq!(find.current, Some(0), "round the end");
    }

    #[test]
    fn a_line_begins_after_the_breaks_before_it_in_characters() {
        let text = "# Tïtle\n\n## Næxt\nbody";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_start(text, 1), 8, "after the first break");
        assert_eq!(line_start(text, 2), 9, "an empty line");
        assert_eq!(line_start(text, 3), 17);
        assert_eq!(line_start(text, 9), text.chars().count(), "past the end");
    }

    #[test]
    fn document_panel_navigates_outline_picks_things_and_toggles_follow() {
        use std::sync::mpsc::channel;

        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        use crate::document::{DocumentView, OutlineRow, PlaceCard, Thing};

        struct Panel {
            doc: Document,
            state: State,
            end: ShellEnd,
        }

        let (asks, asked) = channel();
        let (_tell, events) = channel();
        let view = DocumentView {
            name: "harbour.md".into(),
            path: "/tmp/harbour.md".into(),
            status: "Saved · 2 sections".into(),
            conflict: true,
            follow_cursor: true,
            outline: vec![
                OutlineRow {
                    level: 1,
                    text: "Harbour".into(),
                    section: None,
                    line: None,
                    tier: None,
                },
                OutlineRow {
                    level: 2,
                    text: "The Pier".into(),
                    section: Some(0),
                    line: Some(2),
                    tier: Some("draft"),
                },
            ],
            selected: Some(0),
            card: Some(PlaceCard {
                section: 0,
                heading: Some("The Pier".into()),
                tier: Some("draft"),
                description: Some("A wooden jetty at dusk".into()),
                entities: 3,
                things: vec![Thing {
                    label: "lantern".into(),
                    first: "s00-lantern_1".into(),
                    count: 2,
                }],
                more: 0,
            }),
        };
        let panel = Panel {
            doc: Document::new(),
            state: State {
                document: Some(Box::new(view)),
                text: Some(("# Harbour\n\n## The Pier\nlanterns\n".into(), 1)),
                ..State::default()
            },
            end: ShellEnd { events, asks },
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(420.0, 640.0))
            .allow_missing_glyphs()
            .build_ui_state(
                |ui, panel: &mut Panel| {
                    panel.doc.ui(ui, &panel.state, None, &panel.end);
                },
                panel,
            );
        harness.run_steps(2);

        let _ = harness.get_by_label("The file changed on disk while it was edited here.");
        harness.get_by_label("  The Pier").click();
        harness.run_steps(2);
        harness.get_by_label("lantern ×2").click();
        harness.run_steps(2);
        harness.get_by_label("Follow").click();
        harness.run_steps(2);

        let sent: Vec<FromPage> = asked
            .try_iter()
            .filter(|m| !matches!(m, FromPage::DocCaret { .. }))
            .collect();
        assert_eq!(
            sent,
            [
                FromPage::DocGo {
                    section: 0,
                    line: Some(2)
                },
                FromPage::DocPick {
                    name: "s00-lantern_1".into()
                },
                FromPage::DocFollow { on: false },
            ]
        );
    }
}
