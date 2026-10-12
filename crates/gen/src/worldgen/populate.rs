//! Three-pass population workflow — hero → medium → decorative.
//!
//! Implements WG3.2: each pass places entities at a different tier, building
//! the scene in order of visual importance. Heroes establish focal points,
//! medium elements build structure, and decoratives fill residual space.

use serde::{Deserialize, Serialize};

use super::blockout::RegionDef;
use super::tier::PlacementTier;

/// Which population pass to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopulationPass {
    /// Pass 1: Place hero entities at hero_slots.
    Hero,
    /// Pass 2: Place medium entities with collision checks.
    Medium,
    /// Pass 3: Scatter decorative elements.
    Decorative,
    /// Run all three passes in sequence.
    All,
}

/// Configuration for a population pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationConfig {
    pub pass: PopulationPass,
    pub skip_collision_check: bool,
}

impl Default for PopulationConfig {
    fn default() -> Self {
        Self {
            pass: PopulationPass::All,
            skip_collision_check: false,
        }
    }
}

/// Plan the entities to place in a region for a given pass.
pub fn plan_population(region: &RegionDef, pass: PopulationPass) -> PopulationPlan {
    match pass {
        PopulationPass::Hero => plan_hero_pass(region),
        PopulationPass::Medium => plan_medium_pass(region),
        PopulationPass::Decorative => plan_decorative_pass(region),
        PopulationPass::All => {
            let mut plan = PopulationPlan::default();
            let hero = plan_hero_pass(region);
            let medium = plan_medium_pass(region);
            let decorative = plan_decorative_pass(region);
            plan.placements.extend(hero.placements);
            plan.placements.extend(medium.placements);
            plan.placements.extend(decorative.placements);
            plan.terrain_smooth_zones = hero.terrain_smooth_zones;
            plan
        }
    }
}

fn plan_hero_pass(region: &RegionDef) -> PopulationPlan {
    let mut plan = PopulationPlan::default();

    for slot in &region.hero_slots {
        let x = slot.position[0] + region.bounds.center[0];
        let z = slot.position[2] + region.bounds.center[1];

        plan.placements.push(PlannedPlacement {
            position: [x, 0.0, z],
            tier: PlacementTier::Hero,
            hint: slot.hint.clone(),
            clearance: 2.0,
            needs_ground_snap: true,
        });

        // Add terrain smoothing zone around hero
        plan.terrain_smooth_zones.push(TerrainSmoothZone {
            center: [x, z],
            radius: 5.0,      // 5m flatten radius
            blend_width: 3.0, // 3m transition
        });
    }

    plan
}

fn plan_medium_pass(region: &RegionDef) -> PopulationPlan {
    let mut plan = PopulationPlan::default();
    let density = region.medium_density;
    let area = region.bounds.size[0] * region.bounds.size[1];
    let count = (area * density * 0.05).round() as usize; // ~5 per 100m² at density 1.0

    let cx = region.bounds.center[0];
    let cz = region.bounds.center[1];
    let hx = region.bounds.size[0] / 2.0;
    let hz = region.bounds.size[1] / 2.0;

    // Simple grid-based placement (will be refined by collision check)
    let grid_step = if count > 0 {
        ((area / count as f32).sqrt()).max(2.0)
    } else {
        return plan;
    };

    let mut x = cx - hx + grid_step / 2.0;
    let mut placed = 0;
    while x < cx + hx && placed < count {
        let mut z = cz - hz + grid_step / 2.0;
        while z < cz + hz && placed < count {
            plan.placements.push(PlannedPlacement {
                position: [x, 0.0, z],
                tier: PlacementTier::Medium,
                hint: String::new(),
                clearance: 0.5,
                needs_ground_snap: true,
            });
            placed += 1;
            z += grid_step;
        }
        x += grid_step;
    }

    plan
}

