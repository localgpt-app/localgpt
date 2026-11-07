//! Behavior system — declarative, data-driven entity behaviors.
//!
//! Each behavior is a serializable definition that the tick system evaluates
//! every frame. Behaviors modify entity transforms (position, rotation, scale)
//! based on elapsed time, producing continuous animation from pure data.
//!
//! The format's rule (spec/world.md "Behavior and triggers"): a behavior is a
//! function of folded time from the entity's authored transform, an entity's
//! behaviors apply in array order, each from what the ones before it left,
//! and a spin, pulse or swing acts about a pivot that stays where the
//! authored transform put it. Every frame starts from the authored transform
//! again, so nothing accumulates and a seek lands where the clock says.

use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::TAU;

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
    /// The authored position, captured when the behavior was added and kept
    /// in step by every edit: where each frame starts from.
    pub base_position: Vec3,
    /// The authored rotation, likewise: a spin or a swing composes onto it.
    pub base_rotation: Quat,
    /// The authored scale, likewise: a pulse scales it.
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
        pose(&behaviors.behaviors, t, &center_positions, &mut transform);
    }
}

/// Pose an entity at folded time `t`: its authored transform, then every
/// behavior in array order, each from what the ones before it left — so two
/// spins compose, a bob rides a wheel, and a seek lands where the clock says.
/// The authored transform is the first instance's base (the edits keep every
/// instance's base in step). Nothing to do without a behavior.
pub fn pose(
    behaviors: &[BehaviorInstance],
    t: f64,
    centers: &HashMap<String, Vec3>,
    transform: &mut Transform,
) {
    let Some(first) = behaviors.first() else {
        return;
    };
    transform.translation = first.base_position;
    transform.rotation = first.base_rotation;
    transform.scale = first.base_scale;
    for behavior in behaviors {
        apply_behavior(&behavior.def, t, centers, transform);
    }
}

/// Keep a local point where it stood: the rotation or scale just changed from
/// (`q0`, `s0`) to what `transform` holds now, so move the entity by what that
/// change did to `pivot` — a point in its local frame, metres, before its
/// scale. The pivot stays where the authored transform put it; at the origin
/// nothing moves.
fn hold_pivot(transform: &mut Transform, pivot: Vec3, q0: Quat, s0: Vec3) {
    let before = q0 * (s0 * pivot);
    let after = transform.rotation * (transform.scale * pivot);
    transform.translation += before - after;
}

