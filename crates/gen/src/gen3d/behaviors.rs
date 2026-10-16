//! Behavior system — declarative, data-driven entity behaviors.
//!
//! Each behavior is a serializable definition that the tick system evaluates
//! every frame. Behaviors modify entity transforms (position, rotation, scale)
//! based on elapsed time, producing continuous animation from pure data.

use bevy::prelude::*;
use std::collections::HashMap;

use super::commands::*;
use super::registry::{GenEntity, NameRegistry};

// ---------------------------------------------------------------------------
// Bevy components & resources
// ---------------------------------------------------------------------------

/// A single active behavior instance on an entity.
#[derive(Clone, Debug)]
pub struct BehaviorInstance {
    pub id: String,
    pub def: BehaviorDef,
    /// Base position captured when the behavior was added (for additive behaviors like Bob).
    pub base_position: Vec3,
    /// Base scale captured when added (for Pulse).
    pub base_scale: Vec3,
}

/// Component holding all behaviors for a gen entity.
#[derive(Component, Default, Clone, Debug)]
pub struct EntityBehaviors {
    pub behaviors: Vec<BehaviorInstance>,
}

/// Global behavior state.
#[derive(Resource)]
pub struct BehaviorState {
    pub paused: bool,
    /// Monotonic time accumulator (seconds). Not wall-clock — pauses freeze it.
    pub elapsed: f64,
    /// Counter for generating unique behavior IDs.
    next_id: u64,
}

impl Default for BehaviorState {
    fn default() -> Self {
        Self {
            paused: false,
            elapsed: 0.0,
            next_id: 1,
        }
    }
}

