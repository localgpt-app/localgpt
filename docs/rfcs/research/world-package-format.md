# Designing a world package format for LocalGPT Gen

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed the package format. Superseded in substance: its tool-call-log half by `multiplayer/session-package-format.md`, and its "single shareable archive" idea explicitly rejected by the published spec in favour of a small core plus profiles. The landscape survey stays useful.


> **Note:** This document consolidates research from the former "World packaging formats for AI-driven 3D creation.md" (deleted as ~85% duplicate). The landscape survey content from that doc is preserved below where unique.

**A local-first, AI-generated 3D world needs a format that bundles scene data, deterministic tool call logs, audio, and sandboxed behaviors into a single shareable archive.** No existing format does all of this. But glTF's extension model, ComfyUI's reproducible DAG serialization, OpenClaw's progressive-disclosure skill packaging, and Godot's human-readable TSCN scenes collectively provide a proven design vocabulary. This report distills research across five domains into actionable architecture guidance for a Bevy/Rust world package format.

The core insight: the format should be a **ZIP archive with aligned entries** (inspired by USDZ's mmap-friendly approach), containing a RON/JSON scene manifest, embedded assets, behavior scripts, and a deterministic tool call log that records exactly how the AI generated the world. This combination makes worlds reproducible, shareable, editable, and self-contained.

---

## OpenClaw's AgentSkills standard offers a proven packaging model

OpenClaw follows the **AgentSkills open specification** (agentskills.io), adopted by 26+ platforms including Claude Code, GitHub Copilot, and Gemini CLI. Each skill is a directory with a required `SKILL.md` file containing YAML frontmatter (name, description, license, compatibility, metadata) plus Markdown instructions, alongside optional `scripts/`, `references/`, and `assets/` folders.

The design uses **progressive disclosure** across three tiers: metadata (~100 tokens) loaded at startup for all skills, full instructions (<5,000 tokens) loaded on activation, and resources loaded on demand. This pattern directly applies to world packages — a manifest with lightweight metadata enables browsing, while heavy assets (meshes, textures, audio) load only when the world opens.

OpenClaw's workspace conventions establish a file-per-concern model: `SOUL.md` for persona, `MEMORY.md` for long-term state, `AGENTS.md` for operating instructions, `IDENTITY.md` for agent identity, and daily `memory/YYYY-MM-DD.md` logs. All state is **plain Markdown** — human-readable, git-backable, and diffable. The Lobster workflow engine adds deterministic YAML pipelines with approval checkpoints and resume tokens, stored in `.lobster` files.

For world packages, the key lessons are: use **text-based metadata** for discoverability and version control, adopt a **directory-per-concern** structure, and implement **progressive loading** so package browsers can display thumbnails and descriptions without parsing the full scene. OpenClaw's constraint that published skills contain only text files (Markdown, JSON, YAML, TOML, JS, SVG) also suggests a clean separation between the package manifest/metadata layer (always text) and the binary asset layer.

---

## Tool call serialization has converged on a common pattern but lacks a replay standard

Three major API formats — **Anthropic's content blocks, OpenAI's function calling, and MCP's JSON-RPC 2.0** — share a common structure: each tool invocation carries a unique correlation ID, a tool name, JSON arguments, and a result. Anthropic uses `toolu_*` prefixed IDs with `tool_use`/`tool_result` content blocks embedded in user/assistant messages. OpenAI uses `call_*` IDs with a dedicated `tool` role. MCP wraps everything in JSON-RPC 2.0 request/response pairs with integer IDs, adding `inputSchema`/`outputSchema` for validation and `structuredContent` for typed output.

**ComfyUI's workflow DAG serialization** is the most directly relevant model for reproducible AI generation. It maintains two JSON formats: a minimal **API format** (pure DAG where each node has a `class_type`, `inputs` with literal values or link references like `["4", 0]`, and a string node ID) and a full **Workflow format** (adding UI positions, widget values, link metadata). Fixed seeds plus complete parameter capture achieve deterministic reproduction. PNG outputs embed the full workflow JSON, enabling exact regeneration from any output image.

For observability, **LangSmith** records hierarchical run trees (each with UUID, run_type, inputs, outputs, parent/child relationships, timestamps), while **OpenTelemetry GenAI semantic conventions** use span hierarchies with attributes like `gen_ai.tool.name` and `gen_ai.tool.call.id`. Neither constitutes a replay format — they're observability tools.

**No universal "agent action replay" standard exists yet.** The closest approaches are ComfyUI's workflow JSON (proven reproducibility for generation pipelines) and MCP's structured tool calls (standardized invocation format). For LocalGPT Gen, a tool call log should adopt ComfyUI's dual-format philosophy: a **compact execution log** (minimal DAG of tool calls with arguments and cached results for deterministic replay) and an **enriched format** (adding AI reasoning, alternative paths considered, and UI metadata for editing). Each entry needs: sequential ID, tool name, JSON arguments, JSON result, content-addressed hash of the result, and optional dependency links to prior tool calls.

---

## Interactive world formats reveal two dominant architectures

Research across eight major formats reveals two fundamental approaches to packaging 3D worlds with interactivity:

**Archive-with-manifest** formats bundle all assets into a single file with an index. Godot's `.pck` uses a binary header, file index (paths, offsets, sizes, MD5 hashes), and file data — enabling fast random access. Roblox's `.rbxl` uses LZ4-compressed chunks (META, INST for instance types, PROP for columnar property data, PRNT for parent-child relationships). Unity's AssetBundle wraps `SerializedFile` entries in the `UnityFS` container with LZ4 chunk compression. USD's **USDZ** uses an uncompressed ZIP with **64-byte alignment** enabling direct memory mapping — no decompression needed for reading.

**Extension-based** formats embed interactivity within an existing standard. Mozilla Hubs packages entire worlds as single `.glb` files with `MOZ_hubs_components` extensions on nodes (nav-mesh, spawn-point, audio, particle-emitter). The glTF **KHR_interactivity** extension (approaching Khronos ratification) encodes behavior graphs directly in glTF JSON — nodes with flow sockets and value sockets connected in a DAG, accessing scene properties via JSON Pointer Templates like `/nodes/{nodeIndex}/translation`. PlayCanvas uses pure JSON with numeric asset IDs, flat entity references, and component maps.

The text-based scene formats offer the most relevant patterns for AI generation. Godot's `.tscn` declares nodes with `name`, `type`, `parent` path, and key-value properties in an INI-like format — trivially parseable and generatable. A-Frame proves that even HTML markup can effectively describe 3D scenes through entity-component attributes.

For Bevy specifically, **RON (Rust Object Notation)** is the natural scene serialization format, mapping directly to Bevy's `Reflect` system. The format should support both embedded assets (in-archive for self-contained distribution) and external URI references (for shared asset libraries). USD's composition model — sublayers for department-based overrides, variants for bundled alternatives, payloads for deferred loading — provides the most sophisticated approach to world composition and could be simplified for the local-first use case.

---

## Audio packaging needs a three-tier model with procedural music support

The emerging **KHR_audio** glTF extension defines the most portable audio architecture: **audio data** (file references via URI or embedded bufferView), **sources** (playback state: gain, loop, autoPlay), and **emitters** (spatial sinks: positional or global, with distance model, cone parameters, rolloff). This three-tier model separates storage from playback from spatialization, enabling reuse — one audio clip can feed multiple sources attached to different emitters.

Spatial audio properties are remarkably consistent across formats. Every major system encodes: **distance attenuation** (inverse, linear, or exponential model with refDistance, maxDistance, rolloffFactor), **directional cone** (innerAngle, outerAngle, outerGain), and **gain/volume**. Godot's `AudioStreamPlayer3D` adds a low-pass filter for distance-based frequency attenuation and Doppler tracking. USD's `UsdMediaSpatialAudio` takes a simpler approach — position, auralMode (spatial/nonSpatial), playbackMode, gain, and startTime/endTime in timecodes — deliberately leaving attenuation models to the application.

**Bevy's built-in audio is notably limited.** It uses `rodio` for basic stereo panning and distance-based volume, with no configurable distance models, directional cones, audio buses, effects, or HRTF support. The `bevy_kira_audio` crate adds tweening and channel-based playback but spatial audio remains basic. The world package format should define **richer audio metadata than Bevy currently supports**, designing for future capability.

For procedural music, **ABC notation** is extraordinarily compact — a full tune in 200-500 bytes of ASCII text, trivially parseable and ideal for AI generation. MIDI is more expressive (10-50 KB per song) but requires bundled SoundFont data (5-200 MB) for synthesis. No existing format combines music notation with 3D scene data, representing a genuine innovation opportunity. A world package could embed ABC notation or MIDI sequences as scene-level or node-level components with adaptive music rules inspired by FMOD/Wwise — state machines governing horizontal resequencing (section transitions) and vertical layering (parameter-driven mixing) of procedural tracks.

For compressed audio, **Opus** delivers the best quality-to-size ratio (64-128 kbps, royalty-free, low latency), though its ~80ms padding makes it wasteful for very short sounds. OGG Vorbis remains the most widely supported format in game engines. WAV is essential for short sound effects where decode latency matters.

---

## Scripting and behavior encoding should use a layered security model

Five approaches to encoding object behavior exist on a spectrum from declarative to imperative, each with different sandboxing and expressiveness trade-offs:

**Behavior graphs** (KHR_interactivity model) offer the strongest portability and sandboxing. They define a finite set of node types — events (`onStart`, `onTick`, `onSelect`), flow control (`branch`, `sequence`, `forLoop`), math, logic, variable access, and scene property manipulation via JSON Pointer Templates. No arbitrary code execution is possible. The format is JSON, fully schema-validated, and Turing-complete only in a bounded sense (mitigated by execution time limits). Implementations exist for Unity, Babylon.js, Android XR, and Magic Leap.

**Behavior trees** provide structured NPC intelligence. The BehaviorTree.CPP XML format is the industry standard, organizing nodes into actions (do work), conditions (check state), control (Sequence, Fallback, Parallel), and decorators (Inverter, Repeat, Timeout), with blackboard-based data sharing via typed ports. In Bevy, `bevy_behave` is the most active crate — it supports macro-based tree definition and runs 5K agents with physics, but **file-based serialization is not yet implemented** and would need contribution.

**Rhai scripting** is purpose-built for Rust embedding. It is **sandboxed by default** — an immutable engine cannot mutate its host environment unless explicitly permitted (whitelist approach, unlike Lua's blacklist). It offers serde serialization of AST, zero-overhead Rust type integration, custom syntax support, and runs on no-std and WASM targets. Performance is roughly 2x slower than Python. Integration with Bevy works through `bevy_mod_scripting` (supporting both Rhai and Lua with hot-reloading, multi-script entities, and Reflect-based bindings) or the dedicated `bevy_rhai` crate.

**WASM modules** provide maximum isolation and near-native performance via JIT compilation. The **WASI Component Model** (released February 2024) adds WIT interface definitions for formalizing the game-script API contract and capability-based security grants. `bevy_wasm` provides message-passing integration with protocol versioning. Wasmtime is the recommended runtime. The key overhead is at FFI boundaries — coarse-grained interfaces (batched updates) outperform fine-grained per-property calls.

**Lua** via LuaJIT offers the highest scripting performance but requires manual sandbox configuration (removing `os`, `io`, `file` modules). Defold engine demonstrates the pattern: `.script` files attached as components with lifecycle callbacks (`init`, `update`, `on_message`), referencing shared `.lua` modules via `require`.

For the world package format, a **layered approach** is recommended, ordered by increasing capability and decreasing sandboxing:

- **Layer 1 — Behavior graphs** (JSON, KHR_interactivity-compatible): simple interactions, animations, configurators. Fully deterministic, schema-validated, no runtime needed.
- **Layer 2 — Behavior trees** (XML/JSON, BehaviorTree.CPP-compatible): NPC patrol patterns, decision-making. Deterministic with seeded randomness.
- **Layer 3 — Rhai scripts** (`.rhai` files): event handlers, game logic, property animations. Sandboxed by default, serde-serializable ASTs.
- **Layer 4 — WASM modules** (`.wasm` files): user-generated mods, complex AI, untrusted content. Maximum isolation via WASI capabilities.

Each layer attaches to entities as components — scripts don't own entities, they're properties of them, matching both Godot's `script = ExtResource("id")` pattern and Bevy's ECS architecture.

---

## Synthesis: a proposed world package architecture

Drawing from all five research domains, the optimal format for LocalGPT Gen is a **ZIP archive with 64-byte-aligned entries** (USDZ approach, enabling mmap in Rust) containing:

```
world.lgw/
├── manifest.ron            # Package metadata, version, preview, dependencies
├── toolcalls.jsonl         # Deterministic tool call log (one JSON object per line)
├── toolcalls.dag.json      # ComfyUI-style DAG of tool call dependencies
├── scene.ron               # Entity hierarchy + components (Bevy Reflect)
├── assets/
│   ├── models/             # .glb files
│   ├── textures/           # .png/.ktx2
│   ├── audio/              # .ogg/.wav/.opus
│   └── music/              # .abc/.mid (procedural music notation)
├── behaviors/
│   ├── graphs/             # KHR_interactivity-style behavior graphs (.json)
│   ├── trees/              # Behavior tree definitions (.xml)
│   ├── scripts/            # Rhai scripts (.rhai)
│   └── wasm/               # WASM behavior modules (.wasm)
└── overrides/              # Layer overrides for variants/customization (.ron)
```

The `manifest.ron` follows OpenClaw's progressive-disclosure model — lightweight metadata (name, description, preview image reference, format version, content hashes) loads first for browsing. The `toolcalls.jsonl` records every AI tool invocation with sequential IDs, tool names, arguments, results, and content hashes, enabling exact world regeneration. The `scene.ron` serializes Bevy's ECS world using Reflect, with entity references to behaviors and assets by path. Audio follows KHR_audio's three-tier model (data → sources → emitters) with extensions for ABC notation procedural music. Behaviors attach to entities as components, with the layered security model ensuring untrusted packages can't escape their sandbox.

This architecture is **local-first** (single ZIP file, no cloud dependencies), **reproducible** (tool call log recreates the world from scratch), **shareable** (self-contained archive), **editable** (text-based scene and behavior files), and **native to Rust/Bevy** (RON serialization, Rhai scripting, WASM sandboxing).