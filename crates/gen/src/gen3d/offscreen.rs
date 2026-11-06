//! Offscreen rendering for headless screenshot capture.
//!
//! With no primary window (`--headless`), cameras aimed at the window render
//! nowhere and `Screenshot::primary_window()` captures nothing. The
//! [`attach_offscreen_target`] system gives those cameras an image to render
//! into instead, and `gen_screenshot` captures that image. When the app's
//! shell draws the interface, the cameras render into the texture it made
//! for its world panel instead ([`OffscreenRenderTarget::texture_view`]).

use bevy::camera::ManualTextureViewHandle;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::screenshot::Screenshot;
use std::path::Path;

/// Configuration for the offscreen render target.
#[derive(Resource)]
pub struct OffscreenRenderTarget {
    pub width: u32,
    pub height: u32,
    pub image_handle: Option<Handle<Image>>,
    /// A texture someone else made and keeps the size of — the desktop
    /// app's shell, which shows it as its world panel. When set, cameras
    /// render into it and no image is made.
    pub texture_view: Option<ManualTextureViewHandle>,
}

impl Default for OffscreenRenderTarget {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            image_handle: None,
            texture_view: None,
        }
    }
}

impl OffscreenRenderTarget {
    /// Where a camera without a window renders: the shell's texture, else
    /// the image (made on first use).
    pub fn target(&mut self, images: &mut Assets<Image>) -> bevy::camera::RenderTarget {
        if let Some(view) = self.texture_view {
            return bevy::camera::RenderTarget::TextureView(view);
        }
        let (width, height) = (self.width, self.height);
        let handle = self
            .image_handle
            .get_or_insert_with(|| create_offscreen_image(images, width, height))
            .clone();
        bevy::camera::RenderTarget::Image(handle.into())
    }

    /// A screenshot of what the cameras render into: the shell's texture,
    /// the image, or else the window.
    pub fn screenshot(&self) -> Screenshot {
        match (self.texture_view, self.image_handle.clone()) {
            (Some(view), _) => Screenshot::texture_view(view),
            (None, Some(image)) => Screenshot::image(image),
            (None, None) => Screenshot::primary_window(),
        }
    }
}

/// Without a primary window, point every window-targeted camera at an
/// offscreen image (created once, [`OffscreenRenderTarget`]'s size).
pub fn attach_offscreen_target(
    mut commands: Commands,
    mut offscreen: ResMut<OffscreenRenderTarget>,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<(Entity, &mut bevy::camera::RenderTarget), With<Camera>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !windows.is_empty() {
        return;
    }
    let mut ui_camera_set = false;
    for (camera, mut target) in &mut cameras {
        if matches!(*target, bevy::camera::RenderTarget::Window(_)) {
            *target = offscreen.target(&mut images);
            // `bevy_ui` draws through the camera that targets the primary
            // window; with none, its nodes are laid out against nothing
            // and drawn nowhere. The first retargeted camera is it.
            if !ui_camera_set {
                commands.entity(camera).insert(bevy::ui::IsDefaultUiCamera);
                ui_camera_set = true;
            }
        }
    }
}

/// Create the GPU texture cameras render into when there is no window.
pub fn create_offscreen_image(
    images: &mut Assets<Image>,
    width: u32,
    height: u32,
) -> Handle<Image> {
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };

    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("offscreen_render_target"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);

    images.add(image)
}

/// Save raw BGRA pixel data as a PNG file.
///
/// Handles BGRA → RGBA swizzle and directory creation.
pub fn save_pixels_as_png(
    raw_bgra: &[u8],
    width: u32,
    height: u32,
    path: &Path,
) -> Result<(), String> {
    // BGRA → RGBA swizzle
    let mut rgba_data = raw_bgra.to_vec();
    for pixel in rgba_data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2); // swap B and R
    }

    // Convert to PNG via image crate
    let dynamic_image = image::DynamicImage::ImageRgba8(
        image::RgbaImage::from_raw(width, height, rgba_data)
            .ok_or("Failed to create image buffer")?,
    );

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create screenshot directory: {}", e))?;
    }

    dynamic_image
        .save(path)
        .map_err(|e| format!("Failed to save screenshot: {}", e))?;

    tracing::info!("Screenshot saved to {}", path.display());

    Ok(())
}
