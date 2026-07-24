//! Player character spawning and control.
//!
//! Implements Spec 1.1: `gen_spawn_player` — Player Character

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "character-controller")]
use avian3d::prelude::*;
#[cfg(feature = "character-controller")]
use bevy_tnua::TnuaScheme;
#[cfg(feature = "character-controller")]
use bevy_tnua::builtins::{TnuaBuiltinJumpConfig, TnuaBuiltinWalkConfig};
#[cfg(feature = "character-controller")]
use bevy_tnua::prelude::*;
#[cfg(feature = "character-controller")]
use bevy_tnua_avian3d::*;

/// Player character control scheme for bevy-tnua.
///
/// Defines Walk as the basis and Jump as the single action.
#[cfg(feature = "character-controller")]
#[derive(TnuaScheme)]
#[scheme(basis = TnuaBuiltinWalk)]
pub enum PlayerScheme {
    Jump(TnuaBuiltinJump),
}

/// Marker component for the player entity.
#[derive(Component)]
pub struct Player;

/// Resource tracking whether noclip is active (fly-through-walls mode).
#[derive(Resource, Default)]
pub struct NoclipState {
    pub enabled: bool,
}

/// Configuration for the player character.
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    /// Walk speed in units per second.
    pub walk_speed: f32,
    /// Run speed in units per second.
    pub run_speed: f32,
    /// Jump force (upward velocity).
    pub jump_force: f32,
    /// Camera mode: first-person or third-person.
    pub camera_mode: CameraMode,
    /// Distance from player to camera (third-person only).
    pub camera_distance: f32,
    /// Collision capsule radius.
    pub collision_radius: f32,
    /// Collision capsule height.
    pub collision_height: f32,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            walk_speed: 5.0,
            run_speed: 10.0,
            jump_force: 8.0,
            camera_mode: CameraMode::ThirdPerson,
            camera_distance: 5.0,
            collision_radius: 0.3,
            collision_height: 1.8,
        }
    }
}

/// Camera perspective mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CameraMode {
    /// Camera at eye level, no player mesh visible.
    FirstPerson,
    /// Camera orbits behind player, player mesh visible.
    #[default]
    ThirdPerson,
}

/// Component for tracking player input.
#[derive(Component, Default)]
pub struct PlayerInput {
    pub move_forward: f32,
    pub move_right: f32,
    /// Vertical axis: +1 up, -1 down (used in noclip mode).
    pub move_up: f32,
    pub jump: bool,
    pub run: bool,
}

/// Parameters for spawning a player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPlayerParams {
    /// Spawn position (default: [0, 1, 0]).
    #[serde(default = "default_spawn_position")]
    pub position: [f32; 3],
    /// Spawn rotation in degrees (default: [0, 0, 0]).
    #[serde(default)]
    pub rotation: [f32; 3],
    /// Walk speed (default: 5.0).
    #[serde(default = "default_walk_speed")]
    pub walk_speed: f32,
    /// Run speed (default: 10.0).
    #[serde(default = "default_run_speed")]
    pub run_speed: f32,
    /// Jump force (default: 8.0).
    #[serde(default = "default_jump_force")]
    pub jump_force: f32,
    /// Camera mode (default: "third_person").
    #[serde(default)]
    pub camera_mode: String,
    /// Camera distance for third-person (default: 5.0).
    #[serde(default = "default_camera_distance")]
    pub camera_distance: f32,
    /// Collision capsule radius (default: 0.3).
    #[serde(default = "default_collision_radius")]
    pub collision_radius: f32,
    /// Collision capsule height (default: 1.8).
    #[serde(default = "default_collision_height")]
    pub collision_height: f32,
}

fn default_spawn_position() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}
fn default_walk_speed() -> f32 {
    5.0
}
fn default_run_speed() -> f32 {
    10.0
}
fn default_jump_force() -> f32 {
    8.0
}
fn default_camera_distance() -> f32 {
    5.0
}
fn default_collision_radius() -> f32 {
    0.3
}
fn default_collision_height() -> f32 {
    1.8
}

impl Default for SpawnPlayerParams {
    fn default() -> Self {
        Self {
            position: default_spawn_position(),
            rotation: [0.0, 0.0, 0.0],
            walk_speed: default_walk_speed(),
            run_speed: default_run_speed(),
            jump_force: default_jump_force(),
            camera_mode: "third_person".to_string(),
            camera_distance: default_camera_distance(),
            collision_radius: default_collision_radius(),
            collision_height: default_collision_height(),
        }
    }
}

impl SpawnPlayerParams {
    /// Convert camera_mode string to enum.
    pub fn camera_mode_enum(&self) -> CameraMode {
        match self.camera_mode.to_lowercase().as_str() {
            "first_person" | "firstperson" => CameraMode::FirstPerson,
            _ => CameraMode::ThirdPerson,
        }
    }

