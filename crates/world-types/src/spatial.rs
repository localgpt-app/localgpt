//! Spatial indexing — chunk coordinates for large world streaming.

use serde::{Deserialize, Serialize};

/// Chunk size in world units (64×64, matching SpacetimeDB RFC).
pub const CHUNK_SIZE: f32 = 64.0;

/// 2D chunk coordinate for spatial partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

impl ChunkCoord {
    /// Compute the chunk coordinate for a world-space position.
    pub fn from_world_pos(world_x: f32, world_z: f32) -> Self {
        Self {
            x: (world_x / CHUNK_SIZE).floor() as i32,
            y: (world_z / CHUNK_SIZE).floor() as i32,
        }
    }

    /// Center of this chunk in world space [x, z].
    pub fn center(&self) -> [f32; 2] {
        [
            (self.x as f32 + 0.5) * CHUNK_SIZE,
            (self.y as f32 + 0.5) * CHUNK_SIZE,
        ]
    }

    /// Manhattan distance to another chunk.
    pub fn distance(&self, other: &ChunkCoord) -> i32 {
        (self.x - other.x).abs() + (self.y - other.y).abs()
    }
}

impl std::fmt::Display for ChunkCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chunk_{}_{}", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_from_world_pos() {
        assert_eq!(
            ChunkCoord::from_world_pos(0.0, 0.0),
            ChunkCoord { x: 0, y: 0 }
        );
        assert_eq!(
            ChunkCoord::from_world_pos(65.0, 130.0),
            ChunkCoord { x: 1, y: 2 }
        );
        assert_eq!(
            ChunkCoord::from_world_pos(-10.0, -10.0),
            ChunkCoord { x: -1, y: -1 }
        );
    }

    #[test]
    fn chunk_display() {
        let c = ChunkCoord { x: 3, y: -1 };
        assert_eq!(format!("{}", c), "chunk_3_-1");
    }

    #[test]
    fn chunk_distance() {
        let a = ChunkCoord { x: 0, y: 0 };
        let b = ChunkCoord { x: 3, y: 4 };
        assert_eq!(a.distance(&b), 7);
    }
}
