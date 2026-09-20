//! Offscreen rendering for headless screenshot capture.
//!
//! Creates a render-to-texture pipeline that captures the scene without
//! a visible window. Based on Bevy's `headless_renderer` example pattern.
//!
//! NOTE: In Bevy 0.18, the offscreen render target approach requires
//! using the `RenderTarget` component. The actual screenshot capture
//! in headless mode currently relies on the existing `gen_screenshot`
//! tool (which uses Bevy's `Screenshot` API). The offscreen render
//! target will be wired in Phase 1.5 once the headless pipeline is
//! proven with tool-call-only generation.

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

/// Marker component for the offscreen camera (Phase 1.5).
#[allow(dead_code)]
#[derive(Component)]
pub struct OffscreenCamera;

/// Set up the offscreen render target image (without camera — see Phase 1.5).
///
/// Creates the GPU texture that can be used as a render target.
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
            format: TextureFormat::Bgra8UnormSrgb,
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