    /// Convert to PlayerConfig.
    pub fn to_config(&self) -> PlayerConfig {
        PlayerConfig {
            walk_speed: self.walk_speed,
            run_speed: self.run_speed,
            jump_force: self.jump_force,
            camera_mode: self.camera_mode_enum(),
            camera_distance: self.camera_distance,
            collision_radius: self.collision_radius,
            collision_height: self.collision_height,
        }
    }
}

/// Spawn the player entity (with physics — avian3d + bevy-tnua).
#[cfg(feature = "character-controller")]
pub fn spawn_player(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    scheme_configs: &mut ResMut<Assets<PlayerSchemeConfig>>,
    params: &SpawnPlayerParams,
) -> Entity {
    let position = Vec3::from_array(params.position);
    let rotation = Quat::from_rotation_y(params.rotation[1].to_radians());

    let capsule_mesh = meshes.add(Capsule3d::new(
        params.collision_radius,
        params.collision_height - params.collision_radius * 2.0,
    ));
    let capsule_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.5, 0.8),
        ..default()
    });

    // float_height = half the capsule total height (center of mass above ground)
    let float_height = params.collision_height / 2.0;

    let config_handle = scheme_configs.add(PlayerSchemeConfig {
        basis: TnuaBuiltinWalkConfig {
            speed: params.walk_speed,
            float_height,
            acceleration: 50.0,
            air_acceleration: 20.0,
            coyote_time: 0.15,
            ..Default::default()
        },
        jump: TnuaBuiltinJumpConfig {
            height: params.jump_force,
            ..Default::default()
        },
    });

    commands
        .spawn((
            Name::new("Player"),
            Player,
            params.to_config(),
            PlayerInput::default(),
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Mesh3d(capsule_mesh),
            MeshMaterial3d(capsule_material),
            // Physics: Avian
            RigidBody::Dynamic,
            Collider::capsule(
                params.collision_radius,
                params.collision_height - params.collision_radius * 2.0,
            ),
            LockedAxes::new().lock_rotation_x().lock_rotation_z(),
            // Controller: Tnua
            TnuaController::<PlayerScheme>::default(),
            TnuaConfig::<PlayerScheme>(config_handle),
            TnuaAvian3dSensorShape(Collider::cylinder(params.collision_radius * 0.95, 0.0)),
        ))
        .id()
}

/// Spawn the player entity (without physics — simple transform-based movement).
#[cfg(not(feature = "character-controller"))]
pub fn spawn_player(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    params: &SpawnPlayerParams,
) -> Entity {
    let position = Vec3::from_array(params.position);
    let rotation = Quat::from_rotation_y(params.rotation[1].to_radians());

    let capsule_mesh = meshes.add(Capsule3d::new(
        params.collision_radius,
        params.collision_height - params.collision_radius * 2.0,
    ));
    let capsule_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.5, 0.8),
        ..default()
    });

    commands
        .spawn((
            Name::new("Player"),
            Player,
            params.to_config(),
            PlayerInput::default(),
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Mesh3d(capsule_mesh),
            MeshMaterial3d(capsule_material),
        ))
        .id()
}

/// Despawn any existing player entity.
pub fn despawn_player(mut commands: Commands, player_query: Query<Entity, With<Player>>) {
    for entity in player_query.iter() {
        commands.entity(entity).despawn();
    }
}

/// System to handle player movement input.
pub fn player_input_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    noclip: Res<NoclipState>,
    mut query: Query<&mut PlayerInput, With<Player>>,
) {
    for mut input in query.iter_mut() {
        // Reset input
        input.move_forward = 0.0;
        input.move_right = 0.0;
        input.move_up = 0.0;
        input.jump = false;
        input.run = false;

        // Forward/backward
        if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
            input.move_forward += 1.0;
        }
        if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
            input.move_forward -= 1.0;
        }

        // Left/right
        if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
            input.move_right -= 1.0;
        }
        if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
            input.move_right += 1.0;
        }

        // Vertical (noclip) / Jump (normal)
        if noclip.enabled {
            if keyboard.pressed(KeyCode::Space) {
                input.move_up += 1.0;
            }
            if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
                input.move_up -= 1.0;
            }
        } else {
            input.jump = keyboard.pressed(KeyCode::Space);
        }

        // Run (only in normal mode — shift is used for down in noclip)
        if !noclip.enabled {
            input.run =
                keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
        }
    }
}

