//! Gallery UI — the overlay for browsing generated worlds.
//!
//! Triggered by the `G` keybind or `/gallery` slash command.
//! Displays world cards with thumbnails, filtering, and load actions. The
//! state, the toggle and the load hand-off live here for both stacks; the
//! egui window is below, and with the `feathers` feature the panel is
//! `gallery_scene.rs`, on Bevy's own widgets.
//!
//! IMPORTANT: Gallery systems are chained and run before InspectorPlugin
//! systems to avoid egui context contention. The gallery_ui_system only
//! accesses EguiContexts when gallery.visible is true.

#[cfg(not(feature = "feathers"))]
use std::collections::HashMap;
#[cfg(not(feature = "feathers"))]
use std::path::PathBuf;

use bevy::prelude::*;
#[cfg(not(feature = "feathers"))]
use bevy_egui::{EguiContexts, egui};

use super::gallery::{self, WorldGalleryEntry};
use super::plugin::GenWorkspace;

/// Thumbnail loading state for a single entry.
#[cfg(not(feature = "feathers"))]
enum ThumbnailEntry {
    /// We haven't tried to load this yet.
    NotLoaded,
    /// Successfully loaded — holds the egui texture handle.
    Loaded(egui::TextureHandle),
    /// Loading failed (missing file, decode error, etc.) — don't retry.
    Failed,
}

/// Cache of loaded thumbnail textures, keyed by image file path.
///
/// Inserted as a Bevy `Resource`. Thumbnails are loaded lazily the first
/// time a gallery card is rendered, then cached for the lifetime of the
/// gallery session. The cache is cleared when the gallery entry list is
/// refreshed (e.g., the Refresh button).
#[cfg(not(feature = "feathers"))]
#[derive(Resource, Default)]
pub struct ThumbnailCache {
    textures: HashMap<PathBuf, ThumbnailEntry>,
}

#[cfg(not(feature = "feathers"))]
impl ThumbnailCache {
    /// Look up or lazily load a thumbnail for the given image path.
    ///
    /// Returns `Some(&TextureHandle)` if a texture is available, `None` if
    /// the path has no thumbnail or loading failed.
    fn get_or_load(&mut self, path: &PathBuf, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        // Insert NotLoaded on first access, then try to load.
        let entry = self
            .textures
            .entry(path.clone())
            .or_insert(ThumbnailEntry::NotLoaded);

        if matches!(entry, ThumbnailEntry::NotLoaded) {
            *entry = load_thumbnail_texture(path, ctx);
        }

        match entry {
            ThumbnailEntry::Loaded(handle) => Some(handle),
            _ => None,
        }
    }

    /// Drop all cached textures (called on gallery refresh).
    fn clear(&mut self) {
        self.textures.clear();
    }
}

/// Load an image file from disk and upload it as an egui texture.
#[cfg(not(feature = "feathers"))]
fn load_thumbnail_texture(path: &PathBuf, ctx: &egui::Context) -> ThumbnailEntry {
    let image_bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::debug!("Failed to read thumbnail {}: {}", path.display(), e);
            return ThumbnailEntry::Failed;
        }
    };

    let dynamic_image = match image::load_from_memory(&image_bytes) {
        Ok(img) => img,
        Err(e) => {
            tracing::debug!("Failed to decode thumbnail {}: {}", path.display(), e);
            return ThumbnailEntry::Failed;
        }
    };

    // Resize to a reasonable thumbnail size to keep GPU memory low.
    // 200x150 gives a crisp image at the 100x75 display size (2x for HiDPI).
    let resized = dynamic_image.resize(200, 150, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);

    let color_image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);

    let texture_name = format!("gallery_thumb_{}", path.display());
    let handle = ctx.load_texture(texture_name, color_image, egui::TextureOptions::LINEAR);

    ThumbnailEntry::Loaded(handle)
}

/// Gallery visibility and data state.
#[derive(Resource, Default)]
pub struct GalleryState {
    /// Whether the gallery overlay is visible.
    pub visible: bool,
    /// Scanned world entries.
    pub entries: Vec<WorldGalleryEntry>,
    /// Filter text for searching by name or tag.
    pub filter: String,
    /// Index of the selected entry (for load action).
    pub selected: Option<usize>,
    /// Path of world to load (set by "Load" button, consumed by command handler).
    pub load_request: Option<String>,
    /// How many times the entries were scanned; a thumbnail cache that
    /// kept images for an earlier scan drops them.
    pub scans: u32,
}

/// Bevy plugin that adds the gallery UI systems.
pub struct GalleryPlugin;

/// Pending world load request from the gallery.
///
/// When set, `process_gen_commands` in plugin.rs picks this up and
/// executes the load (same codepath as `gen_load_world` from the agent).
#[derive(Resource, Default)]
pub struct PendingGalleryLoad {
    pub path: Option<String>,
}

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GalleryState>()
            .init_resource::<PendingGalleryLoad>()
            .add_systems(Update, gallery_toggle_system)
            .add_systems(Update, gallery_load_system);
        #[cfg(not(feature = "feathers"))]
        app.init_resource::<ThumbnailCache>()
            .add_systems(Update, gallery_ui_system);
        #[cfg(feature = "feathers")]
        app.add_plugins(super::gallery_scene::GalleryScenePlugin);
    }
}

/// Transfer load_request from GalleryState to PendingGalleryLoad resource.
fn gallery_load_system(mut gallery: ResMut<GalleryState>, mut pending: ResMut<PendingGalleryLoad>) {
    if let Some(path) = gallery.load_request.take() {
        tracing::info!("Gallery: queuing world load from {}", path);
        pending.path = Some(path);
        gallery.visible = false;
    }
}

