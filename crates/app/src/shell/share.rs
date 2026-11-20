//! The bar's Share and MCP buttons, and their panels (`crate::share` is what
//! they drive).
//!
//! Share: start sharing the world in the window — with browsers on this
//! network, and through a relay with anyone — then its PIN and links to copy
//! and Stop; join someone's session in a window of its own, by address or
//! found on this network; and the sessions kept here, each to play back in
//! a window of its own or export as one `.world` file. MCP: Gen's relay over
//! this window's world, on and off, and the line an agent's MCP
//! configuration takes; where it cannot be on, why.

use eframe::egui::{self, RichText};

use super::state::State;
use super::theme;
use crate::share::{HostingView, Mode, ShareView};
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

/// What an agent's MCP configuration starts to reach a window's relay.
pub const CONNECT: &str = "localgpt gen mcp-server --connect";
/// The same, as an MCP client's configuration takes it.
pub const CONNECT_CONFIG: &str = r#"{ "mcpServers": { "localgpt-gen": { "command": "localgpt", "args": ["gen", "mcp-server", "--connect"] } } }"#;

/// What is typed into the panels, kept between frames.
pub struct Share {
    name: String,
    /// The name field took the app's default once.
    named: bool,
    web: bool,
    relay: String,
    addr: String,
    pin: String,
}

impl Default for Share {
    fn default() -> Self {
        Self {
            name: String::new(),
            named: false,
            web: true,
            relay: String::new(),
            addr: String::new(),
            pin: String::new(),
        }
    }
}

impl Share {
    /// The Share button, and the panel under it.
    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
        let Some(view) = state.share.as_deref() else {
            return;
        };
        let (label, live) = match (&view.hosting, view.mode) {
            (_, Mode::Guest) => ("Guest".to_string(), true),
            (Some(hosting), _) => (
                match hosting.guests {
                    0 => "Sharing".to_string(),
                    n => format!("Sharing · {n}"),
                },
                true,
            ),
            (None, _) => ("Share".to_string(), false),
        };
        let button = ui
            .add(egui::Button::new(labelled(&label, live)).frame_when_inactive(false))
            .on_hover_text("Share this world, join someone's, or play a kept session back");
        if button.clicked() {
            end.ask(FromPage::ShareSessions);
        }
        egui::Popup::from_toggle_button_response(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::BOTTOM_END)
            .width(340.0)
            .show(|ui| self.panel(ui, view, end));
    }

    fn panel(&mut self, ui: &mut egui::Ui, view: &ShareView, end: &ShellEnd) {
        ui.set_width(340.0);
        header(ui, "Share this world");
        match (view.mode, &view.hosting) {
            (Mode::Guest, _) => {
                faint(
                    ui,
                    "A guest's window: the world is the host's, and it is theirs to share.",
                );
                if let Some(guest) = &view.guest {
                    ui.label(RichText::new(guest).color(theme::MUTED));
                }
            }
            (_, Some(hosting)) => hosting_ui(ui, hosting, end),
            (_, None) if view.starting => {
                ui.label(RichText::new("Starting …").color(theme::MUTED));
            }
            (_, None) => self.start(ui, view, end),
        }
        if let Some(error) = &view.error {
            ui.colored_label(theme::DANGER, error);
        }

        ui.separator();
        self.join(ui, view, end);

        ui.separator();
        sessions(ui, view, end);

        if let Some(note) = &view.note {
            ui.add_space(4.0);
            ui.label(RichText::new(note).color(theme::MUTED));
        }
    }

    fn start(&mut self, ui: &mut egui::Ui, view: &ShareView, end: &ShellEnd) {
        if !self.named {
            self.name = view.name.clone();
            self.named = true;
        }
        field(ui, "Name", &mut self.name, &view.name);
        ui.checkbox(&mut self.web, "Browsers on this network can join");
        field(
            ui,
            "Relay",
            &mut self.relay,
            "https://… for guests elsewhere (optional)",
        );
        ui.add_space(4.0);
        if wide(ui, "Start sharing") {
            end.ask(FromPage::ShareHost {
                name: self.name.trim().to_string(),
                web: self.web,
                relay: Some(self.relay.trim().to_string()).filter(|r| !r.is_empty()),
            });
        }
        ui.add_space(4.0);
        let whose = if view.mode == Mode::Canvas {
            "Guests watch, walk and talk; they don't edit. Agents keep editing through \
             the world's API."
        } else {
            "Guests watch, walk and talk; they don't edit."
        };
        faint(ui, whose);
        faint(
            ui,
            "Plain HTTP and a PIN: for a network you trust. A relay can read what \
             passes through it.",
        );
    }

    fn join(&mut self, ui: &mut egui::Ui, view: &ShareView, end: &ShellEnd) {
        ui.strong("Join a session");
        field(ui, "Address", &mut self.addr, "host or host:port");
        field(ui, "PIN", &mut self.pin, "if it asks for one");
        ui.horizontal(|ui| {
            let join = ui.add_enabled(
                !view.joining && !self.addr.trim().is_empty(),
                egui::Button::new(if view.joining { "Asking …" } else { "Join" }),
            );
            if join.clicked() {
                end.ask(FromPage::ShareJoin {
                    addr: self.addr.trim().to_string(),
                    pin: Some(self.pin.trim().to_string()).filter(|p| !p.is_empty()),
                });
            }
            let find = ui.add_enabled(
                !view.finding,
                egui::Button::new(if view.finding {
                    "Looking …"
                } else {
                    "Find on this network"
                }),
            );
            if find.clicked() {
                end.ask(FromPage::ShareFind);
            }
        });
        if let Some(found) = &view.found {
            for session in found {
                let pick = ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("{} — {}", session.name, session.addr))
                                .color(theme::INK),
                        )
                        .truncate()
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Join this one");
                if pick.clicked() {
                    self.addr = session.addr.clone();
                }
            }
        }
        faint(ui, "A session opens in a window of its own.");
    }
}

