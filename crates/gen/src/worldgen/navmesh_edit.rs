//! Navmesh editing — manual walkable/blocked area overrides.
//!
//! Allows designers to mark areas as walkable or blocked, overriding
//! the auto-generated navmesh. Also supports off-mesh connections
//! for jump pads, teleporters, and secret passages.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A manual override applied to the navmesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavMeshOverride {
    pub action: OverrideAction,
    pub position: [f32; 3],
    pub shape: OverrideShape,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideAction {
    /// Mark area as non-walkable.
    Block,
    /// Force area to be walkable.
    Allow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideShape {
    Circle { radius: f32 },
    Rectangle { width: f32, depth: f32 },
}

/// An off-mesh connection between two points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavMeshConnection {
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// Whether this connection is bidirectional.
    pub bidirectional: bool,
}

/// Bevy resource storing navmesh overrides and connections.
#[derive(Resource, Default, Debug, Clone, Serialize, Deserialize)]
pub struct NavMeshOverrides {
    pub overrides: Vec<NavMeshOverride>,
    pub connections: Vec<NavMeshConnection>,
}

impl NavMeshOverrides {
    /// Add an area override.
    pub fn add_override(&mut self, area: NavMeshOverride) {
        self.overrides.push(area);
    }

    /// Add an off-mesh connection.
    pub fn add_connection(&mut self, connection: NavMeshConnection) {
        self.connections.push(connection);
    }

    /// Remove a connection near the given start point.
    pub fn remove_connection_near(&mut self, from: [f32; 3], tolerance: f32) -> bool {
        let tol_sq = tolerance * tolerance;
        let before = self.connections.len();
        self.connections.retain(|c| {
            let dx = c.from[0] - from[0];
            let dy = c.from[1] - from[1];
            let dz = c.from[2] - from[2];
            dx * dx + dy * dy + dz * dz > tol_sq
        });
        self.connections.len() < before
    }

    /// Apply overrides to a navgrid. Call this after initial navgrid generation.
    pub fn apply_to_grid(&self, grid: &mut super::navmesh::NavGrid) {
        for ov in &self.overrides {
            let (cols, rows) = grid.dims;
            for row in 0..rows {
                for col in 0..cols {
                    let world = grid.grid_to_world(col, row);
                    let dx = world.x - ov.position[0];
                    let dz = world.z - ov.position[2];

                    let in_shape = match ov.shape {
                        OverrideShape::Circle { radius } => dx * dx + dz * dz <= radius * radius,
                        OverrideShape::Rectangle { width, depth } => {
                            dx.abs() <= width / 2.0 && dz.abs() <= depth / 2.0
                        }
                    };

                    if in_shape {
                        let idx = row * cols + col;
                        match ov.action {
                            OverrideAction::Block => {
                                grid.cells[idx] = super::navmesh::CellState::Blocked;
                            }
                            OverrideAction::Allow => {
                                grid.cells[idx] = super::navmesh::CellState::Walkable;
                            }
                        }
                    }
                }
            }
        }

        // Recompute connected components after overrides
        grid.find_components();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::navmesh::{CellState, NavMeshSettings, build_navgrid};

    #[test]
    fn test_block_override() {
        let settings = NavMeshSettings {
            cell_size: 1.0,
            agent_radius: 0.0,
            ..Default::default()
        };
        let mut grid = build_navgrid(
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            &settings,
            |_, _| Some(0.0),
            &[],
        );
        assert_eq!(grid.walkable_coverage(), 1.0);

        let mut overrides = NavMeshOverrides::default();
        overrides.add_override(NavMeshOverride {
            action: OverrideAction::Block,
            position: [5.0, 0.0, 5.0],
            shape: OverrideShape::Circle { radius: 2.0 },
        });
        overrides.apply_to_grid(&mut grid);

        assert!(grid.walkable_coverage() < 1.0);
    }

    #[test]
    fn test_allow_override() {
        let settings = NavMeshSettings {
            cell_size: 1.0,
            agent_radius: 0.0,
            ..Default::default()
        };
        let obstacle = (Vec3::new(5.0, 0.0, 5.0), Vec3::new(3.0, 1.0, 3.0));
        let mut grid = build_navgrid(
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            &settings,
            |_, _| Some(0.0),
            &[obstacle],
        );
        let blocked_before = grid
            .cells
            .iter()
            .filter(|c| **c == CellState::Blocked)
            .count();

        let mut overrides = NavMeshOverrides::default();
        overrides.add_override(NavMeshOverride {
            action: OverrideAction::Allow,
            position: [5.0, 0.0, 5.0],
            shape: OverrideShape::Circle { radius: 2.0 },
        });
        overrides.apply_to_grid(&mut grid);

        let blocked_after = grid
            .cells
            .iter()
            .filter(|c| **c == CellState::Blocked)
            .count();
        assert!(blocked_after < blocked_before);
    }

    #[test]
    fn test_add_remove_connection() {
        let mut overrides = NavMeshOverrides::default();
        overrides.add_connection(NavMeshConnection {
            from: [0.0, 0.0, 0.0],
            to: [10.0, 0.0, 10.0],
            bidirectional: true,
        });
        assert_eq!(overrides.connections.len(), 1);

        let removed = overrides.remove_connection_near([0.0, 0.0, 0.0], 1.0);
        assert!(removed);
        assert_eq!(overrides.connections.len(), 0);
    }

    #[test]
    fn test_rectangle_override() {
        let settings = NavMeshSettings {
            cell_size: 1.0,
            agent_radius: 0.0,
            ..Default::default()
        };
        let mut grid = build_navgrid(
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            &settings,
            |_, _| Some(0.0),
            &[],
        );

        let mut overrides = NavMeshOverrides::default();
        overrides.add_override(NavMeshOverride {
            action: OverrideAction::Block,
            position: [5.0, 0.0, 5.0],
            shape: OverrideShape::Rectangle {
                width: 4.0,
                depth: 2.0,
            },
        });
        overrides.apply_to_grid(&mut grid);
        assert!(grid.walkable_coverage() < 1.0);
    }
}
