# RFC: Interaction, Triggers & Ambient Audio — Gap Closure

**Status:** Draft
**Author:** Yi
**Date:** 2026-03-22
**Target crates:** `localgpt-gen`
**Depends on:** Phase 1 (avatar/physics — complete)

> **Update (2026-09-29):** `gen_add_trigger` no longer uses the Gen-only
> trigger components below. Its triggers are world-format data
> (`localgpt-world-types` `trigger`), saved with the world, and run by
> `localgpt-world-bevy`'s trigger runtime, which the web viewer mirrors. A
> review found the old path only half-built: `enable`, `disable` and
> `animate` ran as soon as the trigger was added, `show_text`, `spawn` and
> `destroy` never ran, and timer and area triggers ran no action. Doors,
> collectibles and teleporters still use the components described here.

---

## 1. Summary

Phase 2 infrastructure is **~95% complete**. The interaction trigger system, procedural audio engine, and in-world UI are all functional with 14 MCP tools, 12+ Bevy systems, and 100+ unit tests. This RFC documents the **5 remaining gaps** and the in-engine verification checklist to close Phase 2.

### 1.1 What's Already Done

| System | Status | Tools | Tests |
|--------|--------|-------|-------|
| Trigger system (proximity, click, timer, area) | Functional | `gen_add_trigger` | Yes |
| Door interaction (state machine, auto-close) | Functional | `gen_add_door` | Yes |
| Collectible system (pickup, score, respawn) | Functional | `gen_add_collectible` | Yes |
| Teleporter (fade/particle effects) | Functional | `gen_add_teleporter` | Yes |
| Entity event wiring | Functional | `gen_link_entities` | Yes |
| Ambient audio (7 procedural soundscapes) | Functional | `gen_set_ambience` | Yes |
| Spatial audio emitters (5 types + auto-inference) | Functional | `gen_audio_emitter` | Yes |
| Signs (billboard text) | Functional | `gen_add_sign` | Yes |
| HUD (score, health, timer, text) | Functional | `gen_add_hud` | Yes |
| Labels (floating nameplates) | Functional | `gen_add_label` | Yes |
| Tooltips (proximity + look-at) | Functional | `gen_add_tooltip` | Yes |
| Notifications (toast/banner/achievement) | Functional | `gen_add_notification` | Yes |

---

## 2. Remaining Gaps (5 items)

### Gap 2.1: Collision-Based Triggers (Avian Sensor Integration)

**Current state:** `TriggerType::Collision` is accepted by `gen_add_trigger` but no system processes it. Triggers use distance checks (proximity) or keyboard (click), not physics collision events.

**What's needed:** A `collision_trigger_system` that uses Avian sensor colliders to detect overlap.

**Implementation:**

```rust
#[cfg(feature = "physics")]
fn collision_trigger_system(
    mut collision_events: EventReader<CollisionStarted>,
    trigger_q: Query<&AreaTrigger>,
    player_q: Query<Entity, With<Player>>,
    // ... fire actions on collision enter
) { ... }
```

**Effort:** ~2 hours. The `AreaTrigger` component already exists; this just adds a physics-based detection path alongside the existing distance-based one.

**Priority:** Low — proximity triggers cover most use cases. Collision triggers add precision for tight spaces.

---

### Gap 2.2: Sound Playback on Trigger

**Current state:** `PlaySoundAction` component exists and is parsed from `gen_add_trigger` params, but firing the action doesn't dispatch an `AudioEmitterCmd` to the audio engine.

**What's needed:** In `proximity_trigger_system` (and other trigger systems), when a `PlaySoundAction` fires, send the sound name to the audio engine.

**Implementation:** In `interaction/mod.rs`, add to the trigger fire logic:

```rust
if let Some(sound) = entity.get::<PlaySoundAction>() {
    // Send audio emitter command to play the sound
    audio_engine.add_emitter(entity_id, &sound.sound, transform.translation);
}
```

**Effort:** ~1 hour. The audio engine and emitter infrastructure is complete.

**Priority:** Medium — enables doors that creak, collectibles that chime, teleporters that whoosh.

---

### Gap 2.3: Inventory / Key System for Doors

**Current state:** `DoorParams` has a `requires_key: Option<String>` field, but the door system ignores it — doors always open regardless of key.

**What's needed:** A `PlayerInventory` component/resource tracking collected keys, checked by `door_proximity_system` before opening.

**Implementation:**

```rust
#[derive(Resource, Default)]
pub struct PlayerInventory {
    pub keys: HashSet<String>,
    pub items: HashMap<String, u32>,
}
```

When `CollectibleParams` has `category: "key"`, add the collectible's name to `PlayerInventory.keys`. When `Door.requires_key` is set, check the inventory before opening.

**Effort:** ~2 hours. Simple gating logic on existing systems.

**Priority:** Medium — enables locked door puzzles. Without this, all doors are open.

---

### Gap 2.4: Conditional Entity Links

**Current state:** `LinkEntitiesParams` has a `condition: Option<String>` field, but `entity_link_system` fires unconditionally.

**What's needed:** Parse the condition string (e.g., `"score > 10"`, `"has_key:gold"`) and evaluate against `EntityState` / `ScoreBoard` / `PlayerInventory` before firing the linked action.

