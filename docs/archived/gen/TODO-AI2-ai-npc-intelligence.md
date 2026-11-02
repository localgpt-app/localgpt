# AI Integration 2: AI-Driven NPC Intelligence

**Status:** Archived 2026-11-01. The Rust side was built (`NpcBrain` over an Ollama client, `NpcMemory`, perception, `gen_set_npc_brain`, `gen_npc_observe`, `gen_set_npc_memory`, `npcs.ron`) and removed with every other model call in the tree. NPC patrol, wander and dialogue trees stay as data; a brain behind an NPC, if one comes, is an agent outside the window driving it through the world's API.

**Upgrades NPCs from scripted automatons to contextually-aware AI characters.** Uses locally-running small language models (SLMs) for real-time NPC decision-making, combining the SLM network architecture from recent research with SIMA 2-style visual observation. NPCs perceive their environment, make decisions, form memories, and converse — all via local inference, zero cloud dependency.

**Source:** AI World Generation Research — "Small language models for game NPCs" (arXiv:2601.23206), SIMA 2 visual observation architecture (arXiv:2512.04797), Meta Motivo behavioral foundation model

**Dependencies:** Priority 1 (avatar/NPC system must exist), Ollama (local model inference), P2 interaction system (for NPC-trigger integration)

**Priority within AI series:** 2 of 2 (requires P1 NPCs first)

---

## Spec AI2.1: `gen_set_npc_brain` — Attach AI Brain to NPC

**Goal:** Give an NPC a local AI brain that drives its behavior decisions. Instead of scripted patrol/wander/dialogue, the NPC observes its environment and makes decisions via a local SLM. The brain runs as a background tokio task, issuing commands to the NPC entity at a configurable tick rate.

### MCP Tool Schema

```json
{
  "name": "gen_set_npc_brain",
  "description": "Attach an AI brain to an NPC for autonomous behavior driven by a local language model",
  "parameters": {
    "entity": { "type": "string", "required": true, "description": "Name of existing NPC entity" },
    "personality": { "type": "string", "required": true, "description": "Natural language personality and behavioral description" },
    "model": { "type": "string", "default": "llama3.2:3b", "description": "Ollama model name for this NPC's brain" },
    "tick_rate": { "type": "f32", "default": 2.0, "description": "Seconds between brain decisions" },
    "perception_radius": { "type": "f32", "default": 15.0, "description": "How far the NPC can perceive entities" },
    "goals": { "type": "array", "items": "string", "optional": true, "description": "High-level goals driving NPC behavior" },
    "knowledge": { "type": "array", "items": "string", "optional": true, "description": "Facts the NPC knows about the world" }
  }
}
```

### Architecture

```
                    ┌─────────────────┐
                    │   NPC Entity     │
                    │   (Bevy ECS)     │
                    └────────┬────────┘
                             │ perception data
                    ┌────────▼────────┐
                    │   NPC Brain      │
                    │   (tokio task)   │
                    │                  │
                    │ ┌──────────────┐ │
                    │ │ Context      │ │
                    │ │ Builder      │ │
                    │ └──────┬───────┘ │
                    │        │ prompt   │
                    │ ┌──────▼───────┐ │
                    │ │ Ollama SLM   │ │
                    │ │ (1–3B)       │ │
                    │ └──────┬───────┘ │
                    │        │ text     │
                    │ ┌──────▼───────┐ │
                    │ │ Action       │ │
                    │ │ Parser       │ │
                    │ └──────┬───────┘ │
                    └────────┼────────┘
                             │ NpcCommand
                    ┌────────▼────────┐
                    │  Bevy Systems    │
                    │  (move, speak,   │
                    │   interact,      │
                    │   emote)         │
                    └─────────────────┘
```

### Implementation

1. **Perception system:** Every tick, gather context for the NPC:
   - Nearby entities within `perception_radius` (name, type, position, distance, direction)
   - Player position, distance, and facing direction (if within perception radius)
   - Current NPC state (position, current action, equipped items if applicable)
   - Recent events buffer (player spoke to NPC, entity appeared/disappeared, collision, door opened)
   - Environmental context: time of day, weather, ambient sound (if terrain systems active)

