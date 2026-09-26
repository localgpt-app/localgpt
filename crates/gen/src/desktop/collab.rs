//! The Collaborate section: host and join UI for the prompt panel.
//!
//! **Host** — fill in session name + port + open/PIN, click Start → the
//! [`HostControl`] resource transitions through its state machine. The
//! panel shows PIN, guest count, and warnings while hosting.
//!
//! **Join** — browse the LAN (mDNS) or type an address, enter a PIN, and
//! connect. Pairing runs on a background thread; on success a child
//! `localgpt-gen --join` process is spawned with the connect token passed
//! via env so it connects immediately.

use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_egui::egui;

use crate::net::host::{HostControl, HostSessionInfo, HostStartRequest};
use crate::net::mdns::DiscoveredSession;
use crate::net::{DEFAULT_PORT, PROTOCOL_ID};

/// Accent colour, same as `panel.rs`.
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x25, 0xc2, 0xa0);
/// Error colour, same as `panel.rs`.
const ERROR: egui::Color32 = egui::Color32::from_rgb(0xf2, 0x6d, 0x6d);

// ---------------------------------------------------------------------------
// Host UI state
// ---------------------------------------------------------------------------

/// Persistent fields for the "Host a session" form.
#[derive(Debug, Default)]
pub(crate) struct HostForm {
    pub session_name: String,
    pub port: String,
    pub open: bool,
}

impl HostForm {
    pub fn port_or_default(&self) -> u16 {
        self.port.parse().unwrap_or(DEFAULT_PORT)
    }
}

// ---------------------------------------------------------------------------
// Join UI state
// ---------------------------------------------------------------------------

/// Background join task → panel results.
#[derive(Debug, Clone)]
pub(crate) enum JoinResult {
    /// mDNS browse finished; here are the sessions.
    Sessions(Vec<DiscoveredSession>),
    /// Pairing succeeded; the child viewer was spawned.
    Connected,
    /// Something went wrong.
    Error(String),
}

/// What the join section is doing.
#[derive(Debug, Default)]
pub(crate) enum JoinPhase {
    /// Idle — show the Join form.
    #[default]
    Idle,
    /// Browsing mDNS for sessions.
    Browsing,
    /// Sessions found — the user picks one or enters an address.
    Discovered(Vec<DiscoveredSession>),
    /// Pairing in progress.
    Pairing,
    /// A child viewer was spawned.
    Joined,
    /// An error occurred.
    Error(String),
}

/// Persistent state for the "Join a session" form.
#[derive(Debug, Default)]
pub(crate) struct JoinForm {
    pub address: String,
    pub pin: String,
    pub phase: JoinPhase,
    /// Channel carrying results from the background thread.
    pub results: Arc<Mutex<Vec<JoinResult>>>,
}

