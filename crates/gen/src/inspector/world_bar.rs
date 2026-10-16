//! The world bar on Bevy's own widgets.
//!
//! The egui bar (`world_info.rs`) rebuilt on `bevy_ui` + `bevy_feathers`,
//! behind the `feathers` feature: the spike the egui evaluation
//! (docs/rfcs/authoring/egui-or-not.md) asked for, to price moving the
//! rest. Same controls, the same widget names for the smoke harness
//! (`world_info.snap`, `world_info.review`, `menu.terminal here`, …), the
//! same intents into the same authority; the egui bar stays until this one
//! has passed the smoke for a while. What the port met is written up in the
//! RFC's spike section.
//!
//! feathers' widgets are controlled: a checkbox reports a `ValueChange` and
//! leaves `Checked` to the app, so the settings stay the truth and
//! [`sync_controls`] writes them back into the widgets each frame (the `Y`
//! and `L` keys change them too).

#![allow(deprecated)] // the `*_bundle` constructors; their BSN successors want the `bsn!` scene macro

use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{
    ButtonBundleProps, ButtonVariant, FeathersSliderProps, button_bundle, checkbox_bundle,
    slider_bundle,
};
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::rounded_corners::RoundedCorners;
use bevy::feathers::theme::{InheritableThemeTextColor, ThemedText, UiTheme};
use bevy::feathers::{FeathersCorePlugin, FeathersPlugins, tokens};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::{Checked, Pressed};
use bevy::ui_widgets::{Activate, SliderPrecision, SliderStep, SliderValue, ValueChange};
use bevy_egui::egui;
use serde_json::json;

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::BehaviorState;
use crate::gen3d::canvas_edit::{CanvasSettings, PersonIntent};
use crate::gen3d::herdr::AgentPresence;
use crate::gen3d::live::Live;
use crate::gen3d::plugin::{CurrentWorld, FlyCam};
use crate::gen3d::registry::NameRegistry;
use crate::ui::widgets::Widget;

use super::world_info::{BOOKMARKS, add_bookmark, drop_bookmark, tours_with};
use super::{InspectorMode, InspectorState, PanelIntents, PendingCameraPose};

pub(crate) const HEIGHT: f32 = 30.0;
const PANEL: Color = Color::srgba(0.09, 0.10, 0.13, 0.97);
const INK: Color = Color::srgb(0.90, 0.92, 0.95);
const MUTED: Color = Color::srgb(0.60, 0.65, 0.72);
const REFUSED: Color = Color::srgb(0.92, 0.47, 0.39);

pub struct WorldBarPlugin;

impl Plugin for WorldBarPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FeathersCorePlugin>() {
            app.add_plugins(FeathersPlugins);
        }
        app.insert_resource(UiTheme(create_dark_theme()))
            .init_resource::<OpenPopup>()
            .add_systems(Startup, spawn_bar)
            .add_systems(
                Update,
                (place_bar, refresh_texts, sync_controls, build_popups).chain(),
            )
            .add_observer(on_checkbox)
            .add_observer(on_slider)
            .add_observer(on_button);
    }
}

/// The bar's root.
#[derive(Component)]
struct WorldBar;

/// Shown while herdr's socket answers, or a pane is open.
#[derive(Component)]
struct WhenHerdr;

/// A text the bar keeps current — on the node itself, or on a button's
/// caption below it.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Name,
    Entities,
    Behaviors,
    Audio,
    Bookmarks,
    Status,
    Agent,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Snap,
    Local,
    Review,
}

impl Toggle {
    fn name(self) -> &'static str {
        match self {
            Self::Snap => "snap",
            Self::Local => "local",
            Self::Review => "review",
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Knob {
    Grid,
    Angle,
}

impl Knob {
    fn name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Angle => "angle",
        }
    }
}

