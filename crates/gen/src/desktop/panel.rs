//! The prompt panel: type prompts and follow the agent's work inside the Gen
//! window, so Gen works without a terminal.
//!
//! A resizable panel on the right shows the conversation (your prompts, the
//! model's replies, and each tool call with its status), a model menu, and
//! the prompt box. F2 hides and shows it; Enter from the 3D view jumps to the
//! prompt box, and Esc leaves it. While the box has focus,
//! [`hold_game_keys_while_typing`] keeps the typed keys from also moving the
//! camera or toggling the gallery.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPreUpdateSet, EguiPrimaryContextPass, egui};
use tokio::sync::mpsc;

use super::chat::{ChatEvent, PanelChannels};

pub(crate) const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x25, 0xc2, 0xa0);
pub(crate) const ERROR: egui::Color32 = egui::Color32::from_rgb(0xf2, 0x6d, 0x6d);

/// Starter prompts shown before the first message.
const EXAMPLES: [&str; 3] = [
    "A castle on a hill at sunset",
    "A cozy cabin in a snowy forest, with a crackling campfire",
    "An underwater temple with glowing coral",
];

/// World controls, as listed in the terminal's startup help.
const CONTROLS: [(&str, &str); 9] = [
    ("WASD", "Move"),
    ("Right-drag", "Look around"),
    ("Space / Shift", "Up / down (jump / run as avatar)"),
    ("Scroll", "Movement speed"),
    ("Tab", "Free-fly or avatar camera"),
    ("E", "Interact"),
    ("G", "World gallery"),
    ("F1", "Inspector"),
    ("F2", "Hide or show this panel"),
];

/// How the panel starts.
#[derive(Clone, Debug, Default)]
pub struct PanelSettings {
    /// Open at startup (desktop mode) rather than hidden until F2.
    pub open: bool,
    /// Put the cursor in the prompt box at startup.
    pub focus_input: bool,
    /// The config file, offered in the help section.
    pub config_file: Option<PathBuf>,
}

/// Adds the prompt panel. Add it after the gen app so the egui plugin the
/// inspector registers is reused.
pub struct PromptPanelPlugin {
    channels: Mutex<Option<PanelChannels>>,
    settings: PanelSettings,
}

impl PromptPanelPlugin {
    pub fn new(channels: PanelChannels, settings: PanelSettings) -> Self {
        Self {
            channels: Mutex::new(Some(channels)),
            settings,
        }
    }
}

impl Plugin for PromptPanelPlugin {
    fn build(&self, app: &mut App) {
        let channels = self
            .channels
            .lock()
            .expect("prompt panel channels lock poisoned")
            .take()
            .expect("PromptPanelPlugin built twice");

        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }

        app.insert_resource(PanelLink {
            prompt_tx: channels.prompt_tx,
            events_rx: Mutex::new(channels.events_rx),
        })
        .insert_resource(PromptPanel::new(&self.settings))
        .add_systems(
            PreUpdate,
            hold_game_keys_while_typing
                .after(EguiPreUpdateSet::ProcessInput)
                .before(EguiPreUpdateSet::BeginPass),
        )
        .add_systems(Update, receive_chat_events)
        .add_systems(EguiPrimaryContextPass, prompt_panel_ui);
    }
}

#[derive(Resource)]
struct PanelLink {
    prompt_tx: mpsc::UnboundedSender<String>,
    events_rx: Mutex<mpsc::UnboundedReceiver<ChatEvent>>,
}

/// Everything the panel shows.
#[derive(Resource, Debug, Default)]
pub struct PromptPanel {
    /// Whether the panel is showing.
    pub open: bool,
    input: String,
    focus_input: bool,
    entries: Vec<Entry>,
    model: Option<String>,
    model_options: Vec<String>,
    busy: bool,
    stopped: bool,
    show_help: bool,
    config_file: Option<PathBuf>,
    #[cfg(feature = "multiplayer")]
    collab: super::collab::CollabState,
}

