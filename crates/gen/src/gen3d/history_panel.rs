//! The history panel: the log as a graph, with a scrubber and playback.
//!
//! The rail (`history_rail.rs`) lists the log as rows; this is its larger
//! form, opened with `H`, docked above the bottom bar: every entry a node
//! left to right in write order, one lane per branch with a line to its
//! parent — the log is a tree — the head and the entry on screen marked,
//! authors by colour, the turns herdr saw as ticks; the message of the
//! node under the pointer (or of the one on screen) below. A click on a
//! node seeks there, the scrubber under the graph seeks as it is dragged,
//! and play walks the entries in write order at a fixed pace or at the
//! pace they were written, so a person watches the world being made. Every
//! move is a seek — [`SeekTo`], the same path as `POST /goto` — so
//! nothing is written, on any world, git-backed or not.
//!
//! Plain `bevy_ui`, like the rail: in every build, with no egui and no
//! feathers. Widget names for the smoke harness: `panel.history`,
//! `history.node.<id>`, `history.base`, `history.turn.<n>`,
//! `history.scrub`, and the controls `history.first`, `history.prev`,
//! `history.play`, `history.next`, `history.last`, `history.speed`,
//! `history.pace`, `history.loop`, `history.close`.

use bevy::picking::events::{Click, Drag, Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, OverflowAxis, ScrollPosition, UiGlobalTransform};
use bevy::ui_widgets::ScrollArea;
use bevy::window::PrimaryWindow;

use super::live::{HistoryRow, LiveHistory, SeekTo, TurnMark};
use crate::inspector::{InspectorMode, InspectorState, PanelRects, panel_rect};
use crate::ui::widgets::{UiWidgetRects, Widget, rect_of};

/// The panel's height, above the bottom bar.
pub const HEIGHT: f32 = 150.0;
/// One entry's width in the graph.
const STEP: f32 = 18.0;
/// One branch lane's height.
const LANE: f32 = 16.0;
const NODE: f32 = 10.0;
const LEFT: f32 = 16.0;
const TOP: f32 = 12.0;

const PANEL: Color = Color::srgba(0.04, 0.05, 0.08, 0.92);
const INK: Color = Color::srgb(0.92, 0.94, 0.97);
const INK_MUTED: Color = Color::srgb(0.58, 0.64, 0.72);
const LINE: Color = Color::srgb(0.42, 0.48, 0.58);
const BASE: Color = Color::srgb(0.5, 0.55, 0.62);
const HEAD_RING: Color = Color::srgb(1.0, 0.85, 0.4);
const CURRENT_RING: Color = Color::srgb(0.98, 0.98, 1.0);
const TICK: Color = Color::srgb(0.95, 0.72, 0.4);
const BUTTON: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const BUTTON_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);
const TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
const THUMB: Color = Color::srgb(0.35, 0.7, 1.0);

pub struct HistoryPanelPlugin;

impl Plugin for HistoryPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HistoryPanel>()
            .init_resource::<Playback>()
            .add_systems(Startup, spawn_panel)
            .add_systems(
                Update,
                (
                    toggle_panel,
                    place_panel,
                    playback,
                    rebuild_graph,
                    hover_nodes,
                    button_hover,
                    refresh_texts,
                    scroll_to_current,
                )
                    .chain(),
            );
    }
}

/// The panel's state.
#[derive(Resource, Default)]
pub struct HistoryPanel {
    /// Shown in place of the rail.
    pub open: bool,
    /// The row under the pointer, for the status line.
    hovered: Option<usize>,
    /// What the graph was last built from.
    built: String,
    /// The graph's geometry: each row's node x, to keep the current in view.
    node_x: Vec<f32>,
    lanes: usize,
}

