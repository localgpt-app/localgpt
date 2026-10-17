//! The outliner on Bevy's own widgets.
//!
//! The egui tree in `outliner.rs` as a `bevy_ui` panel, with the
//! `feathers` feature: the same tree (`OutlinerCache`, built by the same
//! `build_tree` from the same signature), the same edits — a click
//! selects, Shift or Cmd/Ctrl adds, a double click focuses the camera, a
//! row dragged onto another hangs it under that one (onto the zone at
//! the bottom, at the root), a right click offers duplicate, delete,
//! unparent, group and focus — the same intents into the same authority,
//! and the same widget names for the smoke harness (`outliner.row.<name>`,
//! `menu.<verb>`, `outliner.unparent_zone`), plus `outliner.search`,
//! `outliner.fold.<name>` and `outliner.eye.<name>`.
//!
//! Rows are rebuilt when what they show changes: the tree's signature,
//! the search, the folds, the selection. Row state that the tree does
//! not carry (folds) lives in the cache, keyed by entity, so a rebuild
//! keeps it — the shape Blender's outliner has, per `blender-lessons.md`.

use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::{InheritableThemeTextColor, ThemedText};
use bevy::feathers::tokens;
use bevy::picking::events::{Click, DragDrop, DragStart, Pointer};
use bevy::picking::pointer::{PointerButton, PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::text::{EditableText, FontSourceTemplate, FontWeight, TextEditChange};
use bevy::ui_widgets::{Activate, ScrollArea};

use super::outliner::{OutlinerCache, TreeNode, build_tree, tree_signature};
use super::{
    InspectorMode, InspectorQueries, InspectorSelection, InspectorState, PanelIntents, detail,
};
use crate::gen3d::canvas_edit::{Named, PersonIntent};
use crate::gen3d::live::Live;
use crate::gen3d::registry::{GenEntityType, NameRegistry};
use crate::ui::widgets::{UiWidgetRects, Widget};

const PANEL: Color = Color::srgba(0.10, 0.11, 0.14, 0.97);
const MENU: Color = Color::srgba(0.14, 0.15, 0.19, 0.99);
const ROW_SELECTED: Color = Color::srgba(0.22, 0.45, 0.85, 0.45);
const ROW_PRIMARY: Color = Color::srgba(0.22, 0.45, 0.85, 0.7);
const INK: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.58, 0.64, 0.72);

pub struct OutlinerScenePlugin;

impl Plugin for OutlinerScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OutlinerUi>()
            .add_systems(Startup, spawn_outliner)
            .add_systems(
                Update,
                (
                    place_outliner,
                    rebuild_rows,
                    menu_lifecycle,
                    apply_edits,
                    end_drag,
                )
                    .chain(),
            );
    }
}

/// The panel.
#[derive(Component, Default, Clone)]
struct OutlinerRoot;

/// The scrolling column of rows.
#[derive(Component, Default, Clone)]
struct OutlinerRows;

/// A row: the world entity it stands for (as bits — `bsn!` wants a
/// `Default`).
#[derive(Component, Default, Clone)]
struct OutlinerRow {
    bits: u64,
}

/// The "drop here to unparent" zone, shown while a row is dragged.
#[derive(Component, Default, Clone)]
struct UnparentZone;

/// An open context menu.
#[derive(Component, Default, Clone)]
struct OutlinerMenu;

/// The "N entities" text.
#[derive(Component, Default, Clone)]
struct OutlinerCount;

/// What a row's menu asked for, acted on by [`apply_edits`].
#[derive(Clone)]
enum Action {
    Duplicate(Vec<Entity>),
    Delete(Vec<Entity>),
    Unparent(Entity),
    Group(Vec<Entity>),
    Focus(Entity),
}

/// The outliner's own state between frames.
#[derive(Resource, Default)]
struct OutlinerUi {
    /// The world entity whose row is being dragged.
    drag: Option<Entity>,
    /// Rows dropped: (dragged, new parent — `None` for the root).
    drops: Vec<(Entity, Option<Entity>)>,
    actions: Vec<Action>,
    close_menu: bool,
    /// What the rows were last built from.
    built: String,
}

