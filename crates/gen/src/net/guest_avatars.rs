//! Guest avatars (spec phase 2): each peer with presence gets a colored
//! capsule in the scene, eased toward its latest position.
//!
//! One shared core ([`sync_avatars`]) drives both the host's window (peers
//! from the room authority) and a native client's window (peers from its
//! connection). The markers are pure visuals: no `GenEntity`, no registry
//! entry, so they never replicate, never enter the projection diff, and
//! never save with the world.

use std::collections::HashMap;

use bevy::prelude::*;
use localgpt_world_sync::{PeerId, Presence};

use super::web::WebRoom;

/// Peer id → the avatar entity, on whichever side renders it.
#[derive(Resource, Default)]
pub struct GuestAvatars {
    pub entities: HashMap<PeerId, Entity>,
}

/// Per-peer smoothing state.
#[derive(Component)]
pub struct GuestAvatarTarget {
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

/// Spawn/despawn/ease avatar capsules for the given peers. `present` yields
/// `(peer id, name, latest presence)` for everyone who should be visible.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_avatars(
    commands: &mut Commands,
    present: impl Iterator<Item = (PeerId, String, Presence)>,
    entities: &mut HashMap<PeerId, Entity>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    targets: &mut Query<&mut GuestAvatarTarget>,
    transforms: &mut Query<&mut Transform>,
    time: &Time,
) {
    let present: HashMap<PeerId, (String, Presence)> =
        present.map(|(id, name, pr)| (id, (name, pr))).collect();

    // Despawn avatars for peers that left (or never showed presence).
    let gone: Vec<PeerId> = entities
        .keys()
        .filter(|id| !present.contains_key(*id))
        .copied()
        .collect();
    for id in gone {
        if let Some(entity) = entities.remove(&id) {
            commands.entity(entity).despawn();
        }
    }

    // Spawn avatars for newly-present peers.
    for (id, (name, presence)) in &present {
        if entities.contains_key(id) {
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
                Transform::from_translation(Vec3::from_array(presence.position)),
                GuestAvatarTarget {
                    position: Vec3::from_array(presence.position),
                    look_at: Vec3::from_array(presence.look_at),
                },
            ))
            .id();
        entities.insert(*id, entity);
    }

    // Update targets, then ease toward them.
    for (id, (_, presence)) in &present {
        if let Some(&entity) = entities.get(id)
            && let Ok(mut target) = targets.get_mut(entity)
        {
            target.position = Vec3::from_array(presence.position);
            target.look_at = Vec3::from_array(presence.look_at);
        }
    }
    let blend = 1.0 - (-8.0 * time.delta_secs()).exp();
    for &entity in entities.values() {
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

/// Host side: avatars for the room authority's peers, and cleanup when the
/// session ends.
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
        for entity in avatars.entities.drain().map(|(_, e)| e) {
            commands.entity(entity).despawn();
        }
        return;
    };
    sync_avatars(
        &mut commands,
        room.authority
            .peers()
            .filter_map(|p| p.presence.clone().map(|pr| (p.id, p.name.clone(), pr))),
        &mut avatars.entities,
        &mut meshes,
        &mut materials,
        &mut targets,
        &mut transforms,
        &time,
    );
}
