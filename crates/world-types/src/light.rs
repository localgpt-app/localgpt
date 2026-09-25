//! Light definitions — directional, point, and spot lights.

use serde::{Deserialize, Serialize};

/// Light source attached to an entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LightDef {
    /// Type of light source.
    #[serde(default)]
    pub light_type: LightType,
    /// Light color, sRGB-encoded RGBA in `0..=1`.
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    /// Intensity (lumens for Point/Spot, lux for Directional).
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    /// Direction vector for directional/spot lights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f32; 3]>,
    /// Whether this light casts shadows.
    #[serde(default = "default_true")]
    pub shadows: bool,
    /// Maximum range/radius for point and spot lights (in world units).
    /// `None` uses the engine default. Ignored for directional lights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<f32>,
    /// Outer cone angle in radians (spot lights only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_angle: Option<f32>,
    /// Inner cone angle in radians (spot lights only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_angle: Option<f32>,
}

impl Default for LightDef {
    fn default() -> Self {
        Self {
            light_type: LightType::default(),
            color: default_white(),
            intensity: default_intensity(),
            direction: None,
            shadows: true,
            range: None,
            outer_angle: None,
            inner_angle: None,
        }
    }
}

/// Light source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LightType {
    #[default]
    Directional,
    Point,
    Spot,
}

fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_intensity() -> f32 {
    1000.0
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_type_roundtrip() {
        for lt in [LightType::Directional, LightType::Point, LightType::Spot] {
            let json = serde_json::to_string(&lt).unwrap();
            let back: LightType = serde_json::from_str(&json).unwrap();
            assert_eq!(lt, back);
        }
    }

    #[test]
    fn light_def_defaults() {
        let l = LightDef::default();
        assert_eq!(l.light_type, LightType::Directional);
        assert!(l.shadows);
        assert_eq!(l.intensity, 1000.0);
        assert!(l.range.is_none());
        assert!(l.outer_angle.is_none());
        assert!(l.inner_angle.is_none());
    }

    #[test]
    fn spot_light_roundtrip_with_angles() {
        let light = LightDef {
            light_type: LightType::Spot,
            color: [1.0, 0.9, 0.7, 1.0],
            intensity: 800.0,
            direction: Some([0.0, -1.0, 0.0]),
            shadows: true,
            range: Some(25.0),
            outer_angle: Some(0.7),
            inner_angle: Some(0.5),
        };
        let json = serde_json::to_string(&light).unwrap();
        let back: LightDef = serde_json::from_str(&json).unwrap();
        assert_eq!(light, back);
        assert_eq!(back.range, Some(25.0));
        assert_eq!(back.outer_angle, Some(0.7));
        assert_eq!(back.inner_angle, Some(0.5));
    }

    #[test]
    fn light_without_optional_fields_deserializes() {
        // Simulate loading an old save without range/angle fields
        let json = r#"{"light_type":"point","color":[1,1,1,1],"intensity":500,"shadows":true}"#;
        let light: LightDef = serde_json::from_str(json).unwrap();
        assert_eq!(light.light_type, LightType::Point);
        assert!(light.range.is_none());
        assert!(light.outer_angle.is_none());
        assert!(light.inner_angle.is_none());
    }
}