/// Play state: entries in write order, at a pace.
#[derive(Resource)]
pub struct Playback {
    pub playing: bool,
    /// Entries per second at a fixed pace; a multiplier at the real one.
    pub speed: f32,
    /// Keep the pauses the authors took, scaled by `speed`.
    pub real_pace: bool,
    pub looping: bool,
    since_last: f32,
    /// The seek asked for and not yet shown, so one lands before the next goes.
    pending: Option<Option<String>>,
    /// How long the pending seek has been waited for.
    waited: f32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            playing: false,
            speed: 1.0,
            real_pace: false,
            looping: false,
            since_last: 0.0,
            pending: None,
            waited: 0.0,
        }
    }
}

#[derive(Component)]
struct PanelRoot;
#[derive(Component)]
struct Graph;
#[derive(Component)]
struct GraphCanvas;
#[derive(Component)]
struct ScrubTrack;
#[derive(Component)]
struct ScrubThumb;
#[derive(Component)]
struct StatusText;
/// A node of the graph: a row's index, or `None` for the base.
#[derive(Component)]
struct GraphNode(Option<usize>);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Action {
    First,
    Prev,
    PlayPause,
    Next,
    Last,
    Speed,
    Pace,
    Loop,
    Close,
}

impl Action {
    fn widget(self) -> &'static str {
        match self {
            Self::First => "history.first",
            Self::Prev => "history.prev",
            Self::PlayPause => "history.play",
            Self::Next => "history.next",
            Self::Last => "history.last",
            Self::Speed => "history.speed",
            Self::Pace => "history.pace",
            Self::Loop => "history.loop",
            Self::Close => "history.close",
        }
    }
}

