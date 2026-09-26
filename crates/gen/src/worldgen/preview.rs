//! Styled preview generation from depth map + text prompt.
//!
//! WG7.2: Generates a styled 2D preview image by combining the blockout depth
//! map with a text prompt and style preset. Validates creative direction before
//! full 3D generation. This module composes the prompt; the image comes from
//! ComfyUI through [`super::comfyui`], driven by the `gen_preview_world` tool.

use serde::{Deserialize, Serialize};

/// Style presets for preview generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStyle {
    Realistic,
    Stylized,
    PixelArt,
    Watercolor,
    ConceptArt,
}

impl PreviewStyle {
    /// Return a prompt suffix for each style preset.
    pub fn prompt_suffix(&self) -> &'static str {
        match self {
            Self::Realistic => "photorealistic, high detail, natural lighting",
            Self::Stylized => "stylized 3D render, Pixar style, vibrant colors",
            Self::PixelArt => "pixel art, 16-bit, retro game style",
            Self::Watercolor => "watercolor painting, soft edges, muted colors",
            Self::ConceptArt => "concept art, painterly, atmospheric perspective",
        }
    }
}

/// Configuration for preview generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewConfig {
    pub prompt: String,
    pub depth_map_path: Option<String>,
    pub style_preset: Option<PreviewStyle>,
    pub negative_prompt: Option<String>,
    pub output_path: Option<String>,
}

/// Result of a preview generation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResult {
    pub path: String,
    pub style: String,
    pub depth_map_used: String,
    pub prompt_used: String,
}

/// Build the full prompt by combining user prompt with style preset suffix.
pub fn build_preview_prompt(prompt: &str, style: Option<PreviewStyle>) -> String {
    match style {
        Some(s) => format!("{}, {}", prompt, s.prompt_suffix()),
        None => prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_suffix() {
        assert_eq!(
            PreviewStyle::Realistic.prompt_suffix(),
            "photorealistic, high detail, natural lighting"
        );
        assert_eq!(
            PreviewStyle::PixelArt.prompt_suffix(),
            "pixel art, 16-bit, retro game style"
        );
        assert_eq!(
            PreviewStyle::Watercolor.prompt_suffix(),
            "watercolor painting, soft edges, muted colors"
        );
        assert_eq!(
            PreviewStyle::ConceptArt.prompt_suffix(),
            "concept art, painterly, atmospheric perspective"
        );
    }

    #[test]
    fn test_build_preview_prompt_with_style() {
        let prompt = "a medieval village at sunset";
        let result = build_preview_prompt(prompt, Some(PreviewStyle::Watercolor));
        assert_eq!(
            result,
            "a medieval village at sunset, watercolor painting, soft edges, muted colors"
        );
    }

    #[test]
    fn test_build_preview_prompt_without_style() {
        let prompt = "a medieval village at sunset";
        let result = build_preview_prompt(prompt, None);
        assert_eq!(result, "a medieval village at sunset");
    }

    #[test]
    fn test_preview_style_serde() {
        // Serialize
        let style = PreviewStyle::PixelArt;
        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(json, "\"pixel_art\"");

        // Deserialize
        let deserialized: PreviewStyle = serde_json::from_str("\"concept_art\"").unwrap();
        assert_eq!(deserialized, PreviewStyle::ConceptArt);

        // Round-trip all variants
        for style in [
            PreviewStyle::Realistic,
            PreviewStyle::Stylized,
            PreviewStyle::PixelArt,
            PreviewStyle::Watercolor,
            PreviewStyle::ConceptArt,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            let back: PreviewStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(back, style);
        }
    }
}
