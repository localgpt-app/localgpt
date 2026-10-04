# LocalGPT Gen: from AI assistant to 3D world platform

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed Gen's direction. Its snapshot of the code — "roughly 11 LLM tool calls spawning geometric primitives" — is long overtaken: Gen now has 40+ tools, a shared world format, audio, physics and collaborative rooms.


**LocalGPT Gen exists today as an early proof-of-concept — roughly 11 LLM tool calls spawning geometric primitives in Bevy — inside a well-built Rust AI assistant.** The vision described in this task (SpacetimeDB multiplayer, three-tier WorldSpec architecture, audio generation, procedural worlds) represents an ambitious roadmap with zero code evidence in the repository. This gap matters because the technical choices ahead will determine whether LocalGPT Gen becomes a credible alternative to Meta Horizon Worlds and Roblox's AI tools, or remains a toy demo. The good news: the Rust/Bevy ecosystem has matured enough to support most of the planned architecture, and the local-first positioning creates genuine differentiation against cloud-dependent competitors.

## What the codebase actually contains versus the roadmap

The GitHub repository at `localgpt-app/localgpt` reveals a **Rust-based AI assistant** with ~15k lines across 43 `.rs` files, 378+ stars, and Apache-2.0 licensing. The core product is a single-binary (~27MB) local AI assistant with persistent memory (MEMORY.md, SOUL.md, SQLite FTS5 + sqlite-vec), multi-provider LLM support (Claude, OpenAI, Ollama, GLM), multiple interfaces (CLI, web UI, egui desktop, Telegram bot), and a sophisticated security model (Landlock/seccomp sandboxing, HMAC-signed instructions, hash-chained audit logs).

LocalGPT Gen lives behind a `--features gen` Cargo flag, inflating the binary from 27MB to >100MB. It provides **11 LLM tools** — `gen_spawn_primitive`, `gen_modify_entity`, `gen_set_camera`, `gen_set_light`, `gen_spawn_mesh`, `gen_screenshot` — that spawn spheres, cubes, cylinders, and tori with material control. Bevy runs on the main thread while the agent loop runs on a background Tokio runtime, communicating via async mpsc channels. The author explicitly describes this as **"early and rough," a "proof of concept," and a "fun experiment"** inspired by not having access to Google's Project Genie 3.

None of the following exist in the codebase or documentation: SpacetimeDB integration, the three-tier WorldSpec architecture, Wave Function Collapse or procedural generation, audio generation, wgpu headless rendering, Bevy Remote Protocol usage, voxel support, multiplayer networking, or scene persistence beyond what Bevy provides natively. The localgpt.app website positions the product as "a local AI assistant with persistent memory," not a 3D platform.

This means the entire analysis below treats the described architecture as a **target roadmap** and evaluates its technical feasibility, ecosystem support, and strategic positioning accordingly.

## aichat patterns that survive the pivot to 3D worlds

The aichat project (sigoden/aichat) remains surprisingly relevant even when the primary output shifts from code to 3D scenes — but the relevance map changes significantly.

**High-priority borrowing targets stay the same.** aichat's trait-based LLM provider abstraction (`Client` trait with `chat_completions()`, `embeddings()`, `rerank()` across 20+ providers) is arguably *more* valuable for a 3D platform than for a coding assistant. LocalGPT Gen will need to orchestrate multiple model types — text LLMs for WorldSpec generation, audio models (ACE-Step, Stable Audio Open, Sesame CSM-1B), and potentially vision models for scene evaluation. Extending aichat's `Client` trait into a `GenerationBackend` trait with `generate_audio()` and `evaluate_scene()` methods alongside `chat_completions()` is a clean architectural path. The recursive tool-calling pattern (LLM calls tool → evaluates result → calls another tool) maps directly to iterative scene refinement: generate entities → screenshot → evaluate → adjust.

**Session management becomes more important, not less.** Iterative world building ("make the castle taller," "add fog to the valley," "change the background music") requires persistent context across sessions. aichat's compression system — automatic summarization when token count exceeds `compress_threshold`, injected via `summary_prompt` — prevents context window overflow while preserving world state awareness. For 3D creation, this could maintain a compressed "world memory" summarizing current scene state, style decisions, and user preferences.

