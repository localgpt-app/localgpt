//! The history: a live canvas's entries, newest first, with its transport
//! (`crates/app/ui/src/HistoryPanel.tsx` in the page). A click on an entry
//! shows the world as it was there; the transport steps and plays.

use eframe::egui;

use super::state::State;
use crate::webui::link::ShellEnd;
use crate::webui::{FromPage, Step};

pub fn ui(ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
    let Some(history) = &state.history else {
        return;
    };
    ui.horizontal(|ui| {
        ui.heading("History");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("⏭").on_hover_text("The newest").clicked() {
                end.ask(FromPage::Step { to: Step::Last });
            }
            if ui.button("⏵").on_hover_text("The next entry").clicked() {
                end.ask(FromPage::Step { to: Step::Next });
            }
            let play = if state.playing { "⏸" } else { "▶" };
            if ui.button(play).on_hover_text("Play the history").clicked() {
                end.ask(FromPage::Play {
                    playing: !state.playing,
                });
            }
            if ui.button("⏴").on_hover_text("The entry before").clicked() {
                end.ask(FromPage::Step { to: Step::Prev });
            }
            if ui.button("⏮").on_hover_text("The base").clicked() {
                end.ask(FromPage::Step { to: Step::First });
            }
        });
    });
    if let Some(status) = &history.status {
        ui.weak(status);
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in history.rows.iter().rev() {
            let current = history.current.as_deref() == Some(row.id.as_str());
            let text = format!(
                "{} {}  {}",
                if row.head {
                    "◆"
                } else if row.tip {
                    "◇"
                } else {
                    "●"
                },
                row.summary,
                row.author
            );
            let response = ui.selectable_label(current, text);
            if response.clicked() {
                end.ask(FromPage::Seek {
                    id: Some(row.id.clone()),
                });
            }
            if let Some(message) = &row.message {
                response.on_hover_text(message);
            }
        }
        let base = history.current.is_none();
        if ui.selectable_label(base, "○ the base").clicked() {
            end.ask(FromPage::Seek { id: None });
        }
    });
}
