//! The world gallery on Bevy's own widgets.
//!
//! The egui window in `gallery_ui.rs` as a `bevy_ui` panel, with the
//! `feathers` feature: the same state (`GalleryState`), the same cards —
//! thumbnail, name, counts, description, tags, Load and Select — a filter
//! field, Refresh and Close, in a scrolling list. Written as scenes
//! (`bsn!`), like the review bar; the panel is spawned once and only the
//! list's rows are rebuilt, when what they show changes, so the filter
//! field keeps its focus while the person types.
//!
//! Widget names for the smoke harness: `panel.gallery`, `gallery.filter`,
//! `gallery.refresh`, `gallery.close`, `gallery.load.<name>`,
//! `gallery.select.<name>`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::ecs::template::template;
use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersScrollbar, FeathersTextInput, FeathersTextInputContainer,
};
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::{InheritableThemeTextColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::text::{EditableText, FontSourceTemplate, FontWeight, TextEditChange};
use bevy::ui_widgets::{Activate, ControlOrientation, ScrollArea};

use super::gallery::{self, WorldGalleryEntry};
use super::gallery_ui::GalleryState;
use super::plugin::GenWorkspace;
use crate::ui::widgets::Widget;

const PANEL: Color = Color::srgba(0.10, 0.11, 0.14, 0.98);
const CARD: Color = Color::srgba(0.16, 0.16, 0.16, 0.9);
const SELECTED: Color = Color::srgba(0.24, 0.31, 0.47, 0.9);
const TAG: Color = Color::srgb(0.2, 0.2, 0.2);
const DARK: Color = Color::srgb(0.12, 0.12, 0.12);
const INK: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.6, 0.65, 0.72);

pub struct GalleryScenePlugin;

impl Plugin for GalleryScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThumbnailImages>()
            .add_systems(Startup, spawn_gallery)
            .add_systems(Update, (show_gallery, place_gallery, rebuild_list).chain());
    }
}

/// The panel.
#[derive(Component, Default, Clone)]
struct GalleryRoot;

/// The scrolling column the cards go in.
#[derive(Component, Default, Clone)]
struct GalleryList;

/// The "N worlds" text.
#[derive(Component, Default, Clone)]
struct GalleryCount;

/// Thumbnails as images, loaded from disk once per scan.
#[derive(Resource, Default)]
struct ThumbnailImages {
    images: HashMap<PathBuf, Option<Handle<Image>>>,
    scans: u32,
}

impl ThumbnailImages {
    /// The image for a thumbnail file, loaded the first time it is asked
    /// for; `None` when the file can't be read or decoded (not retried).
    fn get_or_load(&mut self, path: &Path, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(known) = self.images.get(path) {
            return known.clone();
        }
        let loaded = std::fs::read(path)
            .ok()
            .and_then(|bytes| image::load_from_memory(&bytes).ok())
            .map(|img| {
                // 2× the 100×75 the card shows, for a HiDPI screen.
                let resized = img.resize(200, 150, image::imageops::FilterType::Triangle);
                images.add(Image::from_dynamic(
                    resized,
                    true,
                    RenderAssetUsages::RENDER_WORLD,
                ))
            });
        self.images.insert(path.to_path_buf(), loaded.clone());
        loaded
    }
}

/// A button's caption.
fn caption(text: &str) -> Box<dyn SceneList> {
    let text = text.to_string();
    Box::new(bsn_list![(Text({text}) ThemedText)])
}

