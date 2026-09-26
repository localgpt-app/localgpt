//! Offscreen rendering for headless screenshot capture.
//!
//! With no primary window (`--headless`), cameras aimed at the window render
//! nowhere and `Screenshot::primary_window()` captures nothing. The
//! [`attach_offscreen_target`] system gives those cameras an image to render
//! into instead, and `gen_screenshot` captures that image.

use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use std::path::Path;

/// Configuration for the offscreen render target.
#[derive(Resource)]
pub struct OffscreenRenderTarget {
    pub width: u32,
    pub height: u32,
    pub image_handle: Option<Handle<Image>>,
}

impl Default for OffscreenRenderTarget {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            image_handle: None,
        }
    }
}

/// Without a primary window, point every window-targeted camera at an
/// offscreen image (created once, [`OffscreenRenderTarget`]'s size).
pub fn attach_offscreen_target(
    mut offscreen: ResMut<OffscreenRenderTarget>,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<&mut bevy::camera::RenderTarget, With<Camera>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !windows.is_empty() {
        return;
    }
    let (width, height) = (offscreen.width, offscreen.height);
    for mut target in &mut cameras {
        if matches!(*target, bevy::camera::RenderTarget::Window(_)) {
            let handle = offscreen
                .image_handle
                .get_or_insert_with(|| create_offscreen_image(&mut images, width, height))
                .clone();
            *target = bevy::camera::RenderTarget::Image(handle.into());
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
