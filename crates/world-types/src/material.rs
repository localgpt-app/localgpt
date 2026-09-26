//! PBR material definition.

use serde::{Deserialize, Serialize};

/// PBR material properties.  All fields mirror the Bevy `StandardMaterial`
/// subset that the gen tools expose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MaterialDef {
    /// Base color, sRGB-encoded RGBA in `0..=1` (what Bevy's `Color::srgba`
    /// reads; renderers convert to linear).
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    /// Metallic factor (0.0 = dielectric, 1.0 = metal).
    #[serde(default)]
    pub metallic: f32,
    /// Roughness factor (0.0 = mirror, 1.0 = matte).
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// Emissive color, linear RGBA (values above 1 glow). Non-zero =
    /// self-illuminating.
    #[serde(default)]
    pub emissive: [f32; 4],
    /// Alpha blending mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<AlphaModeDef>,
    /// If true, material ignores all lighting (flat shaded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlit: Option<bool>,
    /// If true, both sides of faces are rendered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub double_sided: Option<bool>,
    /// Reflectance at normal incidence (default 0.5 = 4% reflectance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflectance: Option<f32>,

    // Texture maps. Each is an image path relative to the world's `assets/`
    // folder (like `MeshAssetRef::path`), e.g. `textures/wall_albedo.png`,
    // and follows the glTF 2.0 metallic-roughness conventions.
    /// Base colour (albedo) map, sRGB. Multiplied by `color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color_texture: Option<String>,
    /// Metallic-roughness map, linear: roughness in green, metallic in blue.
    /// Multiplied by `roughness` and `metallic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic_roughness_texture: Option<String>,
    /// Tangent-space normal map, linear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_map_texture: Option<String>,
    /// Emissive map, sRGB. Multiplied by `emissive`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emissive_texture: Option<String>,
}

impl MaterialDef {
    /// Every texture path this material references, with its slot.
    pub fn textures(&self) -> impl Iterator<Item = (TextureSlot, &str)> {
        [
            (TextureSlot::BaseColor, &self.base_color_texture),
            (
                TextureSlot::MetallicRoughness,
                &self.metallic_roughness_texture,
            ),
            (TextureSlot::Normal, &self.normal_map_texture),
            (TextureSlot::Emissive, &self.emissive_texture),
        ]
        .into_iter()
        .filter_map(|(slot, path)| path.as_deref().map(|p| (slot, p)))
    }

    /// The texture path in `slot`, mutably (for relocating assets on save).
    pub fn texture_mut(&mut self, slot: TextureSlot) -> &mut Option<String> {
        match slot {
            TextureSlot::BaseColor => &mut self.base_color_texture,
            TextureSlot::MetallicRoughness => &mut self.metallic_roughness_texture,
            TextureSlot::Normal => &mut self.normal_map_texture,
            TextureSlot::Emissive => &mut self.emissive_texture,
        }
    }
}

/// Which map of a [`MaterialDef`] a texture fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TextureSlot {
    BaseColor,
    MetallicRoughness,
    Normal,
    Emissive,
}

impl TextureSlot {
    /// Whether the image holds colour (sRGB) rather than data (linear).
    pub fn is_srgb(self) -> bool {
        matches!(self, Self::BaseColor | Self::Emissive)
    }
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            color: default_color(),
            metallic: 0.0,
            roughness: default_roughness(),
            emissive: [0.0, 0.0, 0.0, 0.0],
            alpha_mode: None,
            unlit: None,
            double_sided: None,
            reflectance: None,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_map_texture: None,
            emissive_texture: None,
        }
    }
}

/// Alpha blending mode (mirrors Bevy `AlphaMode`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum AlphaModeDef {
    Opaque,
    Mask(f32),
    Blend,
    Add,
    Multiply,
}

fn default_color() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}

fn default_roughness() -> f32 {
    0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_material() {
        let m = MaterialDef::default();
        assert_eq!(m.color, [0.8, 0.8, 0.8, 1.0]);
        assert_eq!(m.metallic, 0.0);
        assert_eq!(m.roughness, 0.5);
        assert_eq!(m.emissive, [0.0, 0.0, 0.0, 0.0]);
        assert!(m.alpha_mode.is_none());
        assert!(m.unlit.is_none());
        assert!(m.double_sided.is_none());
        assert!(m.reflectance.is_none());
    }

    #[test]
    fn material_roundtrip() {
        let m = MaterialDef {
            color: [1.0, 0.0, 0.0, 1.0],
            metallic: 0.9,
            roughness: 0.1,
            emissive: [0.5, 0.5, 0.0, 1.0],
            alpha_mode: Some(AlphaModeDef::Blend),
            unlit: Some(true),
            double_sided: Some(true),
            reflectance: Some(0.3),
            base_color_texture: Some("textures/wall.png".into()),
            metallic_roughness_texture: Some("textures/wall_mr.png".into()),
            normal_map_texture: Some("textures/wall_n.png".into()),
            emissive_texture: None,
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: MaterialDef = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn material_without_new_fields_deserializes() {
        // Old saves without alpha_mode/unlit/double_sided
        let json = r#"{"color":[1,0,0,1],"metallic":0.5,"roughness":0.3,"emissive":[0,0,0,0]}"#;
        let m: MaterialDef = serde_json::from_str(json).unwrap();
        assert!(m.alpha_mode.is_none());
        assert!(m.unlit.is_none());
        assert!(m.double_sided.is_none());
    }

    #[test]
    fn textures_lists_only_set_maps() {
        let m = MaterialDef {
            base_color_texture: Some("textures/a.png".into()),
            normal_map_texture: Some("textures/n.png".into()),
            ..Default::default()
        };
        let slots: Vec<_> = m.textures().collect();
        assert_eq!(
            slots,
            vec![
                (TextureSlot::BaseColor, "textures/a.png"),
                (TextureSlot::Normal, "textures/n.png"),
            ]
        );
        assert!(TextureSlot::BaseColor.is_srgb());
        assert!(!TextureSlot::Normal.is_srgb());
    }

    #[test]
    fn untextured_material_serializes_without_texture_keys() {
        let json = serde_json::to_string(&MaterialDef::default()).unwrap();
        assert!(!json.contains("texture"), "{json}");
    }

    #[test]
    fn alpha_mode_mask_roundtrip() {
        let m = MaterialDef {
            alpha_mode: Some(AlphaModeDef::Mask(0.5)),
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: MaterialDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.alpha_mode, Some(AlphaModeDef::Mask(0.5)));
    }
}
