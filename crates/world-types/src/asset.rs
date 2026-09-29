//! Asset references — for imported meshes, textures, and audio files.

use serde::{Deserialize, Serialize};

/// Reference to an imported mesh asset (alternative to parametric Shape).
///
/// Used for glTF/GLB imports where the geometry is too complex for
/// parametric shapes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MeshAssetRef {
    /// Relative path to the asset file within the world's `assets/` directory.
    pub path: String,
    /// Optional node name within the asset (for multi-node glTF files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// SHA-256 of the file's bytes, lowercase hex. Names the exact asset a
    /// world was made with, so a shared world can be checked for missing or
    /// changed files and identical files can be stored once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Changes to named nodes inside the asset, for this entity only: hide a
    /// branch, recolour a part. Nodes are matched by their glTF name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_overrides: Vec<NodeOverride>,
}

impl MeshAssetRef {
    /// A reference to the whole file at `path`.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Whether `bytes` are the file this reference names (always true when
    /// it carries no hash). `digest` is the bytes' SHA-256 in lowercase hex;
    /// the format crate has no hashing dependency, so callers compute it.
    pub fn matches_digest(&self, digest: &str) -> bool {
        self.sha256
            .as_deref()
            .is_none_or(|h| h.eq_ignore_ascii_case(digest))
    }
}

/// A change to one named node inside an imported mesh.
///
/// Applies to the node and everything under it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NodeOverride {
    /// The node's name in the glTF file.
    pub node: String,
    /// Show or hide the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Replace the base colour of the node's materials (sRGB RGBA in `0..=1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 4]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_asset_ref_roundtrip() {
        let r = MeshAssetRef {
            path: "models/tree.glb".to_string(),
            node: Some("trunk".to_string()),
            sha256: Some("ab".repeat(32)),
            node_overrides: vec![NodeOverride {
                node: "leaves".to_string(),
                visible: None,
                color: Some([0.8, 0.2, 0.1, 1.0]),
            }],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: MeshAssetRef = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn mesh_asset_ref_no_node() {
        let r = MeshAssetRef::new("props/barrel.glb");
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"path":"props/barrel.glb"}"#);
    }

    #[test]
    fn digest_check() {
        let mut r = MeshAssetRef::new("a.glb");
        assert!(r.matches_digest("anything"));
        r.sha256 = Some("ABCD".to_string());
        assert!(r.matches_digest("abcd"));
        assert!(!r.matches_digest("abce"));
    }
}
