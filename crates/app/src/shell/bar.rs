//! The bar across the top: what is open, Open… for another document, song
//! or world, the opener's or the history's status, the panels' menu, and
//! the phone remote. The panels' menu shows and hides each panel and puts
//! them back where they began. The phone's button names the attached
//! devices; its panel turns the listener on and off, shows the pairing code
//! and lists the devices, each of which can be forgotten.

use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, RichText};

use super::state::State;
use super::theme;
use super::tiles;
use crate::webui::FromPage;
use crate::webui::RemoteView;
use crate::webui::link::ShellEnd;

#[derive(Default)]
pub struct Bar {
    /// The pairing code's modules, dark or not, row by row, and the address
    /// they encode.
    qr: Option<(String, usize, Vec<bool>)>,
    /// The optional audio models, installed and removed from its panel.
    audio: super::audio::Audio,
}

impl Bar {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &State,
        end: &ShellEnd,
        layout: &mut tiles::Layout,
    ) {
        ui.horizontal(|ui| {
            ui.strong("LocalGPT");
            ui.separator();
            match &state.title {
                Some(title) => ui.label(title),
                None => ui.weak("Nothing open"),
            };
            // A live world opens at launch: there is no Open… in it.
            if !state.mode_history {
                let open = ui
                    .add_enabled(
                        !state.opener_busy,
                        egui::Button::new("Open…").frame_when_inactive(false),
                    )
                    .on_hover_text("A Markdown document, a song, or a world");
                if open.clicked() {
                    end.ask(FromPage::OpenFile);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.phone(ui, state, end);
                self.audio.ui(ui);
                layout.menu(ui, state);
                let status = if state.mode_history {
                    state.history.as_ref().and_then(|h| h.status.as_deref())
                } else {
                    state.opener_status.as_deref()
                };
                if let Some(status) = status {
                    ui.add(egui::Label::new(RichText::new(status).color(theme::FAINT)).truncate());
                }
            });
        });
    }

    /// The Phone button, and the panel under it.
    fn phone(&mut self, ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
        let view = state.remote.as_deref();
        let attached: Vec<&str> = view
            .map(|v| {
                v.devices
                    .iter()
                    .filter(|d| d.attached)
                    .map(|d| d.name.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let label = if !attached.is_empty() {
            attached.join(", ")
        } else if view.is_some_and(|v| v.on) {
            "Phone: on".to_string()
        } else {
            "Phone".to_string()
        };
        let mut text = egui::text::LayoutJob::default();
        let ink = if attached.is_empty() {
            theme::MUTED
        } else {
            theme::INK
        };
        text.append(&label, 0.0, format(ink));
        if !attached.is_empty() {
            text.append("  ●", 0.0, format(theme::ATTACHED));
        }
        let button = ui
            .add(egui::Button::new(text).frame_when_inactive(false))
            .on_hover_text("Use this window from a phone or a tablet on this network");
        let Some(view) = view else {
            return;
        };
        egui::Popup::from_toggle_button_response(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .width(320.0)
            .show(|ui| self.panel(ui, view, state, end));
    }

    fn panel(&mut self, ui: &mut egui::Ui, view: &RemoteView, state: &State, end: &ShellEnd) {
        ui.set_width(320.0);
        ui.horizontal(|ui| {
            ui.strong("A phone or a tablet");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // "×", not "✕": the latter is not in egui's fonts.
                if ui.small_button("×").on_hover_text("Close").clicked() {
                    egui::Popup::close_all(ui.ctx());
                }
            });
        });
        ui.add_space(4.0);
        let wide = |ui: &mut egui::Ui, text: &str| {
            ui.add_sized([ui.available_width(), 26.0], egui::Button::new(text))
                .clicked()
        };
        if !view.on {
            ui.label(
                RichText::new(
                    "The agents' terminals, the history and the world, in the browser of a \
                     phone or a tablet on this network. Off until you turn it on.",
                )
                .color(theme::MUTED),
            );
            ui.add_space(6.0);
            if wide(ui, "Turn on") {
                end.ask(FromPage::RemoteOn { on: true });
            }
        } else {
            match &view.pairing {
                Some(pairing) => {
                    ui.vertical_centered(|ui| {
                        self.code(ui, &pairing.url);
                        let left = state.remote_left();
                        ui.label(
                            RichText::new(format!(
                                "Scan with the device's camera. The code works once, for {}:{:02}.",
                                left / 60,
                                left % 60
                            ))
                            .color(theme::MUTED),
                        );
                        let copy = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(&pairing.url)
                                        .monospace()
                                        .small()
                                        .color(theme::FAINT),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text("Copy the pairing address");
                        if copy.clicked() {
                            ui.ctx().copy_text(pairing.url.clone());
                        }
                    });
                }
                None => {
                    if wide(ui, "Pair a device") {
                        end.ask(FromPage::RemotePair);
                    }
                }
            }
            if !view.devices.is_empty() {
                ui.add_space(6.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    for device in &view.devices {
                        ui.horizontal(|ui| {
                            let dot = if device.attached {
                                theme::ATTACHED
                            } else {
                                theme::FAINT
                            };
                            ui.label(RichText::new("●").small().color(dot));
                            ui.add(egui::Label::new(&device.name).truncate());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Forget").clicked() {
                                        end.ask(FromPage::RemoteForget {
                                            id: device.id.clone(),
                                        });
                                    }
                                    let when = if device.attached {
                                        "attached".to_string()
                                    } else {
                                        since(device.seen_ms)
                                    };
                                    ui.label(RichText::new(when).color(theme::FAINT));
                                },
                            );
                        });
                    }
                });
            }
            if let Some(url) = &view.url {
                ui.add_space(4.0);
                ui.label(RichText::new(url).monospace().small().color(theme::FAINT));
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Plain HTTP: for a network you trust, or a VPN you run. A paired device \
                     can type into the terminals, which is a shell on this computer.",
                )
                .color(theme::FAINT),
            );
            ui.add_space(6.0);
            if wide(ui, "Turn off") {
                end.ask(FromPage::RemoteOn { on: false });
            }
        }
        if let Some(error) = &view.error {
            ui.add_space(4.0);
            ui.colored_label(theme::DANGER, error);
        }
    }

    /// The pairing code, drawn as the page draws its SVG: dark modules on
    /// white, with a quiet border.
    fn code(&mut self, ui: &mut egui::Ui, url: &str) {
        if self.qr.as_ref().is_none_or(|(of, _, _)| of != url) {
            self.qr = qrcode::QrCode::new(url.as_bytes()).ok().map(|code| {
                let dark = code
                    .to_colors()
                    .into_iter()
                    .map(|c| c == qrcode::Color::Dark)
                    .collect();
                (url.to_string(), code.width(), dark)
            });
        }
        let Some((_, width, dark)) = &self.qr else {
            return;
        };
        let side = 176.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::WHITE);
        // Four modules of quiet zone round the code, as a scanner expects.
        let module = side / (*width as f32 + 8.0);
        let origin = rect.min + egui::vec2(4.0 * module, 4.0 * module);
        for (i, _) in dark.iter().enumerate().filter(|(_, d)| **d) {
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            let min = origin + egui::vec2(x * module, y * module);
            painter.rect_filled(
                egui::Rect::from_min_size(min, egui::vec2(module, module)),
                0.0,
                Color32::BLACK,
            );
        }
    }
}