fn hosting_ui(ui: &mut egui::Ui, hosting: &HostingView, end: &ShellEnd) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("●").color(theme::ATTACHED));
        ui.add(
            egui::Label::new(RichText::new(format!("Sharing '{}'", hosting.name)).strong())
                .truncate(),
        );
    });
    let guests = match hosting.guests {
        0 => "No guests yet".to_string(),
        1 => "1 guest".to_string(),
        n => format!("{n} guests"),
    };
    ui.label(RichText::new(guests).color(theme::MUTED));
    ui.add_space(4.0);
    if let Some(pin) = &hosting.pin {
        copy_row(ui, "PIN", pin);
    }
    if let Some(link) = &hosting.browser {
        copy_row(ui, "Browser", link);
    }
    if let Some(address) = &hosting.address {
        copy_row(ui, "LocalGPT", &format!("localgpt-app --join {address}"));
    }
    if hosting.relay {
        match (&hosting.internet, &hosting.relay_error) {
            (Some(link), _) => copy_row(ui, "Internet", link),
            (None, Some(error)) => {
                ui.colored_label(theme::DANGER, format!("Relay: {error}"));
            }
            (None, None) => {
                ui.label(RichText::new("Waiting for the relay …").color(theme::MUTED));
            }
        }
    }
    if let Some(warning) = &hosting.warning {
        faint(ui, warning);
    }
    ui.add_space(6.0);
    if wide(ui, "Stop sharing") {
        end.ask(FromPage::ShareStop);
    }
}

fn sessions(ui: &mut egui::Ui, view: &ShareView, end: &ShellEnd) {
    ui.strong("Sessions");
    if view.sessions.is_empty() {
        faint(
            ui,
            "A session you share is kept here, to play back or to export as one .world file.",
        );
        return;
    }
    for session in &view.sessions {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(&session.name).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Export…")
                    .on_hover_text("Save it as one .world file")
                    .clicked()
                {
                    end.ask(FromPage::ShareExport {
                        dir: session.dir.clone(),
                    });
                }
                if ui
                    .small_button("Replay")
                    .on_hover_text("Watch it build again, in a window of its own")
                    .clicked()
                {
                    end.ask(FromPage::ShareReplay {
                        dir: session.dir.clone(),
                    });
                }
                let edits = match session.revisions {
                    1 => "1 edit".to_string(),
                    n => format!("{n} edits"),
                };
                ui.label(
                    RichText::new(format!(
                        "{edits} · {}",
                        super::bar::since(session.updated_ms)
                    ))
                    .color(theme::FAINT),
                );
            });
        });
    }
}