**RAG shifts from code context to world lore.** aichat's hybrid vector + BM25 search (with reciprocal rank fusion) is useful for retrieving world-building context: biome definitions, architectural style guides, NPC backstories, material libraries. The implementation is text-only and would need extension to handle 3D asset references or spatial relationships, but the retrieval pattern itself transfers cleanly.

**What becomes irrelevant**: terminal rendering (crossterm, nu-ansi-term), REPL input (reedline), CLI parsing emphasis (clap), and code-specific tooling. The **core crates** that transfer directly are tokio, serde/serde_json, reqwest + eventsource (for SSE streaming), async-trait, parking_lot, and futures-util/tokio-stream.

## OpenClaw's SKILL.md needs radical redesign for 3D worlds

The SKILL.md format (AgentSkills specification from the OpenClaw/Cursor ecosystem) is **partially relevant but requires fundamental extension** for 3D world creation.

The current format — YAML frontmatter with name, description, and dependency declarations, plus markdown instruction body, `references/` directory, and `assets/` folder — could define world creation skill packs. A `medieval-castle-builder` skill would contain WorldSpec templates in `references/`, example scenes in `assets/`, and natural language instructions for how the LLM should approach castle generation. The dependency declaration pattern (`requires.bins`, `requires.env`) maps to declaring which generation models must be available.