/// A button's caption, kept current by [`refresh_texts`].
#[derive(Component)]
struct ActionLabel(Action);

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s.to_string()),
        TextLayout::default().with_no_wrap(),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn spawn_panel(mut commands: Commands) {
    commands
        .spawn((
            PanelRoot,
            Name::new("History panel"),
            Widget::new("panel.history"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Px(HEIGHT),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Start,
                align_items: AlignItems::Stretch,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(PANEL),
            ZIndex(43),
        ))
        .with_children(|panel| {
            // The graph: a canvas as wide as the log, in a container that
            // scrolls sideways (the wheel, and the view follows the current).
            panel
                .spawn((
                    Graph,
                    ScrollArea,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        overflow: Overflow {
                            x: OverflowAxis::Scroll,
                            y: OverflowAxis::Clip,
                        },
                        ..default()
                    },
                ))
                .with_children(|graph| {
                    graph.spawn((
                        GraphCanvas,
                        Node {
                            position_type: PositionType::Relative,
                            width: Val::Px(200.0),
                            height: Val::Percent(100.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                });
            // The scrubber: the write order as a track, the thumb where the canvas stands.
            panel
                .spawn((
                    ScrubTrack,
                    Widget::new("history.scrub"),
                    Node {
                        height: Val::Px(12.0),
                        flex_shrink: 0.0,
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(TRACK),
                ))
                .observe(scrub_press)
                .observe(scrub_drag)
                .with_children(|track| {
                    track.spawn((
                        ScrubThumb,
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Px(10.0),
                            height: Val::Px(12.0),
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(THUMB),
                        bevy::picking::Pickable::IGNORE,
                    ));
                });
            // The controls, the close button at the right.
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|row| {
                    for (action, label) in [
                        (Action::First, "|<"),
                        (Action::Prev, "<"),
                        (Action::PlayPause, "play"),
                        (Action::Next, ">"),
                        (Action::Last, ">|"),
                        (Action::Speed, "1x"),
                        (Action::Pace, "fixed pace"),
                        (Action::Loop, "loop off"),
                    ] {
                        button(row, action, label);
                    }
                    row.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    button(row, Action::Close, "close (H)");
                });
            // The status line: the entry under the pointer, or the one on
            // screen. Clipped, never wrapped, so a long message cannot push
            // the controls off the panel.
            panel
                .spawn(Node {
                    height: Val::Px(16.0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((StatusText, text("", 12.0, INK_MUTED)));
                });
        });
}

fn button(row: &mut ChildSpawnerCommands, action: Action, label: &str) {
    row.spawn((
        Button,
        action,
        Widget::new(action.widget()),
        Node {
            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(BUTTON),
    ))
    .observe(move |click: On<Pointer<Click>>, mut acted: ActionParams| {
        if click.event.button == PointerButton::Primary {
            acted.act(action);
        }
    })
    .with_children(|b| {
        b.spawn((ActionLabel(action), text(label, 12.0, INK)));
    });
}

/// What the buttons reach.
#[derive(bevy::ecs::system::SystemParam)]
struct ActionParams<'w> {
    history: Res<'w, LiveHistory>,
    play: ResMut<'w, Playback>,
    panel: ResMut<'w, HistoryPanel>,
    seeks: MessageWriter<'w, SeekTo>,
}

impl ActionParams<'_> {
    fn act(&mut self, action: Action) {
        match action {
            Action::First => {
                self.play.playing = false;
                self.seeks.write(SeekTo(None));
            }
            Action::Prev => {
                self.play.playing = false;
                if let Some(target) = step(&self.history, -1) {
                    self.seeks.write(SeekTo(target));
                }
            }
            Action::Next => {
                self.play.playing = false;
                if let Some(target) = step(&self.history, 1) {
                    self.seeks.write(SeekTo(target));
                }
            }
            Action::Last => {
                self.play.playing = false;
                if let Some(last) = self.history.rows.last() {
                    self.seeks.write(SeekTo(Some(last.id.clone())));
                }
            }
            Action::PlayPause => {
                self.play.playing = !self.play.playing;
                self.play.since_last = 0.0;
                self.play.pending = None;
                self.play.waited = 0.0;
                // Play from the end starts over.
                if self.play.playing
                    && position(&self.history) == self.history.rows.len()
                    && !self.history.rows.is_empty()
                {
                    self.seeks.write(SeekTo(None));
                }
            }
            Action::Speed => {
                self.play.speed = match self.play.speed {
                    s if s < 2.0 => 2.0,
                    s if s < 4.0 => 4.0,
                    s if s < 8.0 => 8.0,
                    _ => 1.0,
                };
            }
            Action::Pace => self.play.real_pace = !self.play.real_pace,
            Action::Loop => self.play.looping = !self.play.looping,
            Action::Close => self.panel.open = false,
        }
    }
}

/// `H` opens and closes the panel, unless a text field has the keyboard.
fn toggle_panel(
    keys: Res<ButtonInput<KeyCode>>,
    typing: crate::ui::focus::TypingState,
    mut panel: ResMut<HistoryPanel>,
) {
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if keys.just_pressed(KeyCode::KeyH) && !command && !typing.typing() {
        panel.open = !panel.open;
    }
}

/// Above the bottom bar, between the inspector's side panels.
fn place_panel(
    panel: Res<HistoryPanel>,
    state: Option<Res<InspectorState>>,
    egui: Option<Res<PanelRects>>,
    seam: Res<UiWidgetRects>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut root: Query<&mut Node, With<PanelRoot>>,
) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let display = if panel.open {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    let (width, height) = window
        .single()
        .map_or((1280.0, 720.0), |w| (w.width(), w.height()));
    let shown = state
        .as_deref()
        .is_some_and(|s| s.mode != InspectorMode::Hidden);
    let rect = |name: &str| match &egui {
        Some(egui) => panel_rect(name, egui, &seam),
        None => seam.0.get(name).copied(),
    };
    let left = match rect("panel.outliner") {
        Some([x, _, w, _]) if shown => x + w,
        _ => 0.0,
    };
    let right = match rect("panel.detail") {
        Some([x, _, _, _]) if shown => (width - x).max(0.0),
        _ => 0.0,
    };
    let bottom = rect("panel.world_info").map_or(0.0, |[_, y, _, _]| (height - y).max(0.0));
    let want = (Val::Px(left), Val::Px(right), Val::Px(bottom));
    if (node.left, node.right, node.bottom) != want {
        (node.left, node.right, node.bottom) = want;
    }
}

/// Which row the canvas shows, as a position in write order: 0 is the
/// base, `n` the n-th entry.
fn position(history: &LiveHistory) -> usize {
    history
        .current
        .as_deref()
        .and_then(|c| history.rows.iter().position(|r| r.id == c))
        .map_or(0, |i| i + 1)
}

/// The seek one step back (-1) or forward (+1) in write order asks for;
/// `None` when there is nowhere to go, else the target (itself `None` for
/// the base).
fn step(history: &LiveHistory, by: isize) -> Option<Option<String>> {
    let there = position(history) as isize + by;
    if there < 0 || there > history.rows.len() as isize {
        return None;
    }
    Some(if there == 0 {
        None
    } else {
        Some(history.rows[there as usize - 1].id.clone())
    })
}

/// A lane per branch: a row continues the lane whose tip is its parent;
/// a row whose parent has moved on (a fork) opens a new lane. Lane 0
/// starts at the base.
fn lanes(rows: &[HistoryRow]) -> (Vec<usize>, usize) {
    // Each lane's tip: `None` is the base, before any entry.
    let mut tips: Vec<Option<String>> = vec![None];
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let lane = match tips
            .iter()
            .position(|tip| tip.as_deref() == row.parent.as_deref())
        {
            Some(lane) => lane,
            None => {
                tips.push(None);
                tips.len() - 1
            }
        };
        tips[lane] = Some(row.id.clone());
        out.push(lane);
    }
    (out, tips.len())
}

/// A short, recognisable form of an entry id, as the rail shows it.
fn short_id(id: &str) -> String {
    match id.strip_prefix("sha256:") {
        Some(hex) => hex.chars().take(7).collect(),
        None => id.to_string(),
    }
}

/// An author's colour: the same name, the same hue.
fn author_color(name: &str) -> Color {
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    Color::hsl((hash % 360) as f32, 0.55, 0.62)
}

/// Rebuilds the graph when the log, the current entry or the turns change.
fn rebuild_graph(
    history: Res<LiveHistory>,
    mut panel: ResMut<HistoryPanel>,
    canvas: Query<Entity, With<GraphCanvas>>,
    mut canvas_node: Query<&mut Node, With<GraphCanvas>>,
    mut thumb: Query<&mut Node, (With<ScrubThumb>, Without<GraphCanvas>)>,
    track: Query<&ComputedNode, With<ScrubTrack>>,
    mut commands: Commands,
) {
    let Ok(canvas) = canvas.single() else {
        return;
    };
    // The thumb follows the current every frame (the track's width is the
    // layout's, known a frame late).
    let n = history.rows.len();
    if let (Ok(mut thumb), Ok(track)) = (thumb.single_mut(), track.single()) {
        let width = track.size().x * track.inverse_scale_factor();
        let fraction = if n == 0 {
            0.0
        } else {
            position(&history) as f32 / n as f32
        };
        let left = (fraction * (width - 10.0).max(0.0)).max(0.0);
        if thumb.left != Val::Px(left) {
            thumb.left = Val::Px(left);
        }
    }
    if !panel.open {
        panel.built.clear();
        return;
    }
    let key = format!(
        "{:?}|{:?}|{:?}",
        history
            .rows
            .iter()
            .map(|r| (&r.id, r.head, r.tip))
            .collect::<Vec<_>>(),
        history.current,
        history
            .turns
            .iter()
            .map(|t| (t.n, t.rows))
            .collect::<Vec<_>>()
    );
    if panel.built == key {
        return;
    }
    panel.built = key;

    let (lane_of, lane_count) = lanes(&history.rows);
    let x_of = |i: usize| LEFT + (i as f32 + 1.0) * STEP;
    let y_of = |lane: usize| TOP + lane as f32 * LANE;
    panel.node_x = (0..n).map(x_of).collect();
    panel.lanes = lane_count;
    if let Ok(mut node) = canvas_node.single_mut() {
        let width = x_of(n) + LEFT;
        if node.width != Val::Px(width) {
            node.width = Val::Px(width);
        }
    }
    let current = history
        .current
        .as_deref()
        .and_then(|c| history.rows.iter().position(|r| r.id == c));
    let index_of = |id: &str| history.rows.iter().position(|r| r.id == id);

    commands.entity(canvas).despawn_children();
    commands.entity(canvas).with_children(|c| {
        // Lines first, nodes over them.
        for (i, row) in history.rows.iter().enumerate() {
            let (px, py) = match row.parent.as_deref().and_then(index_of) {
                Some(p) => (x_of(p), y_of(lane_of[p])),
                None => (LEFT, y_of(0)),
            };
            let (x, y) = (x_of(i), y_of(lane_of[i]));
            // Along the parent's lane, then down (or up) to this one.
            line(c, px, py - 1.0, x - px, 2.0);
            if (y - py).abs() > 0.5 {
                let (top, h) = if y > py { (py, y - py) } else { (y, py - y) };
                line(c, x - 1.0, top, 2.0, h);
            }
        }
        // The base.
        node(
            c,
            None,
            LEFT,
            y_of(0),
            BASE,
            current.is_none(),
            false,
            "history.base",
        );
        for (i, row) in history.rows.iter().enumerate() {
            node(
                c,
                Some(i),
                x_of(i),
                y_of(lane_of[i]),
                author_color(&row.author),
                current == Some(i),
                row.head,
                &format!("history.node.{}", short_id(&row.id)),
            );
        }
        // The turns herdr saw end: a tick between the entries.
        for mark in &history.turns {
            let x = if mark.rows == 0 {
                LEFT + STEP / 2.0
            } else {
                x_of(mark.rows - 1) + STEP / 2.0
            };
            tick(c, mark, x, y_of(lane_count.max(1) - 1) + LANE / 2.0);
        }
    });
}

fn line(c: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    c.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x),
            top: Val::Px(y),
            width: Val::Px(w.max(0.0)),
            height: Val::Px(h.max(0.0)),
            ..default()
        },
        BackgroundColor(LINE),
        bevy::picking::Pickable::IGNORE,
    ));
}

