# RFC: AI NPCs & Advanced Audio — Gap Closure

**Status:** Withdrawn 2026-11-01 — the NPC brain, its memory and the Ollama client this RFC wired were removed with every other model call in the tree (`docs/rfcs/authoring/the-harness-goes.md`). The audio half shipped as `localgpt-world-audio`. Kept as the record of what was built.
**Author:** Yi
**Date:** 2026-03-22
**Target crates:** `localgpt-gen`
**Depends on:** Phase 1 (complete), Phase 2 (complete), Ollama running locally

---

## 1. Summary

Phase 4 scaffolding is **~70% complete**. NPC brain config, memory scoring, perception snapshots, action parsing, dialogue system, and Ollama client are all implemented. The remaining work is **wiring** — connecting the scaffolded pieces into a running brain loop.

### 1.1 What's Already Done

| Component | Status | Notes |
|-----------|--------|-------|
| `gen_set_npc_brain` MCP tool | Scaffolded | Schema, params, command dispatch |
| `gen_npc_observe` MCP tool | Scaffolded | Schema, params, snapshot types |
| `gen_set_npc_memory` MCP tool | Scaffolded | Schema, params, memory storage |
| `NpcBrainConfig` | Complete | Personality, model, tick_rate, goals, knowledge |
| `NpcBrainState` | Complete | Active brain, current action, event ring buffer |
| `NpcAction` enum | Complete | MoveTo, LookAt, Speak, Emote, Interact, Wait, Wander |
| `parse_npc_action()` | Complete | Parses LLM text → NpcAction |
| `build_brain_context()` | Complete | Constructs full LLM prompt from state + perception |
| `NpcMemory` | Complete | Capacity, importance + recency scoring, LRU eviction |
| `PerceptionSnapshot` | Complete | Nearby entities, player info, recent events |
| Dialogue system | Complete | Branching, typewriter, UI, choice selection, movement lock |
| Ollama provider | Complete | HTTP client at localhost:11434, streaming, model routing |
| NPC patrol/wander/idle | Complete | Waypoint navigation, random movement, face-player |

### 1.2 What's Missing (~800-1000 LoC)

| Component | Effort | Description |
|-----------|--------|-------------|
| Brain task loop | 2d | Tokio task per NPC: tick → perceive → prompt Ollama → parse → execute |
| Perception system | 1d | Nearby entity detection, player distance, direction naming |
| Action execution | 1d | MoveTo → nav, Speak → bubble, LookAt → rotate, Emote → particles |
| NPC vision (observe) | 1d | Offscreen render from NPC POV → vision LLM (llava/moondream2) |
| Memory persistence | 0.5d | Serialize NpcMemory to world save, load on world load |
| Asset gen HTTP bridge | 2d | POST to model server, poll status, download GLB, import mesh |

---

## 2. Brain Task Loop Architecture

The core missing piece. Each NPC with a brain gets a tokio task:

```
Every tick_rate seconds:
  1. Gather PerceptionSnapshot (nearby entities, player, events)
  2. Format top memories via NpcMemory::format_for_context()
  3. Build full prompt via build_brain_context()
  4. POST to Ollama at localhost:11434/api/generate
  5. Parse response via parse_npc_action()
  6. Send NpcAction back to Bevy via channel
  7. Bevy system executes the action (move, speak, etc.)
```

### 2.1 Implementation Plan

```rust
// New file: character/npc_brain_system.rs

/// Bevy system that processes NPC brain decisions from the background task.
fn npc_brain_execution_system(
    mut brain_query: Query<(Entity, &mut NpcBrainState, &mut Transform, &Name)>,
    mut commands: Commands,
) {
    for (entity, mut state, mut transform, name) in brain_query.iter_mut() {
        if let Some(action) = state.pending_action.take() {
            match action {
                NpcAction::MoveTo(target) => { /* update transform toward target */ }
                NpcAction::Speak(text) => { /* spawn speech bubble entity */ }
                NpcAction::LookAt(target) => { /* rotate to face target */ }
                NpcAction::Emote(emote_type) => { /* spawn particle effect */ }
                NpcAction::Wait(duration) => { /* set wait timer */ }
                NpcAction::Wander => { /* pick random nearby point */ }
                NpcAction::Interact(target) => { /* trigger interaction system */ }
            }
        }
    }
}
```

### 2.2 Ollama Integration

The Ollama HTTP client already exists in `localgpt-core`. For NPC brains, use a lightweight direct call:

```rust
async fn query_npc_brain(
    model: &str,
    prompt: &str,
    ollama_url: &str,  // default: http://localhost:11434
) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/api/generate", ollama_url))
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
        }))
        .send().await
        .map_err(|e| format!("Ollama request failed: {}", e))?;
    // Parse response...
}
```

### 2.3 Recommended NPC Models

| Model | Size | Speed | Best For |
|-------|------|-------|----------|
| `llama3.2:1b` | 1.3GB | ~50 tok/s | Simple NPCs, fast decisions |
| `llama3.2:3b` | 2.0GB | ~30 tok/s | Default — good personality + reasoning |
| `phi3:mini` | 2.3GB | ~25 tok/s | Structured output, tool-like responses |
| `moondream2` | 1.7GB | ~20 tok/s | Vision model for gen_npc_observe |

---

## 3. Advanced Audio (P4 Items)

### 3.1 Already Complete

- 7 procedural ambient soundscapes (FunDSP synthesis, no files needed)
- 5 spatial audio emitter types with distance attenuation
- Auto-inference from entity names (campfire → fire sound)
- PlaySoundAction wired in trigger systems
- `gen_set_ambience`, `gen_audio_emitter` MCP tools

### 3.2 Not Yet Implemented

| Item | Effort | Notes |
|------|--------|-------|
| AI music generation | 3d | ACE-Step as localhost microservice, content-addressable cache |
| AI SFX generation | 2d | Stable Audio Open, provider fallback chain |
| NPC voice synthesis | 2d | Sesame CSM-1B or ElevenLabs TTS |
| Day/night cycle audio | 1d | Tie ambient layers to time-of-day |
| Weather audio | 1d | Rain/wind intensity tied to weather state |

These are all **external service integrations** — the audio engine (FunDSP + cpal) is complete and can play any sound. The gap is generating the sounds via AI models.

---

## 4. Phase 4 → Phase 5 Transition Criteria

Phase 4 is **complete** when:

1. At least 1 NPC responds to player proximity with LLM-generated dialogue
2. NPC memory persists across interactions (remembers previous conversations)
3. NPC patrol/wander behavior operates autonomously between interactions
4. `gen_npc_observe` returns a text description of what the NPC "sees"

### Acceptable Deferrals to Phase 5+

- AI music/SFX generation (external services)
- NPC voice synthesis (cloud API dependency)
- Day/night cycle (visual system, not NPC-related)
- Weather system (visual + audio, separate workstream)

---

## 5. Estimated Effort

| Item | Estimate | Priority | Blocks |
|------|----------|----------|--------|
| Brain task loop + Ollama wiring | 2d | High | Everything else |
| Perception system | 1d | High | Brain loop |
| Action execution | 1d | High | Brain loop |
| NPC vision (observe) | 1d | Medium | Vision model |
| Memory persistence | 0.5d | Medium | Nothing |
| Asset gen HTTP bridge | 2d | Low | Model server |
| **Total NPC intelligence** | **5.5d** | | |
| **Total with asset gen** | **7.5d** | | |