fn format(color: Color32) -> egui::TextFormat {
    egui::TextFormat {
        color,
        ..Default::default()
    }
}

/// "3 min ago", for when a device was last attached, as the page says it.
fn since(ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let s = now.saturating_sub(ms) / 1000;
    match s {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::webui::{DeviceView, PairingView};

    struct Panel {
        bar: Bar,
        state: State,
        end: ShellEnd,
        layout: tiles::Layout,
    }

    #[test]
    fn bar_opens_files_shows_status_and_drives_phone_remote() {
        let (asks, asked) = channel();
        let (_tell, events) = channel();
        let panel = Panel {
            bar: Bar::default(),
            state: State {
                title: Some("Song · Dawn".into()),
                opener_status: Some("Loaded Dawn".into()),
                remote: Some(Box::new(RemoteView {
                    on: false,
                    url: None,
                    pairing: None,
                    devices: Vec::new(),
                    error: None,
                })),
                ..State::default()
            },
            end: ShellEnd { events, asks },
            layout: tiles::Layout::open(false, None),
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(960.0, 420.0))
            .allow_missing_glyphs()
            .build_ui_state(
                |ui, panel: &mut Panel| {
                    panel
                        .bar
                        .ui(ui, &panel.state, &panel.end, &mut panel.layout);
                },
                panel,
            );
        harness.run_steps(2);

        let _ = harness.get_by_label("Song · Dawn");
        let _ = harness.get_by_label("Loaded Dawn");
        harness.get_by_label("Open…").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(sent, [FromPage::OpenFile]);

        // In live canvas mode (`mode_history`), `Open…` is hidden.
        harness.state_mut().state.mode_history = true;
        harness.run_steps(2);
        assert!(harness.query_by_label("Open…").is_none());

        // Phone popup: turning on, forgetting a device, turning off.
        harness.get_by_label("Phone").click();
        harness.run_steps(2);
        harness.get_by_label("Turn on").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(sent, [FromPage::RemoteOn { on: true }]);

        harness.state_mut().state.remote = Some(Box::new(RemoteView {
            on: true,
            url: Some("http://192.168.1.10:9880".into()),
            pairing: Some(PairingView {
                url: "http://192.168.1.10:9880#pair=abc".into(),
                qr: None,
                left_s: 300,
            }),
            devices: vec![DeviceView {
                id: "dev-1".into(),
                name: "iPhone".into(),
                paired_ms: 1000,
                seen_ms: 1000,
                attached: true,
            }],
            error: None,
        }));
        harness.run_steps(2);
        harness.get_by_label("Forget").click();
        harness.run_steps(2);
        harness.get_by_label("Turn off").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(
            sent,
            [
                FromPage::RemoteForget { id: "dev-1".into() },
                FromPage::RemoteOn { on: false }
            ]
        );
    }
}