#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Prompt {
        text: String,
        from: Option<String>,
        queued: bool,
    },
    Reply(String),
    Tool {
        name: String,
        detail: Option<String>,
        state: ToolState,
    },
    Notice(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
enum ToolState {
    Running,
    Done,
    Failed(String),
}

impl PromptPanel {
    fn new(settings: &PanelSettings) -> Self {
        Self {
            open: settings.open,
            focus_input: settings.open && settings.focus_input,
            config_file: settings.config_file.clone(),
            ..Default::default()
        }
    }

    /// Record a prompt typed here as queued. Returns the text to send, or
    /// `None` when it's blank.
    fn queue_prompt(&mut self, text: &str) -> Option<String> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        self.entries.push(Entry::Prompt {
            text: text.to_string(),
            from: None,
            queued: true,
        });
        Some(text.to_string())
    }

    /// Prompts typed here that the agent hasn't started yet.
    fn queued_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    Entry::Prompt {
                        from: None,
                        queued: true,
                        ..
                    }
                )
            })
            .count()
    }

    /// Fold one agent event into what the panel shows.
    fn apply(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::Ready { model } => self.model = Some(model),
            ChatEvent::ModelOptions(options) => self.model_options = options,
            ChatEvent::Prompt { text, from } => {
                self.busy = true;
                // Our own prompt started: it's the oldest one still queued.
                let ours = from.is_none().then(|| {
                    self.entries.iter_mut().find(|entry| {
                        matches!(
                            entry,
                            Entry::Prompt {
                                from: None,
                                queued: true,
                                ..
                            }
                        )
                    })
                });
                match ours.flatten() {
                    Some(Entry::Prompt { queued, .. }) => *queued = false,
                    _ => self.entries.push(Entry::Prompt {
                        text,
                        from,
                        queued: false,
                    }),
                }
            }
            ChatEvent::Delta(text) => match self.entries.last_mut() {
                Some(Entry::Reply(reply)) => reply.push_str(&text),
                _ if text.trim().is_empty() => {}
                _ => self
                    .entries
                    .push(Entry::Reply(text.trim_start().to_string())),
            },
            ChatEvent::ToolStarted { name, detail } => self.entries.push(Entry::Tool {
                name,
                detail,
                state: ToolState::Running,
            }),
            ChatEvent::ToolFinished { name, error } => {
                let running = self.entries.iter_mut().rev().find(|entry| {
                    matches!(
                        entry,
                        Entry::Tool { name: n, state: ToolState::Running, .. } if *n == name
                    )
                });
                if let Some(Entry::Tool { state, .. }) = running {
                    *state = match error {
                        None => ToolState::Done,
                        Some(error) => ToolState::Failed(error),
                    };
                }
            }
            ChatEvent::TurnFinished { error } => {
                self.busy = false;
                if let Some(error) = error {
                    let message = explain_model_error(self.model.as_deref(), &error);
                    self.entries.push(Entry::Error(message));
                }
            }
            ChatEvent::Notice(text) => self.entries.push(Entry::Notice(text)),
            ChatEvent::Warning(text) => self.entries.push(Entry::Error(text)),
            ChatEvent::Failed(error) => {
                self.busy = false;
                let message = explain_model_error(self.model.as_deref(), &error);
                self.entries.push(Entry::Error(message));
            }
        }
    }

    fn agent_stopped(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.busy = false;
            self.entries.push(Entry::Error(
                "The agent stopped. Restart Gen to keep building.".to_string(),
            ));
        }
    }
}