2. **Context prompt construction:**
   ```
   You are {personality}.

   Your goals: {goals}
   You know: {knowledge}
   {memory context from AI2.3 if enabled}

   ## Current situation
   Position: ({x}, {y}, {z})
   Currently: {current_action}

   ## Nearby ({n} entities within {perception_radius}m)
   - "{entity_name}" ({type}) at {distance}m {direction} {brief_description}
   - Player at {distance}m {direction}, facing {toward_you|away}

   ## Recent events
   - {timestamp}: {event_description}

   ## Available actions (choose exactly ONE)
   - move_to(x, y, z) — walk to position
   - look_at("entity_name") — turn toward entity
   - speak("text") — say aloud (speech bubble visible to player)
   - emote(wave|nod|shrug|point|laugh) — nonverbal expression
   - interact("entity_name") — interact with nearby entity
   - wait — stay still, observe
   - wander — walk to random nearby point

   Respond with ONLY the action. Example: speak("Welcome, traveler!")
   ```

3. **Action parser:** Regex-based parser extracts action name and parameters from SLM response. Validates entity references against nearby entities. Falls back to `wait` on parse failure (never crashes).

4. **NPC command execution via Bevy systems:**
   - `move_to(x, y, z)` → set NPC destination, walk animation, pathfinding if navmesh available (WG2), else direct lerp
   - `look_at("entity")` → smooth rotation toward target entity
   - `speak("text")` → spawn speech bubble UI above NPC head, auto-dismiss after `text.len() * 50ms + 2000ms`
   - `emote(type)` → trigger particle effect or animation at NPC position
   - `interact("entity")` → fire interaction trigger on target entity (P2 system)
   - `wait` → idle animation
   - `wander` → pick random point within 5m, walk toward it

5. **NpcBrain component:**
   ```rust
   #[derive(Component)]
   pub struct NpcBrain {
       pub personality: String,
       pub model: String,
       pub tick_rate: f32,
       pub perception_radius: f32,
       pub goals: Vec<String>,
       pub knowledge: Vec<String>,
       pub last_tick: f64,
       pub current_action: NpcAction,
       pub recent_events: VecDeque<NpcEvent>, // ring buffer, max 20
       pub active: bool, // false when far from player
   }
   ```

6. **Resource management:**
   - Each NPC brain is a tokio task spawned on `IoTaskPool`
   - Maximum concurrent active brains: configurable, default 4
   - **Distance culling:** Brains pause (`active = false`) when NPC is > 50m from player. Resume when player approaches within 40m (hysteresis prevents flapping).
   - Ollama requests use the existing `OllamaClient` from localgpt core if available, or spawn direct HTTP calls to `localhost:11434`

### Acceptance Criteria

- [ ] `gen_set_npc_brain` attaches AI brain to existing NPC entity
- [ ] NPC makes contextual decisions based on nearby entities and player proximity
- [ ] NPC speaks via speech bubble when brain decides to communicate
- [ ] NPC moves toward points of interest autonomously
- [ ] Brain pauses when NPC is far from player (> 50m)
- [ ] Multiple NPCs can have brains simultaneously (up to configurable limit)
- [ ] Brain uses local Ollama model — zero cloud dependency
- [ ] Action parse failures fall back gracefully to wait/wander
- [ ] Brain tick rate is respected (no faster than configured interval)
- [ ] Removing NPC brain (`gen_set_npc_brain` with `model: "none"`) reverts to scripted behavior

### Files to Create/Modify

- `localgpt/crates/gen/src/character/npc_brain.rs` — NpcBrain component, brain tick system, tokio task loop
- `localgpt/crates/gen/src/character/perception.rs` — Perception system: spatial query, event collection, context building
- `localgpt/crates/gen/src/character/npc_actions.rs` — Action parser, NpcCommand enum, command execution systems
- `localgpt/crates/gen/src/mcp/avatar_tools.rs` — Add `gen_set_npc_brain` tool handler
- `localgpt/crates/gen/src/gen3d/commands.rs` — Add `SetNpcBrain` command and response
- `localgpt/crates/world-types/src/entity.rs` — Add `NpcBrainDef` to WorldEntity for serialization

---

## Spec AI2.2: `gen_npc_observe` — Visual Observation for NPCs

**Goal:** Give NPCs the ability to "see" by rendering the world from their perspective and feeding the image to a vision-language model. This enables NPCs to react to visual cues the text-only perception system misses — colors, spatial arrangements, building styles, environmental mood. Inspired by SIMA 2's visual observation architecture where a Gemini model receives rendered frames.

### MCP Tool Schema

