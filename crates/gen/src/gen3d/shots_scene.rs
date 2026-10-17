//! The shot strip on Bevy's own widgets.
//!
//! The egui window in `shots.rs` as a `bevy_ui` panel, with the `feathers`
//! feature: the shots in order, each a card with its rendered thumbnail,
//! name and size, lens and clock, description; a click looks through the
//! shot; a card dragged onto the gap between two others reorders the
//! list (the same `reorder_ops`, through the same authority); the board
//! buttons at the bottom. Written as scenes, like the review bar and the
//! gallery; the rows are rebuilt when what they show changes.
//!
//! Widget names for the smoke harness: `panel.shots`, `shots.row.<name>`,
//! `shots.zone.<n>` (the gap before row `n`), `shots.board`,
//! `shots.compare`.

use bevy::ecs::template::template;
use bevy::feathers::constants::fonts;
use bevy::feathers::controls::FeathersButton;
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::{InheritableThemeTextColor, ThemedText};
use bevy::feathers::tokens;
use bevy::picking::events::{Click, DragDrop, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;

use super::live::{LiveEndpoint, LiveHistory};
use super::shots::{
    LiveShots, LookThrough, ShotOps, ShotThumbs, ShotView, board_url, open_in_browser, reorder_ops,
    trim_seconds,
};
use crate::inspector::{InspectorMode, InspectorState, PanelRects};
use crate::ui::widgets::Widget;

const PANEL: Color = Color::srgba(0.10, 0.11, 0.14, 0.96);
const CARD: Color = Color::srgba(0.16, 0.16, 0.16, 0.9);
const ACTIVE: Color = Color::srgba(0.24, 0.31, 0.47, 0.9);
const DARK: Color = Color::srgb(0.09, 0.09, 0.09);
const MUTED: Color = Color::srgb(0.6, 0.65, 0.72);
const SIZE: Color = Color::srgb(0.68, 0.85, 1.0);
const GAP: Color = Color::srgba(0.35, 0.7, 1.0, 0.0);
const GAP_HOT: Color = Color::srgba(0.35, 0.7, 1.0, 0.8);

pub struct ShotsScenePlugin;

impl Plugin for ShotsScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StripDrag>()
            .add_systems(Startup, spawn_strip)
            .add_systems(
                Update,
                (place_strip, rebuild_strip, end_drag, light_gaps).chain(),
            );
    }
}

/// The panel.
#[derive(Component, Default, Clone)]
struct StripRoot;

/// The column the cards and gaps go in.
#[derive(Component, Default, Clone)]
struct StripList;

/// A shot's card: its camera entity id, and its place in the list.
#[derive(Component, Default, Clone)]
struct ShotCard {
    entity_id: u64,
    index: usize,
}

/// The gap before row `n` (`shots.len()`: after the last).
#[derive(Component, Default, Clone)]
struct DropZone(usize);

/// The card being dragged, so the release that ends the drag is not
/// taken as a click on it.
#[derive(Resource, Default)]
struct StripDrag(Option<Entity>);

fn spawn_strip(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        StripRoot
        Name({"Shot strip".to_string()})
        Widget({"panel.shots".to_string()})
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(12.0),
            width: Val::Px(320.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(4.0),
            padding: UiRect::all(Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
        }
        BackgroundColor(PANEL)
        ZIndex(42)
        InheritableThemeTextColor(tokens::TEXT_MAIN)
        InheritableFont {
            font: fonts::REGULAR,
            font_size: FontSize::Px(13.0),
            weight: FontWeight::NORMAL,
        }
        Children [
            (
                StripList
                Node {
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Start,
                    align_items: AlignItems::Stretch,
                }
            ),
        ]
    });
}

/// Top left of what the inspector's side panels leave free, below the
/// review bar when one is up.
fn place_strip(
    rects: Res<PanelRects>,
    state: Res<InspectorState>,
    seam: Res<crate::ui::widgets::UiWidgetRects>,
    mut root: Query<&mut Node, With<StripRoot>>,
) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let mut left = 12.0;
    if state.mode != InspectorMode::Hidden
        && let Some([x, _, w, _]) = crate::inspector::panel_rect("panel.outliner", &rects, &seam)
    {
        left = x + w + 12.0;
    }
    let mut top = 12.0;
    if let Some([_, y, _, h]) = seam.0.get("panel.review") {
        top = y + h + 12.0;
    }
    if node.left != Val::Px(left) {
        node.left = Val::Px(left);
    }
    if node.top != Val::Px(top) {
        node.top = Val::Px(top);
    }
}

