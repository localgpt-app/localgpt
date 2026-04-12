//! Interaction & Trigger System (P2)
//!
//! These specs add gameplay to LocalGPT Gen.
//! Following the **Trigger → State Change → Behavior Response** pattern.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Marker components
// ---------------------------------------------------------------------------

/// Marker for entities that can be interacted via triggers.
#[derive(Component)]
pub struct InteractionEntity;

// ---------------------------------------------------------------------------
// Trigger components
// ---------------------------------------------------------------------------

/// Marker for proximity triggers.
#[derive(Component, Clone)]
pub struct ProximityTrigger {
    pub radius: f32,
    pub cooldown: f32,
    pub last_triggered: f32,
}

impl Default for ProximityTrigger {
    fn default() -> Self {
        Self {
            radius: 5.0,
            cooldown: 1.0,
            last_triggered: 0.0,
        }
    }
}

/// Marker for click triggers.
#[derive(Component, Clone)]
pub struct ClickTrigger {
    pub max_distance: f32,
    pub prompt_text: Option<String>,
}

impl Default for ClickTrigger {
    fn default() -> Self {
        Self {
            max_distance: 5.0,
            prompt_text: None,
        }
    }
}

/// Marker for area triggers.
#[derive(Component, Clone)]
pub struct AreaTrigger {
    pub is_enter: bool,
}

impl Default for AreaTrigger {
    fn default() -> Self {
        Self { is_enter: true }
    }
}

/// Marker for collision triggers (Avian sensor-based).
///
/// Unlike proximity triggers (distance check), collision triggers use Avian physics
/// sensor colliders to detect overlap. The entity must have an Avian `Collider` +
/// `Sensor` marker for this to work.
#[derive(Component, Clone)]
pub struct CollisionTrigger {
    /// Cooldown between trigger fires (seconds).
    pub cooldown: f32,
    /// Timestamp of last trigger fire.
    pub last_triggered: f32,
    /// Entities currently overlapping this trigger volume.
    pub overlapping: HashSet<Entity>,
}

impl Default for CollisionTrigger {
    fn default() -> Self {
        Self {
            cooldown: 1.0,
            last_triggered: 0.0,
            overlapping: HashSet::new(),
        }
    }
}

/// Marker for timer triggers.
#[derive(Component, Clone)]
pub struct TimerTrigger {
    pub interval: f32,
    pub timer: Timer,
}

impl TimerTrigger {
    pub fn new(interval: f32) -> Self {
        Self {
            interval,
            timer: Timer::from_seconds(interval, TimerMode::Repeating),
        }
    }
}

// ---------------------------------------------------------------------------
// Action components
// ---------------------------------------------------------------------------

/// Action to animate a transform property.
#[derive(Component, Clone)]
pub struct AnimateAction {
    pub property: String,
    pub to: Vec<f32>,
    pub duration: f32,
    pub progress: f32,
}

/// Action to teleport the player.
#[derive(Component, Clone)]
pub struct TeleportAction {
    pub destination: Vec3,
    pub effect: TeleportEffect,
}

/// Action to show floating text.
#[derive(Component, Clone)]
pub struct ShowTextAction {
    pub text: String,
    pub duration: Option<f32>,
}

/// Action to toggle entity state.
#[derive(Component, Clone)]
pub struct ToggleStateAction {
    pub state_key: String,
    pub value: Option<String>,
}

/// Action to add to score.
#[derive(Component, Clone)]
pub struct AddScoreAction {
    pub amount: i32,
    pub category: String,
}

/// Action to play a sound.
#[derive(Component, Clone)]
pub struct PlaySoundAction {
    pub sound: String,
}

/// Action to spawn an entity.
#[derive(Component, Clone)]
pub struct SpawnAction {
    pub template: String,
}

/// Action to destroy the entity.
#[derive(Component, Clone)]
pub struct DestroyAction;

/// Action to enable the entity.
#[derive(Component, Clone)]
pub struct EnableAction;

/// Action to disable the entity.
#[derive(Component, Clone)]
pub struct DisableAction;

/// Marker for triggers that should fire only once.
#[derive(Component, Clone)]
pub struct OnceTrigger;

// ---------------------------------------------------------------------------
// Entity linking
// ---------------------------------------------------------------------------

/// Links a trigger on this entity to an action on another entity.
#[derive(Component, Clone)]
pub struct EntityLink {
    pub source_event: String,
    pub target_entity: String,
    pub target_action: String,
    pub condition: Option<String>,
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Global score tracking.
#[derive(Resource, Clone, Default)]
pub struct ScoreBoard {
    pub scores: HashMap<String, i32>,
}

/// Message fired when score changes.
#[derive(Clone, Debug)]
pub struct ScoreChanged {
    pub category: String,
    pub new_value: i32,
    pub delta: i32,
}

/// Message fired when a trigger activates.
#[derive(Message, Clone, Debug)]
pub struct TriggerFired {
    pub entity: Entity,
    pub trigger_type: TriggerType,
}

/// Entity state storage.
#[derive(Component, Clone, Default)]
pub struct EntityState {
    pub states: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Enums for trigger/interaction parameters
// ---------------------------------------------------------------------------

/// Trigger activation type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    #[default]
    Proximity,
    Click,
    AreaEnter,
    AreaExit,
    Collision,
    Timer,
}

/// Action performed when trigger fires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum TriggerAction {
    #[default]
    Animate,
    Teleport,
    PlaySound,
    ShowText,
    ToggleState,
    Spawn,
    Destroy,
    AddScore,
    Enable,
    Disable,
}

/// Teleporter visual effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "lowercase")]
pub enum TeleportEffect {
    #[default]
    None,
    Fade,
    Particles,
}

/// Door activation trigger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "lowercase")]
pub enum DoorTrigger {
    #[default]
    Proximity,
    Click,
}

// ---------------------------------------------------------------------------
// MCP Tool parameters
// ---------------------------------------------------------------------------

/// Parameters for adding a trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTriggerParams {
    pub entity_id: String,
    pub trigger_type: TriggerType,
    pub action: TriggerAction,
    #[serde(default)]
    pub radius: Option<f32>,
    #[serde(default)]
    pub cooldown: Option<f32>,
    #[serde(default)]
    pub interval: Option<f32>,
    #[serde(default)]
    pub max_distance: Option<f32>,
    #[serde(default)]
    pub prompt_text: Option<String>,
    #[serde(default)]
    pub once: bool,
    // Action-specific parameters
    /// Teleport destination [x, y, z].
    #[serde(default)]
    pub destination: Option<[f32; 3]>,
    /// Text for show_text action.
    #[serde(default)]
    pub text: Option<String>,
    /// Score amount for add_score action.
    #[serde(default)]
    pub amount: Option<i32>,
    /// State key for toggle_state action.
    #[serde(default)]
    pub state_key: Option<String>,
    /// Score category for add_score action.
    #[serde(default)]
    pub category: Option<String>,
    /// Require the player to have this item in inventory before the trigger fires.
    #[serde(default)]
    pub requires_item: Option<String>,
}

