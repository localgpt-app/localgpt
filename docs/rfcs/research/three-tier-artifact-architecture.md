# Three-tier artifact architecture for AI-driven 3D world creation

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed the Prompt / WorldSpec / WorldInstance split. The spec half shipped as `world-types`; the bidirectional-sync problem it surveys is settled differently — see `authoring/world-editor-stack.md`, where the ECS is a one-way projection of a fold.


**No existing system has fully solved the bidirectional sync problem between freeform creative intent and formal specifications—but a compelling architecture emerges from combining lens theory, constrained LLM decoding, content-addressable caching, and Bevy's ECS change detection.** The core insight across all seven research areas is that the Prompt ↔ WorldSpec ↔ WorldInstance pipeline maps directly onto patterns already proven in tools like Substance Designer (node graphs as specs), ComfyUI (JSON DAG workflows), and Terraform (plan/apply for infrastructure changes). What makes LocalGPT Gen unique is that it must solve all of these simultaneously in a local-first, single-binary Rust application—a constraint that eliminates cloud-dependent solutions but enables powerful content-addressed caching and embedded version control.

---

## The bidirectional editing problem has no perfect solution, but three patterns get close

The most critical design challenge in this architecture is maintaining coherence when creators edit at both Tier 1 (freeform prompt) and Tier 2 (formal WorldSpec). Research across design tools, CMS platforms, low-code builders, and safety-critical systems reveals a universal truth: **every production system either restricts editing to avoid the round-trip problem, makes one direction authoritative, or maintains both representations with manual reconciliation.**

Figma's approach is instructive. When a designer edits a component instance (overriding a design token), Figma tracks this as a **local override delta** rather than trying to reverse-engineer which token the raw value should map to. The override is stored separately from the token system. The token→instance direction is lossless; the instance→token direction is fundamentally lossy. This mirrors exactly the Tier 1→Tier 2 challenge: translating freeform text like "a castle with a moody atmosphere on a cliff" into a formal spec is lossy because aesthetic nuance has no formal representation.

The academic framework of **bidirectional transformations (lenses)** from programming language theory provides the theoretical foundation. A lens consists of `get: Source → View` and `put: Source × View → Source`. The critical insight is the **complement**—information in the source (Tier 1) not captured in the view (Tier 2) must be preserved across round-trips. The Boomerang project at UPenn demonstrated this for text format transformations, and the Augeas configuration tool uses lens-based bidirectional processing for Linux config files. The well-behavedness laws (GetPut and PutGet) guarantee round-trip fidelity when the mapping is well-defined.

For LocalGPT Gen, the recommended architecture combines three patterns:

**Complement preservation** treats Tier 1 as the authoritative source. When the LLM translates a prompt to a WorldSpec, it also produces a "complement" document capturing what was lost—aesthetic intent, mood descriptions, ambiguous references. When Tier 2 is edited, the `put` function merges spec changes with the preserved complement to update Tier 1, rather than regenerating from scratch. This preserves the creator's original voice and intent.

**Override/delta tracking** (from Figma) handles direct Tier 2 edits. When a technical creator modifies the WorldSpec directly—changing a castle's wall_height from 12 to 20—this is recorded as an explicit delta. The system can present these deltas during sync: "The spec says wall_height=20, but your prompt says 'towering walls.' Update prompt to match?" This is the same pattern database migrations use with forward/backward scripts.

**AI-mediated reconciliation** handles the inherently ambiguous gap. When structural changes at Tier 2 cannot be mechanically propagated to Tier 1 (e.g., adding a new room that the prompt never mentioned), an LLM rewrites the relevant prompt section to incorporate the change. This is the only approach that can handle the freeform↔formal mapping at scale, though it requires validation.