/// Toggle gallery visibility with G key.
///
/// Does NOT access EguiContexts to avoid contention with the inspector.
/// The G key is only active when no egui text field has focus (checked
/// by gallery_ui_system which owns the egui window).
fn gallery_toggle_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut gallery: ResMut<GalleryState>,
    workspace: Res<GenWorkspace>,
    typing: crate::ui::focus::TypingState,
) {
    // Plain G only: Cmd/Ctrl + G is the live canvas's "group", and a G
    // typed into a text field is a letter.
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if keys.just_pressed(KeyCode::KeyG) && !command && !typing.typing() {
        gallery.visible = !gallery.visible;
        if gallery.visible && gallery.entries.is_empty() {
            gallery.entries = gallery::scan_world_gallery(&workspace.path);
            gallery.scans += 1;
        }
    }
}

/// Main gallery UI system — draws the egui window.
#[cfg(not(feature = "feathers"))]
fn gallery_ui_system(
    mut contexts: EguiContexts,
    mut gallery: ResMut<GalleryState>,
    mut thumb_cache: ResMut<ThumbnailCache>,
    workspace: Res<GenWorkspace>,
) {
    if !gallery.visible {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Collect filtered indices upfront to avoid borrow conflicts
    let filter_lower = gallery.filter.to_lowercase();
    let filtered_indices: Vec<usize> = gallery
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            filter_lower.is_empty()
                || e.name.to_lowercase().contains(&filter_lower)
                || e.style_tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&filter_lower))
                || e.source.to_lowercase().contains(&filter_lower)
        })
        .map(|(i, _)| i)
        .collect();

    let entry_count = gallery.entries.len();
    let filtered_count = filtered_indices.len();

    let mut still_open = true;

    egui::Window::new("World Gallery")
        .open(&mut still_open)
        .resizable(true)
        .default_size([800.0, 600.0])
        .show(ctx, |ui| {
            // Filter bar
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.text_edit_singleline(&mut gallery.filter);
                if ui.button("Refresh").clicked() {
                    gallery.entries = gallery::scan_world_gallery(&workspace.path);
                    gallery.scans += 1;
                    thumb_cache.clear();
                }
                ui.label(format!("{} worlds", entry_count));
            });

            ui.separator();

            if filtered_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label("No worlds found. Generate some first!");
                });
                return;
            }

            // Scrollable list of world cards
            egui::ScrollArea::vertical().show(ui, |ui| {
                for &idx in &filtered_indices {
                    let is_selected = gallery.selected == Some(idx);

                    // Read entry data before the mutable borrow
                    let name = gallery.entries[idx].name.clone();
                    let entity_count = gallery.entries[idx].entity_count;
                    let source = gallery.entries[idx].source.clone();
                    let description = gallery.entries[idx].description.clone();
                    let style_tags = gallery.entries[idx].style_tags.clone();
                    let thumb_path = gallery.entries[idx].thumbnail_path.clone();
                    let path = gallery.entries[idx].path.to_string_lossy().to_string();
                    let date = gallery.entries[idx]
                        .created_at
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_default();

                    let frame = if is_selected {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_premultiplied(60, 80, 120, 200))
                            .corner_radius(4.0)
                            .inner_margin(8.0)
                    } else {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_premultiplied(40, 40, 40, 200))
                            .corner_radius(4.0)
                            .inner_margin(8.0)
                    };

                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Thumbnail area (100x75)
                            let thumb_size = egui::vec2(100.0, 75.0);

                            // Try to load and display the actual thumbnail image
                            let texture_id = thumb_path.as_ref().and_then(|tp| {
                                thumb_cache.get_or_load(tp, ui.ctx()).map(|h| h.id())
                            });

                            if let Some(tex_id) = texture_id {
                                let image = egui::Image::new(egui::load::SizedTexture::new(
                                    tex_id, thumb_size,
                                ))
                                .corner_radius(4.0);
                                ui.add(image);
                            } else {
                                // Placeholder: dark rect with "No Preview" label
                                let (rect, _) =
                                    ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                                ui.painter()
                                    .rect_filled(rect, 4.0, egui::Color32::from_gray(30));
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "No Preview",
                                    egui::FontId::proportional(11.0),
                                    egui::Color32::GRAY,
                                );
                            }

                            // Info column
                            ui.vertical(|ui| {
                                ui.strong(&name);
                                ui.label(format!(
                                    "{} entities | {} | {}",
                                    entity_count, source, date
                                ));

                                if let Some(ref desc) = description {
                                    ui.label(
                                        egui::RichText::new(desc)
                                            .small()
                                            .color(egui::Color32::GRAY),
                                    );
                                }

                                if !style_tags.is_empty() {
                                    ui.horizontal(|ui| {
                                        for tag in &style_tags {
                                            ui.label(
                                                egui::RichText::new(tag)
                                                    .small()
                                                    .background_color(egui::Color32::from_gray(50)),
                                            );
                                        }
                                    });
                                }

                                ui.horizontal(|ui| {
                                    if ui.button("Load").clicked() {
                                        gallery.selected = Some(idx);
                                        gallery.load_request = Some(path.clone());
                                    }
                                    if ui.button("Select").clicked() {
                                        gallery.selected = Some(idx);
                                    }
                                });
                            });
                        });
                    });

                    ui.add_space(4.0);
                }
            });
        });

    if !still_open {
        gallery.visible = false;
    }
}
