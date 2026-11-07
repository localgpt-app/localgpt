# Gen audio system

## Overview

LocalGPT Gen includes a procedural environmental audio system built on FunDSP synthesis inside a kira mixer (via `localgpt-world-audio`, the one mapping from the world format's audio types to sound). The system synthesizes ambient soundscapes and spatial sound emitters that respond to camera position in real time. An external agent authors them through Gen's MCP tools; no model runs inside Gen.

### Architecture

```
┌──────────────────────────────────────┐
│ External agent / MCP client          │
│                                      │
│ gen_set_ambience /                   │
│ gen_audio_emitter → GenCommand       │
└──────────────┬───────────────────────┘
               │ mpsc channel
┌──────────────▼───────────────────────┐
│ Bevy Main Thread                     │
│                                      │
│ • process_gen_commands()             │
│ • spatial_audio_update()             │
│ • auto_infer_audio()                 │
│                                      │
│ Holds AudioEngine metadata + the     │
│ update channel's sender              │
└──────────────┬───────────────────────┘
               │ AudioUpdate messages
┌──────────────▼───────────────────────┐
│ Audio Thread                         │
│                                      │
│ • Owns localgpt-world-audio Engine   │
│   (one kira AudioManager)            │
│ • Emitters are kira spatial tracks:  │
│   distance attenuation and panning   │
│   come from positions alone          │
│ • FunDSP graphs feed the mixer as    │
│   sources (FundspSound)              │
└──────────────────────────────────────┘
```

**Key design choices:**
- **Bevy owns main thread** (macOS GPU/windowing requirement)
- **kira owns the device and the mixer**; FunDSP is a synthesis DSL feeding it, not a rival engine — the graphs are Gen's, unchanged, and the transport is kira's
- **Spatial tracks replace per-frame math**: an emitter is a sub-track with min/max distances, so moving the listener or the emitter attenuates and pans it; the old quadratic falloff is now kira's distance curve
- **Volume is amplitude** (the format's unit) and converts to decibels at the engine edge
- **No output device is silence, not an error** — headless runners get a quiet world

`localgpt-world-audio` is shared with MD and Verse, so one process can play a document world's ambience and a song world's soundtrack at once.

## Sound Types

### Ambient Sounds

Global soundscapes that loop continuously with natural variation:

| Sound | Parameters | Characteristics |
|-------|-----------|-----------------|
| Wind | `speed` (0-1), `gustiness` (0-1) | Pink noise with LFO-modulated lowpass |
| Rain | `intensity` (0-1) | White noise bandpass with AM modulation |
| Forest | `bird_density` (0-1), `wind` (0-1) | Pink noise layer + sine chirps (birds) |
| Ocean | `wave_size` (0-1) | Brown noise with slow amplitude LFO + foam hiss |
| Cave | `drip_rate` (0-1), `resonance` (0-1) | Sine chirps (drips) + quiet brown noise |
| Stream | `flow_rate` (0-1) | Layered white/brown noise + bandpass |
| Silence | (none) | DC output for silence |

All ambient sounds use **LFO modulation** (0.05–0.3 Hz) to ensure natural variation — no two moments sound identical.

### Emitter Sounds

Spatial audio sources that respond to camera distance and direction:

| Sound | Parameters | Characteristics |
|-------|-----------|-----------------|
| Water | `turbulence` (0-1) | White noise bandpass + brown undertone |
| Fire | `intensity` (0-1), `crackle` (0-1) | Brown rumble + noise bursts via envelope |
| Hum | `frequency` (Hz), `warmth` (0-1) | Sine + harmonics with detune for warmth |
| Wind | `pitch` (Hz) | Pink noise with LFO modulation |
| Custom | `waveform`, `filter_cutoff`, `filter_type` | Direct waveform → filter |

Emitters support **spatial rendering**:
- **Volume attenuation:** Quadratic falloff within radius (inverse square)
- **Stereo panning:** Left/right based on camera relative direction
- **Updates per-frame:** Lock-free via Shared params (no graph rebuild needed)

## Agent Tools

### `gen_set_ambience`

Set the global ambient soundscape. Replaces previous ambience.

**Arguments:**
```json
{
  "layers": [
    {
      "name": "background_wind",
      "sound": { "type": "wind", "speed": 0.5, "gustiness": 0.3 },
      "volume": 0.6
    },
    {
      "name": "birds",
      "sound": { "type": "forest", "bird_density": 0.4, "wind": 0.2 },
      "volume": 0.4
    }
  ],
  "master_volume": 0.8
}
```

**Response:** `AmbienceSet`

### `gen_audio_emitter`

Create a spatial audio emitter. Can attach to an existing entity by name or spawn standalone.

**Arguments:**
```json
{
  "name": "campfire_sound",
  "entity": "campfire",  // or omit + use position
  "position": [5.0, 1.0, 3.0],  // optional
  "sound": { "type": "fire", "intensity": 0.6, "crackle": 0.5 },
  "radius": 15.0,
  "volume": 0.8
}
```

**Response:** `AudioEmitterSpawned { name }`

### `gen_modify_audio`

Modify an existing emitter's volume, radius, or sound.

**Arguments:**
```json
{
  "name": "campfire_sound",
  "volume": 0.9,
  "radius": 20.0,
  "sound": { "type": "fire", "intensity": 0.8, "crackle": 0.6 }
}
```

**Response:** `AudioEmitterModified { name }`

### `gen_audio_info`

Query current audio state (ambience layers, active emitters, volumes).

**Arguments:** (none)

**Response:**
```json
{
  "active": true,
  "ambience_layers": ["background_wind", "birds"],
  "emitters": [
    {
      "name": "campfire_sound",
      "sound_type": "fire",
      "volume": 0.9,
      "radius": 20.0,
      "position": [5.0, 1.0, 3.0],
      "attached_to": "campfire"
    }
  ],
  "master_volume": 0.8
}
```

## Auto-Inference System

The system automatically detects entity names and assigns audio. This runs every frame in `auto_infer_audio()`.

**Rules** (case-insensitive substring match):

| Keywords | Sound | Radius |
|----------|-------|--------|
| waterfall, fountain | Water (turbulence: 0.8) | 15.0 |
| river, water | Water (turbulence: 0.5) | 12.0 |
| stream, creek, brook | Water (turbulence: 0.3) | 10.0 |
| fire, campfire, torch, flame, bonfire | Fire (intensity: 0.5, crackle: 0.4) | 10.0 |
| generator, machine, engine, motor | Hum (frequency: 120 Hz, warmth: 0.5) | 8.0 |
| vent, fan, wind_turbine | Wind (pitch: 400 Hz) | 6.0 |

**Example:**
```python
# Agent spawns entity
gen_spawn_primitive(name="campfire", shape="sphere", ...)

# auto_infer_audio detects "campfire" in name
# → Creates AudioEmitter with Fire sound automatically
```

To override: call `gen_audio_emitter` explicitly with different sound.

## Usage Examples

### Example 1: Forest Scene with Stream

```python
# Set ambient forest soundscape
gen_set_ambience({
  "layers": [
    {
      "name": "wind",
      "sound": { "type": "wind", "speed": 0.3, "gustiness": 0.2 },
      "volume": 0.5
    },
    {
      "name": "birds",
      "sound": { "type": "forest", "bird_density": 0.6, "wind": 0.1 },
      "volume": 0.7
    }
  ],
  "master_volume": 0.9
})

# Spawn visual stream
gen_spawn_primitive(name="stream", shape="cuboid", ...)

# Audio is auto-inferred: "stream" → Water emitter (turbulence: 0.3, radius: 10)
# No explicit audio command needed

# Walk camera toward stream → water rushes louder, pans stereo
# Walk away → sound fades to ambient forest
```

### Example 2: Campfire with Custom Hum

```python
# Spawn visual campfire
gen_spawn_primitive(name="campfire_center", shape="sphere", ...)

# Auto-inference detects "campfire" → Fire emitter created

# But also create a hum from a nearby generator
gen_audio_emitter({
  "name": "generator_hum",
  "position": [10.0, 0.5, 0.0],
  "sound": { "type": "hum", "frequency": 60.0, "warmth": 0.8 },
  "radius": 20.0,
  "volume": 0.6
})

# Scene now has: ambient silence, fire sounds from campfire, hum from generator
# All respond to camera position
```

### Example 3: Modify Emitter at Runtime

```python
# Fire grows hotter → increase intensity
gen_modify_audio({
  "name": "campfire_sound",
  "sound": { "type": "fire", "intensity": 0.8, "crackle": 0.7 }
})

# Fire grows closer → increase volume
gen_modify_audio({
  "name": "campfire_sound",
  "volume": 1.0
})
```

## Implementation Details

### FunDSP Graph Patterns

The graphs live in `localgpt-world-audio/src/graphs.rs`, keyed on
`wt::AudioSource` — the format's union of what used to be Gen's
`AmbientSound` and `EmitterSound`. Each sound compiles to a FunDSP `AudioUnit`. Complex sounds use `Net` (dynamic graph) with explicit node connections:

**Ambient Wind (simple):**
```rust
pink() >> lowpass_hz(cutoff_with_lfo)
```

**Ambient Ocean (complex, uses Net):**
```
Net::new(0, 1)
  .push(brown() with slow amplitude LFO)  → id1
  .push(white() >> highpass)              → id2
  .push((pass() + pass()))                → sum (id1 + id2)
  .pipe_output(sum)
```

**Emitter Fire (uses Net for composition):**
```
Net::new(0, 1)
  .push(brown() >> lowpass for rumble)           → id_rumble
  .push(white() >> bandpass with LFO for crackle) → id_crackle
  .push((pass() + pass()))                       → sum
  .pipe_output(sum)
  // Each multiplied by volume, then panned to stereo
```

### Parameter Control

All runtime parameters (`volume`, `pan`, `speed`, `intensity`) are `Shared<f32>` — atomic lock-free updates:

```rust
// Bevy thread (frame update):
emitter_params[name].volume.set(0.8);  // No mutex, no allocation

// Audio thread (immediate effect):
let vol = volume_shared.value();        // Read via atomic load
```

### Graph Rebuild Trigger

Graph rebuilds occur only on structural changes:
- `SetAmbience` → clear all nodes, rebuild ambience layers
- `AddEmitter` → push new emitter node, reconnect output sum
- `RemoveEmitter` → remove emitter node, reconnect output sum

Parameter updates (volume, pan) do NOT rebuild — they just update `Shared` values.

## File Structure

- **`crates/gen/src/gen3d/audio.rs`** (689 lines)
  - `AudioEngine` resource
  - Audio thread startup and management
  - Bevy systems: `spatial_audio_update`, `auto_infer_audio`, command handlers

- **`crates/gen/src/gen3d/audio_graphs.rs`** (358 lines)
  - FunDSP graph builders for all 12 sound types
  - `infer_emitter_from_name()` for auto-detection
  - Keyword-based inference rules

- **Related files:**
  - `crates/gen/src/gen3d/commands.rs` — audio command/response types
  - `crates/gen/src/gen3d/tools.rs` — the four audio tools
  - `crates/gen/src/gen3d/plugin.rs` — Bevy system registration
  - `crates/gen/src/gen3d/registry.rs` — `AudioEmitter` entity type

## Possible extensions

These are ideas, not scheduled features:

- crossfades when switching ambience and smooth parameter interpolation;
- environmental reverb changes;
- musical source formats such as Glicol or ABC, authored by an external agent
  and synthesized through the shared audio engine.

A musical extension needs its own design and must not add a prompt loop or a
model call to Gen.

## Manual check

```bash
cargo run -p localgpt-gen -- mcp-server
```

Connect an MCP client and ask its agent to create a forest clearing with a
stream and a campfire, with forest ambience and spatial water and fire
emitters. Then verify:

1. forest ambience plays;
2. the stream and fire are audible at their positions;
3. moving the camera changes their volume and stereo position;
4. `gen_audio_info` reports the layers and emitters the agent created.

## Performance Notes

- **Audio rendering:** Minimal overhead (FunDSP is highly optimized)
- **Main thread impact:** Only Shared parameter writes (atomic, no locking)
- **Graph rebuilds:** ~1-2ms for typical graphs (infrequent, not per-frame)
- **Memory:** ~100 KB for typical ambience + 5-10 emitters
- **CPU:** ~2-5% usage (on modern hardware) for full complex soundscape

## Debugging

Enable Bevy tracing for audio events:

```bash
RUST_LOG=localgpt_gen::gen3d::audio=debug cargo run -p localgpt-gen -- "your prompt"
```

Logging includes:
- Audio engine startup
- Graph rebuild events
- Auto-inferred emitters
- Spatial audio parameter updates