#[allow(clippy::too_many_arguments)]
fn node(
    c: &mut ChildSpawnerCommands,
    index: Option<usize>,
    x: f32,
    y: f32,
    color: Color,
    current: bool,
    head: bool,
    widget: &str,
) {
    let size = if current { NODE + 4.0 } else { NODE };
    let border = if current {
        CURRENT_RING
    } else if head {
        HEAD_RING
    } else {
        Color::NONE
    };
    c.spawn((
        Button,
        GraphNode(index),
        Widget::new(widget),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x - size / 2.0),
            top: Val::Px(y - size / 2.0),
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(size / 2.0)),
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(border),
        ZIndex(1),
    ))
    .observe(
        move |click: On<Pointer<Click>>,
              history: Res<LiveHistory>,
              mut play: ResMut<Playback>,
              mut seeks: MessageWriter<SeekTo>| {
            if click.event.button != PointerButton::Primary {
                return;
            }
            play.playing = false;
            seeks.write(SeekTo(
                index.and_then(|i| history.rows.get(i).map(|r| r.id.clone())),
            ));
        },
    );
}

fn tick(c: &mut ChildSpawnerCommands, mark: &TurnMark, x: f32, bottom: f32) {
    c.spawn((
        Widget::new(format!("history.turn.{}", mark.n)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x - 1.0),
            top: Val::Px(2.0),
            width: Val::Px(2.0),
            height: Val::Px((bottom - 2.0).max(4.0)),
            ..default()
        },
        BackgroundColor(TICK),
        bevy::picking::Pickable::IGNORE,
    ));
}