/// System to apply player movement via Tnua (physics mode).
///
/// Skipped when noclip is active (no Tnua controller on entity).
#[cfg(feature = "character-controller")]
pub fn player_movement_system(
    noclip: Res<NoclipState>,
    mut query: Query<
        (
            &PlayerConfig,
            &PlayerInput,
            &mut TnuaController<PlayerScheme>,
            &Transform,
        ),
        With<Player>,
    >,
) {
    if noclip.enabled {
        return;
    }
    for (config, input, mut controller, transform) in query.iter_mut() {
        // Calculate movement direction relative to player facing direction
        let forward = transform.forward().as_vec3().xz().normalize_or_zero();
        let right = transform.right().as_vec3().xz().normalize_or_zero();

        // Determine speed
        let speed = if input.run {
            config.run_speed
        } else {
            config.walk_speed
        };

        // Calculate desired velocity (2D → 3D)
        let move_dir = forward * input.move_forward + right * input.move_right;
        let move_dir = move_dir.normalize_or_zero();
        let desired_motion = Vec3::new(move_dir.x, 0.0, move_dir.y) * speed;

        // Apply to Tnua controller — set the basis input directly
        controller.basis = TnuaBuiltinWalk {
            desired_motion,
            ..Default::default()
        };

        // Must call every frame so Tnua knows when actions stop
        controller.initiate_action_feeding();

        // Jump
        if input.jump {
            controller.action(PlayerScheme::Jump(TnuaBuiltinJump::default()));
        }
    }
}

