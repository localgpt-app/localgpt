//! Guest avatars in the host's own window (spec phase 2, host side).
//!
//! Each web guest with presence gets a colored capsule in the host's scene,
//! labeled with its name, lerped toward its latest position. These are pure
//! visuals: no `GenEntity`, no registry entry, so they never replicate, never
//! enter the projection diff, and never save with the world.

use std::collections::HashMap;

use bevy::prelude::*;

use super::web::WebRoom;
use localgpt_world_sync::PeerId;

/// Peer id → the avatar entity in the host's scene.
#[derive(Resource, Default)]
pub(crate) struct GuestAvatars {
    entities: HashMap<PeerId, Entity>,
}

/// Per-peer smoothing state.
#[derive(Component)]
pub(crate) struct GuestAvatarTarget {
    position: Vec3,
    look_at: Vec3,
}

const AVATAR_COLORS: [Color; 8] = [
    Color::srgb(0.36, 0.55, 0.85),
    Color::srgb(0.85, 0.37, 0.37),
    Color::srgb(0.37, 0.85, 0.54),
    Color::srgb(0.85, 0.71, 0.37),
    Color::srgb(0.65, 0.37, 0.85),
    Color::srgb(0.37, 0.82, 0.85),
    Color::srgb(0.85, 0.37, 0.63),
    Color::srgb(0.56, 0.85, 0.37),
];

/// Spawn/despawn avatar capsules as peers gain and lose presence, and drive
/// them toward their latest reported positions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn web_guest_avatars(
    mut commands: Commands,
    room: Option<Res<WebRoom>>,
    mut avatars: ResMut<GuestAvatars>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut targets: Query<&mut GuestAvatarTarget>,
    mut transforms: Query<&mut Transform>,
    time: Res<Time>,
) {
    let Some(room) = room else {
        // Session ended: remove every avatar.
        for entity in avatars.entities.drain().map(|(_, e)| e) {
            commands.entity(entity).despawn();
        }
        return;
    };

    // Current peers that have reported presence at least once.
    let present: HashMap<PeerId, (String, [f32; 3], [f32; 3])> = room
        .authority
        .peers()
        .filter_map(|p| {
            p.presence
                .as_ref()
                .map(|pr| (p.id, (p.name.clone(), pr.position, pr.look_at)))
        })
        .collect();

    // Despawn avatars for peers that left (or never showed presence).
    let gone: Vec<PeerId> = avatars
        .entities
        .keys()
        .filter(|id| !present.contains_key(*id))
        .copied()
        .collect();
    for id in gone {
        if let Some(entity) = avatars.entities.remove(&id) {
            commands.entity(entity).despawn();
        }
    }

    // Spawn avatars for newly-present peers.
    for (id, (name, position, look_at)) in &present {
        if avatars.entities.contains_key(id) {
            continue;
        }
        let color = AVATAR_COLORS[(id % AVATAR_COLORS.len() as u64) as usize];
        let entity = commands
            .spawn((
                Name::new(format!("Guest: {name}")),
                Mesh3d(meshes.add(Capsule3d::new(0.3, 0.9))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    ..default()
                })),
                Transform::from_translation(Vec3::from_array(*position)),
                GuestAvatarTarget {
                    position: Vec3::from_array(*position),
                    look_at: Vec3::from_array(*look_at),
                },
            ))
            .id();
        avatars.entities.insert(*id, entity);
    }

    // Update targets, then ease toward them.
    for (id, (_, position, look_at)) in &present {
        if let Some(&entity) = avatars.entities.get(id)
            && let Ok(mut target) = targets.get_mut(entity)
        {
            target.position = Vec3::from_array(*position);
            target.look_at = Vec3::from_array(*look_at);
        }
    }
    let blend = 1.0 - (-8.0 * time.delta_secs()).exp();
    for &entity in avatars.entities.values() {
        let Ok(target) = targets.get(entity) else {
            continue;
        };
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = transform.translation.lerp(target.position, blend);
            let dir = target.look_at - transform.translation;
            if dir.length_squared() > 0.01 {
                transform.look_at(target.look_at, Vec3::Y);
            }
        }
    }
}