The low-code/no-code world offers a cautionary lesson: **Webflow, Framer, and most visual builders use one-way ejection**—visual→code export is permanent and irreversible. They sidestep bidirectional sync entirely. The systems that do support round-trip editing (UML tools like Enterprise Architect, Retool's inline-code model) succeed only because the mapping between representations is well-constrained. LocalGPT Gen should similarly constrain the bidirectional sync to well-defined structural mappings while using AI for the fuzzy remainder.

---

## Constrained LLM decoding eliminates the structured output reliability problem

The LLM-as-translator between Tier 1 and Tier 2 is the system's most technically demanding component. Research across text-to-SQL, code generation, and structured output systems reveals that **grammar-constrained decoding—not retry loops—is the right approach for a local-first application.**

Text-to-SQL systems provide the best benchmarks for LLM translation accuracy. DIN-SQL achieves **85.3% execution accuracy** on the Spider benchmark using GPT-4 with a four-module pipeline: schema linking → query classification → SQL generation → self-correction. DAIL-SQL reaches **86.6%** with self-consistency voting. The critical finding is that accuracy drops dramatically on real-world databases with large schemas—one study found only **0.35 accuracy** on industrial databases. This suggests that a WorldSpec schema should be kept focused and well-documented, with rich examples in the LLM prompt.

For local inference, constrained decoding guarantees syntactically valid output in a single pass. The key technologies are:

**GBNF grammars** (llama.cpp) define valid token sequences at the grammar level. A developer demonstrated this by constraining LLM output to a custom 8-bit CPU assembly language—the model could only produce valid opcodes. For LocalGPT Gen, a GBNF grammar for the WorldSpec format would ensure every LLM output is parseable, eliminating the need for retry loops. This is critical for local inference where each retry costs seconds.

**Microsoft's llguidance** (Rust-based) achieves ~**50 microseconds CPU time per token** for constrained decoding with a 128K-token vocabulary. It uses a derivative-based regex engine for lexing and an optimized Earley parser for CFG rules. OpenAI credited llguidance as foundational to their Structured Outputs feature. Being Rust-native, it integrates naturally with the LocalGPT Gen stack.

**XGrammar** uses pushdown automata (PDA) to handle recursive schemas that FSM-based tools like Outlines cannot. Since a WorldSpec likely includes nested structures (buildings containing rooms containing furniture), PDA-based constrained decoding is essential. XGrammar precomputes validity bitmasks for ~99% of vocabulary tokens, enabling up to **100x speedup** over traditional grammar methods.

The validation pipeline should be two-stage: constrained decoding ensures syntactic validity, then a separate semantic validator checks domain constraints (objects don't overlap, physics are satisfiable, material references exist). Invalid semantic results get fed back to the LLM with specific error messages—the Instructor library's pattern of appending Pydantic validation errors to conversation history, then re-calling the LLM, achieves convergence within 1-3 retries for most cases.

For local model selection, a **7B parameter model at Q4_K_M quantization** is the sweet spot: ~8GB RAM, 20+ tokens/second on consumer GPU, with reliable structured output. Qwen 2.5 7B Coder and DeepSeek-R1-Distill-Qwen-7B are strong candidates. The Rust inference stack should use either **candle** (pure Rust, HuggingFace-maintained, supports LLaMA/Mistral/Phi) or **llama-cpp-2** (thin Rust bindings to llama.cpp with native GBNF support). Candle produces truly single-binary builds with no Python dependency; llama-cpp-2 offers the broadest model compatibility and battle-tested constrained decoding.

---

## Version control requires a hybrid architecture: CRDTs for specs, content-addressed storage for assets

Versioning heterogeneous content—freeform text, images, JSON specs, generated 3D models—demands a hybrid approach rather than a single VCS. The research across game engines, ML pipelines, and creative tools converges on a three-layer storage architecture.

**For text and structured specs (Tier 1 prompts, Tier 2 WorldSpecs):** Automerge, a CRDT library with its core implementation in Rust, provides automatic merging for JSON-like documents. Each document stores its full edit history, enabling time-travel and branching—exactly what creative exploration requires. The alternative is **Loro**, a newer Rust CRDT library with higher performance, supporting rich text, lists, maps, and movable trees. For a single-user local-first app, even simpler approaches work: the **git2** crate provides full libgit2 functionality statically linked (no Git installation required), enabling embedded version control with branching, diffing, and merging of text-based specs.

**For binary assets (images, sketches, generated 3D models, textures):** A content-addressable blob store following the DVC/Nix pattern. Each asset is stored at `cache/{hash[0:2]}/{hash[2:]}` with SHA-256 content hashing. Small metadata files (like DVC's `.dvc` pointers) track the relationship between spec parameters and generated outputs. This enables **automatic deduplication**—identical assets generated from the same parameters are stored once—and **instant cache lookups** during incremental regeneration.

**For version metadata and indices:** SQLite via **rusqlite** (with the `bundled` feature compiling SQLite from source for zero external dependencies). A version DAG stores commits with parent references, asset manifests per version, pipeline dependencies, and timestamps. This follows Fossil SCM's insight that a single SQLite file is an excellent model for local-first version control—portable, atomic, and queryable.

**Semantic diffs** for non-text content remain an unsolved problem at scale, but useful tools exist. For structured data (WorldSpec JSON/RON), **Graphtage** produces semantic diffs across JSON/YAML/XML with Levenshtein distance for ordered sequences. For images, perceptual hashing (the `image-hasher` Rust crate) detects similarity. For 3D models, **diff3d** provides visual comparison of STL/OBJ files by rendering unchanged parts in gray and differences in red/green. For the WorldSpec specifically, diffing at the semantic level (which objects changed, which properties were modified) is more useful than text-level diffs—Godot's text-based `.tscn` scene format demonstrates that human-readable serialization enables meaningful diffs.

The version control workflow should mirror Godot's success with Git: serialize WorldSpecs as text (RON or a custom human-readable format), track them with embedded Git via git2-rs, and use content-addressed storage for binary assets with pointer files in the Git repository. Branching for creative exploration creates lightweight forks of the spec + prompt pair, while binary assets are deduplicated across branches through content addressing.

---

## Creative AI tools reveal that the spec layer already exists—it's just implicit

Across every creative AI tool researched, an intermediate specification layer exists between user intent and generated output. **The innovation in LocalGPT Gen is making this spec layer explicit, editable, and version-controlled.**

Midjourney's parameters (`--ar 16:9 --stylize 750 --chaos 30`) are effectively a spec layer between the text prompt and the generated image. ComfyUI takes this further: its JSON DAG workflows are the closest existing analog to a formal WorldSpec—every node, connection, parameter value, and processing step is serialized as JSON, automatically embedded in generated images for perfect reproducibility. ComfyUI workflows are version-controllable, shareable, and programmatically editable.

Text-to-3D tools universally use a **two-stage pipeline** that maps naturally to the three-tier architecture. Meshy separates geometry generation (preview mesh, ~60 seconds) from texturing—the preview stage is literally a spec that users evaluate before committing. Rodin by Hyperhuman exposes a rich parameter layer via its API: quality tiers, material type, geometry format, edge sharpness, PBR intensity, seed for reproducibility. This API payload *is* a formal spec: `{ prompt, quality, material, geometry_file_format, condition_mode, seed }`.

**Substance Designer provides the strongest analog** to the full three-tier system. Its node-based procedural graphs are formal specifications that encode every step from primitive inputs to final materials. The `.SBS`/`.SBSAR` file format distinction demonstrates two spec abstraction levels: full editable source versus compiled parametric asset with exposed parameters. Substance Designer's approach proves that **procedural/parametric specs are fundamentally more powerful than static snapshots**—changing one parameter cascades through the entire pipeline, enabling rapid exploration. A WorldSpec should be similarly procedural, not just a static description.

The architecture workflow (concept → schematic → design development → construction documents) adds a crucial insight: **changes get exponentially more expensive as you move toward the output tier.** In architecture, schematic design changes cost hours; construction document changes cost days; post-construction changes cost millions. The three-tier system should surface this cost differential: editing at Tier 1 (prompt) triggers broad regeneration, editing at Tier 2 (spec) enables surgical changes, and Tier 3 (instances) should be treated as derived artifacts that are never directly edited.

For variation exploration, the system should support **forking at any tier**. Midjourney's V1-V4 buttons, Photoshop's Layer Comps, and Google AI Studio's "Branch from here" all demonstrate that branching matches how humans think creatively—in divergent paths, not linear sequences. The WorldSpec format should natively support variant groups: "Generate this castle with stone walls AND with wooden walls, keeping everything else identical."

---

## Incremental regeneration combines Rust's red-green algorithm with Terraform's plan/apply

When a creator edits a prompt or spec, regenerating the entire 3D world is wasteful. The research across build systems, game engines, and infrastructure tools converges on a dependency-graph-driven approach.

**Rust's incremental compilation** uses a query-based model with automatic dependency tracking and a red-green algorithm. On recompilation, each node's input fingerprints are compared: unchanged inputs mark the node green (cached result valid), changed inputs mark it red (recomputation needed). The critical optimization is that if all predecessors are green, a node is green without loading or recomputing it. For LocalGPT Gen, each "query" maps to a generation step: "generate mesh for room X" depends on the room's spec fields and the mesh generation algorithm version. When a spec field changes, only downstream nodes producing different results are recomputed.

**Terraform's plan/apply** provides the user-facing pattern. Before applying spec changes, the system should produce a "world plan" showing what will be created, updated, or destroyed. This preview step—showing "Create 2 rooms, Update 1 material, Destroy 3 deprecated props"—gives creators confidence before committing to potentially expensive regeneration. Terraform's dependency graph construction (implicit dependencies from references + explicit `depends_on`) maps directly to scene graph dependencies (furniture depends on rooms, lighting depends on geometry).

**Content-addressable caching** (Nix store model) makes this efficient. Each generated asset is stored at a path derived from `SHA256(generation_params + algorithm_version)`. If the hash matches an existing cache entry, the asset is reused without regeneration. For procedural content with deterministic generation, expect **90%+ cache hit rates** during iterative editing. This requires that generation algorithms be deterministic—same inputs must produce identical outputs byte-for-byte—which is achievable with fixed random seeds and pinned algorithm versions.

**Bevy's ECS change detection** provides the runtime mechanism. Bevy automatically tracks component additions, removals, and modifications via `Changed<T>` and `Added<T>` query filters. A spec-to-scene pipeline can use this: when a `SpecSource` component changes, a system detects this via `Changed<SpecSource>`, computes the diff against the current scene state, and issues minimal ECS commands (spawn, despawn, insert/remove/update components). This is directly analogous to React's virtual DOM reconciliation but applied to a 3D scene graph.

The **hot-reload loop** ties these together. Bevy's asset hot-reload uses filesystem watching (via the `notify` crate) to detect changes and trigger `AssetEvent::Modified`. For spec-driven regeneration, the loop is: watch spec file → compute content hash → check cache → on miss, run LLM generation → diff old vs. new scene → apply minimal commands. The new **bevy_simple_subsecond_system** (powered by Dioxus's `subsecond` library) even enables hot-patching Rust code at runtime—generation algorithm changes can be tested without restarting the application.

---

## Concrete implementation: the Rust crate stack and data flow

The full data flow for LocalGPT Gen connects these patterns into a single pipeline:

```
Tier 1 (Prompt)                    Tier 2 (WorldSpec)              Tier 3 (WorldInstance)
┌─────────────┐    LLM + GBNF     ┌──────────────┐   Dep Graph    ┌─────────────────┐
│ Freeform     │ ──────────────→   │ Validated     │ ──────────→   │ 3D Scene in     │
│ text/images  │                   │ RON/JSON spec │               │ Bevy ECS        │
│              │ ←────────────── │               │ ←────────── │                 │
│ (Automerge)  │  AI rewrite +     │ (git2-rs)     │  Change       │ (CAS cache)     │
│              │  complement       │               │  detection    │                 │
└─────────────┘                    └──────────────┘               └─────────────────┘
```

The forward path (Tier 1 → Tier 2) uses constrained LLM decoding via llama-cpp-2 with a GBNF grammar defining the WorldSpec format. The LLM also produces a complement document stored alongside the spec. Schema validation via **schemars** (generating JSON Schema from Rust types with `#[derive(JsonSchema)]`) and **jsonschema** (validating against Draft 2020-12) ensures structural correctness. Semantic validation checks domain rules.

The backward path (Tier 2 → Tier 1) uses override tracking for structural changes and AI-mediated rewriting for semantic updates. The complement document ensures freeform details survive the round-trip.

The generation path (Tier 2 → Tier 3) builds a dependency graph from the spec, checks content-addressed cache entries in SQLite, generates only missing assets via LLM/procedural generation, then diffs the result against the current Bevy ECS world to produce minimal update commands.

The key Rust crates form the foundation:

- **Inference**: `candle` (pure Rust ML) or `llama-cpp-2` (llama.cpp bindings with GBNF)
- **Constrained decoding**: GBNF via llama.cpp, or `llguidance` (Rust-native, 50μs/token)
- **Serialization**: `serde` + `ron` (Bevy's preferred format) + `serde_json` (LLM communication)
- **Validation**: `schemars` (schema generation) + `jsonschema` (validation)
- **Version control**: `git2` (embedded libgit2, statically linked) for specs; `automerge` or `loro` for CRDT-based prompt editing
- **Storage**: `rusqlite` with `bundled` feature (zero-dependency SQLite) for metadata and cache index
- **Editor UI**: `bevy_egui` + `bevy_inspector_egui` for property editing and spec inspection
- **Asset embedding**: `rust-embed` (filesystem in dev, embedded in release)
- **File watching**: `notify` crate for hot-reload triggers

The WorldSpec format should use RON (Rusty Object Notation) as Bevy's native serialization format, enabling direct integration with Bevy's `DynamicScene` system and the emerging BSN (Bevy Scene Notation). A custom `WorldSpecLoader` implementing Bevy's `AssetLoader` trait handles parsing, validation, and dependency graph construction.

---

## Conclusion: the three-tier architecture is viable but demands careful scoping of bidirectionality

The research validates the three-tier architecture as sound, with strong precedents across creative tools, build systems, and formal specification workflows. Three insights emerged that weren't obvious at the outset.

First, **the bidirectional sync should be asymmetric by design.** Tier 1 → Tier 2 translation is the "easy" direction (well-suited to constrained LLM decoding). Tier 2 → Tier 1 back-propagation is fundamentally harder and should be scoped carefully—structural changes (adding/removing objects) can be mechanically reflected, while semantic changes (mood, style, atmosphere) require AI-mediated rewriting with human approval. Trying to make both directions equally automatic will produce a fragile system.

Second, **the WorldSpec should be procedural, not declarative.** Substance Designer's node graphs and ComfyUI's JSON DAGs prove that parametric specifications enable far richer exploration than static descriptions. A WorldSpec entry for a castle wall should encode not just `height: 12` but the generation recipe: `base_stone_pattern(seed=42) → weather_erosion(intensity=0.7) → moss_growth(coverage=0.3)`. This makes the spec both a reproducible blueprint and an exploration surface.

Third, **content-addressable caching is the key performance enabler** that makes the entire architecture practical for local-first use. Without caching, every prompt edit triggers full world regeneration (minutes of LLM inference + asset generation). With content-addressed caching and dependency tracking, iterative editing achieves sub-second updates for most changes. The Nix store model—immutable, content-addressed, with automatic deduplication—is the right foundation, implemented via SHA-256 hashing and SQLite indexing in a local cache directory.