/// A button's caption.
fn caption(text: &str) -> Box<dyn SceneList> {
    let text = text.to_string();
    Box::new(bsn_list![(Text({text}) ThemedText)])
}

fn spawn_outliner(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        OutlinerRoot
        Name({"Outliner".to_string()})
        Widget({"panel.outliner".to_string()})
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Px(250.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(6.0),
            padding: UiRect::all(Val::Px(8.0)),
        }
        BackgroundColor(PANEL)
        ZIndex(44)
        InheritableThemeTextColor(tokens::TEXT_MAIN)
        InheritableFont {
            font: fonts::REGULAR,
            font_size: FontSize::Px(13.0),
            weight: FontWeight::NORMAL,
        }
        Children [
            (
                Text({"Outliner".to_string()})
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::BOLD),
                    font_size: FontSize::Px(16.0),
                }
                TextColor(INK)
            ),
            // The container grows along its parent's axis by default — in
            // this column, downward: pinned to a row's height.
            (
                @FeathersTextInputContainer
                Node {
                    flex_grow: 0.0,
                    flex_shrink: 0.0,
                    height: Val::Px(24.0),
                    overflow: Overflow::clip(),
                }
                Children [(
                    @FeathersTextInput {
                        @max_characters: {Some(48usize)},
                    }
                    Widget({"outliner.search".to_string()})
                    on(search_typed)
                )]
            ),
            (
                OutlinerRows
                ScrollArea
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Start,
                    align_items: AlignItems::Stretch,
                    overflow: Overflow::scroll_y(),
                }
            ),
            // Over the footer, out of the rows' flow: showing it while a
            // row is dragged must not move the rows under the pointer.
            (
                UnparentZone
                Widget({"outliner.unparent_zone".to_string()})
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0),
                    right: Val::Px(8.0),
                    bottom: Val::Px(8.0),
                    display: Display::None,
                    padding: UiRect::all(Val::Px(10.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    justify_content: JustifyContent::Center,
                }
                BackgroundColor(PANEL)
                ZIndex(2)
                template_value(BorderColor::all(MUTED))
                on(zone_drop)
                Children [(Text({"drop here to unparent".to_string()}) TextColor(MUTED))]
            ),
            (OutlinerCount Text({String::new()}) ThemedText),
            (
                Text({"shift-click adds - drag a row onto another to parent it - right-click for more".to_string()})
                TextColor(MUTED)
                TextFont {
                    font_size: FontSize::Px(11.0),
                }
            ),
        ]
    });
}

/// The search field's text, as typed.
fn search_typed(
    change: On<TextEditChange>,
    inputs: Query<&EditableText>,
    mut cache: ResMut<OutlinerCache>,
) {
    let Ok(field) = inputs.get(change.event_target()) else {
        return;
    };
    let typed = field.value().to_string();
    if cache.search_text != typed {
        cache.search_text = typed;
    }
}

/// A row dropped on the zone goes to the root.
fn zone_drop(
    drop: On<Pointer<DragDrop>>,
    rows: Query<&OutlinerRow>,
    parents: Query<&ChildOf>,
    mut ui: ResMut<OutlinerUi>,
) {
    if let Some(dragged) = row_of(drop.event.dropped, &rows, &parents) {
        ui.drops.push((dragged, None));
    }
}

/// The world entity of the row a dragged node belongs to: the press that
/// started the drag landed on the row's text as often as on the row.
fn row_of(node: Entity, rows: &Query<&OutlinerRow>, parents: &Query<&ChildOf>) -> Option<Entity> {
    std::iter::successors(Some(node), |&e| parents.get(e).ok().map(ChildOf::parent))
        .take(8)
        .find_map(|e| rows.get(e).ok().map(|row| Entity::from_bits(row.bits)))
}

