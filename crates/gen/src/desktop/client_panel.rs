//! The viewer's prompt panel: when a `--join` viewer has no terminal
//! (launched from the host-or-join panel, or with `--desktop`), prompts for
//! the host's agent are typed here, and host chat plus queue progress are
//! shown from [`ClientPanelLog`].

use std::net::SocketAddr;
use std::sync::Mutex;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPreUpdateSet, EguiPrimaryContextPass, egui};
use tokio::sync::mpsc;

use super::panel::{ACCENT, ERROR, hold_game_keys_while_typing};
use crate::net::ops_client::{ClientEntry, ClientPanelLog};

/// Adds the viewer's prompt panel. Add it after `NetClientPlugin`, which
/// owns [`ClientPanelLog`].
pub struct ClientPanelPlugin {
    prompt_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    host: SocketAddr,
}

impl ClientPanelPlugin {
    pub fn new(prompt_tx: mpsc::UnboundedSender<String>, host: SocketAddr) -> Self {
        Self {
            prompt_tx: Mutex::new(Some(prompt_tx)),
            host,
        }
    }
}

impl Plugin for ClientPanelPlugin {
    fn build(&self, app: &mut App) {
        let prompt_tx = self
            .prompt_tx
            .lock()
            .expect("client panel lock poisoned")
            .take()
            .expect("ClientPanelPlugin built twice");
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(ClientPanel {
            prompt_tx,
            host: self.host,
            open: true,
            input: String::new(),
            focus_input: true,
        })
        .add_systems(
            PreUpdate,
            hold_game_keys_while_typing
                .after(EguiPreUpdateSet::ProcessInput)
                .before(EguiPreUpdateSet::BeginPass),
        )
        .add_systems(EguiPrimaryContextPass, client_panel_ui);
    }
}

#[derive(Resource)]
struct ClientPanel {
    prompt_tx: mpsc::UnboundedSender<String>,
    host: SocketAddr,
    open: bool,
    input: String,
    focus_input: bool,
}

fn client_panel_ui(
    mut contexts: EguiContexts,
    mut panel: ResMut<ClientPanel>,
    log: Res<ClientPanelLog>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let panel = &mut *panel;

    if ctx.input(|input| input.key_pressed(egui::Key::F2)) {
        panel.open = !panel.open;
        panel.focus_input = panel.open;
    }
    if !panel.open {
        egui::Area::new(egui::Id::new("gen_client_reopen"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-14.0, -14.0])
            .show(ctx, |ui| {
                if ui.button("Prompt (F2)").clicked() {
                    panel.open = true;
                    panel.focus_input = true;
                }
            });
        return;
    }
    if !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        panel.focus_input = true;
    }

    // Same reasoning as the prompt panel: bevy_egui hands us a Context.
    #[allow(deprecated)]
    egui::Panel::right("gen_client_panel")
        .default_size(360.0)
        .min_size(280.0)
        .resizable(true)
        .show(ctx, |ui| draw(ui, panel, &log));
}

fn draw(ui: &mut egui::Ui, panel: &mut ClientPanel, log: &ClientPanelLog) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("LocalGPT Gen · Guest")
                .strong()
                .size(16.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Hide")
                .on_hover_text("Hide this panel (F2)")
                .clicked()
            {
                panel.open = false;
            }
        });
    });
    ui.label(
        egui::RichText::new(format!("Joined {}", panel.host))
            .small()
            .color(ACCENT),
    );
    ui.label(
        egui::RichText::new(
            "WASD + Q/E to fly, right-drag to look. Prompts go to the host's agent and are \
             built where you're looking.",
        )
        .small()
        .weak(),
    );
    ui.separator();

    let reserved = 110.0;
    egui::ScrollArea::vertical()
        .id_salt("gen_client_log")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .max_height((ui.available_height() - reserved).max(80.0))
        .show(ui, |ui| {
            if log.entries.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to the host…");
                });
            }
            for entry in &log.entries {
                draw_entry(ui, entry);
            }
        });

    ui.separator();
    let output = egui::TextEdit::multiline(&mut panel.input)
        .id(egui::Id::new("gen_client_input"))
        .hint_text("Ask the host's agent to build something here…")
        .desired_rows(3)
        .desired_width(f32::INFINITY)
        .return_key(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::Enter,
        ))
        .show(ui);
    if std::mem::take(&mut panel.focus_input) {
        output.response.request_focus();
    }
    let enter_pressed = output.response.has_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter) && !input.modifiers.shift);
    ui.horizontal(|ui| {
        let can_send = !panel.input.trim().is_empty();
        let clicked = ui
            .add_enabled(can_send, egui::Button::new("Send"))
            .clicked();
        ui.label(
            egui::RichText::new("Enter sends · /stats · /goto x y z")
                .small()
                .weak(),
        );
        if (clicked || enter_pressed) && can_send {
            let text = std::mem::take(&mut panel.input).trim().to_string();
            let _ = panel.prompt_tx.send(text);
            panel.focus_input = true;
        }
    });
    ui.add_space(4.0);
}

fn draw_entry(ui: &mut egui::Ui, entry: &ClientEntry) {
    match entry {
        ClientEntry::You(text) => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("You").strong());
            ui.label(text);
        }
        ClientEntry::Chat { speaker, text } => {
            ui.add_space(8.0);
            let (name, color) = match speaker.as_str() {
                "host" => ("Gen", Some(ACCENT)),
                "host-user" => ("Host", None),
                "client" => ("A guest", None),
                other => (other, None),
            };
            let mut name = egui::RichText::new(name).strong();
            if let Some(color) = color {
                name = name.color(color);
            }
            ui.label(name);
            ui.label(text.trim_end());
        }
        ClientEntry::Status(text) => {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(text).small().italics().weak());
        }
        ClientEntry::Error(text) => {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).color(ERROR));
        }
    }
}