/// Turn a raw model error into something you can act on.
fn explain_model_error(model: Option<&str>, error: &str) -> String {
    let model = model.unwrap_or("The model");
    let lower = error.to_lowercase();
    if lower.contains("failed to spawn") {
        let program = model
            .split_once("-cli/")
            .map_or("the model's", |(program, _)| program);
        return format!(
            "Gen couldn't start the {program} CLI for {model}. Install it and sign in, then \
             restart Gen, or pick another model from the menu above.\n\n{error}"
        );
    }
    if lower.contains("api key")
        || lower.contains("api_key")
        || lower.contains("401")
        || lower.contains("unauthorized")
    {
        return format!(
            "{model} rejected the request. Check its API key in the config file (see ? above), \
             or pick another model from the menu above.\n\n{error}"
        );
    }
    format!("{model} failed: {error}")
}

/// "gen_spawn_primitive" → "spawn primitive".
fn pretty_tool_name(name: &str) -> String {
    name.strip_prefix("gen_").unwrap_or(name).replace('_', " ")
}

/// While a text field has focus, the keys being typed must not also move
/// the camera, toggle the gallery (G), or trigger interactions (E).
/// bevy_egui's own input absorption also swallows pointer input over any
/// panel (which breaks mouse-look), so this clears only the keyboard, and
/// only while typing.
pub(crate) fn hold_game_keys_while_typing(
    wants: Res<EguiWantsInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    if wants.wants_keyboard_input() {
        keys.reset_all();
    }
}

fn receive_chat_events(link: Res<PanelLink>, mut panel: ResMut<PromptPanel>) {
    let Ok(mut events) = link.events_rx.lock() else {
        return;
    };
    loop {
        match events.try_recv() {
            Ok(event) => panel.apply(event),
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                panel.agent_stopped();
                break;
            }
        }
    }
}

fn send(panel: &mut PromptPanel, link: &PanelLink, text: &str) {
    if let Some(prompt) = panel.queue_prompt(text)
        && link.prompt_tx.send(prompt).is_err()
    {
        panel.agent_stopped();
    }
}

fn prompt_panel_ui(
    mut contexts: EguiContexts,
    mut panel: ResMut<PromptPanel>,
    link: Res<PanelLink>,
    #[cfg(feature = "multiplayer")] host: Option<Res<crate::net::host::HostSessionInfo>>,
    #[cfg(feature = "multiplayer")] mut host_control: ResMut<crate::net::host::HostControl>,
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
        egui::Area::new(egui::Id::new("gen_prompt_reopen"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-14.0, -14.0])
            .show(ctx, |ui| {
                let label = if panel.busy {
                    "Working… (F2)"
                } else {
                    "Prompt (F2)"
                };
                if ui.button(label).clicked() {
                    panel.open = true;
                    panel.focus_input = true;
                }
            });
        return;
    }

    // Enter from the 3D view jumps to the prompt box.
    if !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
        panel.focus_input = true;
    }

    #[cfg(feature = "multiplayer")]
    let host_line = host.as_deref().map(|host| host.summary());
    #[cfg(not(feature = "multiplayer"))]
    let host_line: Option<String> = None;

    // bevy_egui hands systems a Context, and egui 0.34 deprecates top-level
    // panels in favor of show_inside(ui); the inspector panels do the same.
    #[allow(deprecated)]
    egui::Panel::right("gen_prompt_panel")
        .default_size(380.0)
        .min_size(300.0)
        .resizable(true)
        .show(ctx, |ui| {
            #[cfg(feature = "multiplayer")]
            let start_request = {
                let collab_args = Some((&*host_control, host.as_deref()));
                draw_panel(ui, panel, &link, host_line.as_deref(), collab_args)
            };
            #[cfg(not(feature = "multiplayer"))]
            draw_panel(ui, panel, &link, host_line.as_deref());

            #[cfg(feature = "multiplayer")]
            if let Some(request) = start_request {
                *host_control = crate::net::host::HostControl::StartRequested(request);
            }
        });
}

