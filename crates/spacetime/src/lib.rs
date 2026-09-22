//! LocalGPT World - SpacetimeDB Multiplayer Server
//!
//! A multiplayer 3D world server using SpacetimeDB 2.0 for real-time sync.
//! Shares types with the gen crate via localgpt-world-types.

use spacetimedb::{reducer, table, Identity, ReducerContext, Timestamp, Table};

pub mod jobs;

// Re-export world types for clients
pub use localgpt_world_types::{
    EntityId, EntityName, WorldEntity, WorldTransform,
    Shape, MaterialDef, LightDef, BehaviorDef, AudioDef,
    ChunkCoord,
};

/// Chunk size constant (64×64 units)
pub const CHUNK_SIZE: f32 = 64.0;

// ============================================================================
// Tables (SpacetimeDB 2.0 API)
// ============================================================================

/// A player in the world (mobile + web clients)
#[table(accessor = player, public)]
pub struct Player {
    #[primary_key]
    pub identity: Identity,
    pub device: String,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rotation_y: f32,
    pub online: bool,
    pub last_seen: Timestamp,
}

/// A world entity synced from gen or created by players
#[table(accessor = world_entity, public)]
pub struct WorldEntityRow {
    #[primary_key]
    pub id: u64,
    pub name: String,
    pub entity_type: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rot_pitch: f32,
    pub rot_yaw: f32,
    pub rot_roll: f32,
    pub scale: f32,
    pub shape_json: String,
    pub material_json: Option<String>,
    pub light_json: Option<String>,
    pub behaviors_json: String,
    pub audio_json: Option<String>,
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub owner: Option<Identity>,
    pub created_at: Timestamp,
}

/// Chat messages
#[table(accessor = chat_message, public)]
pub struct ChatMessage {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub sender_identity: Identity,
    pub sender_name: String,
    pub message: String,
    pub timestamp: Timestamp,
    pub msg_type: String,
}

