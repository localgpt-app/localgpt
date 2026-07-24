//! Non-player character spawning and behavior.
//!
//! Implements Spec 1.3: `gen_add_npc` — Non-Player Characters

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Marker component for NPC entities.
#[derive(Component, Clone)]
pub struct Npc {
    /// NPC display name.
    pub name: String,
    /// Optional dialogue ID reference.
    pub dialogue_id: Option<String>,
}

/// Marker component for NPC nameplate.
#[derive(Component)]
pub struct NpcNameplate;

/// NPC behavior state machine.
#[derive(Component, Clone, Debug, Default)]
pub enum NpcBehavior {
    /// Stand still, face nearby player.
    #[default]
    Idle,
    /// Move through patrol points in sequence.
    Patrol {
        points: Vec<Vec3>,
        speed: f32,
        current_index: usize,
        wait_timer: f32,
    },
    /// Wander randomly within spawn radius.
    Wander {
        spawn_position: Vec3,
        radius: f32,
        target_position: Option<Vec3>,
        speed: f32,
        wait_timer: f32,
    },
}

/// Parameters for spawning an NPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnNpcParams {
    /// Spawn position [x, y, z].
    pub position: [f32; 3],
    /// NPC display name.
    pub name: String,
    /// Model type: "default_humanoid" or asset URL.
    #[serde(default = "default_model")]
    pub model: String,
    /// Behavior type: "idle", "patrol", or "wander".
    #[serde(default)]
    pub behavior: String,
    /// Patrol waypoints (required if behavior is "patrol").
    #[serde(default)]
    pub patrol_points: Vec<[f32; 3]>,
    /// Patrol movement speed (default: 3.0).
    #[serde(default = "default_patrol_speed")]
    pub patrol_speed: f32,
    /// Optional dialogue ID.
    #[serde(default)]
    pub dialogue_id: Option<String>,
}

fn default_model() -> String {
    "default_humanoid".to_string()
}
fn default_patrol_speed() -> f32 {
    3.0
}

impl Default for SpawnNpcParams {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            name: "NPC".to_string(),
            model: default_model(),
            behavior: "idle".to_string(),
            patrol_points: vec![],
            patrol_speed: default_patrol_speed(),
            dialogue_id: None,
        }
    }
}

/// Spawn an NPC entity.
pub fn spawn_npc(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    params: &SpawnNpcParams,
) -> Entity {
    let position = Vec3::from_array(params.position);

    // NPC visual: capsule mesh (placeholder)
    let capsule_mesh = meshes.add(Capsule3d::new(0.3, 1.2));
    let npc_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.4, 0.3),
        ..default()
    });

    // Determine behavior
    let behavior = match params.behavior.to_lowercase().as_str() {
        "patrol" => {
            let points: Vec<Vec3> = params
                .patrol_points
                .iter()
                .map(|p| Vec3::from_array(*p))
                .collect();
            NpcBehavior::Patrol {
                points,
                speed: params.patrol_speed,
                current_index: 0,
                wait_timer: 0.0,
            }
        }
        "wander" => NpcBehavior::Wander {
            spawn_position: position,
            radius: 8.0,
            target_position: None,
            speed: params.patrol_speed * 0.6,
            wait_timer: 0.0,
        },
        _ => NpcBehavior::Idle,
    };

    let npc_entity = commands
        .spawn((
            Name::new(params.name.clone()),
            Npc {
                name: params.name.clone(),
                dialogue_id: params.dialogue_id.clone(),
            },
            behavior,
            Transform::from_translation(position),
            Visibility::default(),
            Mesh3d(capsule_mesh),
            MeshMaterial3d(npc_material),
            crate::terrain::TerrainFollower,
        ))
        .id();

    // Spawn Nameplate
    commands.spawn((
        Name::new("Nameplate"),
        Text2d::new(params.name.clone()),
        TextColor(Color::WHITE),
        TextLayout::justify(Justify::Center),
        Transform::from_xyz(0.0, 1.1, 0.0).with_scale(Vec3::splat(0.01)), // Scale down text
        NpcNameplate,
        ChildOf(npc_entity),
    ));

    npc_entity
}

/// System for NPC idle behavior - face nearby player.
#[allow(clippy::type_complexity)]
pub fn npc_idle_system(
    player_query: Query<&Transform, With<Player>>,
    mut npc_query: Query<(&mut Transform, &NpcBehavior), (With<Npc>, Without<Player>)>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };

    for (mut transform, behavior) in npc_query.iter_mut() {
        if let NpcBehavior::Idle = behavior {
            let to_player = player_transform.translation - transform.translation;
            let distance = to_player.length();

            // Face player if within 10m
            if distance < 10.0 && distance > 0.1 {
                let look_dir = to_player.xz().normalize();
                let angle = look_dir.y.atan2(look_dir.x);
                transform.rotation = Quat::from_rotation_y(angle - std::f32::consts::FRAC_PI_2);
            }
        }
    }
}