#[cfg(feature = "multiplayer")]
fn draw_panel(
    ui: &mut egui::Ui,
    panel: &mut PromptPanel,
    link: &PanelLink,
    _host_line: Option<&str>,
    collab_args: Option<(
        &crate::net::host::HostControl,
        Option<&crate::net::host::HostSessionInfo>,
    )>,
) -> Option<crate::net::host::HostStartRequest> {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("LocalGPT Gen").strong().size(16.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Hide")
                .on_hover_text("Hide this panel (F2)")
                .clicked()
            {
                panel.open = false;
            }
            if ui
                .small_button("?")
                .on_hover_text("Controls and settings")
                .clicked()
            {
                panel.show_help = !panel.show_help;
            }
        });
    });
    model_menu(ui, panel, link);

    // Collaborate section (replaces the old one-line host status).
    let start_request = if let Some((control, info)) = collab_args {
        super::collab::draw_collaborate(ui, &mut panel.collab, Some(control), info)
    } else {
        None
    };

    if panel.show_help {
        help(ui, panel);
    }
    ui.separator();

    draw_conversation(ui, panel, link);
    start_request
}

fn draw_conversation(ui: &mut egui::Ui, panel: &mut PromptPanel, link: &PanelLink) {
    // The conversation, leaving room for the prompt box below it.
    let reserved = 124.0;
    let mut example = None;
    egui::ScrollArea::vertical()
        .id_salt("gen_prompt_log")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .max_height((ui.available_height() - reserved).max(80.0))
        .show(ui, |ui| {
            if panel.entries.is_empty() {
                example = empty_state(ui);
            } else {
                for entry in &panel.entries {
                    draw_entry(ui, entry);
                }
            }
        });
    if let Some(example) = example {
        send(panel, link, example);
    }

    ui.separator();
    ui.horizontal(|ui| {
        if panel.busy {
            ui.spinner();
            ui.label(egui::RichText::new("Working…").small());
        }
        let queued = panel.queued_count();
        if queued > 0 {
            ui.label(
                egui::RichText::new(format!("{queued} queued"))
                    .small()
                    .weak(),
            );
        }
    });

    let output = egui::TextEdit::multiline(&mut panel.input)
        .id(egui::Id::new("gen_prompt_input"))
        .hint_text("Describe a place, or ask for a change…")
        .desired_rows(3)
        .desired_width(f32::INFINITY)
        // Plain Enter sends; Shift+Enter starts a new line.
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
        let can_send = !panel.input.trim().is_empty() && !panel.stopped;
        let clicked = ui
            .add_enabled(can_send, egui::Button::new("Send"))
            .clicked();
        ui.label(
            egui::RichText::new("Enter sends · Shift+Enter adds a line · Esc leaves the box")
                .small()
                .weak(),
        );
        if (clicked || enter_pressed) && can_send {
            let text = std::mem::take(&mut panel.input);
            send(panel, link, &text);
            panel.focus_input = true;
        }
    });
    ui.add_space(4.0);
}

#[cfg(not(feature = "multiplayer"))]
fn draw_panel(
    ui: &mut egui::Ui,
    panel: &mut PromptPanel,
    link: &PanelLink,
    host_line: Option<&str>,
) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("LocalGPT Gen").strong().size(16.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Hide")
                .on_hover_text("Hide this panel (F2)")
                .clicked()
            {
                panel.open = false;
            }
            if ui
                .small_button("?")
                .on_hover_text("Controls and settings")
                .clicked()
            {
                panel.show_help = !panel.show_help;
            }
        });
    });
    model_menu(ui, panel, link);
    if let Some(line) = host_line {
        ui.label(egui::RichText::new(line).small().color(ACCENT));
    }
    if panel.show_help {
        help(ui, panel);
    }
    ui.separator();

    draw_conversation(ui, panel, link);
}