fn apply_behavior(
    def: &BehaviorDef,
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

        BehaviorDef::Spin {
            axis,
            speed,
            phase,
            pivot,
        } => {
            // Absolute in folded time, composed onto what the frame left — the
            // authored rotation, or an earlier behavior's — about the pivot.
            let spin_axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
            let angle = (speed * t as f32 + phase).to_radians();
            let q0 = transform.rotation;
            transform.rotation = q0 * Quat::from_axis_angle(spin_axis, angle);
            let scale = transform.scale;
            hold_pivot(transform, Vec3::from_array(*pivot), q0, scale);
        }

        BehaviorDef::Bob {
            axis,
            amplitude,
            frequency,
            phase,
        } => {
            // Along its axis from what the frame left: the authored position,
            // or where a spin about a pivot ahead of it moved the entity.
            let bob_axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
            let offset = (TAU * frequency * t as f32 + phase.to_radians()).sin() * *amplitude;
            transform.translation += bob_axis * offset;
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
            phase,
            pivot,
        } => {
            let wave = (TAU * frequency * t as f32 + phase.to_radians()).sin();
            let factor = *min_scale + (*max_scale - *min_scale) * (wave * 0.5 + 0.5);
            let s0 = transform.scale;
            transform.scale = s0 * factor;
            let q = transform.rotation;
            hold_pivot(transform, Vec3::from_array(*pivot), q, s0);
        }

        BehaviorDef::Swing {
            axis,
            amplitude,
            frequency,
            phase,
            pivot,
        } => {
            // A pendulum about the hinge through the pivot — hung from its
            // top with `[0, h/2, 0]`.
            let hinge = Vec3::from_array(*axis).normalize_or(Vec3::X);
            let angle =
                amplitude.to_radians() * (TAU * frequency * t as f32 + phase.to_radians()).sin();
            let q0 = transform.rotation;
            transform.rotation = q0 * Quat::from_axis_angle(hinge, angle);
            let scale = transform.scale;
            hold_pivot(transform, Vec3::from_array(*pivot), q0, scale);
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

        // Carried, never run: the entity draws where it was authored.
        BehaviorDef::Other(_) => {}
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

    // The authored transform: what the entity's behaviors already start from,
    // if it has any — its transform right now is mid-animation — else where
    // it stands.
    let base_transform = transforms.get(entity).copied().unwrap_or_default();
    let mut instance = BehaviorInstance {
        id: behavior_id.clone(),
        def: cmd.behavior,
        base_position: base_transform.translation,
        base_rotation: base_transform.rotation,
        base_scale: base_transform.scale,
    };

    if let Ok(mut behaviors) = behaviors_query.get_mut(entity) {
        if let Some(first) = behaviors.behaviors.first() {
            instance.base_position = first.base_position;
            instance.base_rotation = first.base_rotation;
            instance.base_scale = first.base_scale;
        }
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
        BehaviorDef::Swing { .. } => "swing".to_string(),
        BehaviorDef::PathFollow { .. } => "path_follow".to_string(),
        BehaviorDef::Bounce { .. } => "bounce".to_string(),
        BehaviorDef::Other(_) => "unknown".to_string(),
    }
}

/// " about [x,y,z]" for a pivot off the origin, else nothing.
fn about(pivot: &[f32; 3]) -> String {
    if *pivot == [0.0; 3] {
        String::new()
    } else {
        format!(" about [{:.2},{:.2},{:.2}]", pivot[0], pivot[1], pivot[2])
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
        BehaviorDef::Spin {
            axis, speed, pivot, ..
        } => {
            format!(
                "Spin [{:.1},{:.1},{:.1}] at {:.1} deg/s{}",
                axis[0],
                axis[1],
                axis[2],
                speed,
                about(pivot)
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
            pivot,
            ..
        } => format!(
            "Pulse {:.2}-{:.2}x at {:.2}Hz{}",
            min_scale,
            max_scale,
            frequency,
            about(pivot)
        ),
        BehaviorDef::Swing {
            axis,
            amplitude,
            frequency,
            pivot,
            ..
        } => format!(
            "Swing [{:.1},{:.1},{:.1}] ±{:.1}° at {:.2}Hz{}",
            axis[0],
            axis[1],
            axis[2],
            amplitude,
            frequency,
            about(pivot)
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
        BehaviorDef::Other(value) => format!(
            "A behavior this version does not know ({}), carried and not run",
            value
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("no type")
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
                speed: 90.0,
                phase: 0.0,
                pivot: [0.0; 3],
            }),
            "spin"
        );
        assert_eq!(
            behavior_type_name(&BehaviorDef::Swing {
                axis: [0.0, 0.0, 1.0],
                amplitude: 30.0,
                frequency: 0.5,
                phase: 0.0,
                pivot: [0.0, 0.6, 0.0],
            }),
            "swing"
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
                phase: 0.0,
                pivot: [0.0, -0.5, 0.0],
            },
            base_position: Vec3::ZERO,
            base_rotation: Quat::IDENTITY,
            base_scale: Vec3::ONE,
        };
        let summary = behavior_to_summary(&instance);
        assert_eq!(summary.id, "b1");
        assert_eq!(summary.behavior_type, "pulse");
        assert!(summary.description.contains("0.90"));
        assert!(summary.description.contains("1.10"));
        assert!(summary.description.contains("about [0.00,-0.50,0.00]"));
    }

    /// An instance at an authored transform, as a load or an add captures it.
    fn instance(def: BehaviorDef, at: Transform) -> BehaviorInstance {
        BehaviorInstance {
            id: "b".to_string(),
            def,
            base_position: at.translation,
            base_rotation: at.rotation,
            base_scale: at.scale,
        }
    }

    fn near(actual: Vec3, expected: Vec3) {
        assert!(
            actual.distance(expected) < 1e-4,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn test_apply_spin_behavior() {
        let def = BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 90.0,
            phase: 0.0,
            pivot: [0.0; 3],
        };
        let centers = HashMap::new();
        let mut transform = Transform::IDENTITY;

        // After 1 second at 90 deg/s, rotation should be ~90 degrees around Y
        apply_behavior(&def, 1.0, &centers, &mut transform);
        let (_, angle) = transform.rotation.to_axis_angle();
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.01);
        near(transform.translation, Vec3::ZERO);
    }

    #[test]
    fn a_spin_composes_onto_the_authored_rotation_and_phase_is_time_already_spent() {
        let centers = HashMap::new();
        let tilted = Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
        let spin = |phase| BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 90.0,
            phase,
            pivot: [0.0; 3],
        };
        // About its own Y, wherever the authored rotation laid that.
        let mut at_one = tilted;
        apply_behavior(&spin(0.0), 1.0, &centers, &mut at_one);
        let expected = tilted.rotation * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(
            at_one.rotation.dot(expected).abs() > 0.9999,
            "{:?}",
            at_one.rotation
        );
        // Ninety degrees of phase at time zero is the same pose.
        let mut ahead = tilted;
        apply_behavior(&spin(90.0), 0.0, &centers, &mut ahead);
        assert!(ahead.rotation.dot(at_one.rotation).abs() > 0.9999);
    }

    #[test]
    fn a_spin_about_its_base_keeps_the_base_on_the_hub_and_sweeps_the_tip() {
        // A blade 2 m tall standing on a hub at y = 0.5 (conformance/behaviors.json).
        let def = BehaviorDef::Spin {
            axis: [0.0, 0.0, 1.0],
            speed: 90.0,
            phase: 0.0,
            pivot: [0.0, -1.0, 0.0],
        };
        let centers = HashMap::new();
        let authored = Transform::from_xyz(9.0, 1.5, 0.0);
        for t in [0.0, 0.5, 1.0, 2.3, 7.25] {
            let mut transform = authored;
            apply_behavior(&def, t, &centers, &mut transform);
            near(
                transform.transform_point(Vec3::new(0.0, -1.0, 0.0)),
                Vec3::new(9.0, 0.5, 0.0),
            );
        }
        // A quarter turn about +Z takes the tip from above the hub to its left.
        let mut transform = authored;
        apply_behavior(&def, 1.0, &centers, &mut transform);
        near(
            transform.transform_point(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::new(7.0, 0.5, 0.0),
        );
    }

    #[test]
    fn a_swing_hangs_from_its_top_and_rocks_to_its_amplitude() {
        // A sign 1.2 m tall hung from a hook at y = 2 (conformance/behaviors.json).
        let def = BehaviorDef::Swing {
            axis: [0.0, 0.0, 1.0],
            amplitude: 30.0,
            frequency: 0.5,
            phase: 0.0,
            pivot: [0.0, 0.6, 0.0],
        };
        let centers = HashMap::new();
        let authored = Transform::from_xyz(0.0, 1.4, 0.0);
        for t in [0.0, 0.5, 1.0, 1.5, 3.3] {
            let mut transform = authored;
            apply_behavior(&def, t, &centers, &mut transform);
            near(
                transform.transform_point(Vec3::new(0.0, 0.6, 0.0)),
                Vec3::new(0.0, 2.0, 0.0),
            );
        }
        // A quarter period in, the full amplitude; at the half, back through plumb.
        let mut transform = authored;
        apply_behavior(&def, 0.5, &centers, &mut transform);
        let (axis, angle) = transform.rotation.to_axis_angle();
        assert!(
            (angle - 30f32.to_radians()).abs() < 1e-4 && axis.z > 0.99,
            "{axis} {angle}"
        );
        let mut transform = authored;
        apply_behavior(&def, 1.0, &centers, &mut transform);
        assert!(transform.rotation.to_axis_angle().1.abs() < 1e-3);
    }

    #[test]
    fn a_pulse_about_its_base_breathes_on_the_ground() {
        let def = BehaviorDef::Pulse {
            min_scale: 0.7,
            max_scale: 1.3,
            frequency: 0.8,
            phase: 90.0,
            pivot: [0.0, -0.5, 0.0],
        };
        let centers = HashMap::new();
        let authored = Transform::from_xyz(3.0, 0.5, 0.0);
        for t in [0.0, 0.3, 0.625, 1.0, 4.1] {
            let mut transform = authored;
            apply_behavior(&def, t, &centers, &mut transform);
            near(
                transform.transform_point(Vec3::new(0.0, -0.5, 0.0)),
                Vec3::new(3.0, 0.0, 0.0),
            );
        }
        // Ninety degrees of phase: full at time zero.
        let mut transform = authored;
        apply_behavior(&def, 0.0, &centers, &mut transform);
        near(transform.scale, Vec3::splat(1.3));
    }

    #[test]
    fn test_apply_bob_behavior() {
        let def = BehaviorDef::Bob {
            axis: [0.0, 1.0, 0.0],
            amplitude: 1.0,
            frequency: 1.0,
            phase: 0.0,
        };
        let centers = HashMap::new();
        let mut transform = Transform::from_translation(Vec3::new(0.0, 5.0, 0.0));

        // At t=0.25 (quarter period at 1Hz), sin(2π * 1.0 * 0.25) = sin(π/2) = 1.0
        apply_behavior(&def, 0.25, &centers, &mut transform);
        assert!((transform.translation.y - 6.0).abs() < 0.01); // base 5.0 + amplitude 1.0
    }

    #[test]
    fn pose_starts_from_the_authored_transform_and_applies_in_array_order() {
        // A wheel that bobs on its axle: the quarter turn about the base moves
        // the centre a metre left, the bob's crest lifts it half a metre from
        // there — and a second frame at the same time lands on the same pose.
        let authored = Transform::from_xyz(9.0, 1.5, 0.0);
        let behaviors = [
            instance(
                BehaviorDef::Spin {
                    axis: [0.0, 0.0, 1.0],
                    speed: 90.0,
                    phase: 0.0,
                    pivot: [0.0, -1.0, 0.0],
                },
                authored,
            ),
            instance(
                BehaviorDef::Bob {
                    axis: [0.0, 1.0, 0.0],
                    amplitude: 0.5,
                    frequency: 0.25,
                    phase: 0.0,
                },
                authored,
            ),
        ];
        let centers = HashMap::new();
        let mut transform = Transform::from_xyz(-4.0, -4.0, -4.0);
        pose(&behaviors, 1.0, &centers, &mut transform);
        near(transform.translation, Vec3::new(8.0, 1.0, 0.0));
        pose(&behaviors, 1.0, &centers, &mut transform);
        near(transform.translation, Vec3::new(8.0, 1.0, 0.0));
        pose(&behaviors, 0.0, &centers, &mut transform);
        near(transform.translation, authored.translation);
        assert_eq!(transform.rotation, authored.rotation);
        // Without a behavior the transform is left alone.
        pose(&[], 1.0, &centers, &mut transform);
        near(transform.translation, authored.translation);
    }

    #[test]
    fn a_kind_this_version_does_not_know_is_carried_and_never_run() {
        let value = serde_json::json!({"type": "wobble", "how": 3});
        let def: BehaviorDef = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(def, BehaviorDef::Other(_)));
        assert_eq!(behavior_type_name(&def), "unknown");
        assert!(behavior_description(&def).contains("wobble"));
        assert_eq!(serde_json::to_value(&def).unwrap(), value);
        let centers = HashMap::new();
        let mut transform = Transform::from_xyz(1.0, 2.0, 3.0);
        apply_behavior(&def, 5.0, &centers, &mut transform);
        assert_eq!(transform, Transform::from_xyz(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_spin_without_the_new_fields_reads_their_defaults_and_writes_none() {
        let def: BehaviorDef =
            serde_json::from_str(r#"{"type":"spin","axis":[0,1,0],"speed":45}"#).unwrap();
        assert!(
            matches!(def, BehaviorDef::Spin { phase, pivot, .. } if phase == 0.0 && pivot == [0.0; 3])
        );
        let json = serde_json::to_string(&def).unwrap();
        assert!(!json.contains("phase") && !json.contains("pivot"), "{json}");
    }

    #[test]
    fn test_apply_bounce_settled() {
        let def = BehaviorDef::Bounce {
            height: 3.0,
            gravity: 9.8,
            damping: 0.5,
            surface_y: 0.0,
        };
        let centers = HashMap::new();
        let mut transform = Transform::IDENTITY;

        // After a very long time, should be settled at surface_y
        apply_behavior(&def, 100.0, &centers, &mut transform);
        assert_eq!(transform.translation.y, 0.0);
    }
}