/// Drain any pending results from the background thread into the phase.
fn drain_join_results(form: &mut JoinForm) {
    let Ok(mut results) = form.results.lock() else {
        return;
    };
    for result in results.drain(..) {
        match result {
            JoinResult::Sessions(sessions) => {
                if sessions.is_empty() {
                    form.phase = JoinPhase::Discovered(vec![]);
                } else {
                    form.phase = JoinPhase::Discovered(sessions);
                }
            }
            JoinResult::Connected => form.phase = JoinPhase::Joined,
            JoinResult::Error(e) => form.phase = JoinPhase::Error(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Combined state — stored in `PromptPanel`
// ---------------------------------------------------------------------------

/// All state for the Collaborate section.
#[derive(Debug, Default)]
pub(crate) struct CollabState {
    /// Whether the section is expanded (collapsed by default).
    pub expanded: bool,
    pub host_form: HostForm,
    pub join_form: JoinForm,
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Draw the Collaborate section. Returns the `HostStartRequest` to write
/// into `HostControl` if the user clicked Start (the caller owns the
/// resource).
pub(crate) fn draw_collaborate(
    ui: &mut egui::Ui,
    collab: &mut CollabState,
    host_control: Option<&HostControl>,
    host_info: Option<&HostSessionInfo>,
) -> Option<HostStartRequest> {
    let mut start_request = None;

    // If we're already hosting, always show the status line prominently
    // before the collapsible section.
    if let Some(info) = host_info {
        ui.label(egui::RichText::new(info.summary()).small().color(ACCENT));
    }

    // Collapsible header.
    let heading = if host_info.is_some() || collab.expanded {
        "Collaborate ▾"
    } else {
        "Collaborate ▸"
    };
    if ui
        .add(
            egui::Label::new(egui::RichText::new(heading).small().strong())
                .sense(egui::Sense::click()),
        )
        .clicked()
    {
        collab.expanded = !collab.expanded;
    }

    if !collab.expanded {
        return None;
    }

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // --- Host section ---
        ui.label(egui::RichText::new("Host").strong());

        match host_control {
            Some(HostControl::Active { warning }) => {
                if let Some(info) = host_info {
                    if let Some(pin) = info.pin() {
                        ui.horizontal(|ui| {
                            ui.label("Session PIN:");
                            ui.label(egui::RichText::new(&pin).strong().monospace().size(16.0));
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("Open session — no PIN required")
                                .small()
                                .weak(),
                        );
                    }
                    let guest_word = if info.clients == 1 { "guest" } else { "guests" };
                    ui.label(
                        egui::RichText::new(format!("{} {guest_word} connected", info.clients))
                            .small(),
                    );
                }
                if let Some(warning) = warning {
                    ui.label(egui::RichText::new(warning).small().color(ERROR));
                }
            }
            Some(HostControl::StartRequested(_)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Starting session…");
                });
            }
            Some(HostControl::Failed(reason)) => {
                ui.label(egui::RichText::new(format!("Failed: {reason}")).color(ERROR));
                ui.add_space(4.0);
                start_request = host_form(ui, &mut collab.host_form);
            }
            Some(HostControl::NotHosting) | None => {
                start_request = host_form(ui, &mut collab.host_form);
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // --- Join section ---
        ui.label(egui::RichText::new("Join").strong());
        drain_join_results(&mut collab.join_form);
        join_section(ui, &mut collab.join_form);
    });

    start_request
}

/// The host form: session name, port, open toggle, Start button.
fn host_form(ui: &mut egui::Ui, form: &mut HostForm) -> Option<HostStartRequest> {
    // Lazy-init session name from env.
    if form.session_name.is_empty() {
        form.session_name = crate::net::default_session_name();
    }

    egui::Grid::new("collab_host_form")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Name").small());
            ui.add(
                egui::TextEdit::singleline(&mut form.session_name)
                    .desired_width(180.0)
                    .hint_text("Session name"),
            );
            ui.end_row();

            ui.label(egui::RichText::new("Port").small());
            ui.add(
                egui::TextEdit::singleline(&mut form.port)
                    .desired_width(80.0)
                    .hint_text(DEFAULT_PORT.to_string()),
            );
            ui.end_row();
        });

    ui.checkbox(&mut form.open, "Open session (no PIN)");

    let can_start = !form.session_name.trim().is_empty();
    let mut request = None;
    if ui
        .add_enabled(can_start, egui::Button::new("Start hosting"))
        .clicked()
    {
        request = Some(HostStartRequest {
            session_name: form.session_name.trim().to_string(),
            port: form.port_or_default(),
            open: form.open,
            full_access: false, // safe default; --remote-tools full from CLI
            web: false,         // browser guests are a CLI flag for now
            web_edit: false,
            resume: None,
            relay: None,
        });
    }
    request
}

/// The join section: browse/enter address + PIN + connect.
fn join_section(ui: &mut egui::Ui, form: &mut JoinForm) {
    match &form.phase {
        JoinPhase::Idle => {
            if ui.button("Browse LAN for sessions").clicked() {
                form.phase = JoinPhase::Browsing;
                let results = form.results.clone();
                std::thread::spawn(move || {
                    match crate::net::mdns::browse_sessions(
                        std::time::Duration::from_secs(3),
                        PROTOCOL_ID,
                    ) {
                        Ok(sessions) => {
                            if let Ok(mut r) = results.lock() {
                                r.push(JoinResult::Sessions(sessions));
                            }
                        }
                        Err(e) => {
                            if let Ok(mut r) = results.lock() {
                                r.push(JoinResult::Error(format!("mDNS browse failed: {e}")));
                            }
                        }
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Or connect directly:").small());
            join_manual(ui, form);
        }
        JoinPhase::Browsing => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Browsing the LAN…");
            });
        }
        JoinPhase::Discovered(sessions) => {
            if sessions.is_empty() {
                ui.label(
                    egui::RichText::new("No sessions found on the LAN.")
                        .small()
                        .weak(),
                );
            } else {
                ui.label(egui::RichText::new("Sessions found:").small());
                let sessions = sessions.clone(); // clone to avoid borrow conflict
                for session in &sessions {
                    let label = format!("{} — {}", session.session_name, session.addr);
                    if ui.button(&label).clicked() {
                        form.address = session.addr.to_string();
                    }
                }
            }
            ui.add_space(4.0);
            join_manual(ui, form);
            if ui.small_button("↻ Refresh").clicked() {
                form.phase = JoinPhase::Browsing;
                let results = form.results.clone();
                std::thread::spawn(move || {
                    match crate::net::mdns::browse_sessions(
                        std::time::Duration::from_secs(3),
                        PROTOCOL_ID,
                    ) {
                        Ok(sessions) => {
                            if let Ok(mut r) = results.lock() {
                                r.push(JoinResult::Sessions(sessions));
                            }
                        }
                        Err(e) => {
                            if let Ok(mut r) = results.lock() {
                                r.push(JoinResult::Error(format!("mDNS browse failed: {e}")));
                            }
                        }
                    }
                });
            }
        }
        JoinPhase::Pairing => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Pairing with host…");
            });
        }
        JoinPhase::Joined => {
            ui.label(egui::RichText::new("✓ Viewer window launched.").color(ACCENT));
            if ui.small_button("Join another").clicked() {
                form.phase = JoinPhase::Idle;
                form.address.clear();
                form.pin.clear();
            }
        }
        JoinPhase::Error(e) => {
            ui.label(egui::RichText::new(e.as_str()).color(ERROR));
            if ui.small_button("Try again").clicked() {
                form.phase = JoinPhase::Idle;
            }
        }
    }
}