/// What a button does.
#[derive(Component, Clone, PartialEq)]
enum Action {
    /// Open the bookmarks menu.
    Bookmarks,
    /// Open the agent menu, or bring the open pane to the front.
    Agent,
    FlyTo(usize),
    Drop(usize),
    AddBookmark,
    /// A pane in the world folder, running this (or just the shell).
    Open(Option<String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Popups {
    Bookmarks,
    Agent,
}

/// A menu above a button: shown while [`OpenPopup`] names it, rebuilt when
/// what it lists changes.
#[derive(Component)]
struct Popup {
    which: Option<Popups>,
    built_for: Option<usize>,
}

#[derive(Resource, Default)]
struct OpenPopup(Option<Popups>);

/// Any of the bar's own widgets (for the focus rule in [`sync_controls`]).
type BarControl = Or<(With<Toggle>, With<Knob>, With<Action>)>;

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

fn spawn_bar(
    mut commands: Commands,
    assets: Res<AssetServer>,
    live: Option<Res<Live>>,
    settings: Option<Res<CanvasSettings>>,
    presence: Option<Res<AgentPresence>>,
) {
    let font: Handle<Font> = assets.load(fonts::REGULAR);
    commands
        .spawn((
            WorldBar,
            Name::new("World bar"),
            Widget::new("panel.world_info"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Px(HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(PANEL),
            ZIndex(45),
            InheritableThemeTextColor(tokens::TEXT_MAIN),
            InheritableFont {
                font: font.clone(),
                font_size: FontSize::Px(12.0),
                weight: FontWeight::NORMAL,
            },
        ))
        .with_children(|bar| {
            bar.spawn((Slot::Name, themed("(unnamed world)")));
            divider(bar);
            bar.spawn((Slot::Entities, themed("0 entities")));
            divider(bar);
            bar.spawn((Slot::Behaviors, themed("behaviors")));
            bar.spawn((Slot::Audio, themed("")));
            let Some(live) = live.as_deref() else {
                return;
            };
            divider(bar);
            if let Some(settings) = settings.as_deref() {
                toggle(bar, Toggle::Snap, settings.snap);
                knob(bar, Knob::Grid, settings.grid_m, 0.05, 10.0, 0.05, 2);
                knob(bar, Knob::Angle, settings.angle_degrees, 1.0, 90.0, 1.0, 0);
                toggle(bar, Toggle::Local, settings.local_axes);
                divider(bar);
            }
            toggle(bar, Toggle::Review, live.review);
            divider(bar);
            button(
                bar,
                Action::Bookmarks,
                "world_info.bookmarks",
                "views 0",
                Some(Slot::Bookmarks),
                Some(Popups::Bookmarks),
            );
            bar.spawn((Slot::Status, plain("", &font, INK)));
            if presence.is_some() {
                bar.spawn((
                    WhenHerdr,
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|herdr| {
                    divider(herdr);
                    button(
                        herdr,
                        Action::Agent,
                        "world_info.agent",
                        "agent",
                        None,
                        Some(Popups::Agent),
                    );
                    herdr.spawn((Slot::Agent, plain("", &font, MUTED)));
                });
            }
        });
}

/// Text that takes the bar's themed font and colour.
fn themed(s: &str) -> impl Bundle {
    (
        ThemedText,
        Text::new(s.to_string()),
        TextLayout::default().with_no_wrap(),
    )
}

/// Text with its own colour, which the theme leaves alone.
fn plain(s: &str, font: &Handle<Font>, color: Color) -> impl Bundle {
    (
        Text::new(s.to_string()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(color),
        TextLayout::default().with_no_wrap(),
    )
}

fn divider(bar: &mut ChildSpawnerCommands) {
    bar.spawn((
        Node {
            width: Val::Px(1.0),
            height: Val::Px(16.0),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.16)),
    ));
}

fn toggle(bar: &mut ChildSpawnerCommands, which: Toggle, on: bool) {
    let mut entity = bar.spawn((
        which,
        Widget::new(format!("world_info.{}", which.name())),
        checkbox_bundle((), Spawn(themed(which.name()))),
    ));
    if on {
        entity.insert(Checked);
    }
}

/// A slider in a box of its own width: the feathers slider grows to fill
/// whatever holds it.
fn knob(
    bar: &mut ChildSpawnerCommands,
    which: Knob,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    precision: i32,
) {
    bar.spawn(Node {
        width: Val::Px(84.0),
        height: Val::Px(22.0),
        flex_direction: FlexDirection::Row,
        ..default()
    })
    .with_children(|holder| {
        holder.spawn((
            which,
            Widget::new(format!("world_info.{}", which.name())),
            SliderStep(step),
            SliderPrecision(precision),
            slider_bundle(FeathersSliderProps { value, min, max }, ()),
        ));
    });
}

/// A button in a box of its own width (the feathers button grows too),
/// with its popup — a menu above it — as a hidden child.
fn button(
    bar: &mut ChildSpawnerCommands,
    action: Action,
    widget: &str,
    caption: &str,
    slot: Option<Slot>,
    popup: Option<Popups>,
) {
    bar.spawn(Node {
        flex_direction: FlexDirection::Row,
        ..default()
    })
    .with_children(|holder| {
        let mut entity = holder.spawn((
            action,
            Widget::new(widget),
            button_bundle(
                ButtonBundleProps {
                    variant: ButtonVariant::Plain,
                    corners: RoundedCorners::All,
                },
                (),
                (
                    Spawn(themed(caption)),
                    Spawn((
                        Popup {
                            which: popup,
                            built_for: None,
                        },
                        Node {
                            position_type: PositionType::Absolute,
                            bottom: Val::Px(HEIGHT - 2.0),
                            left: Val::Px(0.0),
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            row_gap: Val::Px(4.0),
                            padding: UiRect::all(Val::Px(6.0)),
                            min_width: Val::Px(190.0),
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        ZIndex(46),
                    )),
                ),
            ),
        ));
        if let Some(slot) = slot {
            entity.insert(slot);
        }
    });
}

/// A menu's item.
fn item(menu: &mut ChildSpawnerCommands, action: Action, widget: &str, caption: &str) {
    menu.spawn((
        action,
        Widget::new(widget),
        button_bundle(
            ButtonBundleProps {
                variant: ButtonVariant::Normal,
                corners: RoundedCorners::All,
            },
            (),
            Spawn(themed(caption)),
        ),
    ));
}

// ---------------------------------------------------------------------------
// Keeping it current
// ---------------------------------------------------------------------------

/// Shown with the full inspector, across the window: egui draws over
/// `bevy_ui`, so [`reserve_space`] keeps the bar's strip free of egui's side
/// panels. The agent control shows while herdr is there to answer.
fn place_bar(
    state: Res<InspectorState>,
    presence: Option<Res<AgentPresence>>,
    mut bar: Query<&mut Node, With<WorldBar>>,
    mut herdr: Query<&mut Node, (With<WhenHerdr>, Without<WorldBar>)>,
) {
    let Ok(mut node) = bar.single_mut() else {
        return;
    };
    let display = if state.mode == InspectorMode::Full {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    let with_herdr = presence
        .as_deref()
        .is_some_and(|p| p.available() || p.pane.is_some());
    for mut node in &mut herdr {
        let display = if with_herdr {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// An empty, frameless egui panel the bar's height, drawn before the side
/// panels so they end above the bar. egui's panels take no pointer input
/// of their own (only its areas and widgets do), so the bar's widgets
/// under the strip still get their clicks.
#[allow(deprecated)] // `Panel::show(ctx)`: bevy_egui exposes a `Context`, not a root `Ui`
pub(crate) fn reserve_space(ctx: &egui::Context) -> egui::Rect {
    egui::Panel::bottom("inspector_world_bar_space")
        .exact_size(HEIGHT)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ctx, |_ui| {})
        .response
        .rect
}

#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn refresh_texts(
    slots: Query<(Entity, &Slot)>,
    mut texts: Query<(&mut Text, Option<&mut TextColor>)>,
    children: Query<&Children>,
    registry: Res<NameRegistry>,
    behavior_state: Res<BehaviorState>,
    audio: Option<Res<AudioEngine>>,
    current_world: Res<CurrentWorld>,
    live: Option<Res<Live>>,
    presence: Option<Res<AgentPresence>>,
    time: Res<Time>,
) {
    for (entity, slot) in &slots {
        let (value, color): (String, Option<Color>) = match slot {
            Slot::Name => (
                current_world
                    .name
                    .clone()
                    .unwrap_or_else(|| "(unnamed world)".to_string()),
                None,
            ),
            Slot::Entities => (format!("{} entities", registry.len()), None),
            Slot::Behaviors => (
                format!(
                    "behaviors: {} {:.1}s",
                    if behavior_state.paused {
                        "paused"
                    } else {
                        "running"
                    },
                    behavior_state.elapsed
                ),
                None,
            ),
            Slot::Audio => (
                audio
                    .as_deref()
                    .map(|engine| {
                        format!(
                            "audio: {} {} emitters",
                            if engine.active { "ON" } else { "OFF" },
                            engine.emitter_meta.len()
                        )
                    })
                    .unwrap_or_default(),
                None,
            ),
            Slot::Bookmarks => (format!("views {}", bookmark_count(live.as_deref())), None),
            Slot::Status => match live.as_deref().and_then(Live::status) {
                Some(status) => (
                    status.to_string(),
                    Some(if status.starts_with("refused") {
                        REFUSED
                    } else {
                        INK
                    }),
                ),
                None => (String::new(), None),
            },
            Slot::Agent => match presence.as_deref() {
                Some(presence) => match presence.line(time.elapsed_secs_f64()) {
                    Some((line, attention)) => {
                        (line, Some(if attention { REFUSED } else { MUTED }))
                    }
                    None => (presence.error.clone().unwrap_or_default(), Some(REFUSED)),
                },
                None => (String::new(), None),
            },
        };
        // The slot's own text, or its first descendant's (a button's caption).
        let target = if texts.contains(entity) {
            Some(entity)
        } else {
            children
                .iter_descendants(entity)
                .find(|e| texts.contains(*e))
        };
        if let Some(target) = target
            && let Ok((mut text, text_color)) = texts.get_mut(target)
        {
            if text.0 != value {
                text.0 = value;
            }
            if let (Some(color), Some(mut text_color)) = (color, text_color)
                && text_color.0 != color
            {
                text_color.0 = color;
            }
        }
    }
}

/// The settings are the truth; the widgets follow them (a `Y` or `L` key
/// press changes them as well as a click).
fn sync_controls(
    settings: Option<Res<CanvasSettings>>,
    live: Option<Res<Live>>,
    toggles: Query<(Entity, &Toggle, Has<Checked>)>,
    knobs: Query<(Entity, &Knob, &SliderValue, Has<Pressed>)>,
    controls: Query<(), BarControl>,
    mut focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    for (entity, toggle, checked) in &toggles {
        let want = match toggle {
            Toggle::Snap => settings.as_deref().map(|s| s.snap),
            Toggle::Local => settings.as_deref().map(|s| s.local_axes),
            Toggle::Review => live.as_deref().map(|l| l.review),
        };
        match want {
            Some(true) if !checked => {
                commands.entity(entity).insert(Checked);
            }
            Some(false) if checked => {
                commands.entity(entity).remove::<Checked>();
            }
            _ => {}
        }
    }
    for (entity, knob, value, pressed) in &knobs {
        if pressed {
            continue;
        }
        let Some(want) = settings.as_deref().map(|s| match knob {
            Knob::Grid => s.grid_m,
            Knob::Angle => s.angle_degrees,
        }) else {
            continue;
        };
        if (want - value.0).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(want));
        }
    }
    // A click leaves a widget focused, and Space — the fly camera's ascend
    // — would then toggle it as well: the bar keeps no focus.
    if let Some(focus) = focus.as_deref_mut()
        && let Some(focused) = focus.get()
        && controls.contains(focused)
    {
        focus.clear();
    }
}

fn build_popups(
    open: Res<OpenPopup>,
    live: Option<Res<Live>>,
    presence: Option<Res<AgentPresence>>,
    mut popups: Query<(Entity, &mut Popup, &mut Node)>,
    mut commands: Commands,
) {
    for (entity, mut popup, mut node) in &mut popups {
        let Some(which) = popup.which else {
            continue;
        };
        let is_open = open.0 == Some(which);
        let display = if is_open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if !is_open {
            popup.built_for = None;
            continue;
        }
        // Rebuilt when opened, and when what it lists changes.
        let key = match which {
            Popups::Bookmarks => bookmark_count(live.as_deref()),
            Popups::Agent => presence.as_deref().map_or(0, |p| p.commands.len() + 1),
        };
        if popup.built_for == Some(key) {
            continue;
        }
        popup.built_for = Some(key);
        commands.entity(entity).despawn_children();
        commands.entity(entity).with_children(|menu| match which {
            Popups::Bookmarks => {
                if let Some(tour) = live
                    .as_deref()
                    .and_then(|l| l.doc().tours.iter().find(|t| t.name == BOOKMARKS))
                {
                    for (n, waypoint) in tour.waypoints.iter().enumerate() {
                        let label = waypoint
                            .description
                            .clone()
                            .unwrap_or_else(|| format!("view {}", n + 1));
                        menu.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|row| {
                            item(
                                row,
                                Action::FlyTo(n),
                                &format!("menu.bookmark.{}", n + 1),
                                &label,
                            );
                            item(row, Action::Drop(n), &format!("menu.drop.{}", n + 1), "x");
                        });
                    }
                }
                item(
                    menu,
                    Action::AddBookmark,
                    "menu.bookmark this view",
                    "+ bookmark this view",
                );
            }
            Popups::Agent => {
                item(
                    menu,
                    Action::Open(None),
                    "menu.terminal here",
                    "terminal here",
                );
                for command in presence
                    .as_deref()
                    .map(|p| p.commands.clone())
                    .unwrap_or_default()
                {
                    item(
                        menu,
                        Action::Open(Some(command.clone())),
                        &format!("menu.{command} here"),
                        &format!("{command} here"),
                    );
                }
            }
        });
    }
}

fn bookmark_count(live: Option<&Live>) -> usize {
    live.and_then(|l| l.doc().tours.iter().find(|t| t.name == BOOKMARKS))
        .map_or(0, |t| t.waypoints.len())
}

// ---------------------------------------------------------------------------
// What the widgets say
// ---------------------------------------------------------------------------

fn on_checkbox(
    change: On<ValueChange<bool>>,
    toggles: Query<&Toggle>,
    mut settings: Option<ResMut<CanvasSettings>>,
    mut live: Option<ResMut<Live>>,
    mut commands: Commands,
) {
    let Ok(toggle) = toggles.get(change.source) else {
        return;
    };
    let value = change.value;
    match toggle {
        Toggle::Snap => {
            if let Some(settings) = settings.as_deref_mut() {
                settings.snap = value;
            }
        }
        Toggle::Local => {
            if let Some(settings) = settings.as_deref_mut() {
                settings.local_axes = value;
            }
        }
        Toggle::Review => {
            if let Some(live) = live.as_deref_mut() {
                live.review = value;
            }
        }
    }
    if value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
}

fn on_slider(
    change: On<ValueChange<f32>>,
    knobs: Query<&Knob>,
    mut settings: Option<ResMut<CanvasSettings>>,
    mut commands: Commands,
) {
    let Ok(knob) = knobs.get(change.source) else {
        return;
    };
    if let Some(settings) = settings.as_deref_mut() {
        match knob {
            Knob::Grid => settings.grid_m = change.value,
            Knob::Angle => settings.angle_degrees = change.value,
        }
    }
    commands
        .entity(change.source)
        .insert(SliderValue(change.value));
}

#[allow(clippy::too_many_arguments)] // a Bevy observer; the repo's convention
fn on_button(
    activate: On<Activate>,
    actions: Query<&Action>,
    mut open: ResMut<OpenPopup>,
    live: Option<Res<Live>>,
    mut intents: ResMut<PanelIntents>,
    mut pose: ResMut<PendingCameraPose>,
    camera: Query<&Transform, With<FlyCam>>,
    presence: Option<Res<AgentPresence>>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    let doc = live.as_deref().map(Live::doc);
    match action {
        Action::Bookmarks => {
            open.0 = (open.0 != Some(Popups::Bookmarks)).then_some(Popups::Bookmarks);
        }
        Action::Agent => match presence.as_deref() {
            Some(presence) if presence.pane.is_some() => presence.focus(),
            _ => open.0 = (open.0 != Some(Popups::Agent)).then_some(Popups::Agent),
        },
        Action::FlyTo(n) => {
            if let Some(waypoint) = doc
                .and_then(|d| d.tours.iter().find(|t| t.name == BOOKMARKS))
                .and_then(|t| t.waypoints.get(*n))
            {
                pose.pose = Some((
                    Vec3::from_array(waypoint.position),
                    Vec3::from_array(waypoint.look_at),
                ));
            }
            open.0 = None;
        }
        Action::Drop(n) => {
            if let Some(doc) = doc {
                intents.0.push(PersonIntent::World {
                    patch: json!({"tours": tours_with(doc, |tours| drop_bookmark(tours, *n))}),
                    what: format!("drop bookmark {}", n + 1),
                });
            }
            open.0 = None;
        }
        Action::AddBookmark => {
            if let (Some(doc), Ok(camera)) = (doc, camera.single()) {
                let position = camera.translation;
                let look_at = position + camera.forward() * 5.0;
                let count = bookmark_count(live.as_deref());
                intents.0.push(PersonIntent::World {
                    patch: json!({"tours": tours_with(doc, |tours| add_bookmark(tours, position, look_at, count + 1))}),
                    what: "bookmark a view".into(),
                });
            }
            open.0 = None;
        }
        Action::Open(command) => {
            if let Some(presence) = presence.as_deref() {
                presence.open(command.as_deref());
            }
            open.0 = None;
        }
    }
}