/// Shown with the inspector; below the review bar when one is up; above
/// the bottom bar with the full inspector.
fn place_outliner(
    state: Res<InspectorState>,
    seam: Res<UiWidgetRects>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut root: Query<&mut Node, With<OutlinerRoot>>,
) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let display = if state.mode == InspectorMode::Hidden {
        Display::None
    } else {
        Display::Flex
    };
    if node.display != display {
        node.display = display;
    }
    let top = seam.0.get("panel.review").map_or(0.0, |[_, y, _, h]| y + h);
    let height = window.single().map_or(720.0, Window::height);
    let bottom = seam
        .0
        .get("panel.world_info")
        .map_or(0.0, |[_, y, _, _]| (height - y).max(0.0));
    if node.top != Val::Px(top) {
        node.top = Val::Px(top);
    }
    if node.bottom != Val::Px(bottom) {
        node.bottom = Val::Px(bottom);
    }
}

/// The rows, from the cache's tree: rebuilt when the tree, the search,
/// the folds or the selection change.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn rebuild_rows(
    state: Res<InspectorState>,
    registry: Res<NameRegistry>,
    params: InspectorQueries,
    selection: Res<InspectorSelection>,
    live: Option<Res<Live>>,
    mut cache: ResMut<OutlinerCache>,
    mut ui: ResMut<OutlinerUi>,
    rows: Query<Entity, With<OutlinerRows>>,
    mut count: Query<&mut Text, With<OutlinerCount>>,
    mut commands: Commands,
) {
    if state.mode == InspectorMode::Hidden {
        return;
    }
    let Ok(rows) = rows.single() else {
        return;
    };
    let signature = tree_signature(
        &registry,
        &params.gen_entities,
        &params.visibility_q,
        &params.parent_q,
    );
    if signature != cache.last_signature {
        cache.nodes = build_tree(
            &registry,
            &params.gen_entities,
            &params.visibility_q,
            &params.children_q,
            &params.parent_q,
        );
        cache.last_signature = signature;
    }
    let mut folded: Vec<u64> = cache.collapsed.iter().map(|e| e.to_bits()).collect();
    folded.sort_unstable();
    let key = format!(
        "{signature}|{}|{folded:?}|{:?}|{:?}|{}",
        cache.search_text,
        selection.entity,
        selection.others,
        live.is_some()
    );
    if ui.built == key {
        return;
    }
    ui.built = key;
    if let Ok(mut text) = count.single_mut() {
        text.0 = format!("{} entities", registry.len());
    }

    let search = cache.search_text.to_lowercase();
    let live = live.is_some();
    let mut scenes: Vec<Box<dyn SceneList>> = Vec::with_capacity(cache.nodes.len());
    let mut skip_depth: Option<u32> = None;
    for node in &cache.nodes {
        // Children of a folded node stay out.
        if let Some(depth) = skip_depth {
            if node.depth > depth {
                continue;
            }
            skip_depth = None;
        }
        if !search.is_empty() && !node.name.to_lowercase().contains(&search) {
            continue;
        }
        let folded = cache.collapsed.contains(&node.entity);
        if folded && node.has_children {
            skip_depth = Some(node.depth);
        }
        scenes.push(row_scene(
            node,
            folded,
            selection.contains(node.entity),
            selection.entity == Some(node.entity),
            live,
        ));
    }
    commands.entity(rows).despawn_children();
    commands
        .entity(rows)
        .queue_spawn_related_scenes::<Children>(scenes);
}

/// The colour a type's mark takes, in place of egui's glyphs (the
/// embedded font has none of them).
fn type_color(kind: GenEntityType) -> Color {
    match kind {
        GenEntityType::Primitive => Color::srgb(0.75, 0.78, 0.84),
        GenEntityType::Light => Color::srgb(1.0, 0.85, 0.4),
        GenEntityType::Camera => Color::srgb(0.55, 0.8, 1.0),
        GenEntityType::Mesh => Color::srgb(0.7, 0.95, 0.7),
        GenEntityType::Group => Color::srgb(0.85, 0.7, 1.0),
        GenEntityType::AudioEmitter => Color::srgb(1.0, 0.6, 0.6),
    }
}