/// The MCP button, and the panel under it.
pub fn mcp(ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
    let Some(view) = state.share.as_deref() else {
        return;
    };
    let mcp = &view.mcp;
    let button = ui
        .add(
            egui::Button::new(labelled(if mcp.on { "MCP: on" } else { "MCP" }, mcp.on))
                .frame_when_inactive(false),
        )
        .on_hover_text("Gen's tools over this window's world, for an agent");
    egui::Popup::from_toggle_button_response(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .width(340.0)
        .show(|ui| {
            ui.set_width(340.0);
            header(ui, "Gen's tools for an agent");
            if let Some(why) = mcp.refused {
                ui.label(RichText::new(why).color(theme::MUTED));
            } else if mcp.on {
                if let Some(port) = mcp.port {
                    ui.label(
                        RichText::new(format!("The relay listens on port {port}."))
                            .color(theme::MUTED),
                    );
                }
                ui.label(RichText::new("An agent's MCP server:").color(theme::MUTED));
                copy_row(ui, "Command", CONNECT);
                copy_row(ui, "Config", CONNECT_CONFIG);
                ui.add_space(6.0);
                if wide(ui, "Turn off") {
                    end.ask(FromPage::ShareMcp { on: false });
                }
            } else {
                ui.label(
                    RichText::new(
                        "Gen's 88 tools, over the world in this window, for an agent whose \
                         MCP server is `localgpt gen mcp-server --connect`. Off until you \
                         turn it on.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                if wide(ui, "Turn on") {
                    end.ask(FromPage::ShareMcp { on: true });
                }
            }
            if let Some(error) = &mcp.error
                && mcp.refused.is_none()
            {
                ui.colored_label(theme::DANGER, error);
            }
        });
}

/// A button's text, with a green dot while something is live.
fn labelled(label: &str, live: bool) -> egui::text::LayoutJob {
    let mut text = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat {
        color,
        ..Default::default()
    };
    text.append(
        label,
        0.0,
        format(if live { theme::INK } else { theme::MUTED }),
    );
    if live {
        text.append("  ●", 0.0, format(theme::ATTACHED));
    }
    text
}

fn header(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // "×", not "✕": the latter is not in egui's fonts.
            if ui.small_button("×").on_hover_text("Close").clicked() {
                egui::Popup::close_all(ui.ctx());
            }
        });
    });
    ui.add_space(4.0);
}

/// A labelled one-line field; the label names it to assistive technology.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        let label = ui.add_sized(
            [64.0, 18.0],
            egui::Label::new(RichText::new(label).color(theme::MUTED)),
        );
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
    });
}

/// What it is, and the text to copy with a click.
fn copy_row(ui: &mut egui::Ui, what: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [64.0, 18.0],
            egui::Label::new(RichText::new(what).color(theme::MUTED)),
        );
        let copy = ui
            .add(
                egui::Label::new(RichText::new(text).monospace().small().color(theme::INK))
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_text(format!("Copy: {text}"));
        if copy.clicked() {
            ui.ctx().copy_text(text.to_string());
        }
    });
}

fn faint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::FAINT));
}