**Implementation:** A simple expression evaluator:

```rust
fn evaluate_condition(condition: &str, state: &EntityState, score: &ScoreBoard, inventory: &PlayerInventory) -> bool {
    // Parse "score > 10", "has_key:gold", "state:door1:open"
    // Return true if condition is met
}
```

**Effort:** ~3 hours. Condition parsing adds complexity.

**Priority:** Low — enables puzzle chains but most worlds work without conditional links.

---

### Gap 2.5: Click Prompt Rendering ("Press E")

**Current state:** `ClickTrigger` has a `prompt_text: Option<String>` field, and `ClickPromptText` is defined, but no UI renders the prompt when the player is near a click-interactable.

**What's needed:** A system that queries `ClickTrigger` entities within `max_distance` of the player and renders the prompt text (e.g., "Press E to open") as a screen-space HUD element.

**Implementation:** Use the existing `NotificationPlugin` or a simple egui overlay:

```rust
fn click_prompt_display_system(
    player_q: Query<&Transform, With<Player>>,
    trigger_q: Query<(&Transform, &ClickTrigger), Without<Player>>,
    // Render prompt as centered screen text when within distance
) { ... }
```

**Effort:** ~1.5 hours.

**Priority:** Medium — improves discoverability of interactive objects.

---

## 3. In-Engine Verification Checklist

These items are implemented but need visual/interactive testing:

### 3.1 Trigger System

```
[ ] gen_add_trigger(entity_id="door1", trigger_type="proximity", radius=3.0, action="animate", ...)
    → walk near → action fires
[ ] gen_add_trigger(entity_id="switch1", trigger_type="click", action="toggle_state", ...)
    → press E near entity → state toggles
[ ] gen_add_trigger(entity_id="trap1", trigger_type="timer", interval=5.0, action="animate", ...)
    → animation repeats every 5 seconds
[ ] gen_add_trigger(entity_id="zone1", trigger_type="area", radius=5.0, action="show_text", ...)
    → enter area → text appears, exit → text disappears
```

### 3.2 Interactive Objects

```
[ ] gen_add_door(entity_id="door1", open_angle=90.0, auto_close_delay=3.0)
    → walk near → door rotates open → walk away → door closes after 3s
[ ] gen_add_collectible(entity_id="coin1", value=10, category="gold", pickup_effect="sparkle")
    → walk into → entity despawns → score increases → HUD updates
[ ] gen_add_teleporter(position=[0,0,5], destination=[20,0,20], effect="fade")
    → enter trigger → screen fades → player at destination → screen unfades
[ ] gen_link_entities(source="button1", target="gate1", source_event="activate", target_action="animate")
    → activate button → gate animates open
```

### 3.3 Audio

```
[ ] gen_set_ambience(layers=[{sound:"forest", volume:0.7}, {sound:"wind", volume:0.3}])
    → ambient forest + wind audible
[ ] gen_audio_emitter(entity_id="campfire1", sound="fire", volume=0.8, radius=10.0)
    → fire crackle audible near campfire, fades with distance
[ ] Auto-inference: spawn entity named "waterfall" → water sound auto-attaches
```

### 3.4 UI Elements

```
[ ] gen_add_sign(position=[5,2,0], text="Welcome to Willowmere", billboard=true)
    → text visible, rotates to face camera
[ ] gen_add_hud(element_type="score", position="top_right", label="Gold")
    → score counter visible in top-right corner
[ ] gen_add_label(entity_id="npc1", text="Village Elder", color=[1,1,0,1])
    → yellow nameplate floats above NPC
[ ] gen_add_notification(text="Quest Complete!", style="achievement", duration=3.0)
    → achievement banner appears, animates in, holds, fades out
```

---

## 4. Phase 2 → Phase 3 Transition Criteria

Phase 2 is **complete** when:

1. All 5 gap items are either resolved or explicitly deferred to Phase 3
2. The in-engine verification checklist (Section 3) passes for at least:
   - 1 door that opens/closes
   - 1 collectible that awards score
   - 1 teleporter that moves the player
   - Ambient audio playing
   - At least 1 UI element (sign or HUD) visible
3. A test world demonstrates the full interaction loop: spawn world → spawn player → interact with objects → collect items → see score → hear audio

### Recommended Deferral

Gaps 2.1 (collision triggers) and 2.4 (conditional links) can safely defer to Phase 3 — they're enhancement, not foundation. Gaps 2.2 (sound on trigger), 2.3 (inventory/keys), and 2.5 (click prompts) should be closed now for a complete Phase 2 experience.

---

## 5. Estimated Effort

| Gap | Estimate | Priority |
|-----|----------|----------|
| 2.1 Collision triggers | 2h | Low (defer) |
| 2.2 Sound on trigger | 1h | Medium |
| 2.3 Inventory/keys | 2h | Medium |
| 2.4 Conditional links | 3h | Low (defer) |
| 2.5 Click prompt | 1.5h | Medium |
| In-engine testing | 2h | High |
| **Total (if closing all)** | **11.5h** | |
| **Total (closing medium+ only)** | **6.5h** | |