/// System for NPC patrol behavior.
pub fn npc_patrol_system(
    time: Res<Time>,
    mut npc_query: Query<(&mut Transform, &mut NpcBehavior), With<Npc>>,
) {
    for (mut transform, mut behavior) in npc_query.iter_mut() {
        if let NpcBehavior::Patrol {
            points,
            speed,
            current_index,
            wait_timer,
        } = behavior.as_mut()
        {
            if points.is_empty() {
                continue;
            }

            let target = points[*current_index];
            let direction = target - transform.translation;
            let distance = direction.length();

            if *wait_timer > 0.0 {
                // Waiting at patrol point
                *wait_timer -= time.delta_secs();
            } else if distance > 0.1 {
                // Move toward patrol point
                let move_dir = direction.normalize();
                transform.translation += move_dir * (*speed) * time.delta_secs();

                // Face movement direction
                if direction.xz().length() > 0.01 {
                    let look_dir = direction.xz().normalize();
                    let angle = look_dir.y.atan2(look_dir.x);
                    transform.rotation = Quat::from_rotation_y(angle - std::f32::consts::FRAC_PI_2);
                }
            } else {
                // Reached patrol point, advance to next
                *current_index = (*current_index + 1) % points.len();
                *wait_timer = 1.0; // Wait 1 second at each point
            }
        }
    }
}

/// System for NPC wander behavior.
pub fn npc_wander_system(
    time: Res<Time>,
    mut npc_query: Query<(&mut Transform, &mut NpcBehavior), With<Npc>>,
) {
    for (mut transform, mut behavior) in npc_query.iter_mut() {
        if let NpcBehavior::Wander {
            spawn_position,
            radius,
            target_position,
            speed,
            wait_timer,
        } = behavior.as_mut()
        {
            if *wait_timer > 0.0 {
                // Waiting
                *wait_timer -= time.delta_secs();
            } else if let Some(target) = target_position {
                let direction = *target - transform.translation;
                let distance = direction.length();

                if distance > 0.1 {
                    // Move toward target
                    let move_dir = direction.normalize();
                    transform.translation += move_dir * (*speed) * time.delta_secs();

                    // Face movement direction
                    if direction.xz().length() > 0.01 {
                        let look_dir = direction.xz().normalize();
                        let angle = look_dir.y.atan2(look_dir.x);
                        transform.rotation =
                            Quat::from_rotation_y(angle - std::f32::consts::FRAC_PI_2);
                    }
                } else {
                    // Reached target, pick new one
                    *target_position = None;
                    *wait_timer = 2.0 + (transform.translation.x * 3.0).rem_euclid(3.0); // 2-5 seconds
                }
            } else {
                // Pick new random target within radius
                use rand::RngExt;
                let mut rng = rand::rng();
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let dist = rng.random_range(2.0..*radius);
                *target_position =
                    Some(*spawn_position + Vec3::new(angle.cos() * dist, 0.0, angle.sin() * dist));
            }
        }
    }
}

/// System to billboard nameplates (face camera).
pub fn nameplate_billboard_system(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut nameplate_query: Query<&mut Transform, With<NpcNameplate>>,
) {
    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    for mut transform in nameplate_query.iter_mut() {
        transform.look_at(camera_transform.translation(), Vec3::Y);
        // Flip it because look_at makes it face AWAY from camera for Text usually?
        // Or Text faces +Z.
        // Actually look_at makes -Z point to target. Text faces +Z.
        // So we might need to rotate 180 Y.
        transform.rotate_local_y(std::f32::consts::PI);
    }
}

// Import from player module
use super::player::Player;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_npc_params_default() {
        let params = SpawnNpcParams::default();
        assert_eq!(params.name, "NPC");
        assert_eq!(params.model, "default_humanoid");
        assert_eq!(params.behavior, "idle");
        assert_eq!(params.patrol_speed, 3.0);
        assert!(params.patrol_points.is_empty());
        assert!(params.dialogue_id.is_none());
    }

    #[test]
    fn test_npc_behavior_default() {
        assert!(matches!(NpcBehavior::default(), NpcBehavior::Idle));
    }

    #[test]
    fn test_npc_behavior_patrol() {
        let behavior = NpcBehavior::Patrol {
            points: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)],
            speed: 3.0,
            current_index: 0,
            wait_timer: 0.0,
        };
        assert!(matches!(behavior, NpcBehavior::Patrol { speed, .. } if speed == 3.0));
    }

    #[test]
    fn test_npc_behavior_wander() {
        let behavior = NpcBehavior::Wander {
            spawn_position: Vec3::ZERO,
            radius: 8.0,
            target_position: None,
            speed: 1.8,
            wait_timer: 0.0,
        };
        assert!(matches!(behavior, NpcBehavior::Wander { radius, .. } if radius == 8.0));
    }

    #[test]
    fn test_npc_component() {
        let npc = Npc {
            name: "Guard".to_string(),
            dialogue_id: Some("guard_intro".to_string()),
        };
        assert_eq!(npc.name, "Guard");
        assert_eq!(npc.dialogue_id.as_deref(), Some("guard_intro"));
    }
}

/// Plugin for NPC systems.
pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                npc_idle_system,
                npc_patrol_system,
                npc_wander_system,
                nameplate_billboard_system,
            ),
        );
    }
}