```json
{
  "name": "gen_npc_observe",
  "description": "Have an NPC visually observe the scene from its perspective and describe what it sees",
  "parameters": {
    "entity": { "type": "string", "required": true, "description": "Name of NPC entity" },
    "question": { "type": "string", "optional": true, "description": "Specific question the NPC should answer about what it sees" },
    "auto_observe": { "type": "bool", "default": false, "description": "If true, automatically observe every N brain ticks" },
    "observe_interval": { "type": "i32", "default": 5, "description": "Brain ticks between auto-observations (if auto_observe=true)" },
    "fov": { "type": "f32", "default": 90.0 },
    "resolution": { "type": "vec2", "default": [512, 512] }
  }
}
```

### Implementation

1. **NPC camera:** Spawn a secondary `Camera3d` at NPC's eye position (entity position + `Vec3::Y * 1.6`), looking in NPC's forward direction. Render one frame to off-screen texture via Bevy's render-to-texture.

2. **Vision query:** Send rendered image + optional question to vision-capable local model via Ollama:
   - **Preferred:** LLaVA 1.6 7B (~5 GB VRAM) — good scene understanding
   - **Lightweight:** moondream2 (~2 GB) — faster, less detailed
   - **Best quality:** Qwen2-VL 7B (~8 GB) — strongest spatial reasoning

3. **Observation result:** Vision model returns natural-language description. Parsed and stored as `NpcObservation`:
   ```rust
   pub struct NpcObservation {
       pub timestamp: f64,
       pub description: String,
       pub question: Option<String>,
       pub answer: Option<String>,
   }
   ```

4. **Integration with brain:** Latest observation injected into brain context:
   ```
   ## What you see
   {observation.description}
   ```
   When `auto_observe` is enabled, the brain system automatically triggers an observation every N ticks and includes the latest result in context.

5. **Performance:** Vision queries take 2–5 seconds. Run on separate tokio task, never blocks brain ticks. Brain uses previous observation until new one completes. Render-to-texture uses low resolution (512×512 default) to minimize GPU impact.

### Acceptance Criteria

- [ ] NPC renders scene from its perspective via off-screen camera
- [ ] Vision model describes what NPC "sees" in natural language
- [ ] Observation feeds into next brain decision context
- [ ] Works with local vision models via Ollama (LLaVA, moondream2)
- [ ] Auto-observe mode triggers observations at configurable interval
- [ ] Vision query does not block brain tick loop
- [ ] NPC camera does not interfere with main player camera

### Files to Create/Modify

- `localgpt/crates/gen/src/character/npc_vision.rs` — NPC camera setup, render-to-texture, vision query task
- `localgpt/crates/gen/src/character/npc_brain.rs` — Integrate observations into brain context prompt
- `localgpt/crates/gen/src/mcp/avatar_tools.rs` — Add `gen_npc_observe` tool handler
- `localgpt/crates/gen/src/gen3d/commands.rs` — Add `NpcObserve` command and response

---

## Spec AI2.3: `gen_set_npc_memory` — Persistent NPC Memory

**Goal:** NPCs remember interactions across brain ticks and world reloads. Enables NPCs that learn from player behavior — greeting returning players by name, remembering past conversations, adapting to player preferences. Memory is local to each NPC and serialized with the world.

### MCP Tool Schema

```json
{
  "name": "gen_set_npc_memory",
  "description": "Configure persistent memory for an AI NPC",
  "parameters": {
    "entity": { "type": "string", "required": true, "description": "Name of NPC entity" },
    "memory_capacity": { "type": "i32", "default": 50, "description": "Maximum number of memory entries" },
    "initial_memories": { "type": "array", "items": "string", "optional": true, "description": "Pre-seeded memories establishing NPC backstory" },
    "auto_memorize": { "type": "bool", "default": true, "description": "Automatically form memories from significant interactions" }
  }
}
```

### Implementation

1. **NpcMemory component:**
   ```rust
   #[derive(Component, Serialize, Deserialize)]
   pub struct NpcMemory {
       pub capacity: usize,
       pub entries: Vec<MemoryEntry>,
       pub auto_memorize: bool,
   }

   #[derive(Serialize, Deserialize)]
   pub struct MemoryEntry {
       pub timestamp: f64,
       pub content: String,
       pub importance: f32,  // 0.0–1.0, used for eviction
   }
   ```

2. **Memory formation (auto):** After each brain tick where the NPC took a meaningful action (spoke, interacted, observed something new), the brain generates a one-line memory summary as part of its output:
   ```
   ## After choosing your action, also output:
   MEMORY: {one sentence summarizing what just happened, if notable}
   ```
   Parse this from the SLM response. Skip if the NPC chose `wait` or `wander` (not notable). Assign importance based on action type: speak=0.7, interact=0.8, observed new entity=0.5.