/// One row.
fn row_scene(
    node: &TreeNode,
    folded: bool,
    selected: bool,
    primary: bool,
    live: bool,
) -> Box<dyn SceneList> {
    let entity = node.entity;
    let bits = entity.to_bits();
    let name = node.name.clone();
    let widget = format!("outliner.row.{name}");
    let indent = 6.0 + node.depth as f32 * 16.0;
    let depth = node.depth;
    let background = if primary {
        ROW_PRIMARY
    } else if selected {
        ROW_SELECTED
    } else {
        Color::NONE
    };
    let mark = type_color(node.entity_type);
    let ink = if node.visible { INK } else { MUTED };
    let font = if primary { fonts::BOLD } else { fonts::REGULAR };
    let fold: Box<dyn SceneList> = if node.has_children {
        let label = if folded { "+" } else { "-" };
        Box::new(bsn_list![(
            @FeathersButton {
                @caption: {caption(label)},
                @variant: ButtonVariant::Plain,
            }
            Node {
                min_width: Val::Px(18.0),
                height: Val::Px(18.0),
                padding: UiRect::all(Val::Px(0.0)),
            }
            Widget({format!("outliner.fold.{name}")})
            on(move |_: On<Activate>, mut cache: ResMut<OutlinerCache>| {
                if !cache.collapsed.remove(&entity) {
                    cache.collapsed.insert(entity);
                }
            })
        )])
    } else {
        Box::new(bsn_list![
            (Node {
                width: Val::Px(18.0)
            })
        ])
    };
    let eye = if node.visible { "\u{2022}" } else { "\u{00B7}" };
    let eye_widget = format!("outliner.eye.{name}");
    let title = name.clone();
    Box::new(bsn_list![(
        OutlinerRow { bits: bits }
        Widget({widget})
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            min_height: Val::Px(22.0),
            padding: UiRect {
                left: Val::Px(indent),
                right: Val::Px(2.0),
            },
            column_gap: Val::Px(4.0),
            border_radius: BorderRadius::all(Val::Px(3.0)),
        }
        BackgroundColor(background)
        on(move |click: On<Pointer<Click>>,
                 keys: Res<ButtonInput<KeyCode>>,
                 mut selection: ResMut<InspectorSelection>,
                 mut cache: ResMut<OutlinerCache>,
                 ui: Res<OutlinerUi>,
                 root: Query<Entity, With<OutlinerRoot>>,
                 rects: Res<UiWidgetRects>,
                 mut commands: Commands| {
            match click.event.button {
                PointerButton::Secondary if live => {
                    let targets: Vec<Entity> = if selection.contains(entity) {
                        selection.all().collect()
                    } else {
                        vec![entity]
                    };
                    let at = click.pointer_location.position;
                    let origin = rects.0.get("panel.outliner").map_or([0.0, 0.0], |r| [r[0], r[1]]);
                    if let Ok(root) = root.single() {
                        commands
                            .entity(root)
                            .queue_spawn_related_scenes::<Children>(menu_scene(
                                at.x - origin[0],
                                at.y - origin[1],
                                targets,
                                entity,
                                depth,
                            ));
                    }
                }
                PointerButton::Primary if ui.drag.is_none() => {
                    if click.event.count >= 2 {
                        // A double click: select, and bring the camera.
                        selection.set(Some(entity));
                        cache.pending_focus = Some(entity);
                        return;
                    }
                    let extend = keys.any_pressed([
                        KeyCode::ShiftLeft,
                        KeyCode::ShiftRight,
                        KeyCode::SuperLeft,
                        KeyCode::SuperRight,
                        KeyCode::ControlLeft,
                        KeyCode::ControlRight,
                    ]);
                    if extend {
                        selection.toggle(entity);
                    } else {
                        selection.set(Some(entity));
                    }
                }
                _ => {}
            }
        })
        on(move |start: On<Pointer<DragStart>>, mut ui: ResMut<OutlinerUi>| {
            if live && start.event.button == PointerButton::Primary {
                ui.drag = Some(entity);
            }
        })
        on(move |drop: On<Pointer<DragDrop>>,
                 rows: Query<&OutlinerRow>,
                 parents: Query<&ChildOf>,
                 mut ui: ResMut<OutlinerUi>| {
            if let Some(dragged) = row_of(drop.event.dropped, &rows, &parents)
                && dragged != entity
            {
                ui.drops.push((dragged, Some(entity)));
            }
        })
        Children [
            {fold},
            (
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(8.0),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                }
                BackgroundColor(mark)
            ),
            (
                Text({title})
                TextFont {
                    font: FontSourceTemplate::Handle(font),
                    font_size: FontSize::Px(13.0),
                }
                TextColor(ink)
            ),
            (Node { flex_grow: 1.0 }),
            (
                @FeathersButton {
                    @caption: {caption(eye)},
                    @variant: ButtonVariant::Plain,
                }
                Node {
                    min_width: Val::Px(20.0),
                    height: Val::Px(18.0),
                    padding: UiRect::all(Val::Px(0.0)),
                }
                Widget({eye_widget})
                on(move |_: On<Activate>, mut cache: ResMut<OutlinerCache>| {
                    cache.pending_visibility_toggles.push(entity);
                })
            ),
        ]
    )])
}