/// System to apply player movement directly via Transform (no physics).
#[cfg(not(feature = "character-controller"))]
pub fn player_movement_system(
    time: Res<Time>,
    noclip: Res<NoclipState>,
    mut query: Query<(&PlayerConfig, &PlayerInput, &mut Transform), With<Player>>,
) {
    if noclip.enabled {
        return;
    }
    for (config, input, mut transform) in query.iter_mut() {
        // Calculate movement direction relative to player facing direction
        let forward = transform.forward().as_vec3().xz().normalize_or_zero();
        let right = transform.right().as_vec3().xz().normalize_or_zero();

        // Determine speed
        let speed = if input.run {
            config.run_speed
        } else {
            config.walk_speed
        };

        // Calculate desired velocity (2D → 3D)
        let move_dir = forward * input.move_forward + right * input.move_right;
        let move_dir = move_dir.normalize_or_zero();

        // Apply movement directly to transform
        let velocity = Vec3::new(move_dir.x, 0.0, move_dir.y) * speed;
        transform.translation += velocity * time.delta_secs();

        // Simple jump (just move up briefly — no real gravity without physics)
        if input.jump {
            transform.translation.y += config.jump_force * time.delta_secs();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_player_params_default() {
        let params = SpawnPlayerParams::default();
        assert_eq!(params.position, [0.0, 1.0, 0.0]);
        assert_eq!(params.walk_speed, 5.0);
        assert_eq!(params.run_speed, 10.0);
        assert_eq!(params.jump_force, 8.0);
        assert_eq!(params.camera_mode, "third_person");
        assert_eq!(params.camera_distance, 5.0);
    }

    #[test]
    fn test_camera_mode_enum() {
        let params = SpawnPlayerParams {
            camera_mode: "first_person".to_string(),
            ..Default::default()
        };
        assert_eq!(params.camera_mode_enum(), CameraMode::FirstPerson);

        let params2 = SpawnPlayerParams {
            camera_mode: "firstperson".to_string(),
            ..Default::default()
        };
        assert_eq!(params2.camera_mode_enum(), CameraMode::FirstPerson);

        let params3 = SpawnPlayerParams::default();
        assert_eq!(params3.camera_mode_enum(), CameraMode::ThirdPerson);
    }

    #[test]
    fn test_player_config_from_params() {
        let params = SpawnPlayerParams {
            walk_speed: 7.0,
            run_speed: 14.0,
            jump_force: 10.0,
            camera_mode: "first_person".to_string(),
            camera_distance: 3.0,
            ..Default::default()
        };
        let config = params.to_config();
        assert_eq!(config.walk_speed, 7.0);
        assert_eq!(config.run_speed, 14.0);
        assert_eq!(config.jump_force, 10.0);
        assert_eq!(config.camera_mode, CameraMode::FirstPerson);
        assert_eq!(config.camera_distance, 3.0);
    }

    #[test]
    fn test_player_config_default() {
        let config = PlayerConfig::default();
        assert_eq!(config.walk_speed, 5.0);
        assert_eq!(config.run_speed, 10.0);
        assert_eq!(config.jump_force, 8.0);
        assert_eq!(config.camera_mode, CameraMode::ThirdPerson);
        assert_eq!(config.collision_radius, 0.3);
        assert_eq!(config.collision_height, 1.8);
    }

    #[test]
    fn test_player_input_default() {
        let input = PlayerInput::default();
        assert_eq!(input.move_forward, 0.0);
        assert_eq!(input.move_right, 0.0);
        assert_eq!(input.move_up, 0.0);
        assert!(!input.jump);
        assert!(!input.run);
    }

    #[test]
    fn test_noclip_state_default() {
        let state = NoclipState::default();
        assert!(!state.enabled);
    }

    #[test]
    fn test_spawn_params_collision() {
        let params = SpawnPlayerParams {
            collision_radius: 0.5,
            collision_height: 2.0,
            ..Default::default()
        };
        assert_eq!(params.collision_radius, 0.5);
        assert_eq!(params.collision_height, 2.0);
        let config = params.to_config();
        assert_eq!(config.collision_radius, 0.5);
        assert_eq!(config.collision_height, 2.0);
    }
}

/// System to toggle noclip mode with the N key.
///
/// Noclip ON: removes `Collider` and `RigidBody`, enabling free flight through geometry.
/// Noclip OFF: re-inserts physics components so the player falls with gravity.
#[cfg(feature = "character-controller")]
pub fn noclip_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut noclip: ResMut<NoclipState>,
    mut commands: Commands,
    player_query: Query<(Entity, &PlayerConfig), With<Player>>,
    mut notifications: MessageWriter<crate::ui::notification::NotificationEvent>,
) {
    if !keyboard.just_pressed(KeyCode::KeyN) {
        return;
    }

    let Ok((entity, config)) = player_query.single() else {
        return;
    };

    noclip.enabled = !noclip.enabled;

    if noclip.enabled {
        // Remove physics components — player now flies freely
        commands
            .entity(entity)
            .remove::<Collider>()
            .remove::<RigidBody>()
            .remove::<TnuaController<PlayerScheme>>()
            .remove::<TnuaAvian3dSensorShape>()
            .remove::<LockedAxes>();
        info!("Noclip ON");
        notifications.write(crate::ui::notification::NotificationEvent {
            text: "Noclip ON".into(),
            style: crate::ui::notification::NotificationStyle::Toast,
            position: crate::ui::notification::NotificationPosition::Top,
            icon: crate::ui::notification::NotificationIcon::None,
        });
    } else {
        // Re-insert physics components
        commands.entity(entity).insert((
            RigidBody::Dynamic,
            Collider::capsule(
                config.collision_radius,
                config.collision_height - config.collision_radius * 2.0,
            ),
            LockedAxes::new().lock_rotation_x().lock_rotation_z(),
            TnuaController::<PlayerScheme>::default(),
            TnuaAvian3dSensorShape(Collider::cylinder(config.collision_radius * 0.95, 0.0)),
        ));
        info!("Noclip OFF");
        notifications.write(crate::ui::notification::NotificationEvent {
            text: "Noclip OFF".into(),
            style: crate::ui::notification::NotificationStyle::Toast,
            position: crate::ui::notification::NotificationPosition::Top,
            icon: crate::ui::notification::NotificationIcon::None,
        });
    }
}

/// Noclip toggle stub (no-op without physics feature).
#[cfg(not(feature = "character-controller"))]
pub fn noclip_toggle_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut noclip: ResMut<NoclipState>,
    mut notifications: MessageWriter<crate::ui::notification::NotificationEvent>,
) {
    if !keyboard.just_pressed(KeyCode::KeyN) {
        return;
    }
    noclip.enabled = !noclip.enabled;
    let label = if noclip.enabled { "ON" } else { "OFF" };
    info!("Noclip {label}");
    notifications.write(crate::ui::notification::NotificationEvent {
        text: format!("Noclip {label}"),
        style: crate::ui::notification::NotificationStyle::Toast,
        position: crate::ui::notification::NotificationPosition::Top,
        icon: crate::ui::notification::NotificationIcon::None,
    });
}

/// Noclip movement: direct Transform translation (6DOF, no gravity).
///
/// Only runs when `NoclipState.enabled` is true.
pub fn noclip_movement_system(
    time: Res<Time>,
    noclip: Res<NoclipState>,
    mut query: Query<(&PlayerConfig, &PlayerInput, &mut Transform), With<Player>>,
) {
    if !noclip.enabled {
        return;
    }

    for (config, input, mut transform) in query.iter_mut() {
        let speed = config.run_speed; // noclip always uses run speed

        // Horizontal movement relative to player facing direction
        let forward = transform.forward().as_vec3().xz().normalize_or_zero();
        let right = transform.right().as_vec3().xz().normalize_or_zero();

        let move_dir = forward * input.move_forward + right * input.move_right;
        let move_dir = move_dir.normalize_or_zero();

        let velocity = Vec3::new(move_dir.x, input.move_up, move_dir.y) * speed * time.delta_secs();
        transform.translation += velocity;
    }
}

/// Plugin for player systems.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NoclipState>().add_systems(
            Update,
            (
                noclip_toggle_system,
                player_input_system,
                noclip_movement_system,
                player_movement_system,
            )
                .chain()
                .run_if(crate::gen3d::avatar::in_player_mode),
        );
    }
}