fn plan_decorative_pass(region: &RegionDef) -> PopulationPlan {
    let mut plan = PopulationPlan::default();
    let density = region.decorative_density;
    let area = region.bounds.size[0] * region.bounds.size[1];
    let count = (area * density * 0.2).round() as usize; // ~20 per 100m² at density 1.0

    let cx = region.bounds.center[0];
    let cz = region.bounds.center[1];
    let hx = region.bounds.size[0] / 2.0;
    let hz = region.bounds.size[1] / 2.0;

    // Scatter placement using a simple hash-based approach
    for i in 0..count {
        let hash = ((i as f32 * std::f32::consts::GOLDEN_RATIO) % 1.0) * 2.0 - 1.0;
        let hash2 = ((i as f32 * std::f32::consts::E) % 1.0) * 2.0 - 1.0;
        let x = cx + hash * hx;
        let z = cz + hash2 * hz;

        plan.placements.push(PlannedPlacement {
            position: [x, 0.0, z],
            tier: PlacementTier::Decorative,
            hint: String::new(),
            clearance: 0.0,
            needs_ground_snap: true,
        });
    }

    plan
}

/// Plan for populating a region.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PopulationPlan {
    pub placements: Vec<PlannedPlacement>,
    pub terrain_smooth_zones: Vec<TerrainSmoothZone>,
}

/// A planned entity placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedPlacement {
    pub position: [f32; 3],
    pub tier: PlacementTier,
    pub hint: String,
    pub clearance: f32,
    pub needs_ground_snap: bool,
}

/// Zone where terrain should be flattened around a hero entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainSmoothZone {
    pub center: [f32; 2],
    pub radius: f32,
    pub blend_width: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::blockout::{HeroSlot, RegionBounds};

    fn make_region() -> RegionDef {
        RegionDef {
            id: "village".to_string(),
            bounds: RegionBounds {
                center: [0.0, 0.0],
                size: [20.0, 20.0],
            },
            region_type: Default::default(),
            density: 0.5,
            walkable: true,
            hero_slots: vec![
                HeroSlot {
                    position: [0.0, 0.0, 0.0],
                    size: [4.0, 5.0, 4.0],
                    role: "landmark".to_string(),
                    hint: "town_hall".to_string(),
                },
                HeroSlot {
                    position: [8.0, 0.0, 0.0],
                    size: [3.0, 3.0, 3.0],
                    role: "focal_point".to_string(),
                    hint: "market".to_string(),
                },
            ],
            medium_density: 0.5,
            decorative_density: 0.5,
        }
    }

    #[test]
    fn test_hero_pass_places_at_slots() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Hero);
        assert_eq!(plan.placements.len(), 2);
        assert!(
            plan.placements
                .iter()
                .all(|p| p.tier == PlacementTier::Hero)
        );
        assert_eq!(plan.terrain_smooth_zones.len(), 2);
    }

    #[test]
    fn test_medium_pass_respects_density() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Medium);
        assert!(!plan.placements.is_empty());
        assert!(
            plan.placements
                .iter()
                .all(|p| p.tier == PlacementTier::Medium)
        );
    }

    #[test]
    fn test_decorative_pass_scatters() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Decorative);
        assert!(!plan.placements.is_empty());
        assert!(
            plan.placements
                .iter()
                .all(|p| p.tier == PlacementTier::Decorative)
        );
    }

    #[test]
    fn test_all_passes_combined() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::All);
        // Should have hero + medium + decorative
        assert!(
            plan.placements
                .iter()
                .any(|p| p.tier == PlacementTier::Hero)
        );
        assert!(
            plan.placements
                .iter()
                .any(|p| p.tier == PlacementTier::Medium)
        );
        assert!(
            plan.placements
                .iter()
                .any(|p| p.tier == PlacementTier::Decorative)
        );
    }

    #[test]
    fn test_hero_clearance() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Hero);
        assert!(plan.placements.iter().all(|p| p.clearance == 2.0));
    }

    #[test]
    fn test_medium_clearance() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Medium);
        assert!(plan.placements.iter().all(|p| p.clearance == 0.5));
    }

    #[test]
    fn test_decorative_no_clearance() {
        let region = make_region();
        let plan = plan_population(&region, PopulationPass::Decorative);
        assert!(plan.placements.iter().all(|p| p.clearance == 0.0));
    }
}
