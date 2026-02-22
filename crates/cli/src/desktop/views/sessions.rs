//! Sessions view - list and manage sessions

use eframe::egui::{Color32, RichText, ScrollArea, Ui};

use crate::desktop::state::{UiMessage, UiState};

pub struct SessionsView;

impl SessionsView {
    pub fn show(ui: &mut Ui, state: &mut UiState) -> Option<UiMessage> {
        let mut message_to_send = None;

        ui.heading("Sessions");
        ui.add_space(10.0);

        // New session button
        if ui.button("New Session").clicked() {
            message_to_send = Some(UiMessage::NewSession);
        }

        // Refresh button
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                message_to_send = Some(UiMessage::RefreshSessions);
            }
        });

        ui.add_space(10.0);

        // Current session info
        if let Some(ref current) = state.current_session {
            ui.group(|ui| {
                ui.label(RichText::new("Current Session").strong());
                ui.label(format!(
                    "ID: {}...",
                    &current.id[..current.id.floor_char_boundary(8)]
                ));
                ui.label(format!("Messages: {}", current.message_count));
            });
            ui.add_space(10.0);
        }

        // Session list
        ui.label(RichText::new("Available Sessions").strong());
        ui.add_space(5.0);

        if state.sessions.is_empty() {
            ui.label(RichText::new("No saved sessions").color(Color32::GRAY));
        } else {
            ScrollArea::vertical()
                .id_salt("sessions_list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for session in &state.sessions {
                        let is_current = state
                            .current_session
                            .as_ref()
                            .map(|c| c.id == session.id)
                            .unwrap_or(false);

                        ui.horizontal(|ui| {
                            let short_id = &session.id[..session.id.floor_char_boundary(8)];
                            let date = session.created_at.format("%Y-%m-%d %H:%M");

                            if is_current {
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({} msgs, {})",
                                        short_id, session.message_count, date
                                    ))
                                    .color(Color32::from_rgb(46, 204, 113)),
                                );
                                ui.label(RichText::new("(current)").small().color(Color32::GRAY));
                            } else {
                                ui.label(format!(
                                    "{} ({} msgs, {})",
                                    short_id, session.message_count, date
                                ));
                                if ui.small_button("Resume").clicked() {
                                    message_to_send =
                                        Some(UiMessage::ResumeSession(session.id.clone()));
                                }
                            }
                        });
                    }
                });
        }

        message_to_send
    }
}
