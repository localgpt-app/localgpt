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
    /// The keys go to the tab in front when it next draws: one opened,
    /// picked or left in front by a hand here is typed into at once, as in
    /// any terminal, without a click in it first.
    focus: bool,
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
            focus: false,
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
        self.focus = true;
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
        let mut picked: Option<String> = None;
        let mut new = false;
        ui.horizontal(|ui| {
            // The tabs scroll when they are more than the panel holds, and
            // "+" stays at their end in reach: a strip wider than the panel
            // would widen the grid under it past what shows, its cursor
            // and its lines' ends out of sight.
            let plus = 2.0 * ui.spacing().button_padding.x + 16.0 + ui.spacing().item_spacing.x;
            egui::ScrollArea::horizontal()
                .id_salt("the terminals' tabs")
                .max_width((ui.available_width() - plus).max(0.0))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in &self.tabs {
                            let full = tab.label.as_deref().unwrap_or("Terminal");
                            let selected = self.active.as_deref() == Some(tab.id.as_str());
                            // The dot is painted, not a glyph: "●" is in
                            // none of egui's own fonts, and looking it up in
                            // the system's costs the first tab a hitch.
                            let dot = egui::IdSalt::new(("tab dot", &tab.id));
                            let shown = egui::Button::selectable(
                                selected,
                                (
                                    egui::Atom::custom(dot, egui::vec2(8.0, 8.0)),
                                    shorten(full, 28),
                                ),
                            )
                            .atom_ui(ui);
                            if let Some(at) = shown.rect(dot) {
                                let ink = ui
                                    .style()
                                    .interact_selectable(&shown, selected)
                                    .text_color();
                                let (center, radius) = (at.center(), 3.5);
                                if tab.ended {
                                    ui.painter().circle_stroke(
                                        center,
                                        radius,
                                        egui::Stroke::new(1.0, ink),
                                    );
                                } else {
                                    ui.painter().circle_filled(center, radius, ink);
                                }
                            }
                            let response = shown.response.on_hover_text(full);
                            if selected && self.focus {
                                // Just opened or picked: in view.
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                picked = Some(tab.id.clone());
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
                    });
                });
            new = ui.button("+").on_hover_text("New terminal").clicked();
        });
        if let Some(id) = picked {
            self.active = Some(id);
            self.focus = true;
        }
        if new {
            self.open();
        }
        if let Some(id) = close {
            end.ask(FromPage::TermClose { id: id.clone() });
            self.tabs.retain(|t| t.id != id);
            if self.active.as_deref() == Some(id.as_str()) {
                self.active = self.tabs.last().map(|t| t.id.clone());
                self.focus = true;
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
        let focus = std::mem::take(&mut self.focus);
        let asks = tab.screen.show(ui, id, &mut self.style, blink, focus);
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{Receiver, channel};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// The panel as the shell has it, made on its first frame (its fonts
    /// want a context), with the app's end of the link kept to read.
    struct Panel {
        terminals: Option<Terminals>,
        end: ShellEnd,
    }

    fn panel(width: f32) -> (Harness<'static, Panel>, Receiver<FromPage>) {
        let (asks, asked) = channel();
        let (_tell, events) = channel();
        let panel = Panel {
            terminals: None,
            end: ShellEnd { events, asks },
        };
        let harness = Harness::builder()
            .with_size(egui::vec2(width, 600.0))
            .build_ui_state(
                |ui, panel: &mut Panel| {
                    panel
                        .terminals
                        .get_or_insert_with(|| Terminals::new(ui.ctx(), false))
                        .ui(ui, &panel.end);
                },
                panel,
            );
        (harness, asked)
    }

    /// The tabs opened so far, in order, and the sizes they opened at.
    fn opened(asked: &[FromPage]) -> Vec<(String, u16)> {
        asked
            .iter()
            .filter_map(|ask| match ask {
                FromPage::TermOpen { id, cols, .. } => Some((id.clone(), *cols)),
                _ => None,
            })
            .collect()
    }

    fn typed_into(asked: &[FromPage], text: &str) -> Option<String> {
        asked.iter().find_map(|ask| match ask {
            FromPage::TermInput { id, data } if data == text => Some(id.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_new_terminal_is_typed_into_without_a_click() {
        let (mut harness, asked) = panel(480.0);
        harness.run_steps(2);
        harness.get_by_label("+").click();
        harness.run_steps(3);
        harness.event(egui::Event::Text("ls".into()));
        harness.run_steps(2);
        let asked: Vec<FromPage> = asked.try_iter().collect();
        let tabs = opened(&asked);
        assert_eq!(tabs.len(), 1, "one terminal opened");
        assert_eq!(typed_into(&asked, "ls"), Some(tabs[0].0.clone()));
    }

    #[test]
    fn a_tab_picked_takes_the_keys() {
        let (mut harness, asked) = panel(480.0);
        harness.run_steps(2);
        harness.get_by_label("+").click();
        harness.run_steps(3);
        harness.get_by_label("+").click();
        harness.run_steps(3);
        // Two tabs, the second in front; the first picked by its label.
        harness.get_all_by_label("Terminal").next().unwrap().click();
        harness.run_steps(3);
        harness.event(egui::Event::Text("pwd".into()));
        harness.run_steps(2);
        let asked: Vec<FromPage> = asked.try_iter().collect();
        let tabs = opened(&asked);
        assert_eq!(tabs.len(), 2);
        assert_eq!(typed_into(&asked, "pwd"), Some(tabs[0].0.clone()));
    }

    #[test]
    fn long_tab_labels_never_widen_the_grid_past_the_panel() {
        let (mut harness, asked) = panel(480.0);
        harness.run_steps(2);
        harness.get_by_label("+").click();
        harness.run_steps(3);
        let first = opened(&asked.try_iter().collect::<Vec<_>>())[0].clone();
        // Each tab titled with a long folder, as a shell's prompt sets it.
        let long = "/Users/someone/Library/Application Support/LocalGPT/worlds/harbour";
        for _ in 0..2 {
            harness.get_by_label("+").click();
            harness.run_steps(3);
            let ids: Vec<String> = harness
                .state()
                .terminals
                .as_ref()
                .unwrap()
                .tabs
                .iter()
                .map(|t| t.id.clone())
                .collect();
            let mut state = State {
                terms: ids
                    .into_iter()
                    .map(|id| TermEvent::Label(id, Some(long.into())))
                    .collect(),
                ..State::default()
            };
            harness
                .state_mut()
                .terminals
                .as_mut()
                .unwrap()
                .take(&mut state);
            harness.run_steps(3);
        }
        let asked: Vec<FromPage> = asked.try_iter().collect();
        let widest = asked
            .iter()
            .filter_map(|ask| match ask {
                FromPage::TermOpen { cols, .. } | FromPage::TermResize { cols, .. } => Some(*cols),
                _ => None,
            })
            .max()
            .unwrap();
        assert_eq!(opened(&asked).len(), 2);
        assert!(
            widest <= first.1,
            "a grid took {widest} columns where the panel holds {}",
            first.1
        );
    }
}