The critical gap is that **SKILL.md is text-instruction-only**. For 3D world generation, skills need to reference 3D assets, audio samples, material libraries, and spatial constraint rules (WFC adjacency definitions). The proposed Composable Skills RFC (OpenClaw issue #11919) moves in the right direction with `requires.interfaces` (abstract capabilities like TTS, camera), `provides` declarations, and skill composition — a `scene-with-audio` skill declaring `requires.interfaces: [music-gen, sfx-gen, dialogue-gen]` is exactly the pattern needed.

OpenClaw's TTS/STT architecture offers a useful **provider abstraction pattern** (interface with ElevenLabs/OpenAI/Edge implementations + automatic fallback chains), even though the scope mismatches. The model override directive pattern (`[[tts:provider=elevenlabs voiceId=...]]`) could inform inline audio generation parameters: `[[music:style=ambient tempo=80 key=Cmin]]` or `[[sfx:type=footstep surface=stone]]`.

**Neither aichat nor OpenClaw has any meaningful patterns for multiplayer collaboration**, conflict resolution, or shared context management. Both are fundamentally single-user tools. The multiplayer architecture must come from SpacetimeDB, CRDTs, and game networking patterns.

## The competitive landscape reveals LocalGPT Gen's real opportunity

The AI-powered 3D world generation market splits into two categories: world/asset generators and NPC intelligence platforms. No competitor yet delivers a complete local-first, open-architecture solution.

**Meta Horizon Worlds has the most comprehensive integrated suite** shipping today. Its Desktop Editor (launched February 2025) includes text-to-3D mesh generation (~3-6 minutes per generation), texture generation, skybox generation (6 styles), sound effects and ambient audio generation, TypeScript code assist, and a Creator Assistant agent. AssetGen 2.0 (announced May 2025) uses a single-stage 3D diffusion model for higher-quality meshes. WorldGen (research phase) generates trimesh 3D worlds from text but is limited to 50×50m spaces. Everything runs on Meta's cloud. Worlds are instantly multiplayer. Free for creators with a $50M Creator Fund.

**Roblox Cube 3D is the most open competitor.** Launched March 2025 at GDC, it's a fully open-source 3D foundation model using novel autoregressive 3D tokenization (VQ-VAE + GPT-style transformer predicting next 3D tokens). It generates game-ready meshes in seconds, available both in Roblox Studio and as a Lua API for in-experience generation. The v0.5 update added higher fidelity and bounding box conditioning. Roblox also ships Text Generation, TTS/STT, and real-time translation APIs. Their "4D Creation" vision targets functional objects (a generated car you can drive).

**NVIDIA ChatUSD / USD Code NIM targets enterprise**, using Llama-3.1-70B fine-tuned on USD functions to generate Python-USD code for 3D scenes. It's code generation, not neural meshes — the LLM writes programmatic scene descriptions rendered via Omniverse RTX. Enterprise-priced, deployed as NIM microservices.

**Google Genie represents a paradigm shift**: pure neural video generation simulating 3D environments frame-by-frame at 24fps, 720p. No actual 3D geometry is produced. Project Genie launched as a consumer prototype in January 2026 for Google AI Ultra subscribers ($249.99/month), with 60-second sessions and imperfect physics. Radical approach but limited practical utility today.

**Unity Muse is transitioning to Unity AI** (Unity 6.2), adding sound generation and third-party model support (Scenario, Layer LoRAs on Stable Diffusion/Flux). Notably, **Unity still lacks native 3D mesh generation** — a conspicuous gap. The Muse→AI transition is disruptive, with no data migration path.

**Inworld AI and Convai** compete for NPC intelligence middleware. Inworld's Character Engine orchestrates LLMs, emotions, TTS (#1 ranked, sub-200ms latency), STT, animations, and memory, serving Disney, Ubisoft, and Xbox. Convai differentiates on spatial cognition — characters perceive and interact with their environment. Neither generates 3D assets; they provide the "brain" for characters.

**SceneCraft** (Google DeepMind/Caltech, ICML 2024) is the closest architectural analog to LocalGPT Gen's approach: text → scene graph → Python code → Blender renders, with an inner refinement loop and outer library learning loop. It's research-only (no public code), uses GPT-4V, and achieves 45% improvement over BlenderGPT on CLIP scores.

## The Bevy ecosystem can support the vision today

The Rust/Bevy crate ecosystem has reached sufficient maturity to build the planned platform, though several integration bridges don't exist yet.

**For audio, bevy_seedling + Firewheel is the correct strategic choice.** bevy_seedling (v0.6.1, Bevy 0.17.2 compatible) is the official Bevy integration layer for Firewheel, a mid-level audio graph engine designed to become Bevy's default. Its DAG-based audio graph enables complex routing — effects chains, mixers, spatial processing — ideal for 3D worlds with multiple audio sources. **FunDSP** (v0.23.0) complements this perfectly for procedural audio synthesis, with an inline Rust DSL (`noise() >> lowpass_hz(1000.0, 1.0)`) that could generate dynamic soundscapes and physics-driven sounds. FunDSP can be integrated as a custom Firewheel node. bevy_kira_audio (v0.24.0, ~9,300 downloads/month) is more mature but architecturally simpler — useful as a fallback.

For AI-generated audio, **ACE-Step v1.5** (released January 2026) generates full songs in under 2 seconds on A100 with <4GB VRAM, supports 50+ languages, and is commercially safe (trained on licensed data). **Stable Audio Open** handles SFX and ambient audio (up to 47s stereo at 44.1kHz), with a "Small" variant (341M params) that runs on mobile. **Sesame CSM-1B** provides context-aware conversational speech synthesis with voice cloning for NPC dialogue. All three are Python/PyTorch models requiring a service bridge to the Rust stack.

**For procedural generation, ghx_proc_gen delivers WFC in 3D.** It supports both 2D and 3D Cartesian grids with socket-based adjacency constraints and automatic rotation variants. The Bevy integration (bevy_ghx_proc_gen) provides step-by-step visualization. Combine with the **noise** crate for terrain heightmaps and **bevy_voxel_world** for voxel-based editing (multithreaded meshing, infinite worlds with only modified voxels stored, LOD support).

**Bevy Remote Protocol (BRP) is the critical bridge** between LLM-driven generation and the Bevy runtime. Built into Bevy since 0.15, BRP provides JSON-RPC 2.0 over HTTP for entity CRUD (spawn, insert, remove, destroy), component get/set, queries, and type registry schema. The **bevy_brp_mcp** community crate already enables AI assistants to control Bevy apps via BRP. This should replace the current mpsc channel approach, enabling any external process (Python AI service, web editor, multiplayer server) to manipulate the Bevy world.

**Built-in bevy_picking** (upstreamed from bevy_mod_picking in Bevy 0.15) provides pointer events, modular raycasting backends, and event bubbling through entity hierarchies — essential for interactive world editing. **Avian Physics** (avian3d v0.5) offers ECS-native 3D physics with a 3x performance improvement in v0.4. **Blenvy** enables Blender-to-Bevy scene workflows with ECS component data embedded in GLTF.

## SpacetimeDB plus Loro CRDTs is the right multiplayer architecture

For collaborative 3D world editing, the recommended architecture is a **layered hybrid: SpacetimeDB for authoritative infrastructure + Loro CRDTs for fine-grained collaborative editing**.

SpacetimeDB (v1.0+ since March 2024, now v1.12+) merges database and application server — clients connect directly and execute Rust/WASM modules with ACID transactions. The **bevy_spacetimedb** community crate (~9,270 downloads) provides Bevy integration via `StdbPlugin`, `StdbConnection` resource, and event readers for table changes. BitCraft Online demonstrates the spatial sharding pattern: multiple SpacetimeDB instances each managing a spatial partition of the world, with application-level coordination for shard boundaries.

However, **SpacetimeDB alone is insufficient for real-time collaborative scene editing**. Its conflict resolution is transaction-level serialization (last-writer-wins at the transaction level), not property-level merging. Two users simultaneously moving different sub-objects in the same hierarchy would result in serialized transactions — one wins, one retries — rather than clean merging.

**Loro** (Rust CRDT framework) fills this gap with its **MovableTree** data type, which implements Kleppmann et al.'s algorithm for replicated trees with ordered siblings via fractional indexing. A 3D scene graph maps directly: parent-child hierarchies as MovableTree nodes, per-object properties as LWW Map entries (transform, material, geometry), render ordering as fractional indices. Loro provides Git-like version history, branching, undo/redo, and incremental update export — all designed for Figma-style canvas applications.

The hybrid works because SpacetimeDB handles infrastructure problems (persistence, subscriptions, auth, spatial sharding, AI job orchestration) while Loro handles collaborative editing problems (concurrent merging, property-level conflict resolution, undo/redo). AI-generated content enters as CRDT operations from a virtual peer, merging cleanly with human edits. This mirrors **Figma's architecture**: authoritative Rust server + CRDT-inspired conflict resolution + WebSocket sync. No existing Loro↔Bevy bridge exists — building one is a meaningful engineering investment but architecturally straightforward (Loro document changes → Bevy entity updates via observers).

## Updated crate priorities for 3D world generation

The crate priority list shifts dramatically from the coding-assistant assumption. Here are the tiers:

**Tier 1 — Core infrastructure (already in aichat, directly reusable):** tokio (async runtime), serde + serde_json (WorldSpec serialization), reqwest + eventsource (LLM API calls), async-trait (provider abstraction), parking_lot (shared Bevy state), futures-util + tokio-stream (streaming generation).

**Tier 2 — Bevy ecosystem (not in aichat, higher priority than anything aichat-specific):** bevy (rendering + ECS), bevy_seedling + Firewheel (audio graph), bevy_picking (interactive editing), bevy_egui (editor UI), avian3d (physics), bevy_spacetimedb (multiplayer), bevy_voxel_world (voxel editing), ghx_proc_gen/bevy_ghx_proc_gen (WFC), noise (terrain), blenvy (Blender workflow).

**Tier 3 — Audio synthesis and AI (new, not in aichat):** FunDSP (procedural audio), ACE-Step v1.5 + Stable Audio Open + Sesame CSM-1B (AI audio generation via Python service bridge), Loro (CRDT for collaborative editing).

**Tier 4 — aichat patterns to adapt (not crates, but architectural patterns):** LLM provider trait abstraction → extend to GenerationBackend, recursive tool calling → scene refinement loop, RAG hybrid search → world lore retrieval, session compression → world memory, HTTP server → unified generation API.

**Tier 5 — Drop entirely:** reedline, crossterm, nu-ansi-term, clap (terminal-centric crates irrelevant to a 3D GUI application). The fastembed and sqlite-vec crates from core LocalGPT remain useful for the memory system but are not 3D-specific.

## Strategic differentiation against cloud platforms and proprietary ecosystems

LocalGPT Gen's positioning against the competitive landscape reveals three defensible advantages and two significant risks.

**Advantage 1: Local-first architecture.** Every major competitor — Meta Horizon Worlds, Roblox Cube 3D (API), Unity AI (cloud generators), Inworld AI, Convai, Google Genie — requires cloud connectivity for AI generation. LocalGPT Gen's single-binary approach with local model inference (Ollama, fastembed, potentially local ACE-Step/Stable Audio Open) means **zero latency dependency, full offline capability, and no per-generation API costs**. Stable Audio Open's "Small" variant (341M params) already runs on smartphones. As models shrink, this advantage compounds.

**Advantage 2: Open architecture with no platform lock-in.** Meta requires Quest ecosystem. Roblox requires publishing on Roblox. Unity requires Unity licensing. NVIDIA requires Omniverse. LocalGPT Gen with Bevy + wgpu + OpenUSD/glTF export could target any platform — VR headsets, web browsers (WASM), desktop, or mobile — without platform tax. The Rust + Apache-2.0 licensing means no corporate dependency.

**Advantage 3: Composable AI pipeline.** Rather than one monolithic AI system, the three-tier architecture (prompt → WorldSpec → WorldInstance) with pluggable generation backends allows mixing best-of-breed models: Roblox's open-source Cube 3D for mesh generation, ACE-Step for music, Inworld-style patterns for NPC behavior, ghx_proc_gen for procedural layout. No competitor offers this composability.

**Risk 1: Execution gap.** The distance between the current proof-of-concept (11 tool calls spawning primitives) and the vision (MMO-scale multiplayer world co-creation with audio, procedural generation, and NPC behavior) is enormous. Meta has thousands of engineers. Roblox has a decade of platform infrastructure. The critical path requires building the WorldSpec schema, BRP-based generation pipeline, Loro↔Bevy bridge, SpacetimeDB integration, and at least one compelling procedural generation demo.

**Risk 2: Model quality at the edge.** Local inference means smaller models with lower quality. Meta's cloud-based AssetGen 2.0 will produce better meshes than anything running locally on consumer hardware in 2026. The mitigation is the hybrid approach — local for iteration speed, cloud-optional for final quality — but this partially undermines the local-first story.

## Conclusion

The analysis reveals that LocalGPT Gen's greatest asset is not its current codebase — which is a solid AI assistant with a minimal 3D experiment — but its **architectural vision and ecosystem positioning**. The Rust/Bevy ecosystem now provides every major component needed: Firewheel for audio graphs, BRP for external scene control, Loro for collaborative CRDTs, SpacetimeDB for multiplayer infrastructure, ghx_proc_gen for WFC, and bevy_voxel_world for voxel editing. None of these integrations exist in the codebase today.

The most impactful next steps, in order: (1) replace the mpsc channel architecture with BRP-based scene control, enabling any external process to manipulate the Bevy world; (2) define the WorldSpec schema as a serializable Rust type system that bridges LLM output and Bevy ECS; (3) integrate ghx_proc_gen to demonstrate procedural generation beyond primitive spawning; (4) add bevy_seedling + FunDSP for spatial audio that makes generated worlds feel alive. SpacetimeDB multiplayer and Loro CRDTs are architecturally important but can wait until single-player generation is compelling.

The competitive window is real. No one has built the "local-first, open-architecture, composable-AI world creation platform." Meta and Roblox own the cloud-and-platform-lock-in space. NVIDIA owns enterprise. Google Genie is research. The gap for an open, hackable, offline-capable world creation tool exists — but only if the proof-of-concept evolves into a functional pipeline before the incumbents close it.