/// Parameters for teleporter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeleporterParams {
    pub position: [f32; 3],
    pub destination: [f32; 3],
    #[serde(default = "default_teleporter_size")]
    pub size: [f32; 3],
    #[serde(default)]
    pub effect: TeleportEffect,
    #[serde(default)]
    pub sound: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

fn default_teleporter_size() -> [f32; 3] {
    [2.0, 3.0, 2.0]
}

/// Pickup visual effect for collectibles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "lowercase")]
pub enum PickupEffect {
    None,
    #[default]
    Sparkle,
    Dissolve,
}

/// Parameters for collectible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectibleParams {
    pub entity_id: String,
    #[serde(default)]
    pub value: i32,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub pickup_sound: Option<String>,
    #[serde(default)]
    pub pickup_effect: PickupEffect,
    #[serde(default)]
    pub respawn_time: Option<f32>,
}

impl Default for CollectibleParams {
    fn default() -> Self {
        Self {
            entity_id: String::new(),
            value: 1,
            category: "points".to_string(),
            pickup_sound: None,
            pickup_effect: PickupEffect::default(),
            respawn_time: None,
        }
    }
}

/// Parameters for door.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorParams {
    pub entity_id: String,
    #[serde(default)]
    pub trigger: DoorTrigger,
    #[serde(default = "default_open_angle")]
    pub open_angle: f32,
    #[serde(default = "default_open_duration")]
    pub open_duration: f32,
    #[serde(default = "default_auto_close")]
    pub auto_close: bool,
    #[serde(default = "default_auto_close_delay")]
    pub auto_close_delay: f32,
    #[serde(default)]
    pub sound_open: Option<String>,
    #[serde(default)]
    pub sound_close: Option<String>,
    #[serde(default)]
    pub requires_key: Option<String>,
}

fn default_open_angle() -> f32 {
    90.0
}
fn default_open_duration() -> f32 {
    1.5
}
fn default_auto_close() -> bool {
    true
}
fn default_auto_close_delay() -> f32 {
    3.0
}

impl Default for DoorParams {
    fn default() -> Self {
        Self {
            entity_id: String::new(),
            trigger: DoorTrigger::default(),
            open_angle: default_open_angle(),
            open_duration: default_open_duration(),
            auto_close: default_auto_close(),
            auto_close_delay: default_auto_close_delay(),
            sound_open: None,
            sound_close: None,
            requires_key: None,
        }
    }
}

/// Parameters for entity linking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkEntitiesParams {
    pub source_id: String,
    pub source_event: String,
    pub target_id: String,
    pub target_action: String,
    #[serde(default)]
    pub condition: Option<String>,
}

// ---------------------------------------------------------------------------
// Collectible component
// ---------------------------------------------------------------------------

/// Collectible component with idle animation.
#[derive(Component, Clone)]
pub struct Collectible {
    pub value: i32,
    pub category: String,
    pub pickup_effect: PickupEffect,
    pub respawn_time: Option<f32>,
    pub original_position: Vec3,
    pub respawn_timer: Option<Timer>,
}

// ---------------------------------------------------------------------------
// Door component
// ---------------------------------------------------------------------------

/// Door state machine.
#[derive(Debug, Clone, Default)]
pub enum DoorState {
    #[default]
    Closed,
    Opening {
        progress: f32,
    },
    Open {
        close_timer: Timer,
    },
    Closing {
        progress: f32,
    },
}

/// Door component.
#[derive(Component, Clone)]
pub struct Door {
    pub state: DoorState,
    pub open_angle: f32,
    pub open_duration: f32,
    pub auto_close: bool,
    pub auto_close_delay: f32,
    pub requires_key: Option<String>,
    pub original_rotation: Quat,
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

/// Resource for tracking player inventory.
///
/// Stores collected item IDs as a set. Used for gating interactions
/// (doors with `requires_key`, triggers with `requires_item`).
#[derive(Resource, Clone, Default)]
pub struct PlayerInventory {
    /// Item IDs the player has collected.
    pub items: HashSet<String>,
}

impl PlayerInventory {
    /// Check if the player has a specific key/item.
    pub fn has_key(&self, key: &str) -> bool {
        self.items.contains(key)
    }

    /// Check if the player has a specific item (alias for has_key).
    pub fn has_item(&self, item: &str) -> bool {
        self.items.contains(item)
    }

    /// Add an item to the inventory.
    pub fn add_item(&mut self, item: String) {
        self.items.insert(item);
    }

    /// Remove an item from the inventory. Returns true if it was present.
    pub fn remove_item(&mut self, item: &str) -> bool {
        self.items.remove(item)
    }

