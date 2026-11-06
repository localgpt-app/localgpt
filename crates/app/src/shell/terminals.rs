//! The terminals: tabs over the app's PTYs, each a login shell in the open
//! world's folder where the person runs whatever agent they have
//! (`crates/app/ui/src/TerminalsPanel.tsx` in the page). The app knows
//! nothing about what runs in a tab; its title is the tab's label. A tab a
//! paired phone opened shows up here too, attached rather than started.

use std::time::Instant;

use eframe::egui;

use super::state::{State, TermEvent};
use super::terminal::{Screen, Style};
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

/// A tab's label kept to `max` characters, its end kept: a title is often a
/// path, and the end says where.
fn shorten(label: &str, max: usize) -> String {
    let count = label.chars().count();
    if count <= max {
        return label.to_string();
    }
    let tail: String = label.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

struct Tab {
    id: String,
    screen: Screen,
    label: Option<String>,
    ended: bool,
    /// Opened by another page: shown, not started.
    attach: bool,
    /// Its open or attach has been asked, at the size its view fits.
    asked: bool,
}

pub struct Terminals {
    tabs: Vec<Tab>,
    active: Option<String>,
    style: Style,
    born: Instant,
    made: u32,
}

impl Terminals {
    /// `open` starts with a tab, as `LOCALGPT_APP_UI_OPEN=terminal` asks.
    pub fn new(ctx: &egui::Context, open: bool) -> Self {
        let mut terminals = Self {
            tabs: Vec::new(),
            active: None,
            style: Style::install(ctx, 13.0),
            born: Instant::now(),
            made: 0,
        };
        if open {
            terminals.open();
        }
        terminals
    }

    /// A tab id no other page has: a phone opens tabs too.
    fn new_id(&mut self) -> String {
        self.made += 1;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        format!("w{stamp:x}{}", self.made)
    }

    fn open(&mut self) {
        let id = self.new_id();
        self.tabs.push(Tab {
            id: id.clone(),
            screen: Screen::new(80, 24),
            label: None,
            ended: false,
            attach: false,
            asked: false,
        });
        self.active = Some(id);
    }

    fn tab(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// What the app said about the tabs since the last frame.
    pub fn take(&mut self, state: &mut State) {
        // Tabs this window does not have: a phone's, or this window's
        // before it was reopened. They are attached, and the app sends
        // their screens.
        for listed in &state.tabs {
            if !self.tabs.iter().any(|t| t.id == listed.id) {
                self.tabs.push(Tab {
                    id: listed.id.clone(),
                    screen: Screen::new(listed.cols, listed.rows),
                    label: listed.label.clone(),
                    ended: false,
                    attach: true,
                    asked: false,
                });
                if self.active.is_none() {
                    self.active = Some(listed.id.clone());
                }
            }
        }
        for event in state.terms.drain(..) {
            match event {
                TermEvent::Data(id, bytes) => {
                    if let Some(tab) = self.tab(&id) {
                        tab.screen.feed(&bytes);
                    }
                }
                TermEvent::Exit(id, code, error) => {
                    if let Some(tab) = self.tab(&id) {
                        tab.ended = true;
                        let why = error.unwrap_or_else(|| match code {
                            Some(code) => format!("exited with {code}"),
                            None => "ended".into(),
                        });
                        tab.screen
                            .feed(format!("\r\n\x1b[2m[{why}]\x1b[0m\r\n").as_bytes());
                    }
                }
                TermEvent::Size(id, cols, rows) => {
                    if let Some(tab) = self.tab(&id) {
                        tab.screen.resize(cols, rows);
                    }
                }
                TermEvent::Label(id, label) => {
                    if let Some(tab) = self.tab(&id) {
                        tab.label = label;
                    }
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, end: &ShellEnd) {
        let mut close: Option<String> = None;
        ui.horizontal(|ui| {
            for tab in &self.tabs {
                let dot = if tab.ended { "○" } else { "●" };
                let full = tab.label.as_deref().unwrap_or("Terminal");
                let label = format!("{dot} {}", shorten(full, 28));
                let selected = self.active.as_deref() == Some(tab.id.as_str());
                let response = ui.selectable_label(selected, label).on_hover_text(full);
                if response.clicked() {
                    self.active = Some(tab.id.clone());
                }
                if ui
                    .small_button("×")
                    .on_hover_text("Close this terminal")
                    .clicked()
                {
                    close = Some(tab.id.clone());
                }
                ui.separator();
            }
            if ui.button("+").on_hover_text("New terminal").clicked() {
                self.open();
            }
        });
        if let Some(id) = close {
            end.ask(FromPage::TermClose { id: id.clone() });
            self.tabs.retain(|t| t.id != id);
            if self.active.as_deref() == Some(id.as_str()) {
                self.active = self.tabs.last().map(|t| t.id.clone());
            }
        }
        ui.separator();

        let Some(active) = self.active.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.label("A terminal opens in this world's folder. Run your agent in it.");
                if ui.button("New terminal").clicked() {
                    self.open();
                }
            });
            return;
        };
        let blink = ((self.born.elapsed().as_secs_f32() * 2.0) as u64).is_multiple_of(2);
        let fits = Screen::fits(ui, &mut self.style);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == active) else {
            return;
        };
        if !tab.asked {
            tab.asked = true;
            let (cols, rows) = fits;
            tab.screen.resize(cols, rows);
            let id = tab.id.clone();
            end.ask(if tab.attach {
                FromPage::TermAttach { id, cols, rows }
            } else {
                FromPage::TermOpen { id, cols, rows }
            });
        }
        let id = ui.scope_id().with(("terminal", &tab.id));
        let asks = tab.screen.show(ui, id, &mut self.style, blink);
        if asks.focused {
            end.ask(FromPage::TermFocus { id: tab.id.clone() });
        }
        if let Some((cols, rows)) = asks.resized {
            end.ask(FromPage::TermResize {
                id: tab.id.clone(),
                cols,
                rows,
            });
        }
        if !asks.input.is_empty() {
            end.ask(FromPage::TermInput {
                id: tab.id.clone(),
                data: asks.input.concat(),
            });
        }
    }
}
