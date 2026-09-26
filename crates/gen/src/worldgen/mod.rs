//! WorldGen — procedural world generation pipeline.
//!
//! Implements the blockout-first workflow:
//! 1. `gen_plan_layout` — text → structured BlockoutSpec
//! 2. `gen_apply_blockout` — BlockoutSpec → coarse 3D scene
//! 3. `gen_populate_region` — fill regions with content
//! 4. Navmesh generation and traversability validation (WG2)
//! 5. Connectivity-ordered generation (WG6.2)
//! 6. GLTF mesh segmentation (WG6.3)

pub mod blockout;
pub mod collision_check;
pub mod comfyui;
pub mod depth;
pub mod navmesh;
pub mod navmesh_edit;
pub mod ordering;
pub mod panorama;
pub mod populate;
pub mod preview;
pub mod regenerate;
pub mod segment;
pub mod tier;

pub use blockout::*;
pub use collision_check::*;
pub use depth::*;
pub use navmesh::*;
pub use navmesh_edit::*;
pub use ordering::*;
pub use populate::*;
pub use preview::*;
pub use regenerate::*;
pub use segment::*;
pub use tier::*;
