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
