//! World validation — budget limits and constraint checking.

use crate::entity::WorldEntity;
use crate::spatial::ChunkCoord;
use crate::world::WorldManifest;
use std::collections::HashMap;

/// Resource budget limits for a world.
#[derive(Debug, Clone)]
pub struct WorldLimits {
    /// Maximum entities per chunk.
    pub max_entities_per_chunk: usize,
    /// Maximum estimated triangles per chunk.
    pub max_triangles_per_chunk: usize,
    /// Maximum extent (half-size) of any single entity in any axis.
    pub max_entity_extent: f32,
    /// Maximum number of behaviors on a single entity.
    pub max_behaviors_per_entity: usize,
    /// Maximum number of modulations on a single entity.
    pub max_modulations_per_entity: usize,
}

impl Default for WorldLimits {
    fn default() -> Self {
        Self {
            max_entities_per_chunk: 1000,
            max_triangles_per_chunk: 500_000,
            max_entity_extent: 200.0,
            max_behaviors_per_entity: 8,
            max_modulations_per_entity: 8,
        }
    }
}

/// A validation issue found in a world.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

/// Validate a set of entities against world limits.
pub fn validate_entities(entities: &[WorldEntity], limits: &WorldLimits) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // Per-entity checks
    for entity in entities {
        // Behavior count
        if entity.behaviors.len() > limits.max_behaviors_per_entity {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!(
                    "Entity '{}' has {} behaviors (limit: {})",
                    entity.name,
                    entity.behaviors.len(),
                    limits.max_behaviors_per_entity,
                ),
            });
        }

        // Modulation count and numbers
        if entity.modulations.len() > limits.max_modulations_per_entity {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!(
                    "Entity '{}' has {} modulations (limit: {})",
                    entity.name,
                    entity.modulations.len(),
                    limits.max_modulations_per_entity,
                ),
            });
        }
        for (i, modulation) in entity.modulations.iter().enumerate() {
            if !modulation.is_valid() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    message: format!(
                        "Entity '{}' modulation {} has a non-finite range, a negative \
                         smoothing, or a non-positive oscillator frequency",
                        entity.name, i,
                    ),
                });
            }
        }

        // Shape extent check
        if let Some(ref shape) = entity.shape {
            let aabb = shape.local_aabb_half();
            let max_extent = aabb[0].max(aabb[1]).max(aabb[2]);
            if max_extent > limits.max_entity_extent {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    message: format!(
                        "Entity '{}' shape extent {:.1} exceeds limit {:.1}",
                        entity.name, max_extent, limits.max_entity_extent,
                    ),
                });
            }
        }
    }

    // Per-chunk checks
    let mut chunk_entities: HashMap<ChunkCoord, usize> = HashMap::new();
    let mut chunk_triangles: HashMap<ChunkCoord, usize> = HashMap::new();

    for entity in entities {
        let chunk = entity.chunk.unwrap_or_else(|| {
            ChunkCoord::from_world_pos(entity.transform.position[0], entity.transform.position[2])
        });

        *chunk_entities.entry(chunk).or_default() += 1;
        if let Some(ref shape) = entity.shape {
            *chunk_triangles.entry(chunk).or_default() += shape.estimate_triangles();
        }
    }

    for (chunk, count) in &chunk_entities {
        if *count > limits.max_entities_per_chunk {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                message: format!(
                    "Chunk {} has {} entities (limit: {})",
                    chunk, count, limits.max_entities_per_chunk,
                ),
            });
        }
    }

    for (chunk, tris) in &chunk_triangles {
        if *tris > limits.max_triangles_per_chunk {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                message: format!(
                    "Chunk {} has ~{} triangles (limit: {})",
                    chunk, tris, limits.max_triangles_per_chunk,
                ),
            });
        }
    }

    issues
}

/// Validate a whole manifest: its entities, plus the soundtrack's numbers
/// and any modulation that needs a soundtrack the manifest doesn't carry.
pub fn validate_manifest(manifest: &WorldManifest, limits: &WorldLimits) -> Vec<ValidationIssue> {
    let mut issues = validate_entities(&manifest.entities, limits);

    match &manifest.soundtrack {
        Some(soundtrack) => {
            if !soundtrack.is_valid() {
                issues.push(ValidationIssue {
                    severity: Severity::Error,
                    message: "Soundtrack has out-of-range or non-finite numbers (duration, \
                              bpm, beat_offset, energy, sections, stems)"
                        .to_string(),
                });
            }
            if soundtrack.path.is_some() && soundtrack.license.is_none() {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    message: "Soundtrack ships an audio file without a license".to_string(),
                });
            }
        }
        None => {
            for entity in &manifest.entities {
                if entity
                    .modulations
                    .iter()
                    .any(|m| m.signal.needs_soundtrack())
                {
                    issues.push(ValidationIssue {
                        severity: Severity::Warning,
                        message: format!(
                            "Entity '{}' is modulated by a soundtrack signal but the world has \
                             no soundtrack (the modulation stays inactive)",
                            entity.name,
                        ),
                    });
                }
            }
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Shape;

    #[test]
    fn validates_behavior_count() {
        let mut entity = WorldEntity::new(1, "busy");
        for _ in 0..10 {
            entity.behaviors.push(crate::behavior::BehaviorDef::Spin {
                axis: [0.0, 1.0, 0.0],
                speed: 90.0,
            });
        }

        let issues = validate_entities(&[entity], &WorldLimits::default());
        assert!(issues.iter().any(|i| i.message.contains("behaviors")));
    }

    #[test]
    fn validates_shape_extent() {
        let entity = WorldEntity::new(1, "huge").with_shape(Shape::Cuboid {
            x: 1000.0,
            y: 1000.0,
            z: 1000.0,
        });

        let issues = validate_entities(&[entity], &WorldLimits::default());
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.message.contains("extent"))
        );
    }

    #[test]
    fn valid_entities_produce_no_issues() {
        let entities = vec![
            WorldEntity::new(1, "cube").with_shape(Shape::Cuboid {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
            WorldEntity::new(2, "sphere").with_shape(Shape::Sphere { radius: 1.0 }),
        ];
        let issues = validate_entities(&entities, &WorldLimits::default());
        assert!(issues.is_empty());
    }
}