fn model_menu(ui: &mut egui::Ui, panel: &mut PromptPanel, link: &PanelLink) {
    let current = panel
        .model
        .clone()
        .unwrap_or_else(|| "starting…".to_string());
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Model").small().weak());
        egui::ComboBox::from_id_salt("gen_model_menu")
            .selected_text(egui::RichText::new(&current).small())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                if panel.model_options.len() <= 1 {
                    ui.label(egui::RichText::new("No other models found").small().weak());
                }
                for option in &panel.model_options {
                    let selected = panel.model.as_deref() == Some(option.as_str());
                    if ui.selectable_label(selected, option.as_str()).clicked() && !selected {
                        chosen = Some(option.clone());
                    }
                }
            });
    });
    if let Some(model) = chosen {
        panel
            .entries
            .push(Entry::Notice(format!("Switching to {model}…")));
        if link.prompt_tx.send(format!("/model {model}")).is_err() {
            panel.agent_stopped();
        }
    }
}

fn help(ui: &mut egui::Ui, panel: &PromptPanel) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("gen_prompt_controls")
            .num_columns(2)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                for (keys, action) in CONTROLS {
                    ui.label(egui::RichText::new(keys).small().monospace());
                    ui.label(egui::RichText::new(action).small());
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Esc leaves the prompt box so the keys move you again; Enter jumps back. \
                 The model menu switches for this session; to change the default, set \
                 agent.default_model in the config file.",
            )
            .small()
            .weak(),
        );
        if let Some(path) = &panel.config_file
            && ui
                .small_button("Open config file")
                .on_hover_text(path.display().to_string())
                .clicked()
        {
            open_in_editor(path);
        }
    });
}

/// Starter prompts before the first message. Returns the one clicked.
fn empty_state(ui: &mut egui::Ui) -> Option<&'static str> {
    ui.add_space(8.0);
    ui.label("Describe a place and Gen builds it. Then walk in and keep asking for changes.");
    ui.add_space(10.0);
    ui.label(egui::RichText::new("Try one:").small().weak());
    let mut chosen = None;
    for example in EXAMPLES {
        if ui.add(egui::Button::new(example).wrap()).clicked() {
            chosen = Some(example);
        }
    }
    chosen
}

fn draw_entry(ui: &mut egui::Ui, entry: &Entry) {
    match entry {
        Entry::Prompt { text, from, queued } => {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(from.as_deref().unwrap_or("You")).strong());
                if *queued {
                    ui.label(egui::RichText::new("queued").small().weak());
                }
            });
            ui.label(text);
        }
        Entry::Reply(text) => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Gen").strong().color(ACCENT));
            ui.label(text.trim_end());
        }
        Entry::Tool {
            name,
            detail,
            state,
        } => {
            ui.horizontal_wrapped(|ui| {
                match state {
                    ToolState::Running => {
                        ui.add(egui::Spinner::new().size(10.0));
                    }
                    ToolState::Done => status_dot(ui, ACCENT),
                    ToolState::Failed(_) => status_dot(ui, ERROR),
                }
                ui.label(
                    egui::RichText::new(pretty_tool_name(name))
                        .small()
                        .monospace(),
                );
                if let Some(detail) = detail {
                    ui.label(egui::RichText::new(detail).small().weak());
                }
            });
            if let ToolState::Failed(error) = state {
                ui.label(egui::RichText::new(error).small().color(ERROR));
            }
        }
        Entry::Notice(text) => {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).small().italics().weak());
        }
        Entry::Error(text) => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(text).color(ERROR));
        }
    }
}

fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