/// The node under the pointer, for the status line.
fn hover_nodes(
    nodes: Query<(&Interaction, &GraphNode), Changed<Interaction>>,
    mut panel: ResMut<HistoryPanel>,
) {
    for (interaction, node) in &nodes {
        match interaction {
            Interaction::Hovered | Interaction::Pressed => panel.hovered = node.0,
            Interaction::None if panel.hovered == node.0 => panel.hovered = None,
            Interaction::None => {}
        }
    }
}

fn button_hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor, &Action), Changed<Interaction>>,
) {
    for (interaction, mut color, _) in &mut buttons {
        color.0 = if *interaction == Interaction::None {
            BUTTON
        } else {
            BUTTON_HOVER
        };
    }
}

/// The captions that change, and the status line.
fn refresh_texts(
    history: Res<LiveHistory>,
    play: Res<Playback>,
    panel: Res<HistoryPanel>,
    mut labels: Query<(&ActionLabel, &mut Text), Without<StatusText>>,
    mut status: Query<&mut Text, With<StatusText>>,
) {
    for (label, mut text) in &mut labels {
        let want = match label.0 {
            Action::PlayPause => {
                if play.playing {
                    "pause".to_string()
                } else {
                    "play".to_string()
                }
            }
            Action::Speed => format!("{}x", play.speed as u32),
            Action::Pace => {
                if play.real_pace {
                    "real pace".to_string()
                } else {
                    "fixed pace".to_string()
                }
            }
            Action::Loop => {
                if play.looping {
                    "loop on".to_string()
                } else {
                    "loop off".to_string()
                }
            }
            _ => continue,
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    let Ok(mut text) = status.single_mut() else {
        return;
    };
    let shown = panel
        .hovered
        .or_else(|| {
            history
                .current
                .as_deref()
                .and_then(|c| history.rows.iter().position(|r| r.id == c))
        })
        .and_then(|i| history.rows.get(i));
    let want = match shown {
        Some(row) => format!(
            "r{} of {} - {} - {}",
            row.revision,
            history.rows.len(),
            row.author,
            row.message.clone().unwrap_or_else(|| row.summary.clone())
        ),
        None if history.rows.is_empty() => "no entries yet".to_string(),
        None => format!("the base - {} entries ahead", history.rows.len()),
    };
    if text.0 != want {
        text.0 = want;
    }
}

/// Play: the next entry in write order, at the pace; at the end, stop or
/// start over.
fn playback(
    time: Res<Time>,
    history: Res<LiveHistory>,
    mut play: ResMut<Playback>,
    mut seeks: MessageWriter<SeekTo>,
) {
    if !play.playing {
        return;
    }
    // The last seek has to show before the next goes; one that never
    // lands (refused: a proposal pending, a drag held) ends the play.
    if let Some(pending) = &play.pending {
        if history.current != *pending {
            play.waited += time.delta_secs();
            if play.waited > 2.0 {
                play.playing = false;
                play.pending = None;
            }
            return;
        }
        play.pending = None;
        play.waited = 0.0;
    }
    play.since_last += time.delta_secs();
    let at = position(&history);
    let n = history.rows.len();
    let interval = if play.real_pace && at > 0 && at < n {
        let gap_ms = history.rows[at]
            .timestamp_ms
            .saturating_sub(history.rows[at - 1].timestamp_ms);
        ((gap_ms as f32 / 1000.0) / play.speed).clamp(0.05, 3.0)
    } else {
        0.6 / play.speed
    };
    if play.since_last < interval {
        return;
    }
    play.since_last = 0.0;
    if at >= n {
        if play.looping && n > 0 {
            play.pending = Some(None);
            seeks.write(SeekTo(None));
        } else {
            play.playing = false;
        }
        return;
    }
    let target = Some(history.rows[at].id.clone());
    play.pending = Some(target.clone());
    seeks.write(SeekTo(target));
}

/// The graph keeps the current node in view.
fn scroll_to_current(
    history: Res<LiveHistory>,
    panel: Res<HistoryPanel>,
    mut graph: Query<(&ComputedNode, &mut ScrollPosition), With<Graph>>,
    mut last: Local<Option<Option<String>>>,
) {
    if !panel.open || last.as_ref() == Some(&history.current) {
        return;
    }
    *last = Some(history.current.clone());
    let Ok((node, mut scroll)) = graph.single_mut() else {
        return;
    };
    let width = node.size().x * node.inverse_scale_factor();
    let at = position(&history);
    let x = match at {
        0 => LEFT,
        _ => panel.node_x.get(at - 1).copied().unwrap_or(LEFT),
    };
    let want = (x - width / 2.0).max(0.0);
    if (scroll.x - want).abs() > 1.0 {
        scroll.x = want;
    }
}

/// A press on the track seeks to the entry under it.
fn scrub_press(
    press: On<Pointer<Press>>,
    track: Query<(&ComputedNode, &UiGlobalTransform), With<ScrubTrack>>,
    history: Res<LiveHistory>,
    mut play: ResMut<Playback>,
    mut seeks: MessageWriter<SeekTo>,
) {
    if press.event.button != PointerButton::Primary {
        return;
    }
    play.playing = false;
    scrub_to(
        press.pointer_location.position.x,
        &track,
        &history,
        &mut seeks,
    );
}

/// A drag along the track seeks as it goes.
fn scrub_drag(
    drag: On<Pointer<Drag>>,
    track: Query<(&ComputedNode, &UiGlobalTransform), With<ScrubTrack>>,
    history: Res<LiveHistory>,
    mut seeks: MessageWriter<SeekTo>,
) {
    if drag.event.button != PointerButton::Primary {
        return;
    }
    scrub_to(
        drag.pointer_location.position.x,
        &track,
        &history,
        &mut seeks,
    );
}

fn scrub_to(
    pointer_x: f32,
    track: &Query<(&ComputedNode, &UiGlobalTransform), With<ScrubTrack>>,
    history: &LiveHistory,
    seeks: &mut MessageWriter<SeekTo>,
) {
    let Ok((node, transform)) = track.single() else {
        return;
    };
    let Some([x, _, w, _]) = rect_of(node, transform) else {
        return;
    };
    let n = history.rows.len();
    if n == 0 || w <= 0.0 {
        return;
    }
    let fraction = ((pointer_x - x) / w).clamp(0.0, 1.0);
    let want = (fraction * n as f32).round() as usize;
    if want == position(history) {
        return;
    }
    seeks.write(SeekTo(if want == 0 {
        None
    } else {
        Some(history.rows[want - 1].id.clone())
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, parent: Option<&str>, revision: u64) -> HistoryRow {
        HistoryRow {
            id: id.into(),
            parent: parent.map(str::to_string),
            revision,
            author: "a".into(),
            summary: String::new(),
            message: None,
            timestamp_ms: revision * 1000,
            tip: false,
            head: false,
        }
    }

    #[test]
    fn a_linear_log_is_one_lane() {
        let rows = vec![
            row("e1", None, 1),
            row("e2", Some("e1"), 2),
            row("e3", Some("e2"), 3),
        ];
        assert_eq!(lanes(&rows), (vec![0, 0, 0], 1));
    }

    #[test]
    fn a_fork_opens_a_lane_and_each_branch_keeps_its_own() {
        let rows = vec![
            row("e1", None, 1),
            row("e2", Some("e1"), 2),
            row("e3", Some("e1"), 3), // a branch from e1, after e2 moved on
            row("e4", Some("e3"), 4), // continues the branch
            row("e5", Some("e2"), 5), // main again
        ];
        assert_eq!(lanes(&rows), (vec![0, 0, 1, 1, 0], 2));
    }

    #[test]
    fn stepping_walks_write_order_with_the_base_first() {
        let rows = vec![row("e1", None, 1), row("e2", Some("e1"), 2)];
        let mut history = LiveHistory {
            rows,
            ..Default::default()
        };
        assert_eq!(position(&history), 0);
        assert_eq!(step(&history, -1), None);
        assert_eq!(step(&history, 1), Some(Some("e1".into())));
        history.current = Some("e2".into());
        assert_eq!(position(&history), 2);
        assert_eq!(step(&history, 1), None);
        assert_eq!(step(&history, -1), Some(Some("e1".into())));
    }
}