impl BehaviorState {
    pub fn next_id(&mut self) -> String {
        let id = format!("b{}", self.next_id);
        self.next_id += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// Behavior tick system
// ---------------------------------------------------------------------------

/// Advance behavior time and apply all behaviors to their entities.
pub fn behavior_tick(
    time: Res<Time>,
    mut state: ResMut<BehaviorState>,
    registry: Res<NameRegistry>,
    mut query: Query<(
        Entity,
        &mut Transform,
        &mut EntityBehaviors,
        Option<&GenEntity>,
    )>,
    targets: Query<(Entity, &Transform, &GenEntity), Without<EntityBehaviors>>,
) {
    if state.paused {
        return;
    }

    state.elapsed += time.delta_secs_f64();
    let t = state.elapsed;

    // Where every world entity stands, under its name and under its id key
    // (`__id_<n>`, what an id-form reference converts to). The format's log
    // carries ids, so a world loaded from disk or edited live refers to its
    // targets by id, never by name: keyed by name alone, every such orbit
    // circled the origin and every such look-at did nothing. Entities with
    // no behaviors are targets too — the lantern a moon orbits — so the
    // second query covers them.
    let mut center_positions: HashMap<String, Vec3> = HashMap::new();
    let mut note = |entity: Entity, tag: Option<&GenEntity>, at: Vec3| {
        if let Some(name) = registry.get_name(entity) {
            center_positions.insert(name.to_string(), at);
        }
        if let Some(tag) = tag {
            center_positions.insert(super::compat::id_key(tag.world_id.0), at);
        }
    };
    for (entity, transform, _, tag) in query.iter() {
        note(entity, tag, transform.translation);
    }
    for (entity, transform, tag) in targets.iter() {
        note(entity, Some(tag), transform.translation);
    }

    for (_entity, mut transform, behaviors, _) in query.iter_mut() {
        if behaviors.behaviors.is_empty() {
            continue;
        }

        for behavior in &behaviors.behaviors {
            apply_behavior(
                &behavior.def,
                behavior,
                t,
                &center_positions,
                &mut transform,
            );
        }
    }
}

fn apply_behavior(
    def: &BehaviorDef,
    instance: &BehaviorInstance,
    t: f64,
    centers: &HashMap<String, Vec3>,
    transform: &mut Transform,
) {
    match def {
        BehaviorDef::Orbit {
            center,
            center_point,
            radius,
            speed,
            axis,
            phase,
            tilt,
        } => {
            let center_pos = if let Some(name) = center {
                centers.get(name).copied().unwrap_or(Vec3::ZERO)
            } else if let Some(point) = center_point {
                Vec3::from_array(*point)
            } else {
                Vec3::ZERO
            };

            let angle_rad = (speed * t as f32 + phase).to_radians();
            let orbit_axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);

            // Build an orthonormal basis around the orbit axis
            let (tangent, bitangent) = orbit_axis.any_orthonormal_pair();

            // Apply tilt (rotation of the orbit plane)
            let tilt_rad = tilt.to_radians();
            let tilted_tangent =
                tangent * tilt_rad.cos() + orbit_axis.cross(tangent) * tilt_rad.sin();

            let offset = (tilted_tangent * angle_rad.cos() + bitangent * angle_rad.sin()) * *radius;

            transform.translation = center_pos + offset;
        }

        BehaviorDef::Spin { axis, speed } => {
            let spin_axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
            let delta_angle = speed.to_radians() * (t as f32 - (t - 1.0 / 60.0).max(0.0) as f32);
            // Accumulate rotation directly from total elapsed time for stability
            let total_angle = speed.to_radians() * t as f32;
            let base_rotation = Quat::from_axis_angle(spin_axis, total_angle);
            transform.rotation = base_rotation;
            let _ = delta_angle; // used total angle approach instead
        }

        BehaviorDef::Bob {
            axis,
            amplitude,
            frequency,
            phase,
        } => {
            let bob_axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
            let phase_rad = phase.to_radians();
            let offset =
                (2.0 * std::f32::consts::PI * frequency * t as f32 + phase_rad).sin() * *amplitude;
            // Apply bob relative to base position
            // Only modify the component along the bob axis, preserving other position changes
            let current_along_axis = transform.translation.dot(bob_axis);
            let base_along_axis = instance.base_position.dot(bob_axis);
            let diff = (base_along_axis + offset) - current_along_axis;
            transform.translation += bob_axis * diff;
        }

        BehaviorDef::LookAt { target } => {
            if let Some(target_pos) = centers.get(target) {
                let direction = *target_pos - transform.translation;
                if direction.length_squared() > 0.0001 {
                    transform.look_at(*target_pos, Vec3::Y);
                }
            }
        }

        BehaviorDef::Pulse {
            min_scale,
            max_scale,
            frequency,
        } => {
            let t_val = (2.0 * std::f32::consts::PI * frequency * t as f32).sin();
            let scale_factor = *min_scale + (*max_scale - *min_scale) * (t_val * 0.5 + 0.5);
            transform.scale = instance.base_scale * scale_factor;
        }

        BehaviorDef::PathFollow {
            waypoints,
            speed,
            mode,
            orient_to_path,
        } => {
            if waypoints.len() < 2 {
                return;
            }

            // Compute total path length and segment lengths
            let mut segment_lengths = Vec::with_capacity(waypoints.len() - 1);
            let mut total_length: f32 = 0.0;
            for i in 0..waypoints.len() - 1 {
                let a = Vec3::from_array(waypoints[i]);
                let b = Vec3::from_array(waypoints[i + 1]);
                let len = a.distance(b);
                segment_lengths.push(len);
                total_length += len;
            }
            if total_length < 0.001 {
                return;
            }

            // Distance traveled along path
            let raw_distance = speed * t as f32;

            let effective_distance = match mode {
                PathMode::Loop => raw_distance % total_length,
                PathMode::PingPong => {
                    let cycle = total_length * 2.0;
                    let pos = raw_distance % cycle;
                    if pos <= total_length {
                        pos
                    } else {
                        cycle - pos
                    }
                }
                PathMode::Once => raw_distance.min(total_length),
            };

            // Find which segment and interpolate
            let mut remaining = effective_distance;
            let mut pos = Vec3::from_array(waypoints[0]);
            let mut direction = Vec3::ZERO;
            for (i, seg_len) in segment_lengths.iter().enumerate() {
                if remaining <= *seg_len {
                    let frac = if *seg_len > 0.001 {
                        remaining / *seg_len
                    } else {
                        0.0
                    };
                    let a = Vec3::from_array(waypoints[i]);
                    let b = Vec3::from_array(waypoints[i + 1]);
                    pos = a.lerp(b, frac);
                    direction = (b - a).normalize_or(Vec3::ZERO);
                    break;
                }
                remaining -= *seg_len;
            }

            transform.translation = pos;

            if *orient_to_path && direction.length_squared() > 0.001 {
                let target = transform.translation + direction;
                transform.look_at(target, Vec3::Y);
            }
        }

        BehaviorDef::Bounce {
            height,
            gravity,
            damping,
            surface_y,
        } => {
            // Simulate bouncing: each bounce cycle has a parabolic arc.
            // Time for one full bounce at a given height h: t = 2 * sqrt(2h/g)
            let base_velocity = (2.0 * gravity * height).sqrt();
            let mut vel = base_velocity;
            let mut time_remaining = t as f32;
            let mut bounce_count = 0u32;

            // Skip through completed bounces
            loop {
                if vel < 0.01 || bounce_count > 100 {
                    // Settled
                    transform.translation.y = *surface_y;
                    return;
                }
                let bounce_duration = 2.0 * vel / gravity;
                if time_remaining <= bounce_duration {
                    break;
                }
                time_remaining -= bounce_duration;
                vel *= damping.sqrt(); // damping applies to energy, sqrt for velocity
                bounce_count += 1;
            }

            // Within current bounce: y = v*t - 0.5*g*t^2
            let y = vel * time_remaining - 0.5 * gravity * time_remaining * time_remaining;
            transform.translation.y = surface_y + y.max(0.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

pub fn handle_add_behavior(
    cmd: AddBehaviorCmd,
    state: &mut BehaviorState,
    commands: &mut Commands,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    behaviors_query: &mut Query<&mut EntityBehaviors>,
) -> GenResponse {
    let Some(entity) = registry.get_entity(&cmd.entity) else {
        return GenResponse::Error {
            message: format!("Entity '{}' not found", cmd.entity),
        };
    };

    let behavior_id = cmd.behavior_id.unwrap_or_else(|| state.next_id());

    let base_transform = transforms.get(entity).copied().unwrap_or_default();

    let instance = BehaviorInstance {
        id: behavior_id.clone(),
        def: cmd.behavior,
        base_position: base_transform.translation,
        base_scale: base_transform.scale,
    };

    if let Ok(mut behaviors) = behaviors_query.get_mut(entity) {
        behaviors.behaviors.push(instance);
    } else {
        commands.entity(entity).insert(EntityBehaviors {
            behaviors: vec![instance],
        });
    }

    GenResponse::BehaviorAdded {
        entity: cmd.entity,
        behavior_id,
    }
}

pub fn handle_remove_behavior(
    entity_name: &str,
    behavior_id: Option<&str>,
    registry: &NameRegistry,
    behaviors_query: &mut Query<&mut EntityBehaviors>,
) -> GenResponse {
    let Some(entity) = registry.get_entity(entity_name) else {
        return GenResponse::Error {
            message: format!("Entity '{}' not found", entity_name),
        };
    };

    let Ok(mut behaviors) = behaviors_query.get_mut(entity) else {
        return GenResponse::BehaviorRemoved {
            entity: entity_name.to_string(),
            count: 0,
        };
    };

    let before = behaviors.behaviors.len();
    if let Some(id) = behavior_id {
        behaviors.behaviors.retain(|b| b.id != id);
    } else {
        behaviors.behaviors.clear();
    }
    let removed = before - behaviors.behaviors.len();

    GenResponse::BehaviorRemoved {
        entity: entity_name.to_string(),
        count: removed,
    }
}

pub fn handle_list_behaviors(
    entity_filter: Option<&str>,
    state: &BehaviorState,
    registry: &NameRegistry,
    behaviors_query: &Query<&mut EntityBehaviors>,
) -> GenResponse {
    let mut entities_summary = Vec::new();

    for (name, entity) in registry.all_names() {
        if let Some(filter) = entity_filter
            && name != filter
        {
            continue;
        }

        let Ok(behaviors) = behaviors_query.get(entity) else {
            continue;
        };

        if behaviors.behaviors.is_empty() {
            continue;
        }

        let summaries: Vec<BehaviorSummary> = behaviors
            .behaviors
            .iter()
            .map(behavior_to_summary)
            .collect();

        entities_summary.push(EntityBehaviorsSummary {
            entity: name.to_string(),
            behaviors: summaries,
        });
    }

    GenResponse::BehaviorList(BehaviorListResponse {
        paused: state.paused,
        entities: entities_summary,
    })
}

/// Convert a BehaviorInstance to a BehaviorSummary for reporting.
pub fn behavior_to_summary(instance: &BehaviorInstance) -> BehaviorSummary {
    BehaviorSummary {
        id: instance.id.clone(),
        behavior_type: behavior_type_name(&instance.def),
        description: behavior_description(&instance.def),
    }
}

fn behavior_type_name(def: &BehaviorDef) -> String {
    match def {
        BehaviorDef::Orbit { .. } => "orbit".to_string(),
        BehaviorDef::Spin { .. } => "spin".to_string(),
        BehaviorDef::Bob { .. } => "bob".to_string(),
        BehaviorDef::LookAt { .. } => "look_at".to_string(),
        BehaviorDef::Pulse { .. } => "pulse".to_string(),
        BehaviorDef::PathFollow { .. } => "path_follow".to_string(),
        BehaviorDef::Bounce { .. } => "bounce".to_string(),
    }
}

fn behavior_description(def: &BehaviorDef) -> String {
    match def {
        BehaviorDef::Orbit {
            center,
            center_point,
            radius,
            speed,
            ..
        } => {
            let around = center
                .as_deref()
                .map(|s| format!("'{}'", s))
                .or_else(|| center_point.map(|p| format!("[{},{},{}]", p[0], p[1], p[2])))
                .unwrap_or_else(|| "origin".to_string());
            format!(
                "Orbit around {} at radius {:.1}, {:.1} deg/s",
                around, radius, speed
            )
        }
        BehaviorDef::Spin { axis, speed } => {
            format!(
                "Spin [{:.1},{:.1},{:.1}] at {:.1} deg/s",
                axis[0], axis[1], axis[2], speed
            )
        }
        BehaviorDef::Bob {
            amplitude,
            frequency,
            ..
        } => format!("Bob amp={:.2} freq={:.2}Hz", amplitude, frequency),
        BehaviorDef::LookAt { target } => format!("Look at '{}'", target),
        BehaviorDef::Pulse {
            min_scale,
            max_scale,
            frequency,
        } => format!(
            "Pulse {:.2}-{:.2}x at {:.2}Hz",
            min_scale, max_scale, frequency
        ),
        BehaviorDef::PathFollow {
            waypoints,
            speed,
            mode,
            ..
        } => format!(
            "PathFollow {} waypoints at {:.1} u/s ({:?})",
            waypoints.len(),
            speed,
            mode
        ),
        BehaviorDef::Bounce {
            height,
            gravity,
            damping,
            ..
        } => format!(
            "Bounce h={:.1} g={:.1} damp={:.2}",
            height, gravity, damping
        ),
    }
}

// ---------------------------------------------------------------------------
// Serialization helpers (for world save/load)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behavior_state_next_id() {
        let mut state = BehaviorState::default();
        assert!(!state.paused);
        assert_eq!(state.next_id(), "b1");
        assert_eq!(state.next_id(), "b2");
        assert_eq!(state.next_id(), "b3");
    }

    #[test]
    fn test_behavior_type_name() {
        assert_eq!(
            behavior_type_name(&BehaviorDef::Spin {
                axis: [0.0, 1.0, 0.0],
                speed: 90.0
            }),
            "spin"
        );
        assert_eq!(
            behavior_type_name(&BehaviorDef::Bob {
                axis: [0.0, 1.0, 0.0],
                amplitude: 0.5,
                frequency: 1.0,
                phase: 0.0
            }),
            "bob"
        );
        assert_eq!(
            behavior_type_name(&BehaviorDef::LookAt {
                target: "player".to_string()
            }),
            "look_at"
        );
        assert_eq!(
            behavior_type_name(&BehaviorDef::Bounce {
                height: 3.0,
                gravity: 9.8,
                damping: 0.7,
                surface_y: 0.0
            }),
            "bounce"
        );
    }

    #[test]
    fn test_behavior_description_orbit() {
        let desc = behavior_description(&BehaviorDef::Orbit {
            center: Some("Sun".to_string()),
            center_point: None,
            radius: 5.0,
            speed: 36.0,
            axis: [0.0, 1.0, 0.0],
            phase: 0.0,
            tilt: 0.0,
        });
        assert!(desc.contains("'Sun'"));
        assert!(desc.contains("5.0"));
    }

    #[test]
    fn test_behavior_description_path_follow() {
        let desc = behavior_description(&BehaviorDef::PathFollow {
            waypoints: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            speed: 2.0,
            mode: PathMode::PingPong,
            orient_to_path: false,
        });
        assert!(desc.contains("3 waypoints"));
        assert!(desc.contains("PingPong"));
    }

    #[test]
    fn test_behavior_to_summary() {
        let instance = BehaviorInstance {
            id: "b1".to_string(),
            def: BehaviorDef::Pulse {
                min_scale: 0.9,
                max_scale: 1.1,
                frequency: 0.5,
            },
            base_position: Vec3::ZERO,
            base_scale: Vec3::ONE,
        };
        let summary = behavior_to_summary(&instance);
        assert_eq!(summary.id, "b1");
        assert_eq!(summary.behavior_type, "pulse");
        assert!(summary.description.contains("0.90"));
        assert!(summary.description.contains("1.10"));
    }

    #[test]
    fn test_apply_spin_behavior() {
        let def = BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 90.0,
        };
        let instance = BehaviorInstance {
            id: "s1".to_string(),
            def: def.clone(),
            base_position: Vec3::ZERO,
            base_scale: Vec3::ONE,
        };
        let centers = HashMap::new();
        let mut transform = Transform::IDENTITY;

        // After 1 second at 90 deg/s, rotation should be ~90 degrees around Y
        apply_behavior(&def, &instance, 1.0, &centers, &mut transform);
        let (_, angle) = transform.rotation.to_axis_angle();
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    }

    #[test]
    fn test_apply_bob_behavior() {
        let def = BehaviorDef::Bob {
            axis: [0.0, 1.0, 0.0],
            amplitude: 1.0,
            frequency: 1.0,
            phase: 0.0,
        };
        let instance = BehaviorInstance {
            id: "bob1".to_string(),
            def: def.clone(),
            base_position: Vec3::new(0.0, 5.0, 0.0),
            base_scale: Vec3::ONE,
        };
        let centers = HashMap::new();
        let mut transform = Transform::from_translation(Vec3::new(0.0, 5.0, 0.0));

        // At t=0.25 (quarter period at 1Hz), sin(2π * 1.0 * 0.25) = sin(π/2) = 1.0
        apply_behavior(&def, &instance, 0.25, &centers, &mut transform);
        assert!((transform.translation.y - 6.0).abs() < 0.01); // base 5.0 + amplitude 1.0
    }

    #[test]
    fn test_apply_bounce_settled() {
        let def = BehaviorDef::Bounce {
            height: 3.0,
            gravity: 9.8,
            damping: 0.5,
            surface_y: 0.0,
        };
        let instance = BehaviorInstance {
            id: "bounce1".to_string(),
            def: def.clone(),
            base_position: Vec3::ZERO,
            base_scale: Vec3::ONE,
        };
        let centers = HashMap::new();
        let mut transform = Transform::IDENTITY;

        // After a very long time, should be settled at surface_y
        apply_behavior(&def, &instance, 100.0, &centers, &mut transform);
        assert_eq!(transform.translation.y, 0.0);
    }
}