/// The rows, rebuilt when the shots, the look-through, the thumbnails or
/// the boards' tips change; hidden when the world has no shots.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn rebuild_strip(
    shots: Res<LiveShots>,
    view: Res<ShotView>,
    thumbs: Res<ShotThumbs>,
    history: Option<Res<LiveHistory>>,
    endpoint: Option<Res<LiveEndpoint>>,
    mut root: Query<&mut Node, With<StripRoot>>,
    list: Query<Entity, With<StripList>>,
    mut built: Local<String>,
    mut commands: Commands,
) {
    let (Ok(mut node), Ok(list)) = (root.single_mut(), list.single()) else {
        return;
    };
    let display = if shots.shots.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    if node.display != display {
        node.display = display;
    }
    if shots.shots.is_empty() {
        built.clear();
        return;
    }
    let current = history.as_ref().and_then(|h| h.current.clone());
    let head = history
        .as_ref()
        .and_then(|h| h.rows.iter().find(|r| r.head).map(|r| r.id.clone()));
    // What the rows show, in one line: a change rebuilds them.
    let key = format!(
        "{:?}|{:?}|{:?}|{:?}",
        shots
            .shots
            .iter()
            .map(|e| {
                (
                    e.row.entity_id,
                    &e.row.shot,
                    &e.row.size,
                    &e.row.description,
                    e.row.frame_aspect.to_bits(),
                    e.row.focal_length_mm.to_bits(),
                    thumbs.image_of(e.row.entity_id).is_some(),
                )
            })
            .collect::<Vec<_>>(),
        view.active,
        current,
        head
    );
    if *built == key {
        return;
    }
    *built = key;

    let count = shots.shots.len();
    let mut rows: Vec<Box<dyn SceneList>> = Vec::with_capacity(count * 2 + 3);
    rows.push(gap(0));
    for (index, entry) in shots.shots.iter().enumerate() {
        let row = &entry.row;
        let active = view.active == Some(row.entity_id);
        let thumb = thumbs.image_of(row.entity_id);
        let aspect = row.frame_aspect.max(0.1) as f32;
        let (w, h) = (120.0_f32, 120.0_f32 / aspect);
        let picture: Box<dyn SceneList> = match thumb {
            Some(handle) => Box::new(bsn_list![(
                Node {
                    width: Val::Px(w),
                    height: Val::Px(h),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                }
                template(move |_| Ok(ImageNode::new(handle.clone())))
            )]),
            None => Box::new(bsn_list![(
                Node {
                    width: Val::Px(w),
                    height: Val::Px(h),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                }
                BackgroundColor(DARK)
            )]),
        };
        let title = format!("{}. {}", index + 1, row.shot);
        let size = row.size.clone();
        let lens = format!(
            "{} mm | {:.2}:1 | {}-{} s",
            localgpt_previs::shots::focal(row.focal_length_mm),
            row.frame_aspect,
            trim_seconds(row.in_s),
            trim_seconds(row.out_s),
        );
        let description: Box<dyn SceneList> = if row.description.is_empty() {
            Box::new(bsn_list!())
        } else {
            let text = row.description.clone();
            Box::new(bsn_list![(Text({text}) TextColor(MUTED))])
        };
        let id = row.entity_id;
        rows.push(Box::new(bsn_list![(
            ShotCard { entity_id: id, index: index }
            Widget({format!("shots.row.{}", row.shot)})
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
            }
            BackgroundColor({if active { ACTIVE } else { CARD }})
            // A click looks through; the release that ends a drag is not one.
            on(move |click: On<Pointer<Click>>, drag: Res<StripDrag>, mut looks: MessageWriter<LookThrough>| {
                if click.event.button == PointerButton::Primary && drag.0.is_none() {
                    looks.write(LookThrough(Some(id)));
                }
            })
            on(|start: On<Pointer<DragStart>>, mut drag: ResMut<StripDrag>| {
                if start.event.button == PointerButton::Primary {
                    drag.0 = Some(start.entity);
                }
            })
            Children [
                {picture},
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        flex_grow: 1.0,
                    }
                    Children [
                        (
                            Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(6.0),
                            }
                            Children [
                                (Text({title}) ThemedText),
                                (Text({size}) TextColor(SIZE)),
                            ]
                        ),
                        (Text({lens}) ThemedText),
                        {description},
                    ]
                ),
            ]
        )]));
        rows.push(gap(index + 1));
    }
    rows.push(Box::new(bsn_list![(
        Text({"click to look through - Esc back - 1-9 jump - drag to a gap to reorder".to_string()})
        TextColor(MUTED)
    )]));
    if let Some(endpoint) = endpoint.as_deref() {
        let board = board_url(endpoint, current.as_deref(), None);
        let comparable = head.is_some() && head != current;
        let compare = board_url(endpoint, current.as_deref(), head.as_deref());
        let mut buttons: Vec<Box<dyn SceneList>> = vec![Box::new(bsn_list![(
            @FeathersButton {
                @caption: {caption("Board")},
            }
            Widget({"shots.board".to_string()})
            on(move |_: On<Activate>| open_in_browser(&board))
        )])];
        buttons.push(if comparable {
            Box::new(bsn_list![(
                @FeathersButton {
                    @caption: {caption("Compare with head")},
                }
                Widget({"shots.compare".to_string()})
                on(move |_: On<Activate>| open_in_browser(&compare))
            )])
        } else {
            Box::new(bsn_list![(
                @FeathersButton {
                    @caption: {caption("Compare with head")},
                }
                Widget({"shots.compare".to_string()})
                InteractionDisabled
            )])
        });
        rows.push(Box::new(bsn_list![(
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                margin: UiRect {
                    top: Val::Px(4.0),
                },
            }
            Children [{buttons}]
        )]));
    }
    commands.entity(list).despawn_children();
    commands
        .entity(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// A button's caption.
fn caption(text: &str) -> Box<dyn SceneList> {
    let text = text.to_string();
    Box::new(bsn_list![(Text({text}) ThemedText)])
}

/// The gap before row `n`: a card dropped on it goes there.
fn gap(n: usize) -> Box<dyn SceneList> {
    Box::new(bsn_list![(
        DropZone(n)
        Widget({format!("shots.zone.{n}")})
        Node {
            height: Val::Px(8.0),
            margin: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
        }
        BackgroundColor(GAP)
        on(|drop: On<Pointer<DragDrop>>,
            zones: Query<&DropZone>,
            cards: Query<&ShotCard>,
            parents: Query<&ChildOf>,
            shots: Res<LiveShots>,
            mut edits: MessageWriter<ShotOps>| {
            // What was dragged is the node under the press — a card's text
            // as often as the card: the card is the first ancestor that is one.
            let dragged = std::iter::successors(Some(drop.event.dropped), |&e| parents.get(e).ok().map(ChildOf::parent))
                .take(8)
                .find(|&e| cards.contains(e));
            let (Ok(zone), Some(card)) = (zones.get(drop.entity), dragged.and_then(|e| cards.get(e).ok())) else {
                return;
            };
            if let Some(ops) = reorder_ops(&shots.shots, card.index, zone.0)
                && let Some(entry) = shots.shots.get(card.index)
            {
                edits.write(ShotOps {
                    ops,
                    what: format!("reorder shot {}", entry.row.shot),
                });
            }
        })
    )])
}

/// The drag ends with the button; the click that comes with the release
/// has been ignored by then.
fn end_drag(mouse: Res<ButtonInput<MouseButton>>, mut drag: ResMut<StripDrag>) {
    if drag.0.is_some() && !mouse.pressed(MouseButton::Left) {
        drag.0 = None;
    }
}

/// While a card is dragged, the gaps show; the one under the pointer brighter.
fn light_gaps(
    drag: Res<StripDrag>,
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    mut zones: Query<(Entity, &mut BackgroundColor), With<DropZone>>,
) {
    let dragging = drag.0.is_some();
    for (entity, mut color) in &mut zones {
        let hot = dragging
            && hover.as_ref().is_some_and(|map| {
                map.get(&bevy::picking::pointer::PointerId::Mouse)
                    .is_some_and(|hits| hits.contains_key(&entity))
            });
        let want = if hot {
            GAP_HOT
        } else if dragging {
            Color::srgba(0.35, 0.7, 1.0, 0.3)
        } else {
            GAP
        };
        if color.0 != want {
            color.0 = want;
        }
    }
}