fn spawn_gallery(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        GalleryRoot
        Name({"World gallery".to_string()})
        Widget({"panel.gallery".to_string()})
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(240.0),
            top: Val::Px(60.0),
            width: Val::Px(800.0),
            height: Val::Px(600.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
        }
        BackgroundColor(PANEL)
        ZIndex(60)
        InheritableThemeTextColor(tokens::TEXT_MAIN)
        InheritableFont {
            font: fonts::REGULAR,
            font_size: FontSize::Px(13.0),
            weight: FontWeight::NORMAL,
        }
        Children [
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                }
                Children [
                    (
                        Text({"World Gallery".to_string()})
                        TextFont {
                            font: FontSourceTemplate::Handle(fonts::BOLD),
                            font_size: FontSize::Px(16.0),
                        }
                        TextColor(INK)
                    ),
                    (Node { width: Val::Px(12.0) }),
                    (Text({"Filter:".to_string()}) ThemedText),
                    (
                        @FeathersTextInputContainer
                        Node {
                            width: Val::Px(240.0),
                            flex_grow: 0.0,
                            overflow: Overflow::clip(),
                        }
                        Children [(
                            @FeathersTextInput {
                                @max_characters: {Some(48usize)},
                            }
                            Widget({"gallery.filter".to_string()})
                            on(filter_typed)
                        )]
                    ),
                    (
                        @FeathersButton {
                            @caption: {caption("Refresh")},
                        }
                        Widget({"gallery.refresh".to_string()})
                        on(refresh)
                    ),
                    (GalleryCount Text({String::new()}) ThemedText),
                    (Node { flex_grow: 1.0 }),
                    (
                        @FeathersButton {
                            @caption: {caption("Close (G)")},
                        }
                        Widget({"gallery.close".to_string()})
                        on(close)
                    ),
                ]
            ),
            (
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Row,
                }
                Children [
                    (
                        #list
                        GalleryList
                        ScrollArea
                        Node {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Start,
                            align_items: AlignItems::Stretch,
                            row_gap: Val::Px(4.0),
                            overflow: Overflow::scroll_y(),
                            padding: UiRect {
                                right: Val::Px(10.0),
                            },
                        }
                    ),
                    (
                        @FeathersScrollbar {
                            @target: #list,
                            @orientation: {ControlOrientation::Vertical},
                        }
                        Node {
                            width: Val::Px(6.0),
                        }
                    ),
                ]
            ),
        ]
    });
}

/// The filter field's text, as typed.
fn filter_typed(
    change: On<TextEditChange>,
    inputs: Query<&EditableText>,
    mut gallery: ResMut<GalleryState>,
) {
    let Ok(field) = inputs.get(change.event_target()) else {
        return;
    };
    let typed = field.value().to_string();
    if gallery.filter != typed {
        gallery.filter = typed;
    }
}

fn refresh(_: On<Activate>, mut gallery: ResMut<GalleryState>, workspace: Res<GenWorkspace>) {
    gallery.entries = gallery::scan_world_gallery(&workspace.path);
    gallery.scans += 1;
}

fn close(_: On<Activate>, mut gallery: ResMut<GalleryState>) {
    gallery.visible = false;
}

/// Where the panel sits: centred in what the inspector's side panels
/// leave free (egui draws over `bevy_ui`, so under a side panel it would
/// be hidden), 800 px wide when there is room.
fn place_gallery(
    rects: Res<crate::inspector::PanelRects>,
    seam: Res<crate::ui::widgets::UiWidgetRects>,
    state: Res<crate::inspector::InspectorState>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut root: Query<&mut Node, With<GalleryRoot>>,
) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let Ok(window) = window.single() else {
        return;
    };
    let (mut x0, mut x1) = (0.0, window.width());
    if state.mode != crate::inspector::InspectorMode::Hidden {
        if let Some([x, _, w, _]) = crate::inspector::panel_rect("panel.outliner", &rects, &seam) {
            x0 = x + w;
        }
        if let Some([x, _, _, _]) = crate::inspector::panel_rect("panel.detail", &rects, &seam) {
            x1 = x;
        }
    }
    let width = (x1 - x0 - 24.0).clamp(320.0, 800.0);
    let left = x0 + ((x1 - x0) - width) / 2.0;
    let height = (window.height() - 90.0).clamp(240.0, 600.0);
    let top = (window.height() - height) / 2.0;
    let want = (Val::Px(left), Val::Px(top), Val::Px(width), Val::Px(height));
    if (node.left, node.top, node.width, node.height) != want {
        (node.left, node.top, node.width, node.height) = want;
    }
}

/// Shown while the gallery is.
fn show_gallery(gallery: Res<GalleryState>, mut root: Query<&mut Node, With<GalleryRoot>>) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let display = if gallery.visible {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
}