    /// Number of items in inventory.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the inventory is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Component for items that can be picked up and added to inventory.
///
/// When the player walks within pickup range of an entity with this component,
/// the item_id is added to `PlayerInventory` and the entity is despawned.
#[derive(Component, Clone)]
pub struct CollectibleItem {
    /// Unique item identifier (e.g., "gold_key", "red_gem").
    pub item_id: String,
    /// Pickup range (distance from player to trigger pickup).
    pub pickup_range: f32,
    /// Optional notification text shown on pickup.
    pub pickup_message: Option<String>,
}

impl Default for CollectibleItem {
    fn default() -> Self {
        Self {
            item_id: String::new(),
            pickup_range: 2.0,
            pickup_message: None,
        }
    }
}

/// Component that gates a trigger on having a specific inventory item.
///
/// Added alongside trigger components (ProximityTrigger, ClickTrigger, etc.)
/// to require the player to possess a specific item before the trigger fires.
#[derive(Component, Clone)]
pub struct RequiresItem {
    /// Item ID required in PlayerInventory.
    pub item_id: String,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Runtime systems
// ---------------------------------------------------------------------------

/// System: check proximity triggers against the player each frame.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn proximity_trigger_system(
    time: Res<Time>,
    mut player_query: Query<
        &mut Transform,
        (With<crate::character::Player>, Without<ProximityTrigger>),
    >,
    mut trigger_query: Query<(
        Entity,
        &Transform,
        &mut ProximityTrigger,
        Option<&TeleportAction>,
        Option<&AddScoreAction>,
        Option<&ToggleStateAction>,
        Option<&PlaySoundAction>,
        Option<&OnceTrigger>,
        Option<&RequiresItem>,
    )>,
    mut score_board: ResMut<ScoreBoard>,
    mut trigger_events: MessageWriter<TriggerFired>,
    mut commands: Commands,
    mut audio_engine: ResMut<crate::gen3d::audio::AudioEngine>,
    inventory: Res<PlayerInventory>,
) {
    let player_pos = if let Ok(player_transform) = player_query.single() {
        player_transform.translation
    } else {
        return;
    };
    let now = time.elapsed_secs();

    for (entity, transform, mut trigger, teleport, score, toggle, play_sound, once, requires) in
        trigger_query.iter_mut()
    {
        let distance = player_pos.distance(transform.translation);
        if distance > trigger.radius {
            continue;
        }
        if now - trigger.last_triggered < trigger.cooldown {
            continue;
        }
        // Check item requirement (Gap 2.3)
        if let Some(req) = requires
            && !inventory.has_item(&req.item_id)
        {
            continue;
        }
        trigger.last_triggered = now;

        // Emit trigger event (for EntityLink chain reactions)
        trigger_events.write(TriggerFired {
            entity,
            trigger_type: TriggerType::Proximity,
        });

        // Execute actions
        if let Some(teleport_action) = teleport {
            match teleport_action.effect {
                TeleportEffect::Fade => {
                    spawn_teleport_fade(&mut commands, teleport_action.destination);
                }
                _ => {
                    if let Ok(mut pt) = player_query.single_mut() {
                        pt.translation = teleport_action.destination;
                    }
                }
            }
        }
        if let Some(score_action) = score {
            let entry = score_board
                .scores
                .entry(score_action.category.clone())
                .or_insert(0);
            *entry += score_action.amount;
        }
        if let Some(_toggle) = toggle {
            // Toggle state handled via EntityState component
            if let Ok(mut estate) = commands.get_entity(entity) {
                estate.insert(EntityState::default());
            }
        }
        if let Some(sound_action) = play_sound {
            audio_engine.play_emitter_at(&sound_action.sound, transform.translation);
        }

        // Remove trigger if once
        if once.is_some() {
            commands.entity(entity).remove::<ProximityTrigger>();
            commands.entity(entity).remove::<OnceTrigger>();
        }
    }
}

/// System: handle click triggers (E key press within max_distance).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn click_trigger_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut player_query: Query<
        &mut Transform,
        (With<crate::character::Player>, Without<ClickTrigger>),
    >,
    click_query: Query<(
        Entity,
        &Transform,
        &ClickTrigger,
        Option<&TeleportAction>,
        Option<&AddScoreAction>,
        Option<&ToggleStateAction>,
        Option<&PlaySoundAction>,
        Option<&OnceTrigger>,
        Option<&RequiresItem>,
    )>,
    mut score_board: ResMut<ScoreBoard>,
    mut trigger_events: MessageWriter<TriggerFired>,
    mut commands: Commands,
    mut audio_engine: ResMut<crate::gen3d::audio::AudioEngine>,
    inventory: Res<PlayerInventory>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }

    let player_pos = if let Ok(player_transform) = player_query.single() {
        player_transform.translation
    } else {
        return;
    };

    // Find the closest click-triggerable entity within range (respecting item requirements)
    let mut closest: Option<(Entity, f32)> = None;
    for (entity, transform, trigger, _, _, _, _, _, requires) in click_query.iter() {
        let distance = player_pos.distance(transform.translation);
        if distance > trigger.max_distance {
            continue;
        }
        // Check item requirement (Gap 2.3)
        if let Some(req) = requires
            && !inventory.has_item(&req.item_id)
        {
            continue;
        }
        if closest.is_none() || distance < closest.unwrap().1 {
            closest = Some((entity, distance));
        }
    }

    let Some((target_entity, _)) = closest else {
        return;
    };

    // Fire actions on the closest entity
    if let Ok((
        entity,
        trigger_transform,
        _trigger,
        teleport,
        score,
        toggle,
        play_sound,
        once,
        _requires,
    )) = click_query.get(target_entity)
    {
        // Emit trigger event (for EntityLink chain reactions)
        trigger_events.write(TriggerFired {
            entity,
            trigger_type: TriggerType::Click,
        });

        if let Some(teleport_action) = teleport {
            match teleport_action.effect {
                TeleportEffect::Fade => {
                    spawn_teleport_fade(&mut commands, teleport_action.destination);
                }
                _ => {
                    if let Ok(mut pt) = player_query.single_mut() {
                        pt.translation = teleport_action.destination;
                    }
                }
            }
        }
        if let Some(score_action) = score {
            let entry = score_board
                .scores
                .entry(score_action.category.clone())
                .or_insert(0);
            *entry += score_action.amount;
        }
        if toggle.is_some()
            && let Ok(mut estate) = commands.get_entity(entity)
        {
            estate.insert(EntityState::default());
        }
        if let Some(sound_action) = play_sound {
            audio_engine.play_emitter_at(&sound_action.sound, trigger_transform.translation);
        }

        if once.is_some() {
            commands.entity(entity).remove::<ClickTrigger>();
            commands.entity(entity).remove::<OnceTrigger>();
        }
    }
}

/// System: tick timer triggers and fire their actions.
pub fn timer_trigger_system(
    time: Res<Time>,
    mut query: Query<(Entity, &mut TimerTrigger, Option<&AddScoreAction>)>,
    mut score_board: ResMut<ScoreBoard>,
) {
    for (_entity, mut trigger, score) in query.iter_mut() {
        trigger.timer.tick(time.delta());
        if trigger.timer.just_finished()
            && let Some(score_action) = score
        {
            let entry = score_board
                .scores
                .entry(score_action.category.clone())
                .or_insert(0);
            *entry += score_action.amount;
        }
    }
}

/// System: animate doors through their state machine.
pub fn door_system(time: Res<Time>, mut query: Query<(&mut Door, &mut Transform)>) {
    let dt = time.delta_secs();

    for (mut door, mut transform) in query.iter_mut() {
        match door.state.clone() {
            DoorState::Opening { progress } => {
                let new_progress = (progress + dt / door.open_duration).min(1.0);
                let angle = door.open_angle.to_radians() * new_progress;
                transform.rotation = door.original_rotation * Quat::from_rotation_y(angle);

                if new_progress >= 1.0 {
                    if door.auto_close {
                        door.state = DoorState::Open {
                            close_timer: Timer::from_seconds(
                                door.auto_close_delay,
                                TimerMode::Once,
                            ),
                        };
                    } else {
                        door.state = DoorState::Open {
                            close_timer: Timer::from_seconds(f32::MAX, TimerMode::Once),
                        };
                    }
                } else {
                    door.state = DoorState::Opening {
                        progress: new_progress,
                    };
                }
            }
            DoorState::Open { mut close_timer } => {
                close_timer.tick(time.delta());
                if close_timer.just_finished() {
                    door.state = DoorState::Closing { progress: 1.0 };
                } else {
                    door.state = DoorState::Open { close_timer };
                }
            }
            DoorState::Closing { progress } => {
                let new_progress = (progress - dt / door.open_duration).max(0.0);
                let angle = door.open_angle.to_radians() * new_progress;
                transform.rotation = door.original_rotation * Quat::from_rotation_y(angle);

                if new_progress <= 0.0 {
                    door.state = DoorState::Closed;
                    transform.rotation = door.original_rotation;
                } else {
                    door.state = DoorState::Closing {
                        progress: new_progress,
                    };
                }
            }
            DoorState::Closed => {}
        }
    }
}