/// Manual address + PIN + Connect button.
fn join_manual(ui: &mut egui::Ui, form: &mut JoinForm) {
    egui::Grid::new("collab_join_form")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Address").small());
            ui.add(
                egui::TextEdit::singleline(&mut form.address)
                    .desired_width(160.0)
                    .hint_text("192.168.1.5:9879"),
            );
            ui.end_row();

            ui.label(egui::RichText::new("PIN").small());
            ui.add(
                egui::TextEdit::singleline(&mut form.pin)
                    .desired_width(100.0)
                    .hint_text("(if required)"),
            );
            ui.end_row();
        });

    let can_connect = !form.address.trim().is_empty();
    if ui
        .add_enabled(can_connect, egui::Button::new("Connect"))
        .clicked()
    {
        let addr_str = form.address.trim().to_string();
        let pin = form.pin.trim().to_string();
        let results = form.results.clone();

        form.phase = JoinPhase::Pairing;

        std::thread::spawn(move || {
            let outcome = pair_and_spawn(&addr_str, if pin.is_empty() { None } else { Some(&pin) });
            if let Ok(mut r) = results.lock() {
                match outcome {
                    Ok(()) => r.push(JoinResult::Connected),
                    Err(e) => r.push(JoinResult::Error(e)),
                }
            }
        });
    }
}

/// Check the host's session info and spawn a child viewer process, handing
/// the PIN over through the environment. Runs on a background thread.
fn pair_and_spawn(addr_str: &str, pin: Option<&str>) -> Result<(), String> {
    let addr = crate::net::parse_peer_addr(addr_str).map_err(|e| e.to_string())?;

    let url = format!("http://{addr}/session-info");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let info = rt
        .block_on(async {
            let resp = reqwest::Client::new()
                .get(&url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(|e: String| {
            format!(
                "Couldn't reach the host at tcp://{addr} ({e}). \
             Is the session running and the port open?"
            )
        })?;

    let protocol = info["protocol"].as_u64().unwrap_or(0);
    if protocol != localgpt_world_sync::PROTOCOL_VERSION as u64 {
        return Err(format!(
            "Protocol mismatch: host speaks {} but we speak {} — use matching versions",
            protocol,
            localgpt_world_sync::PROTOCOL_VERSION
        ));
    }
    if info["secret_required"].as_bool().unwrap_or(false) && pin.is_none() {
        return Err("This session requires a PIN".into());
    }

    // Spawn the child viewer process.
    let exe = std::env::current_exe().map_err(|e| format!("can't find localgpt-gen: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--join").arg(addr.to_string());
    if let Some(pin) = pin {
        cmd.env(crate::net::JOIN_PIN_ENV, pin);
    }
    cmd.spawn()
        .map_err(|e| format!("failed to start viewer: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_form_defaults() {
        let form = HostForm::default();
        assert_eq!(form.port_or_default(), DEFAULT_PORT);
    }

    #[test]
    fn host_form_custom_port() {
        let form = HostForm {
            port: "12345".into(),
            ..Default::default()
        };
        assert_eq!(form.port_or_default(), 12345);
    }

    #[test]
    fn host_form_bad_port_falls_back() {
        let form = HostForm {
            port: "abc".into(),
            ..Default::default()
        };
        assert_eq!(form.port_or_default(), DEFAULT_PORT);
    }

    #[test]
    fn drain_join_results_applies_sessions() {
        let mut form = JoinForm::default();
        form.results
            .lock()
            .unwrap()
            .push(JoinResult::Sessions(vec![]));
        drain_join_results(&mut form);
        assert!(matches!(form.phase, JoinPhase::Discovered(ref s) if s.is_empty()));
    }

    #[test]
    fn drain_join_results_applies_error() {
        let mut form = JoinForm::default();
        form.results
            .lock()
            .unwrap()
            .push(JoinResult::Error("fail".into()));
        drain_join_results(&mut form);
        assert!(matches!(form.phase, JoinPhase::Error(ref e) if e == "fail"));
    }

    #[test]
    fn drain_join_results_applies_connected() {
        let mut form = JoinForm::default();
        form.results.lock().unwrap().push(JoinResult::Connected);
        drain_join_results(&mut form);
        assert!(matches!(form.phase, JoinPhase::Joined));
    }
}
