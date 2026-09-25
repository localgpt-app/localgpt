//! Region-based entity storage for multi-file worlds.
//!
//! Large worlds split entities into per-region files.  Each region
//! covers a spatial bounding box and owns a contiguous ID range.

use serde::{Deserialize, Serialize};

use crate::entity::WorldEntity;

/// A set of entities belonging to one spatial region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RegionEntities {
    /// Unique region identifier (e.g., "region_north", "courtyard").
    pub region_id: String,
    /// Spatial bounding box for this region.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RegionBounds>,
    /// Entity ID range (inclusive start, exclusive end).
    pub id_range: (u32, u32),
    /// Entities in this region.
    #[serde(default)]
    pub entities: Vec<WorldEntity>,
}

/// Axis-aligned bounding box for a region.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RegionBounds {
    /// Center point [x, y, z].
    pub center: [f32; 3],
    /// Half-extents [x, y, z].
    pub size: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_entities_roundtrip_json() {
        let region = RegionEntities {
            region_id: "courtyard".to_string(),
            bounds: Some(RegionBounds {
                center: [10.0, 0.0, 10.0],
                size: [20.0, 5.0, 20.0],
            }),
            id_range: (100, 200),
            entities: vec![WorldEntity::new(100, "fountain")],
        };
        let json = serde_json::to_string_pretty(&region).unwrap();
        let back: RegionEntities = serde_json::from_str(&json).unwrap();
        assert_eq!(back.region_id, "courtyard");
        assert_eq!(back.id_range, (100, 200));
        assert_eq!(back.entities.len(), 1);
    }

    #[test]
    fn region_entities_roundtrip_ron() {
        let region = RegionEntities {
            region_id: "north".to_string(),
            bounds: None,
            id_range: (0, 50),
            entities: Vec::new(),
        };
        let ron_str = ron::to_string(&region).unwrap();
        let back: RegionEntities = ron::from_str(&ron_str).unwrap();
        assert_eq!(back.region_id, "north");
        assert!(back.bounds.is_none());
    }

    #[test]
    fn region_bounds_roundtrip() {
        let bounds = RegionBounds {
            center: [5.0, 0.0, -3.0],
            size: [10.0, 4.0, 8.0],
        };
        let json = serde_json::to_string(&bounds).unwrap();
        let back: RegionBounds = serde_json::from_str(&json).unwrap();
        assert_eq!(back.center, [5.0, 0.0, -3.0]);
        assert_eq!(back.size, [10.0, 4.0, 8.0]);
    }
}