/// The context menu at a point of the panel, for a row (`node`, at
/// `depth`) and the rows it speaks for (`targets`).
fn menu_scene(
    x: f32,
    y: f32,
    targets: Vec<Entity>,
    node: Entity,
    depth: u32,
) -> Vec<Box<dyn SceneList>> {
    let many = targets.len() > 1;
    let label = |verb: &str| {
        if many {
            format!("{verb} {} selected", targets.len())
        } else {
            verb.to_string()
        }
    };
    let mut entries: Vec<(&'static str, String, Action)> = vec![
        (
            "Duplicate",
            label("Duplicate"),
            Action::Duplicate(targets.clone()),
        ),
        ("Delete", label("Delete"), Action::Delete(targets.clone())),
    ];
    if depth > 0 {
        entries.push(("Unparent", "Unparent".to_string(), Action::Unparent(node)));
    }
    if many {
        entries.push(("Group", label("Group"), Action::Group(targets.clone())));
    }
    entries.push((
        "Focus camera",
        "Focus camera".to_string(),
        Action::Focus(node),
    ));
    let items: Vec<Box<dyn SceneList>> = entries
        .into_iter()
        .map(|(verb, text, action)| {
            let widget = format!("menu.{verb}");
            Box::new(bsn_list![(
                @FeathersButton {
                    @caption: {caption(&text)},
                    @variant: ButtonVariant::Plain,
                }
                Node {
                    justify_content: JustifyContent::Start,
                }
                Widget({widget})
                on(move |_: On<Activate>, mut ui: ResMut<OutlinerUi>| {
                    ui.actions.push(action.clone());
                    ui.close_menu = true;
                })
            )]) as Box<dyn SceneList>
        })
        .collect();
    vec![Box::new(bsn_list![(
        OutlinerMenu
        Widget({"menu".to_string()})
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x),
            top: Val::Px(y),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            min_width: Val::Px(170.0),
            padding: UiRect::all(Val::Px(4.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
        }
        BackgroundColor(MENU)
        ZIndex(120)
        Children [{items}]
    )])]
}