/// Open a text file in the platform's default editor.
fn open_in_editor(path: &Path) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-t")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("notepad").arg(path).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = result {
        warn!("Couldn't open {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> PromptPanel {
        PromptPanel::new(&PanelSettings {
            open: true,
            ..Default::default()
        })
    }

    #[test]
    fn our_prompt_is_queued_until_the_agent_starts_it() {
        let mut panel = panel();
        assert_eq!(
            panel.queue_prompt("  a castle  ").as_deref(),
            Some("a castle")
        );
        assert_eq!(panel.queue_prompt("a moat"), Some("a moat".to_string()));
        assert_eq!(panel.queued_count(), 2);

        panel.apply(ChatEvent::Prompt {
            text: "a castle".into(),
            from: None,
        });
        assert!(panel.busy);
        assert_eq!(panel.queued_count(), 1);
        // Started in place, not echoed as a second entry.
        assert_eq!(panel.entries.len(), 2);
    }

    #[test]
    fn blank_prompts_are_not_sent() {
        let mut panel = panel();
        assert_eq!(panel.queue_prompt("   \n"), None);
        assert!(panel.entries.is_empty());
    }

    #[test]
    fn prompts_from_elsewhere_are_shown_with_their_sender() {
        let mut panel = panel();
        panel.apply(ChatEvent::Prompt {
            text: "a lighthouse".into(),
            from: Some("Friend".into()),
        });
        assert_eq!(
            panel.entries,
            [Entry::Prompt {
                text: "a lighthouse".into(),
                from: Some("Friend".into()),
                queued: false
            }]
        );
    }

    #[test]
    fn deltas_stream_into_one_reply_and_tools_split_replies() {
        let mut panel = panel();
        panel.apply(ChatEvent::Delta("\nRaising ".into()));
        panel.apply(ChatEvent::Delta("walls.".into()));
        panel.apply(ChatEvent::ToolStarted {
            name: "gen_spawn_primitive".into(),
            detail: Some("wall".into()),
        });
        panel.apply(ChatEvent::ToolFinished {
            name: "gen_spawn_primitive".into(),
            error: None,
        });
        panel.apply(ChatEvent::Delta("Done.".into()));

        assert_eq!(
            panel.entries,
            [
                Entry::Reply("Raising walls.".into()),
                Entry::Tool {
                    name: "gen_spawn_primitive".into(),
                    detail: Some("wall".into()),
                    state: ToolState::Done
                },
                Entry::Reply("Done.".into()),
            ]
        );
    }

    #[test]
    fn a_failed_tool_keeps_its_error() {
        let mut panel = panel();
        panel.apply(ChatEvent::ToolStarted {
            name: "gen_load_world".into(),
            detail: None,
        });
        panel.apply(ChatEvent::ToolFinished {
            name: "gen_load_world".into(),
            error: Some("no such world".into()),
        });
        assert!(matches!(
            &panel.entries[0],
            Entry::Tool { state: ToolState::Failed(e), .. } if e == "no such world"
        ));
    }

    #[test]
    fn turn_errors_are_explained_and_end_the_busy_state() {
        let mut panel = panel();
        panel.apply(ChatEvent::Ready {
            model: "claude-cli/opus".into(),
        });
        panel.apply(ChatEvent::Prompt {
            text: "a castle".into(),
            from: Some("Terminal".into()),
        });
        panel.apply(ChatEvent::TurnFinished {
            error: Some(
                "Failed to spawn Claude CLI: No such file or directory (os error 2)".into(),
            ),
        });
        assert!(!panel.busy);
        let Some(Entry::Error(message)) = panel.entries.last() else {
            panic!("expected an error entry");
        };
        assert!(message.contains("couldn't start the claude CLI"));
    }

    #[test]
    fn explains_common_model_failures() {
        assert!(
            explain_model_error(Some("anthropic/claude-sonnet-4-6"), "HTTP 401 Unauthorized")
                .contains("API key")
        );
        assert_eq!(
            explain_model_error(Some("ollama/llama3"), "connection refused"),
            "ollama/llama3 failed: connection refused"
        );
    }

    #[test]
    fn tool_names_read_like_words() {
        assert_eq!(pretty_tool_name("gen_spawn_primitive"), "spawn primitive");
        assert_eq!(pretty_tool_name("memory_search"), "memory search");
    }

    #[test]
    fn a_closed_link_is_reported_once() {
        let mut panel = panel();
        panel.agent_stopped();
        panel.agent_stopped();
        assert_eq!(panel.entries.len(), 1);
        assert!(panel.stopped);
    }
}
