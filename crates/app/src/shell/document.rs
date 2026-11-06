//! The document: its outline, the selected section's place, and the text
//! (`crates/app/ui/src/DocumentPanel.tsx` in the page). Each edit goes to
//! the app, which rebuilds the sections that changed in place; the caret's
//! line follows the world when Follow is on; an outline row or a thing on
//! the place card flies the camera there.
//!
//! The editor is egui's `TextEdit`, enough for a scene's pages
//! (`docs/rfcs/authoring/all-rust.md`, "The document"); CodeMirror's search
//! and commands are the bar it has yet to reach.

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
}

impl Document {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            rev: None,
            caret_line: None,
        }
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
        egui::ScrollArea::vertical()
            .id_salt("text")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let output = egui::TextEdit::multiline(&mut self.text)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .show(ui);
                if output.response.changed() {
                    end.ask(FromPage::DocEdit {
                        text: self.text.clone(),
                    });
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
    fn a_line_begins_after_the_breaks_before_it_in_characters() {
        let text = "# Tïtle\n\n## Næxt\nbody";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_start(text, 1), 8, "after the first break");
        assert_eq!(line_start(text, 2), 9, "an empty line");
        assert_eq!(line_start(text, 3), 17);
        assert_eq!(line_start(text, 9), text.chars().count(), "past the end");
    }
}