/// The menu goes when an item was taken, on Escape, or on a press
/// outside it.
fn menu_lifecycle(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    rects: Res<UiWidgetRects>,
    mut ui: ResMut<OutlinerUi>,
    menus: Query<Entity, With<OutlinerMenu>>,
    mut commands: Commands,
) {
    if menus.is_empty() {
        ui.close_menu = false;
        return;
    }
    let pressed = mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right);
    let outside = pressed && {
        let at = pointers
            .iter()
            .find(|(id, _)| **id == PointerId::Mouse)
            .and_then(|(_, location)| location.location().map(|l| l.position));
        match (at, rects.0.get("menu")) {
            (Some(p), Some([x, y, w, h])) => {
                !(p.x >= *x && p.x <= x + w && p.y >= *y && p.y <= y + h)
            }
            _ => true,
        }
    };
    if ui.close_menu || keys.just_pressed(KeyCode::Escape) || outside {
        for menu in &menus {
            commands.entity(menu).despawn();
        }
        ui.close_menu = false;
    }
}

/// Drops and menu actions become intents — the same ones the egui tree
/// makes — once a frame, with the scene's queries at hand.
fn apply_edits(
    mut ui: ResMut<OutlinerUi>,
    params: InspectorQueries,
    registry: Res<NameRegistry>,
    live: Option<Res<Live>>,
    mut intents: ResMut<PanelIntents>,
    mut cache: ResMut<OutlinerCache>,
) {
    if ui.drops.is_empty() && ui.actions.is_empty() {
        return;
    }
    let drops = std::mem::take(&mut ui.drops);
    let actions = std::mem::take(&mut ui.actions);
    let named = |entity: Entity| -> Option<Named> {
        let id = params.gen_entities.get(entity).ok()?.world_id.0;
        let name = registry.get_name(entity)?.to_string();
        Some(Named { id, name })
    };
    let Some(live) = live else {
        // Not a live world: focus still works; the edits have no authority.
        for action in actions {
            if let Action::Focus(entity) = action {
                cache.pending_focus = Some(entity);
            }
        }
        return;
    };
    let doc = live.doc();
    for (dragged, parent) in drops {
        let Some(child) = named(dragged) else {
            continue;
        };
        // Not under itself: a row can't hang below its own subtree.
        if let Some(parent) = parent
            && let Some(parent_named) = named(parent)
            && doc
                .subtree_entities_parent_first(child.id)
                .iter()
                .any(|e| e.id.0 == parent_named.id)
        {
            continue;
        }
        let parent = parent.and_then(|p| {
            let n = named(p)?;
            Some((p, n.id, n.name))
        });
        if let Some(intent) = detail::reparent_intent(
            &params,
            dragged,
            child.id,
            &child.name,
            parent
                .as_ref()
                .map(|(e, id, name)| (*e, *id, name.as_str())),
        ) {
            intents.0.push(intent);
        }
    }
    for action in actions {
        match action {
            Action::Duplicate(targets) => {
                let ids: Vec<u64> = targets.iter().filter_map(|e| Some(named(*e)?.id)).collect();
                if !ids.is_empty() {
                    intents.0.push(PersonIntent::Duplicate(ids));
                }
            }
            Action::Delete(targets) => {
                let named: Vec<Named> = targets.iter().filter_map(|e| named(*e)).collect();
                if !named.is_empty() {
                    intents.0.push(PersonIntent::Delete(named));
                }
            }
            Action::Unparent(entity) => {
                if let Some(child) = named(entity)
                    && let Some(intent) =
                        detail::reparent_intent(&params, entity, child.id, &child.name, None)
                {
                    intents.0.push(intent);
                }
            }
            Action::Group(targets) => {
                let named: Vec<Named> = targets.iter().filter_map(|e| named(*e)).collect();
                if named.len() > 1 {
                    intents.0.push(PersonIntent::Group(named));
                }
            }
            Action::Focus(entity) => cache.pending_focus = Some(entity),
        }
    }
}

/// The drag ends with the button; the unparent zone shows while one is on.
fn end_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    mut ui: ResMut<OutlinerUi>,
    mut zone: Query<&mut Node, With<UnparentZone>>,
) {
    if ui.drag.is_some() && !mouse.pressed(MouseButton::Left) {
        ui.drag = None;
    }
    if let Ok(mut node) = zone.single_mut() {
        let display = if ui.drag.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}