/// System: open doors when player is near (proximity-triggered doors).
pub fn door_proximity_system(
    player_query: Query<&Transform, With<crate::character::Player>>,
    mut door_query: Query<
        (&Transform, &mut Door, &ProximityTrigger),
        Without<crate::character::Player>,
    >,
    inventory: Res<PlayerInventory>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (transform, mut door, trigger) in door_query.iter_mut() {
        let distance = player_pos.distance(transform.translation);
        if distance <= trigger.radius && matches!(door.state, DoorState::Closed) {
            // Check key requirement
            if let Some(ref key) = door.requires_key
                && !inventory.has_key(key)
            {
                continue;
            }
            door.state = DoorState::Opening { progress: 0.0 };
        }
    }
}

/// System: handle collectible pickup.
pub fn collectible_system(
    time: Res<Time>,
    player_query: Query<&Transform, With<crate::character::Player>>,
    mut collectible_query: Query<(
        Entity,
        &Transform,
        &mut Collectible,
        &mut Visibility,
        Option<&Name>,
    )>,
    mut score_board: ResMut<ScoreBoard>,
    mut inventory: ResMut<PlayerInventory>,
    mut commands: Commands,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (entity, transform, mut collectible, mut visibility, name) in collectible_query.iter_mut() {
        // Handle respawn timer
        if let Some(ref mut timer) = collectible.respawn_timer {
            timer.tick(time.delta());
            if timer.just_finished() {
                collectible.respawn_timer = None;
                *visibility = Visibility::Inherited;
            }
            continue; // Skip pickup check while respawning
        }

        // Check if visible (already collected items are hidden)
        if *visibility == Visibility::Hidden {
            continue;
        }

        let distance = player_pos.distance(transform.translation);
        if distance > 2.0 {
            continue;
        }

        // Collect!
        let entry = score_board
            .scores
            .entry(collectible.category.clone())
            .or_insert(0);
        *entry += collectible.value;

        // Add to inventory if category is "key" (unlocks doors with requires_key)
        if collectible.category == "key" {
            let key_name = name
                .map(|n| n.as_str().to_string())
                .unwrap_or_else(|| format!("key_{}", entity.index()));
            inventory.add_item(key_name);
        }

        // Apply pickup effect
        match collectible.pickup_effect {
            PickupEffect::Sparkle => {
                spawn_sparkle_burst(&mut commands, transform.translation);
            }
            PickupEffect::Dissolve => {
                // Start dissolve animation instead of instant removal
                commands.entity(entity).insert(DissolveEffect {
                    progress: 0.0,
                    duration: 0.3,
                    original_scale: transform.scale,
                });
                // Skip normal hide/despawn — dissolve system handles it
                continue;
            }
            PickupEffect::None => {}
        }

        if let Some(respawn_time) = collectible.respawn_time {
            // Hide and start respawn timer
            *visibility = Visibility::Hidden;
            collectible.respawn_timer = Some(Timer::from_seconds(respawn_time, TimerMode::Once));
        } else {
            // Permanently remove
            commands.entity(entity).despawn();
        }
    }
}

/// System: sync ScoreBoard → HudScore so the HUD reflects interaction scores.
pub fn score_sync_system(score_board: Res<ScoreBoard>, mut hud_score: ResMut<crate::ui::HudScore>) {
    // Sum all score categories into the HUD score
    let total: i32 = score_board.scores.values().sum();
    hud_score.score = total as i64;
}