fn wide(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add_sized([ui.available_width(), 26.0], egui::Button::new(text))
        .clicked()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc::{Receiver, channel};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::share::{FoundView, McpView, SessionView};

    struct Panel {
        share: Share,
        state: State,
        end: ShellEnd,
    }

    fn bar_with(view: ShareView) -> (Harness<'static, Panel>, Receiver<FromPage>) {
        let (asks, asked) = channel();
        let (_tell, events) = channel();
        let panel = Panel {
            share: Share::default(),
            state: State {
                share: Some(Box::new(view)),
                ..State::default()
            },
            end: ShellEnd { events, asks },
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(900.0, 900.0))
            .allow_missing_glyphs()
            .build_ui_state(
                |ui, panel: &mut Panel| {
                    ui.horizontal(|ui| {
                        panel.share.ui(ui, &panel.state, &panel.end);
                        mcp(ui, &panel.state, &panel.end);
                    });
                },
                panel,
            );
        harness.run_steps(2);
        (harness, asked)
    }

    fn view(mode: Mode) -> ShareView {
        ShareView {
            mode,
            name: "Maya's world".into(),
            mcp: McpView {
                refused: mode.mcp_refused(),
                ..McpView::default()
            },
            ..ShareView::default()
        }
    }

    fn type_into(harness: &mut Harness<'static, Panel>, field: &str, text: &str) {
        harness
            .get_by_role_and_label(egui::Role::TextInput, field)
            .focus();
        harness.run_steps(1);
        harness
            .get_by_role_and_label(egui::Role::TextInput, field)
            .type_text(text);
        harness.run_steps(1);
    }

    #[test]
    fn share_starts_shows_its_links_and_stops() {
        let (mut harness, asked) = bar_with(view(Mode::Scene));
        harness.get_by_label("Share").click();
        harness.run_steps(2);
        harness.get_by_label("Start sharing").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(
            sent,
            [
                FromPage::ShareSessions,
                FromPage::ShareHost {
                    name: "Maya's world".into(),
                    web: true,
                    relay: None,
                }
            ]
        );

        // Live: the PIN, the links, the guests, the relay on its way.
        let share = harness.state_mut().state.share.as_mut().unwrap();
        share.hosting = Some(HostingView {
            name: "Maya's world".into(),
            pin: Some("482913".into()),
            browser: Some("http://192.168.1.5:9879/#t=abc".into()),
            address: Some("192.168.1.5:9879".into()),
            relay: true,
            guests: 2,
            ..HostingView::default()
        });
        harness.run_steps(2);
        // The button says so, with how many have come.
        let _ = harness.get_by_label("Sharing · 2  ●");
        let _ = harness.get_by_label("Sharing 'Maya's world'");
        let _ = harness.get_by_label("2 guests");
        let _ = harness.get_by_label("482913");
        let _ = harness.get_by_label("http://192.168.1.5:9879/#t=abc");
        let _ = harness.get_by_label("localgpt-app --join 192.168.1.5:9879");
        let _ = harness.get_by_label("Waiting for the relay …");
        harness.get_by_label("Stop sharing").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(sent, [FromPage::ShareStop]);
    }

    #[test]
    fn join_find_and_the_sessions_kept() {
        let mut shown = view(Mode::Scene);
        shown.sessions = vec![SessionView {
            dir: PathBuf::from("/ws/sessions/castle"),
            name: "Castle".into(),
            revisions: 42,
            updated_ms: 0,
        }];
        let (mut harness, asked) = bar_with(shown);
        harness.get_by_label("Share").click();
        harness.run_steps(2);
        let _ = asked.try_iter().count();

        type_into(&mut harness, "Address", "192.168.1.5");
        type_into(&mut harness, "PIN", "482 913");
        harness.get_by_label("Join").click();
        harness.run_steps(2);
        harness.get_by_label("Find on this network").click();
        harness.run_steps(2);
        harness.get_by_label("Replay").click();
        harness.run_steps(2);
        harness.get_by_label("Export…").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        let castle = PathBuf::from("/ws/sessions/castle");
        assert_eq!(
            sent,
            [
                FromPage::ShareJoin {
                    addr: "192.168.1.5".into(),
                    pin: Some("482 913".into()),
                },
                FromPage::ShareFind,
                FromPage::ShareReplay {
                    dir: castle.clone()
                },
                FromPage::ShareExport { dir: castle },
            ]
        );

        // A session found on this network fills the address in.
        harness.state_mut().state.share.as_mut().unwrap().found = Some(vec![FoundView {
            name: "Rin's world".into(),
            addr: "192.168.1.9:9879".into(),
        }]);
        harness.run_steps(2);
        harness
            .get_by_label("Rin's world — 192.168.1.9:9879")
            .click();
        harness.run_steps(2);
        harness.get_by_label("Join").click();
        harness.run_steps(2);
        let sent: Vec<FromPage> = asked.try_iter().collect();
        assert_eq!(
            sent,
            [FromPage::ShareJoin {
                addr: "192.168.1.9:9879".into(),
                pin: Some("482 913".into()),
            }]
        );
    }

    #[test]
    fn mcp_turns_on_and_off_and_says_where_it_cannot() {
        let (mut harness, asked) = bar_with(view(Mode::Scene));
        harness.get_by_label("MCP").click();
        harness.run_steps(2);
        harness.get_by_label("Turn on").click();
        harness.run_steps(2);
        assert_eq!(
            asked.try_iter().collect::<Vec<_>>(),
            [FromPage::ShareMcp { on: true }]
        );

        harness.state_mut().state.share.as_mut().unwrap().mcp = McpView {
            on: true,
            port: Some(9878),
            ..McpView::default()
        };
        harness.run_steps(2);
        let _ = harness.get_by_label("The relay listens on port 9878.");
        let _ = harness.get_by_label(CONNECT);
        harness.get_by_label("Turn off").click();
        harness.run_steps(2);
        assert_eq!(
            asked.try_iter().collect::<Vec<_>>(),
            [FromPage::ShareMcp { on: false }]
        );

        // On a live canvas the panel says why, and offers nothing to turn on.
        let (mut harness, _asked) = bar_with(view(Mode::Canvas));
        harness.get_by_label("MCP").click();
        harness.run_steps(2);
        let _ = harness.get_by_label(Mode::Canvas.mcp_refused().unwrap());
        assert!(harness.query_by_label("Turn on").is_none());
    }

    #[test]
    fn a_guests_window_shares_nothing_of_its_own() {
        let mut shown = view(Mode::Guest);
        shown.guest = Some("connected".into());
        let (mut harness, _asked) = bar_with(shown);
        harness.get_by_label("Guest  ●").click();
        harness.run_steps(2);
        let _ = harness.get_by_label("connected");
        assert!(harness.query_by_label("Start sharing").is_none());
        // Joining another session, and the sessions kept, are still here.
        let _ = harness.get_by_label("Find on this network");
    }
}
