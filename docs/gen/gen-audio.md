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
- **kira owns the device and the mixer**; FunDSP is a synthesis DSL feeding it, not a rival engine
- **Emitters are spatial tracks**: an emitter is a sub-track with min/max distances (1.0 and its radius), so moving the listener or the emitter attenuates and pans it. The Bevy side sends positions each frame and computes no volume or pan
- **Volume is amplitude** (the format's unit) and converts to decibels at the engine edge
- **No output device is silence, not an error** — headless runners get a quiet world

`localgpt-world-audio` is shared with Verse, and Gen's engine streams a world's soundtrack on the same mixer (`AudioEngine::play_soundtrack`), so one process can play a document world's ambience and a song world's soundtrack at once. `sync_soundtrack_clock` puts the song's position on the modulation clock while it plays.

## Sound Types

Seven ambient sounds (`wind`, `rain`, `forest`, `ocean`, `cave`, `stream`, `silence`) loop as global layers, and five emitter sounds (`water`, `fire`, `hum`, `wind`, `custom`) play from a position. Their parameters are in the tool schemas (`crates/gen/src/gen3d/tools.rs`) and the user docs (`website/content/docs/gen/audio.md`). What those leave out:

- The tools take Gen's own `AmbientSound` and `EmitterSound` (`gen3d/commands.rs`), and `gen3d/compat.rs` converts them to the format's `wt::AudioSource`, the union of the two. The emitter `wind` is `AudioSource::WindEmitter`.
- `silence` is the absence of a voice: the layer is listed and nothing plays.
- `cave` takes `resonance`, and its graph does not use it.
- `custom` has no pitch parameter: a `sine` runs at half of `filter_cutoff`, a `saw` or a `square` at a quarter.
- The format's `File` and `Abc` sources have no graph, and Gen's own types cannot carry them. A world load skips audio with such a source; an applied op (undo, a room's or a live canvas's commit) plays it as a 220 Hz hum on an emitter and as silence in the ambience (`convert_audio_def_to_emitter_sound` and `convert_audio_source_to_ambient_sound` in `gen3d/plugin.rs`).

## Agent Tools

`gen_set_ambience`, `gen_audio_emitter`, `gen_modify_audio` and `gen_audio_info` (`crates/gen/src/gen3d/tools.rs`). Their arguments are in `AGENTS.md` and, with examples, in the user docs. What the handlers do beyond the schema:

- `gen_set_ambience` replaces every layer. A layer's `volume` defaults to 0.5. `master_volume` is clamped to 0–1 and keeps its last value (0.8 at start) when omitted.
- `gen_audio_emitter` defaults `radius` to 10.0 and `volume` to 0.7. With `entity` the emitter follows that entity; with `position` alone it spawns a standalone entity of type `audio_emitter`. A name already in use replaces that emitter's voice.
- `gen_modify_audio` sets `volume` on the live track; a `radius` or a `sound` rebuilds the voice. A `sound` that does not parse is ignored without an error.
- `gen_audio_info` answers with `AudioInfoResponse` (`gen3d/commands.rs`): `active`, `ambience_layers`, `emitters`, `master_volume`.
- No tool removes one emitter, and deleting the entity it follows does not stop it. `gen_undo` of the spawn does, and `gen_clear_scene` removes them all.

## Auto-Inference System

`auto_infer_audio()` runs every frame over `Primitive` and `Mesh` entities that carry no emitter, and gives one to any whose name matches. The rules are `infer_emitter_from_name` in `crates/world-audio/src/graphs.rs`: a case-insensitive substring match, tried in this order.

| Keywords | Sound | Radius |
|----------|-------|--------|
| campfire, bonfire, fireplace, torch, brazier, fire | Fire (intensity: 0.6, crackle: 0.7) | 8.0 |
| waterfall, fountain, river, creek, water | Water (turbulence: 0.6) | 12.0 |
| generator, engine, machine, reactor, hum | Hum (frequency: 60 Hz, warmth: 0.5) | 10.0 |

A substring match is loose: `human` matches `hum`, `firefly` matches `fire`.

The emitter is named `<entity>_audio`, plays at volume 0.6 and follows the entity. To change it, call `gen_modify_audio` on that name. A `gen_audio_emitter` call under another name adds a second voice and leaves the inferred one playing.

## Implementation Details

### FunDSP Graph Patterns

The graphs live in `crates/world-audio/src/graphs.rs`, keyed on
`wt::AudioSource`. `graphs::build` returns a mono `AudioUnit` for a source,
or `None` for `File`, `Abc` and `Silence`. A single-chain sound (rain, the
wind emitter) is one FunDSP expression. A layered one (ocean, fire, hum) is
a `Net::new(0, 1)`: each layer is pushed as a node, and `pass() + pass()`
nodes wired with `set_source` sum them into the output.

The graph ends at the mono sum. Volume, panning and distance belong to the
kira track that plays it. The module needs no audio device, so a test in
`source.rs` renders every source and checks that it is finite and not
silent; it does not check what a sound sounds like.

### Voice Rebuilds

A sound's parameters (`speed`, `intensity`, …) are constants inside its
graph, and an emitter's distances are fixed when its spatial track is
built. So:

- `SetAmbience` drops every layer and builds the new ones;
- `AddEmitter` replaces any emitter of that name;
- a new radius or a new `sound` on an emitter rebuilds its voice;
- volume, an emitter's position and the listener are set on the live track.

## File Structure

- **`crates/gen/src/gen3d/audio.rs`** — the Bevy-side half
  - `AudioEngine` resource: names, base volumes, radii, `last_ambience` for world saves
  - The audio thread and the `AudioUpdate` messages it drains
  - Bevy systems: `spatial_audio_update`, `auto_infer_audio`, `sync_soundtrack_clock`, and the command handlers

- **`crates/world-audio/src/`** — `localgpt-world-audio`
  - `engine.rs` — `Engine`: the kira `AudioManager`, ambience layers, spatial emitters, the soundtrack
  - `graphs.rs` — the FunDSP graph for each `AudioSource`, and `infer_emitter_from_name()`
  - `source.rs` — `FundspSound`, a graph as a kira sound

- **Related files:**
  - `crates/gen/src/gen3d/commands.rs` — audio command/response types
  - `crates/gen/src/gen3d/compat.rs` — conversions between Gen's sound types and `wt::AudioSource`
  - `crates/gen/src/gen3d/tools.rs` — the four audio tools
  - `crates/gen/src/gen3d/plugin.rs` — Bevy system registration
  - `crates/gen/src/gen3d/registry.rs` — `AudioEmitter` entity type

## Possible extensions

These are ideas, not scheduled features:

- crossfades when switching ambience and smooth parameter interpolation;
- environmental reverb (`AmbienceCmd::reverb` exists; nothing sets or plays
  it);
- musical source formats such as Glicol or ABC, authored by an external agent
  and synthesized through the shared audio engine (the format has
  `AudioSource::Abc`, with no graph yet).

A musical extension needs its own design and must not add a prompt loop or a
model call to Gen.

## Manual check

The engine's tests skip what needs an output device, so a change to the
audible path is checked by ear:

```bash
cargo run -p localgpt-gen -- --mcp-relay
```

Point an MCP client at the window (`localgpt-gen mcp-server --connect`) and
ask its agent to create a forest clearing with a stream and a campfire, with
forest ambience and spatial water and fire emitters. Then verify:

1. forest ambience plays;
2. the stream and fire are audible at their positions;
3. moving the camera changes their volume and stereo position;
4. `gen_audio_info` reports the layers and emitters the agent created.

## Debugging

Gen logs at `warn` unless `-v` or `RUST_LOG` says otherwise:

```bash
RUST_LOG=localgpt_gen::gen3d::audio=debug,localgpt_world_audio=debug cargo run -p localgpt-gen -- --mcp-relay
```

Logging includes:
- Audio engine startup (`info`)
- A missing output device, a full mixer, a voice or a soundtrack that would not play (`warn`)
- A layer or an emitter skipped because its source has no graph (`debug`)

Emitter adds, voice rebuilds and position updates are not logged.