/// System: animate entities with AnimateAction (tweens transform properties).
pub fn animate_action_system(
    time: Res<Time>,
    mut query: Query<(Entity, &mut AnimateAction, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();

    for (entity, mut action, mut transform) in query.iter_mut() {
        action.progress = (action.progress + dt / action.duration.max(0.01)).min(1.0);
        let t = action.progress;

        match action.property.as_str() {
            "position" | "translation" if action.to.len() >= 3 => {
                let target = Vec3::new(action.to[0], action.to[1], action.to[2]);
                transform.translation = transform.translation.lerp(target, t);
            }
            "scale" if action.to.len() >= 3 => {
                let target = Vec3::new(action.to[0], action.to[1], action.to[2]);
                transform.scale = transform.scale.lerp(target, t);
            }
            "scale" if action.to.len() == 1 => {
                let target = Vec3::splat(action.to[0]);
                transform.scale = transform.scale.lerp(target, t);
            }
            "rotation" if action.to.len() >= 3 => {
                let target = Quat::from_euler(
                    EulerRot::YXZ,
                    action.to[1].to_radians(),
                    action.to[0].to_radians(),
                    action.to[2].to_radians(),
                );
                transform.rotation = transform.rotation.slerp(target, t);
            }
            _ => {}
        }

        if action.progress >= 1.0 {
            commands.entity(entity).remove::<AnimateAction>();
        }
    }
}

/// System: handle enable/disable actions toggling visibility.
pub fn enable_disable_system(
    mut commands: Commands,
    enable_query: Query<Entity, Added<EnableAction>>,
    disable_query: Query<Entity, Added<DisableAction>>,
) {
    for entity in enable_query.iter() {
        commands
            .entity(entity)
            .insert(Visibility::Inherited)
            .remove::<EnableAction>();
    }
    for entity in disable_query.iter() {
        commands
            .entity(entity)
            .insert(Visibility::Hidden)
            .remove::<DisableAction>();
    }
}

// ---------------------------------------------------------------------------
// GAP-P2-02: Teleport fade overlay
// ---------------------------------------------------------------------------

/// State machine for teleport fade-to-black transition.
#[derive(Component)]
pub struct TeleportFadeOverlay {
    /// Phase: FadeIn (0→1 alpha), Teleport (instant), FadeOut (1→0 alpha).
    pub phase: TeleportFadePhase,
    /// Progress within current phase (0.0–1.0).
    pub progress: f32,
    /// Duration of each phase in seconds.
    pub fade_duration: f32,
    /// Destination to teleport to.
    pub destination: Vec3,
}

/// Teleport fade phases.
pub enum TeleportFadePhase {
    FadeIn,
    Teleport,
    FadeOut,
}

/// System: animate the teleport fade overlay (fade in → teleport → fade out).
pub fn teleport_fade_system(
    time: Res<Time>,
    mut overlay_query: Query<(Entity, &mut TeleportFadeOverlay, &mut BackgroundColor)>,
    mut player_query: Query<&mut Transform, With<crate::character::Player>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();

    for (entity, mut overlay, mut bg_color) in overlay_query.iter_mut() {
        overlay.progress += dt / overlay.fade_duration.max(0.01);

        match overlay.phase {
            TeleportFadePhase::FadeIn => {
                let alpha = overlay.progress.min(1.0);
                *bg_color = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, alpha));
                if overlay.progress >= 1.0 {
                    overlay.phase = TeleportFadePhase::Teleport;
                    overlay.progress = 0.0;
                }
            }
            TeleportFadePhase::Teleport => {
                // Teleport player
                if let Ok(mut pt) = player_query.single_mut() {
                    pt.translation = overlay.destination;
                }
                overlay.phase = TeleportFadePhase::FadeOut;
                overlay.progress = 0.0;
            }
            TeleportFadePhase::FadeOut => {
                let alpha = 1.0 - overlay.progress.min(1.0);
                *bg_color = BackgroundColor(Color::srgba(0.0, 0.0, 0.0, alpha));
                if overlay.progress >= 1.0 {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// Spawn a fade overlay for a teleport with the Fade effect.
fn spawn_teleport_fade(commands: &mut Commands, destination: Vec3) {
    commands.spawn((
        TeleportFadeOverlay {
            phase: TeleportFadePhase::FadeIn,
            progress: 0.0,
            fade_duration: 0.3,
            destination,
        },
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        ZIndex(100),
    ));
}

// ---------------------------------------------------------------------------
// GAP-P2-03: Collectible pickup effects
// ---------------------------------------------------------------------------

/// Component for a dissolve animation (scale to zero over duration).
#[derive(Component)]
pub struct DissolveEffect {
    pub progress: f32,
    pub duration: f32,
    pub original_scale: Vec3,
}

/// System: animate dissolve effect (scale entity to zero, then despawn).
pub fn dissolve_effect_system(
    time: Res<Time>,
    mut query: Query<(Entity, &mut DissolveEffect, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut effect, mut transform) in query.iter_mut() {
        effect.progress += dt / effect.duration.max(0.01);
        let t = effect.progress.min(1.0);
        // Smooth scale-to-zero
        transform.scale = effect.original_scale * (1.0 - t);
        if effect.progress >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Component for a sparkle burst (small particles flying upward).
#[derive(Component)]
pub struct SparkleParticle {
    pub velocity: Vec3,
    pub lifetime: f32,
    pub age: f32,
}

/// System: animate sparkle particles (move upward, fade, despawn).
pub fn sparkle_particle_system(
    time: Res<Time>,
    mut query: Query<(
        Entity,
        &mut SparkleParticle,
        &mut Transform,
        &mut Visibility,
    )>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform, mut vis) in query.iter_mut() {
        particle.age += dt;
        if particle.age >= particle.lifetime {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += particle.velocity * dt;
        // Slow down velocity
        particle.velocity *= 0.95;
        // Fade out in the last 30%
        let fade_start = particle.lifetime * 0.7;
        if particle.age > fade_start {
            *vis = Visibility::Hidden; // Simple approach: hide near end
        }
    }
}

/// Spawn sparkle burst particles at position.
fn spawn_sparkle_burst(commands: &mut Commands, position: Vec3) {
    use std::f32::consts::PI;
    for i in 0..12 {
        let angle = (i as f32 / 12.0) * PI * 2.0;
        let velocity = Vec3::new(angle.cos() * 2.0, 3.0 + (i as f32 % 3.0), angle.sin() * 2.0);
        commands.spawn((
            SparkleParticle {
                velocity,
                lifetime: 0.8,
                age: 0.0,
            },
            Transform::from_translation(position),
            Visibility::Inherited,
        ));
    }
}

// ---------------------------------------------------------------------------
// GAP-P2-01: EntityLink resolution system (chain reactions)
// ---------------------------------------------------------------------------

/// System: resolve entity links when triggers fire.
///
/// When a `TriggerFired` event is received, checks all `EntityLink` components on the
/// source entity. For each link whose `source_event` matches the trigger type,
/// resolves the `target_entity` name via `NameRegistry` and executes `target_action`.
pub fn entity_link_system(
    mut trigger_events: MessageReader<TriggerFired>,
    link_query: Query<&EntityLink>,
    registry: Res<crate::gen3d::registry::NameRegistry>,
    mut commands: Commands,
) {
    for event in trigger_events.read() {
        let trigger_name = match event.trigger_type {
            TriggerType::Proximity => "proximity",
            TriggerType::Click => "click",
            TriggerType::AreaEnter => "area_enter",
            TriggerType::AreaExit => "area_exit",
            TriggerType::Collision => "collision",
            TriggerType::Timer => "timer",
        };

        // Get all EntityLink components on the source entity
        if let Ok(link) = link_query.get(event.entity) {
            // Check if this link matches the trigger event
            if link.source_event == trigger_name || link.source_event == "any" {
                // Resolve target entity name
                if let Some(target) = registry.get_entity(&link.target_entity) {
                    match link.target_action.as_str() {
                        "toggle_state" => {
                            commands.entity(target).insert(EntityState::default());
                        }
                        "enable" => {
                            commands.entity(target).insert(EnableAction);
                        }
                        "disable" => {
                            commands.entity(target).insert(DisableAction);
                        }
                        "destroy" => {
                            commands.entity(target).despawn();
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GAP-P2-05: Area trigger sensor detection
// ---------------------------------------------------------------------------

/// Tracks whether the player is currently inside an area trigger zone.
#[derive(Component, Default)]
pub struct AreaInsideTracker {
    pub was_inside: bool,
}

/// System: detect player entering/exiting area trigger zones.
///
/// Uses distance-based overlap detection (no physics required).
/// Emits `TriggerFired` events with `AreaEnter` or `AreaExit` type.
pub fn area_trigger_system(
    player_query: Query<&Transform, With<crate::character::Player>>,
    mut area_query: Query<(
        Entity,
        &Transform,
        &AreaTrigger,
        &mut AreaInsideTracker,
        Option<&ProximityTrigger>,
    )>,
    mut trigger_events: MessageWriter<TriggerFired>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (entity, transform, area, mut tracker, prox) in area_query.iter_mut() {
        // Use ProximityTrigger radius if available, otherwise default 3.0
        let radius = prox.map_or(3.0, |p| p.radius);
        let distance = player_pos.distance(transform.translation);
        let is_inside = distance <= radius;

        if is_inside && !tracker.was_inside {
            // Player entered the area
            if area.is_enter {
                trigger_events.write(TriggerFired {
                    entity,
                    trigger_type: TriggerType::AreaEnter,
                });
            }
        } else if !is_inside && tracker.was_inside {
            // Player exited the area
            if !area.is_enter {
                trigger_events.write(TriggerFired {
                    entity,
                    trigger_type: TriggerType::AreaExit,
                });
            }
        }

        tracker.was_inside = is_inside;
    }
}

// ---------------------------------------------------------------------------
// GAP-P2-04: Click trigger prompt text rendering (world-space)
// ---------------------------------------------------------------------------

/// Marker for prompt text child entities spawned by click triggers.
#[derive(Component)]
pub struct ClickPromptText {
    pub parent_trigger: Entity,
}

/// System: show/hide floating prompt text near click triggers when player is in range.
pub fn click_prompt_system(
    player_query: Query<&Transform, With<crate::character::Player>>,
    trigger_query: Query<(Entity, &Transform, &ClickTrigger)>,
    mut prompt_query: Query<(Entity, &ClickPromptText, &mut Visibility)>,
    mut commands: Commands,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (trigger_entity, transform, trigger) in trigger_query.iter() {
        let Some(ref prompt_text) = trigger.prompt_text else {
            continue;
        };

        let distance = player_pos.distance(transform.translation);
        let in_range = distance <= trigger.max_distance;

        // Check if prompt child already exists
        let mut found = false;
        for (_prompt_entity, prompt, mut vis) in prompt_query.iter_mut() {
            if prompt.parent_trigger == trigger_entity {
                *vis = if in_range {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                found = true;
            }
        }

        // Spawn prompt text if it doesn't exist and player is in range
        if !found && in_range {
            let text_entity = commands
                .spawn((
                    ClickPromptText {
                        parent_trigger: trigger_entity,
                    },
                    Text2d::new(prompt_text.clone()),
                    TextFont {
                        font_size: 24.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Transform::from_translation(transform.translation + Vec3::new(0.0, 2.0, 0.0)),
                    Visibility::Inherited,
                ))
                .id();
            // Make it a child so it moves with the trigger entity
            commands.entity(trigger_entity).add_child(text_entity);
        }
    }
}

// ---------------------------------------------------------------------------
// GAP 2.5: Screen-space click interaction prompt ("Press E")
// ---------------------------------------------------------------------------

/// Marker for the screen-space interaction prompt HUD node.
#[derive(Component)]
pub struct InteractionPromptHud;

/// Resource tracking which entity's prompt is currently displayed.
#[derive(Resource, Default)]
pub struct ActiveInteractionPrompt {
    /// The entity whose prompt is being shown (if any).
    pub target: Option<Entity>,
    /// The prompt text being displayed.
    pub text: String,
}

/// System: display a screen-space "Press E" prompt when the player is near a
/// click-interactable entity.
///
/// This renders a centered bottom-screen HUD prompt (not world-space text),
/// making it easy for the player to discover interactive objects.
pub fn interaction_prompt_hud_system(
    player_query: Query<&Transform, With<crate::character::Player>>,
    trigger_query: Query<(Entity, &Transform, &ClickTrigger, Option<&RequiresItem>)>,
    inventory: Res<PlayerInventory>,
    mut prompt_state: ResMut<ActiveInteractionPrompt>,
    mut hud_query: Query<(Entity, &mut Text, &mut Visibility), With<InteractionPromptHud>>,
    mut commands: Commands,
) {
    let Ok(player_transform) = player_query.single() else {
        // No player — hide any existing prompt
        prompt_state.target = None;
        for (_entity, _text, mut vis) in hud_query.iter_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let player_pos = player_transform.translation;

    // Find the closest in-range click trigger
    let mut closest: Option<(Entity, f32, String)> = None;
    for (entity, transform, trigger, requires) in trigger_query.iter() {
        let distance = player_pos.distance(transform.translation);
        if distance > trigger.max_distance {
            continue;
        }
        // Check item requirement
        if let Some(req) = requires
            && !inventory.has_item(&req.item_id)
        {
            continue;
        }
        if closest.is_none() || distance < closest.as_ref().unwrap().1 {
            let text = trigger
                .prompt_text
                .clone()
                .unwrap_or_else(|| "Press E to interact".to_string());
            closest = Some((entity, distance, text));
        }
    }

    match closest {
        Some((entity, _, text)) => {
            prompt_state.target = Some(entity);
            prompt_state.text.clone_from(&text);

            if let Ok((_hud_entity, mut hud_text, mut vis)) = hud_query.single_mut() {
                // Update existing HUD node
                **hud_text = text;
                *vis = Visibility::Inherited;
            } else {
                // Spawn the HUD prompt node (screen-space UI)
                commands.spawn((
                    InteractionPromptHud,
                    Text::new(text),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                    TextLayout::new_with_justify(bevy::text::Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(60.0),
                        left: Val::Percent(50.0),
                        // Negative margin to center (approximate)
                        margin: UiRect {
                            left: Val::Px(-100.0),
                            ..default()
                        },
                        width: Val::Px(200.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    ZIndex(50),
                ));
            }
        }
        None => {
            // No interactable in range — hide prompt
            prompt_state.target = None;
            for (_entity, _text, mut vis) in hud_query.iter_mut() {
                *vis = Visibility::Hidden;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GAP 2.1: Collision-based triggers (Avian sensor integration)
// ---------------------------------------------------------------------------

/// System: fire collision triggers when a physics body (player) enters a sensor collider.
///
/// Uses Avian's `Collisions` system param to detect overlap between the player's
/// rigid body and entities marked with `CollisionTrigger` + `Sensor`.
/// Fires the same actions as proximity/click triggers.
#[cfg(feature = "physics")]
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn collision_trigger_system(
    time: Res<Time>,
    collisions: avian3d::prelude::Collisions,
    player_query: Query<Entity, With<crate::character::Player>>,
    mut trigger_query: Query<(
        Entity,
        &Transform,
        &mut CollisionTrigger,
        Option<&TeleportAction>,
        Option<&AddScoreAction>,
        Option<&PlaySoundAction>,
        Option<&OnceTrigger>,
        Option<&RequiresItem>,
    )>,
    mut score_board: ResMut<ScoreBoard>,
    mut trigger_events: MessageWriter<TriggerFired>,
    mut commands: Commands,
    mut audio_engine: ResMut<crate::gen3d::audio::AudioEngine>,
    inventory: Res<PlayerInventory>,
) {
    let Ok(player_entity) = player_query.single() else {
        return;
    };
    let now = time.elapsed_secs();

    for (entity, transform, mut trigger, teleport, score, play_sound, once, requires) in
        trigger_query.iter_mut()
    {
        // Check if player is colliding with this sensor
        let is_colliding = collisions.contains(player_entity, entity);
        let was_inside = trigger.overlapping.contains(&player_entity);

        if is_colliding && !was_inside {
            // Player just entered the trigger volume
            trigger.overlapping.insert(player_entity);

            // Check cooldown
            if now - trigger.last_triggered < trigger.cooldown {
                continue;
            }

            // Check item requirement
            if let Some(req) = requires
                && !inventory.has_item(&req.item_id)
            {
                continue;
            }

            trigger.last_triggered = now;

            // Emit trigger event
            trigger_events.write(TriggerFired {
                entity,
                trigger_type: TriggerType::Collision,
            });

            // Execute actions (same pattern as proximity_trigger_system)
            if let Some(teleport_action) = teleport {
                match teleport_action.effect {
                    TeleportEffect::Fade => {
                        spawn_teleport_fade(&mut commands, teleport_action.destination);
                    }
                    _ => {
                        commands
                            .entity(player_entity)
                            .insert(Transform::from_translation(teleport_action.destination));
                    }
                }
            }
            if let Some(score_action) = score {
                let entry = score_board
                    .scores
                    .entry(score_action.category.clone())
                    .or_insert(0);
                *entry += score_action.amount;
            }
            if let Some(sound_action) = play_sound {
                audio_engine.play_emitter_at(&sound_action.sound, transform.translation);
            }

            // Remove trigger if once
            if once.is_some() {
                commands.entity(entity).remove::<CollisionTrigger>();
                commands.entity(entity).remove::<OnceTrigger>();
            }
        } else if !is_colliding && was_inside {
            // Player left the trigger volume
            trigger.overlapping.remove(&player_entity);
        }
    }
}

/// Stub collision trigger system when physics feature is disabled.
#[cfg(not(feature = "physics"))]
pub fn collision_trigger_system() {
    // No-op: collision triggers require the physics feature (Avian3d).
}

// ---------------------------------------------------------------------------
// GAP 2.3: Collectible item pickup system
// ---------------------------------------------------------------------------

/// System: handle CollectibleItem pickup (inventory gating).
///
/// When the player walks within range of an entity with `CollectibleItem`,
/// the item_id is added to `PlayerInventory` and the entity is despawned.
pub fn collectible_item_system(
    player_query: Query<&Transform, With<crate::character::Player>>,
    item_query: Query<(Entity, &Transform, &CollectibleItem)>,
    mut inventory: ResMut<PlayerInventory>,
    mut commands: Commands,
    mut notifications: MessageWriter<crate::ui::notification::NotificationEvent>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (entity, transform, item) in item_query.iter() {
        let distance = player_pos.distance(transform.translation);
        if distance > item.pickup_range {
            continue;
        }

        // Add to inventory
        inventory.add_item(item.item_id.clone());

        // Send notification if configured
        if let Some(ref msg) = item.pickup_message {
            notifications.write(crate::ui::notification::NotificationEvent {
                text: msg.clone(),
                style: crate::ui::notification::NotificationStyle::Toast,
                position: crate::ui::notification::NotificationPosition::Top,
                icon: crate::ui::notification::NotificationIcon::None,
            });
        }

        // Despawn the item entity
        commands.entity(entity).despawn();
    }
}

/// Plugin for interaction systems.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ScoreBoard::default())
            .insert_resource(PlayerInventory::default())
            .insert_resource(ActiveInteractionPrompt::default())
            .add_message::<TriggerFired>()
            // Group 1: Trigger detection + chain resolution (max 16 systems per tuple)
            .add_systems(
                Update,
                (
                    proximity_trigger_system,
                    click_trigger_system,
                    timer_trigger_system,
                    area_trigger_system,
                    collision_trigger_system,
                    entity_link_system
                        .after(proximity_trigger_system)
                        .after(click_trigger_system)
                        .after(area_trigger_system),
                    door_system,
                    door_proximity_system,
                    collectible_system,
                    collectible_item_system,
                    score_sync_system,
                    animate_action_system,
                    enable_disable_system,
                ),
            )
            // Group 2: UI prompts + visual effects
            .add_systems(
                Update,
                (
                    click_prompt_system,
                    interaction_prompt_hud_system,
                    teleport_fade_system,
                    dissolve_effect_system,
                    sparkle_particle_system,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scoreboard() {
        let mut board = ScoreBoard::default();
        assert!(board.scores.is_empty());
        board.scores.insert("points".to_string(), 10);
        assert_eq!(board.scores["points"], 10);
    }

    #[test]
    fn test_player_inventory() {
        let mut inv = PlayerInventory::default();
        assert!(!inv.has_key("gold_key"));
        inv.add_item("gold_key".to_string());
        assert!(inv.has_key("gold_key"));
    }

    #[test]
    fn test_door_params_defaults() {
        let params = DoorParams::default();
        assert_eq!(params.open_angle, 90.0);
        assert!(params.auto_close);
        assert_eq!(params.auto_close_delay, 3.0);
        assert!(params.requires_key.is_none());
    }

    #[test]
    fn test_timer_trigger() {
        let trigger = TimerTrigger::new(2.0);
        assert_eq!(trigger.interval, 2.0);
    }

    #[test]
    fn test_click_trigger_default() {
        let trigger = ClickTrigger::default();
        assert_eq!(trigger.max_distance, 5.0);
        assert!(trigger.prompt_text.is_none());
    }

    #[test]
    fn test_area_trigger_default() {
        let trigger = AreaTrigger::default();
        assert!(trigger.is_enter);
    }

    #[test]
    fn test_proximity_trigger_default() {
        let trigger = ProximityTrigger::default();
        assert_eq!(trigger.radius, 5.0);
        assert_eq!(trigger.cooldown, 1.0);
        assert_eq!(trigger.last_triggered, 0.0);
    }

    #[test]
    fn test_collectible_params_default() {
        let params = CollectibleParams::default();
        assert_eq!(params.value, 1);
        assert_eq!(params.category, "points");
        assert!(params.pickup_sound.is_none());
        assert_eq!(params.pickup_effect, PickupEffect::Sparkle);
        assert!(params.respawn_time.is_none());
    }

    #[test]
    fn test_teleporter_params_size_default() {
        let size = default_teleporter_size();
        assert_eq!(size, [2.0, 3.0, 2.0]);
    }

    #[test]
    fn test_entity_link_fields() {
        let link = EntityLink {
            source_event: "clicked".to_string(),
            target_entity: "door_1".to_string(),
            target_action: "toggle_state:is_open".to_string(),
            condition: Some("source.is_active".to_string()),
        };
        assert_eq!(link.source_event, "clicked");
        assert!(link.condition.is_some());
    }

    #[test]
    fn test_add_trigger_params_action_fields() {
        let params = AddTriggerParams {
            entity_id: "box".to_string(),
            trigger_type: TriggerType::Proximity,
            action: TriggerAction::AddScore,
            radius: Some(5.0),
            cooldown: None,
            interval: None,
            max_distance: None,
            prompt_text: None,
            once: false,
            destination: None,
            text: None,
            amount: Some(10),
            state_key: None,
            category: Some("coins".to_string()),
            requires_item: None,
        };
        assert_eq!(params.amount, Some(10));
        assert_eq!(params.category.as_deref(), Some("coins"));
    }

    #[test]
    fn test_add_trigger_params_requires_item() {
        let params = AddTriggerParams {
            entity_id: "locked_chest".to_string(),
            trigger_type: TriggerType::Click,
            action: TriggerAction::Animate,
            radius: None,
            cooldown: None,
            interval: None,
            max_distance: Some(3.0),
            prompt_text: Some("Press E to open".to_string()),
            once: true,
            destination: None,
            text: None,
            amount: None,
            state_key: None,
            category: None,
            requires_item: Some("gold_key".to_string()),
        };
        assert_eq!(params.requires_item.as_deref(), Some("gold_key"));
        assert!(params.once);
    }

    #[test]
    fn test_door_state_default() {
        assert!(matches!(DoorState::default(), DoorState::Closed));
    }

    #[test]
    fn test_collectible_component() {
        let c = Collectible {
            value: 5,
            category: "gems".to_string(),
            pickup_effect: PickupEffect::Sparkle,
            respawn_time: Some(10.0),
            original_position: Vec3::ZERO,
            respawn_timer: None,
        };
        assert_eq!(c.value, 5);
        assert_eq!(c.category, "gems");
        assert_eq!(c.respawn_time, Some(10.0));
    }

    #[test]
    fn test_scoreboard_add_multiple() {
        let mut board = ScoreBoard::default();
        board.scores.insert("coins".to_string(), 10);
        board.scores.insert("gems".to_string(), 3);
        assert_eq!(board.scores.len(), 2);
        assert_eq!(board.scores["coins"], 10);
        assert_eq!(board.scores["gems"], 3);
    }

    #[test]
    fn test_player_inventory_multiple() {
        let mut inv = PlayerInventory::default();
        inv.add_item("key_a".to_string());
        inv.add_item("key_b".to_string());
        assert!(inv.has_key("key_a"));
        assert!(inv.has_key("key_b"));
        assert!(!inv.has_key("key_c"));
    }

    #[test]
    fn test_teleporter_params_custom() {
        let params = TeleporterParams {
            position: [1.0, 0.0, 2.0],
            destination: [10.0, 0.0, 20.0],
            size: [3.0, 4.0, 3.0],
            effect: TeleportEffect::Particles,
            sound: Some("whoosh".to_string()),
            label: Some("Portal".to_string()),
        };
        assert_eq!(params.label.as_deref(), Some("Portal"));
        assert_eq!(params.size, [3.0, 4.0, 3.0]);
    }

    #[test]
    fn test_link_entities_params() {
        let params = LinkEntitiesParams {
            source_id: "button".to_string(),
            source_event: "pressed".to_string(),
            target_id: "door".to_string(),
            target_action: "open".to_string(),
            condition: None,
        };
        assert_eq!(params.source_id, "button");
        assert!(params.condition.is_none());
    }

    #[test]
    fn test_pickup_effect_default() {
        assert_eq!(PickupEffect::default(), PickupEffect::Sparkle);
    }

    #[test]
    fn test_pickup_effect_variants() {
        assert_ne!(PickupEffect::None, PickupEffect::Sparkle);
        assert_ne!(PickupEffect::Sparkle, PickupEffect::Dissolve);
        assert_ne!(PickupEffect::Dissolve, PickupEffect::None);
    }

    #[test]
    fn test_pickup_effect_serde() {
        let json = serde_json::to_string(&PickupEffect::Dissolve).unwrap();
        assert_eq!(json, "\"dissolve\"");
        let parsed: PickupEffect = serde_json::from_str("\"sparkle\"").unwrap();
        assert_eq!(parsed, PickupEffect::Sparkle);
        let parsed_none: PickupEffect = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(parsed_none, PickupEffect::None);
    }

    #[test]
    fn test_trigger_type_default_and_serde() {
        assert_eq!(TriggerType::default(), TriggerType::Proximity);
        let json = serde_json::to_string(&TriggerType::AreaEnter).unwrap();
        assert_eq!(json, "\"area_enter\"");
        let parsed: TriggerType = serde_json::from_str("\"click\"").unwrap();
        assert_eq!(parsed, TriggerType::Click);
    }

    #[test]
    fn test_trigger_action_default_and_serde() {
        assert_eq!(TriggerAction::default(), TriggerAction::Animate);
        let json = serde_json::to_string(&TriggerAction::AddScore).unwrap();
        assert_eq!(json, "\"add_score\"");
        let parsed: TriggerAction = serde_json::from_str("\"toggle_state\"").unwrap();
        assert_eq!(parsed, TriggerAction::ToggleState);
    }

    #[test]
    fn test_teleport_effect_default_and_serde() {
        assert_eq!(TeleportEffect::default(), TeleportEffect::None);
        let json = serde_json::to_string(&TeleportEffect::Fade).unwrap();
        assert_eq!(json, "\"fade\"");
        let parsed: TeleportEffect = serde_json::from_str("\"particles\"").unwrap();
        assert_eq!(parsed, TeleportEffect::Particles);
    }

    #[test]
    fn test_door_trigger_default_and_serde() {
        assert_eq!(DoorTrigger::default(), DoorTrigger::Proximity);
        let json = serde_json::to_string(&DoorTrigger::Click).unwrap();
        assert_eq!(json, "\"click\"");
        let parsed: DoorTrigger = serde_json::from_str("\"proximity\"").unwrap();
        assert_eq!(parsed, DoorTrigger::Proximity);
    }

    // --- Gap 2.1: CollisionTrigger tests ---

    #[test]
    fn test_collision_trigger_default() {
        let trigger = CollisionTrigger::default();
        assert_eq!(trigger.cooldown, 1.0);
        assert_eq!(trigger.last_triggered, 0.0);
        assert!(trigger.overlapping.is_empty());
    }

    // --- Gap 2.3: Inventory / CollectibleItem tests ---

    #[test]
    fn test_player_inventory_has_item() {
        let mut inv = PlayerInventory::default();
        assert!(inv.is_empty());
        assert_eq!(inv.len(), 0);
        inv.add_item("gold_key".to_string());
        assert!(inv.has_item("gold_key"));
        assert!(inv.has_key("gold_key")); // Alias
        assert!(!inv.has_item("silver_key"));
        assert_eq!(inv.len(), 1);
    }

    #[test]
    fn test_player_inventory_remove_item() {
        let mut inv = PlayerInventory::default();
        inv.add_item("torch".to_string());
        assert!(inv.has_item("torch"));
        assert!(inv.remove_item("torch"));
        assert!(!inv.has_item("torch"));
        assert!(!inv.remove_item("nonexistent"));
    }

    #[test]
    fn test_player_inventory_dedup() {
        let mut inv = PlayerInventory::default();
        inv.add_item("key".to_string());
        inv.add_item("key".to_string()); // Duplicate
        assert_eq!(inv.len(), 1); // HashSet deduplicates
    }

    #[test]
    fn test_collectible_item_default() {
        let item = CollectibleItem::default();
        assert!(item.item_id.is_empty());
        assert_eq!(item.pickup_range, 2.0);
        assert!(item.pickup_message.is_none());
    }

    #[test]
    fn test_requires_item_component() {
        let req = RequiresItem {
            item_id: "master_key".to_string(),
        };
        assert_eq!(req.item_id, "master_key");
    }

    // --- Gap 2.5: ActiveInteractionPrompt tests ---

    #[test]
    fn test_active_interaction_prompt_default() {
        let prompt = ActiveInteractionPrompt::default();
        assert!(prompt.target.is_none());
        assert!(prompt.text.is_empty());
    }
}