/// The cards, rebuilt when what they show changes: the scan, the filter,
/// the selection. Thumbnails load on first use.
#[allow(clippy::type_complexity)] // the key of what was last built
fn rebuild_list(
    gallery: Res<GalleryState>,
    mut thumbs: ResMut<ThumbnailImages>,
    mut images: ResMut<Assets<Image>>,
    list: Query<Entity, With<GalleryList>>,
    mut count: Query<&mut Text, With<GalleryCount>>,
    mut built: Local<Option<(u32, String, Option<usize>, usize)>>,
    mut commands: Commands,
) {
    let Ok(list) = list.single() else {
        return;
    };
    if !gallery.visible {
        *built = None;
        return;
    }
    let key = (
        gallery.scans,
        gallery.filter.clone(),
        gallery.selected,
        gallery.entries.len(),
    );
    if built.as_ref() == Some(&key) {
        return;
    }
    *built = Some(key);
    if thumbs.scans != gallery.scans {
        thumbs.images.clear();
        thumbs.scans = gallery.scans;
    }
    let filter = gallery.filter.to_lowercase();
    let mut rows: Vec<Box<dyn SceneList>> = Vec::new();
    for (idx, entry) in gallery.entries.iter().enumerate() {
        if !matches(entry, &filter) {
            continue;
        }
        let thumb = entry
            .thumbnail_path
            .as_ref()
            .and_then(|p| thumbs.get_or_load(p, &mut images));
        rows.push(card(idx, entry, gallery.selected == Some(idx), thumb));
    }
    if rows.is_empty() {
        rows.push(Box::new(bsn_list![(
            Text({"No worlds found. Generate some first!".to_string()})
            TextColor(MUTED)
        )]));
    }
    if let Ok(mut text) = count.single_mut() {
        text.0 = format!("{} worlds", gallery.entries.len());
    }
    commands.entity(list).despawn_children();
    commands
        .entity(list)
        .queue_spawn_related_scenes::<Children>(rows);
}

/// The egui window's filter: name, a tag, or the source.
fn matches(entry: &WorldGalleryEntry, filter: &str) -> bool {
    filter.is_empty()
        || entry.name.to_lowercase().contains(filter)
        || entry
            .style_tags
            .iter()
            .any(|t| t.to_lowercase().contains(filter))
        || entry.source.to_lowercase().contains(filter)
}

/// One world's card.
fn card(
    idx: usize,
    entry: &WorldGalleryEntry,
    selected: bool,
    thumb: Option<Handle<Image>>,
) -> Box<dyn SceneList> {
    let name = entry.name.clone();
    let path = entry.path.to_string_lossy().into_owned();
    let line = format!(
        "{} entities | {} | {}",
        entry.entity_count,
        entry.source,
        entry
            .created_at
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default()
    );
    let description: Box<dyn SceneList> = match &entry.description {
        Some(description) => {
            let description = description.clone();
            Box::new(bsn_list![(Text({description}) TextColor(MUTED))])
        }
        None => Box::new(bsn_list!()),
    };
    let tags: Vec<Box<dyn SceneList>> = entry
        .style_tags
        .iter()
        .map(|tag| {
            let tag = tag.clone();
            Box::new(bsn_list![(
                Node {
                    padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                }
                BackgroundColor(TAG)
                Children [(Text({tag}) ThemedText)]
            )]) as Box<dyn SceneList>
        })
        .collect();
    let picture: Box<dyn SceneList> = match thumb {
        Some(handle) => Box::new(bsn_list![(
            Node {
                width: Val::Px(100.0),
                height: Val::Px(75.0),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(4.0)),
            }
            // `ImageNode` is no scene template: built in place, with its handle.
            template(move |_| Ok(ImageNode::new(handle.clone())))
        )]),
        None => Box::new(bsn_list![(
            Node {
                width: Val::Px(100.0),
                height: Val::Px(75.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(4.0)),
            }
            BackgroundColor(DARK)
            Children [(Text({"No Preview".to_string()}) TextColor(MUTED))]
        )]),
    };
    let load_path = path;
    let load_widget = format!("gallery.load.{name}");
    let select_widget = format!("gallery.select.{name}");
    let title = name;
    Box::new(bsn_list![(
        Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            flex_shrink: 0.0,
        }
        BackgroundColor({if selected { SELECTED } else { CARD }})
        Children [
            {picture},
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    flex_grow: 1.0,
                }
                Children [
                    (Text({title}) ThemedText),
                    (Text({line}) ThemedText),
                    {description},
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(4.0),
                        }
                        Children [{tags}]
                    ),
                    (
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(6.0),
                        }
                        Children [
                            (
                                @FeathersButton {
                                    @caption: {caption("Load")},
                                    @variant: ButtonVariant::Primary,
                                }
                                Widget({load_widget})
                                on(move |_: On<Activate>, mut gallery: ResMut<GalleryState>| {
                                    gallery.selected = Some(idx);
                                    gallery.load_request = Some(load_path.clone());
                                })
                            ),
                            (
                                @FeathersButton {
                                    @caption: {caption("Select")},
                                }
                                Widget({select_widget})
                                on(move |_: On<Activate>, mut gallery: ResMut<GalleryState>| {
                                    gallery.selected = Some(idx);
                                })
                            ),
                        ]
                    ),
                ]
            ),
        ]
    )])
}
