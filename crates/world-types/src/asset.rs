//! Asset references — for imported meshes, textures, and audio files.

use serde::{Deserialize, Serialize};

/// Reference to an imported mesh asset (alternative to parametric Shape).
///
/// Used for glTF/GLB imports where the geometry is too complex for
/// parametric shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MeshAssetRef {
    /// Relative path to the asset file within the world's `assets/` directory.
    pub path: String,
    /// Optional node name within the asset (for multi-node glTF files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_asset_ref_roundtrip() {
        let r = MeshAssetRef {
            path: "models/tree.glb".to_string(),
            node: Some("trunk".to_string()),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: MeshAssetRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn mesh_asset_ref_no_node() {
        let r = MeshAssetRef {
            path: "props/barrel.glb".to_string(),
            node: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("node"));
    }
}