/// World metadata (singleton)
#[table(accessor = world_info, public)]
pub struct WorldInfo {
    #[primary_key]
    pub id: u64,
    pub name: String,
    pub description: String,
    pub seed: u64,
    pub width: f32,
    pub depth: f32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Entity ID counter
#[table(accessor = entity_counter)]
pub struct EntityCounter {
    #[primary_key]
    pub id: u8,
    pub count: u64,
}

/// Chunk subscriptions for mobile streaming
#[table(accessor = chunk_subscription)]
pub struct ChunkSubscription {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub identity: Identity,
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub subscribed_at: Timestamp,
}

// ============================================================================
// Lifecycle
// ============================================================================

#[reducer]
pub fn __identity_connected(ctx: &ReducerContext) {
    let sender = ctx.sender();

    // Initialize or reconnect player
    if let Some(mut player) = ctx.db.player().identity().find(&sender) {
        player.online = true;
        player.last_seen = ctx.timestamp;
        ctx.db.player().identity().update(player);
    } else {
        ctx.db.player().insert(Player {
            identity: sender,
            device: "unknown".to_string(),
            name: format!("Player_{}", &sender.to_hex()[..8]),
            x: 0.0,
            y: 1.0,
            z: 0.0,
            rotation_y: 0.0,
            online: true,
            last_seen: ctx.timestamp,
        });
    }

    // Initialize world if needed
    if ctx.db.world_info().id().find(&0).is_none() {
        let seed = ctx.timestamp.to_micros_since_unix_epoch() as u64;
        ctx.db.world_info().insert(WorldInfo {
            id: 0,
            name: "LocalGPT World".to_string(),
            description: "A procedurally generated multiplayer world".to_string(),
            seed,
            width: 200.0,
            depth: 200.0,
            created_at: ctx.timestamp,
            updated_at: ctx.timestamp,
        });

        generate_world(ctx, seed);
    }
}

#[reducer]
pub fn __identity_disconnected(ctx: &ReducerContext) {
    if let Some(mut player) = ctx.db.player().identity().find(&ctx.sender()) {
        player.online = false;
        player.last_seen = ctx.timestamp;
        ctx.db.player().identity().update(player);
    }

    // Cancel queued prompts / requeue a disconnected worker's claim.
    jobs::on_disconnect(ctx);

    // Clean up chunk subscriptions
    let sender = ctx.sender();
    let subs: Vec<_> = ctx.db.chunk_subscription().iter()
        .filter(|s| s.identity == sender)
        .collect();
    for sub in subs {
        ctx.db.chunk_subscription().id().delete(sub.id);
    }
}

// ============================================================================
// Player Actions
// ============================================================================

#[reducer]
pub fn set_device(ctx: &ReducerContext, device: String) {
    if let Some(mut player) = ctx.db.player().identity().find(&ctx.sender()) {
        player.device = device;
        ctx.db.player().identity().update(player);
    }
}

#[reducer]
pub fn set_player_name(ctx: &ReducerContext, name: String) {
    if let Some(mut player) = ctx.db.player().identity().find(&ctx.sender()) {
        let name: String = name.chars().take(32).collect();
        player.name = name;
        ctx.db.player().identity().update(player);
    }
}

#[reducer]
pub fn move_player(ctx: &ReducerContext, x: f32, y: f32, z: f32, rotation_y: f32) {
    if let Some(mut player) = ctx.db.player().identity().find(&ctx.sender()) {
        player.x = x;
        player.y = y;
        player.z = z;
        player.rotation_y = rotation_y;
        player.last_seen = ctx.timestamp;
        ctx.db.player().identity().update(player);
    }
}

#[reducer]
pub fn send_chat(ctx: &ReducerContext, message: String) {
    if let Some(player) = ctx.db.player().identity().find(&ctx.sender()) {
        let message: String = message.chars().take(500).collect();
        if !message.trim().is_empty() {
            ctx.db.chat_message().insert(ChatMessage {
                id: 0,
                sender_identity: ctx.sender(),
                sender_name: player.name.clone(),
                message,
                timestamp: ctx.timestamp,
                msg_type: "chat".to_string(),
            });
        }
    }
}

#[reducer]
pub fn subscribe_chunk(ctx: &ReducerContext, chunk_x: i32, chunk_y: i32) {
    ctx.db.chunk_subscription().insert(ChunkSubscription {
        id: 0,
        identity: ctx.sender(),
        chunk_x,
        chunk_y,
        subscribed_at: ctx.timestamp,
    });
}

#[reducer]
pub fn unsubscribe_chunk(ctx: &ReducerContext, chunk_x: i32, chunk_y: i32) {
    let sender = ctx.sender();
    let subs: Vec<_> = ctx.db.chunk_subscription().iter()
        .filter(|s| s.identity == sender && s.chunk_x == chunk_x && s.chunk_y == chunk_y)
        .collect();
    for sub in subs {
        ctx.db.chunk_subscription().id().delete(sub.id);
    }
}

// ============================================================================
// World Generation
// ============================================================================

fn generate_world(ctx: &ReducerContext, seed: u64) {
    use rand::prelude::*;

    let mut rng = StdRng::seed_from_u64(seed);
    let mut counter = 0u64;

    // Generate trees
    for _ in 0..80 {
        let x = rng.gen_range(-100.0..100.0);
        let z = rng.gen_range(-100.0..100.0);
        let entity = create_tree_entity(next_entity_id(&mut counter), x, z, &mut rng, ctx.timestamp);
        ctx.db.world_entity().insert(entity);
    }

    // Generate rocks
    for _ in 0..40 {
        let x = rng.gen_range(-100.0..100.0);
        let z = rng.gen_range(-100.0..100.0);
        let entity = create_rock_entity(next_entity_id(&mut counter), x, z, &mut rng, ctx.timestamp);
        ctx.db.world_entity().insert(entity);
    }

    // Central building
    ctx.db.world_entity().insert(WorldEntityRow {
        id: next_entity_id(&mut counter),
        name: "town_center".to_string(),
        entity_type: "building".to_string(),
        x: 0.0, y: 0.0, z: 0.0,
        rot_pitch: 0.0, rot_yaw: 0.0, rot_roll: 0.0,
        scale: 1.0,
        shape_json: serde_json::to_string(&Shape::Cuboid { x: 4.0, y: 3.0, z: 4.0 }).unwrap_or_default(),
        material_json: Some(serde_json::to_string(&MaterialDef::default()).unwrap_or_default()),
        light_json: None,
        behaviors_json: "[]".to_string(),
        audio_json: None,
        chunk_x: 0,
        chunk_y: 0,
        owner: None,
        created_at: ctx.timestamp,
    });

    ctx.db.entity_counter().insert(EntityCounter { id: 0, count: counter });
}

fn create_tree_entity(id: u64, x: f32, z: f32, rng: &mut impl rand::Rng, timestamp: Timestamp) -> WorldEntityRow {
    let scale = rng.gen_range(0.8..1.3);
    WorldEntityRow {
        id,
        name: format!("tree_{}", id),
        entity_type: "tree".to_string(),
        x, y: 0.0, z,
        rot_pitch: 0.0, rot_yaw: rng.gen_range(0.0..360.0), rot_roll: 0.0,
        scale,
        shape_json: serde_json::to_string(&Shape::Cylinder { radius: 0.3, height: 2.0 }).unwrap_or_default(),
        material_json: Some(serde_json::to_string(&MaterialDef { color: [0.4, 0.25, 0.15, 1.0], ..Default::default() }).unwrap_or_default()),
        light_json: None,
        behaviors_json: "[]".to_string(),
        audio_json: None,
        chunk_x: (x / CHUNK_SIZE).floor() as i32,
        chunk_y: (z / CHUNK_SIZE).floor() as i32,
        owner: None,
        created_at: timestamp,
    }
}

fn create_rock_entity(id: u64, x: f32, z: f32, rng: &mut impl rand::Rng, timestamp: Timestamp) -> WorldEntityRow {
    let scale = rng.gen_range(0.5..1.5);
    WorldEntityRow {
        id,
        name: format!("rock_{}", id),
        entity_type: "rock".to_string(),
        x, y: 0.0, z,
        rot_pitch: 0.0, rot_yaw: rng.gen_range(0.0..360.0), rot_roll: 0.0,
        scale,
        shape_json: serde_json::to_string(&Shape::Sphere { radius: 0.5 }).unwrap_or_default(),
        material_json: Some(serde_json::to_string(&MaterialDef { color: [0.5, 0.5, 0.5, 1.0], roughness: 0.9, ..Default::default() }).unwrap_or_default()),
        light_json: None,
        behaviors_json: "[]".to_string(),
        audio_json: None,
        chunk_x: (x / CHUNK_SIZE).floor() as i32,
        chunk_y: (z / CHUNK_SIZE).floor() as i32,
        owner: None,
        created_at: timestamp,
    }
}

fn next_entity_id(counter: &mut u64) -> u64 {
    *counter += 1;
    *counter
}

// ============================================================================
// Entity Management
// ============================================================================

#[reducer]
pub fn spawn_entity(
    ctx: &ReducerContext,
    name: String,
    entity_type: String,
    x: f32, y: f32, z: f32,
    rot_pitch: f32, rot_yaw: f32, rot_roll: f32,
    scale: f32,
    shape_json: String,
    material_json: Option<String>,
    light_json: Option<String>,
    behaviors_json: String,
    audio_json: Option<String>,
) {
    let mut counter = ctx.db.entity_counter().id().find(&0).map(|c| c.count).unwrap_or(0);
    counter += 1;
    ctx.db.entity_counter().id().update(EntityCounter { id: 0, count: counter });

    ctx.db.world_entity().insert(WorldEntityRow {
        id: counter,
        name,
        entity_type,
        x, y, z,
        rot_pitch, rot_yaw, rot_roll,
        scale,
        shape_json,
        material_json,
        light_json,
        behaviors_json,
        audio_json,
        chunk_x: (x / CHUNK_SIZE).floor() as i32,
        chunk_y: (z / CHUNK_SIZE).floor() as i32,
        owner: Some(ctx.sender()),
        created_at: ctx.timestamp,
    });

    if let Some(mut info) = ctx.db.world_info().id().find(&0) {
        info.updated_at = ctx.timestamp;
        ctx.db.world_info().id().update(info);
    }
}

#[reducer]
pub fn remove_entity(ctx: &ReducerContext, entity_id: u64) {
    ctx.db.world_entity().id().delete(entity_id);
}

#[reducer]
pub fn regenerate_world(ctx: &ReducerContext, seed: Option<u64>) {
    for entity in ctx.db.world_entity().iter() {
        ctx.db.world_entity().id().delete(entity.id);
    }

    let seed = seed.unwrap_or_else(|| ctx.timestamp.to_micros_since_unix_epoch() as u64);

    if let Some(mut info) = ctx.db.world_info().id().find(&0) {
        info.seed = seed;
        info.updated_at = ctx.timestamp;
        ctx.db.world_info().id().update(info);
    }

    ctx.db.entity_counter().id().update(EntityCounter { id: 0, count: 0 });
    generate_world(ctx, seed);
}

#[reducer]
pub fn clear_world(ctx: &ReducerContext) {
    for entity in ctx.db.world_entity().iter() {
        ctx.db.world_entity().id().delete(entity.id);
    }
    ctx.db.entity_counter().id().update(EntityCounter { id: 0, count: 0 });

    if let Some(mut info) = ctx.db.world_info().id().find(&0) {
        info.updated_at = ctx.timestamp;
        ctx.db.world_info().id().update(info);
    }
}