3. **Memory retrieval:** On each brain tick, include the 5 most relevant memories in context. Relevance = simple scoring: recent memories score higher, important memories score higher. No embedding similarity needed (SLM context is small enough):
   ```
   ## Your memories
   - "The player asked about the old ruins to the east. I told them it's dangerous." (recent)
   - "A merchant named Thalia passed through yesterday." (older)
   ```

4. **Memory eviction:** When entries exceed `capacity`, drop the entry with lowest `importance * recency_score`. Recency score decays exponentially with age. Initial memories (backstory) get importance=1.0 so they persist longest.

5. **Persistence:** `NpcMemory` serialized as part of `WorldEntity` in world.ron via `NpcMemoryDef`:
   ```rust
   // In localgpt-world-types
   #[derive(Serialize, Deserialize)]
   pub struct NpcMemoryDef {
       pub capacity: usize,
       pub entries: Vec<MemoryEntryDef>,
       pub auto_memorize: bool,
   }
   ```
   Survives save/load cycles. NPCs remember across sessions.

### Acceptance Criteria

- [ ] NPC accumulates memories from meaningful interactions
- [ ] Memories influence brain decisions (visible in dialogue — NPC references past events)
- [ ] Memories persist across world save/load (`gen_save_world` / `gen_load_world`)
- [ ] Memory capacity prevents unbounded growth
- [ ] Pre-seeded `initial_memories` establish NPC backstory
- [ ] Memory eviction prioritizes dropping old, low-importance entries
- [ ] Auto-memorize can be disabled for NPCs that shouldn't remember

### Files to Create/Modify

- `localgpt/crates/gen/src/character/npc_memory.rs` — NpcMemory component, memory formation, eviction
- `localgpt/crates/world-types/src/entity.rs` — Add `NpcMemoryDef` to WorldEntity
- `localgpt/crates/gen/src/character/npc_brain.rs` — Integrate memory retrieval into context, parse MEMORY line from output
- `localgpt/crates/gen/src/mcp/avatar_tools.rs` — Add `gen_set_npc_memory` tool handler
- `localgpt/crates/gen/src/gen3d/commands.rs` — Add `SetNpcMemory` command and response
- `localgpt/crates/gen/src/gen3d/world.rs` — Serialize/deserialize NpcMemory in world save/load

---

## Design Notes

### SLM Network Architecture

The January 2026 paper (arXiv:2601.23206) proposes using **networks of task-specific fine-tuned SLMs** rather than one monolithic LLM for game NPCs. The rationale: game NPCs need real-time responses (<500ms), multiple concurrent NPCs, and bounded resource usage — constraints that make 70B+ models impractical.

For LocalGPT Gen, the practical approach is:
- **Phase 1 (this spec):** Single SLM per NPC via Ollama. 1–3B models (Llama 3.2 1B/3B, Phi-3 mini, Qwen 2.5 1.5B). Simple but effective.
- **Phase 2 (future):** Task-specific SLMs — separate models for dialogue, navigation planning, and emotional state. Route to appropriate model based on current context. More resource-efficient for many concurrent NPCs.

### Resource Budget

Rough VRAM/RAM budget for concurrent NPC brains:

| NPCs | Model | VRAM | Inference | Tick Rate |
|------|-------|------|-----------|-----------|
| 1–4 | Llama 3.2 3B Q4 | ~2 GB | ~200ms | 2s |
| 4–8 | Llama 3.2 1B Q4 | ~1 GB | ~100ms | 3s |
| 8–16 | Phi-3 mini Q4 | ~1.5 GB | ~150ms | 5s |

All models share the same Ollama instance. Ollama handles model caching — if all NPCs use the same model, it's loaded once.

### Integration with P1 NPC System

AI brains are **additive** to the P1 NPC system, not a replacement:
- P1 defines NPC spawning, visual appearance, patrol routes, scripted dialogue trees
- AI2 adds an AI brain that **overrides** scripted behavior when active
- Removing the brain (or if Ollama is unavailable) reverts the NPC to its P1 scripted behavior
- Brain can coexist with patrol routes: brain uses patrol waypoints as navigation suggestions but can deviate

### Privacy

All inference is local via Ollama. No NPC dialogue, memory, or player interaction data leaves the machine. This is a fundamental architectural constraint, not an optimization — it enables content that players would never allow if it were cloud-processed.
