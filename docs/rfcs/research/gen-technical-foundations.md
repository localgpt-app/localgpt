# LocalGPT Gen: technical foundations for an AI-driven explorable world creator

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed the dual-artifact pattern (an editable specification plus a concrete output) that became `world-types` and the `.world` package.


**An AI-powered tool that generates explorable 3D worlds from text, images, and sketches is now architecturally feasible — but requires careful design decisions at every layer.** The field has moved rapidly: Google DeepMind's Genie generates interactive worlds frame-by-frame from images, World Labs ships commercial 3D world generation via API, and Tencent's open-source HunyuanWorld produces navigable environments with mesh export. The critical insight for LocalGPT Gen is that none of these systems implement the dual-artifact pattern (editable specification + concrete output) that makes professional creative tools powerful. By combining LLM-driven specification generation with procedural and AI-based world construction on Bevy's ECS architecture, LocalGPT Gen can occupy a unique position: the first tool where worlds have "source code" that creators can version, branch, and evolve.

---

## How DeepMind's Genie generates interactive worlds from single images

Google DeepMind's Genie represents the frontier of AI world generation — but its approach is fundamentally different from what LocalGPT Gen needs. Understanding why clarifies the design space.

**Genie 1** (February 2024, ICML 2024) is an **11-billion-parameter** model that generates playable 2D platformer environments from a single image. Its architecture has three components: a Spatiotemporal Video Tokenizer (ST-ViViT) that compresses video frames into discrete tokens using a VQ-VAE with spatiotemporal transformer blocks; a Latent Action Model (LAM) that infers **8 discrete action codes** from unlabeled video by learning what changes between consecutive frames; and a MaskGIT-based dynamics model that predicts the next frame's tokens given current state plus action. The system trained on **30,000 hours** of platformer gameplay video (6.8 million clips) without any action labels — the LAM discovers actions like "move left" and "jump" purely from visual patterns. At inference, a user provides a starting image and selects actions; the model generates each new frame autoregressively. The critical limitation: **160×90 resolution at approximately 1 FPS**, with consistency lasting only **16 frames (~1.6 seconds)**.

**Genie 2** (December 2024) made a dramatic leap to **3D worlds at ~720p** using an autoregressive latent diffusion model instead of discrete tokens. It handles keyboard and mouse input, generates photorealistic environments from any image (photos, concept art, AI-generated images), and exhibits emergent physical simulation — gravity, water effects, lighting, reflections, and object interactions — all learned from video data with no explicit physics engine. A distilled variant achieves real-time generation. Memory of off-screen world regions persists for **10–60 seconds** before coherence degrades. **Genie 3** (August 2025) extended this to 720p at 24 FPS with multi-minute consistency, powering the Project Genie consumer preview launched in January 2026 at $249.99/month for Google AI Ultra subscribers.

The key distinction from LocalGPT Gen's approach is fundamental: **Genie generates pixels, not 3D assets**. There is no scene graph, no mesh, no material — just predicted video frames. This means worlds cannot be edited, assets cannot be extracted, and the output cannot be imported into a game engine. Genie is a world simulator; LocalGPT Gen needs to be a world constructor. The frame-by-frame generation also requires massive compute (minimum 8 TPU v5 chips for Genie 3) and accumulates drift errors over time. None of Genie's models are open-source, though community reimplementations exist (open-genie on GitHub, GenieRedux achieving comparable tokenizer quality).

---

## The landscape of AI systems that actually produce explorable 3D worlds

The systems most relevant to LocalGPT Gen are those generating navigable 3D environments with extractable assets — a much smaller set than the broader "AI + 3D" space.

**World Labs Marble** (launched November 2025, founded by Fei-Fei Li, $230M+ funding) is the most production-ready system generating explorable 3D worlds. It accepts text, images, video, panoramas, or coarse 3D layouts as input and produces **persistent, navigable 3D worlds** with Gaussian splat and triangle mesh export compatible with Unity and Unreal Engine. Their Chisel editor enables structure-style separation — blocking out walls and volumes, then applying visual style via text prompts (analogous to HTML/CSS separation). Pricing ranges from free (4 generations) to $95/month (75 generations). Their World API enables programmatic generation, and Spark provides open-source Three.js web rendering. Current limitation: room-sized environments, not open worlds.

**Tencent HunyuanWorld** (July 2025) is the **first open-source system** for simulation-capable 3D world generation. It generates explorable worlds from text or single images using panoramic world proxies for 360° immersion, with mesh export for CG pipeline compatibility. The 1.5 release (WorldPlay, December 2025) achieves **real-time interaction at 24 FPS** with long-term geometric consistency. Requires 60–80GB GPU memory minimum. Available on GitHub and HuggingFace.

**Meta WorldGen** (2025) represents the most architecturally instructive approach for LocalGPT Gen. Its modular pipeline mirrors traditional game development: an LLM parses the text prompt and configures a procedural generator to create a "blockout" (basic spatial layout); an image-to-3D model builds initial geometry; an AutoPartGen module decomposes the scene into individual manipulable objects; and a texture enhancement pass refines visual quality. Output is standard textured trimesh with navigation mesh, **directly compatible with Unity and Unreal without conversion**. It generates 50×50 meter traversable scenes in approximately 5 minutes. The weakness is grid-like layouts and inability to handle multi-floor environments.

**3D-GPT** and **SceneCraft** demonstrate the LLM-agent pattern most directly applicable to LocalGPT Gen. 3D-GPT uses three collaborative agents — a Task Dispatch Agent, Conceptualization Agent (expanding "misty spring morning" into specific tree branch lengths and leaf types), and Modeling Agent (generating Python code for Blender's Infinigen procedural generator). SceneCraft (ICML 2024) adds a dual-loop self-improvement mechanism: an inner loop generates Blender scripts, renders them, and uses GPT-4V to critique and refine, while an outer loop compiles reusable functions into a library. Both produce Blender scenes via executable code rather than pixel prediction — making the output editable and re-renderable.

For **asset-level generation** to populate worlds, the current leaders are: **Meta 3D AssetGen** (PBR meshes in ~30 seconds, 72% human preference over competitors), **InstantMesh** (multi-view reconstruction in ~10 seconds, superior geometry to TripoSR), and **Hunyuan3D 2.0** (open-source, complete pipeline with PBR texture baking). All output standard mesh formats (.obj, .glb, .fbx) directly usable in game engines. **TripoSR** remains the fastest at under 0.5 seconds but produces only vertex colors, not PBR materials. For texturing existing meshes, Meta's TextureGen retextures any mesh from text in ~20 seconds.

---

## Designing the World Spec format: lessons from scene description standards

The choice of specification format is perhaps LocalGPT Gen's most consequential design decision. After analyzing seven major formats, a clear design emerges: **combine A-Frame's LLM-friendly conciseness with USD's composition power and Godot's compact hierarchy**.

**USD (Universal Scene Description)** has the most sophisticated composition system of any format. Its USDA text format is clean and hierarchical, with six composition arcs enabling layering, referencing, variants, and lazy loading. A VariantSet can encode LOD levels, damaged/undamaged states, or style alternatives switchable at runtime. The `over` specifier allows patching existing prims without redefining them — directly enabling the "override" pattern needed for WorldSpec evolution. USD's opinion strength ordering (LIVRPS: Local > Inherits > VariantSets > References > Payloads > Specializes) resolves conflicts deterministically. The ecosystem is massive: Pixar, NVIDIA Omniverse, Apple, Adobe, Autodesk, Blender 4.0+. However, USD's conceptual complexity (six composition arcs, each with subtle semantics) makes it challenging for LLM generation of complex compositions.

A simple USD scene illustrates the format:
```usda
#usda 1.0
(defaultPrim = "World")

def Xform "World" {
    def Sphere "MySphere" {
        double radius = 2.0
        color3f[] primvars:displayColor = [(0.1, 0.5, 0.8)]
    }
    def Cube "MyCube" {
        double size = 1.0
        double3 xformOp:translate = (3.0, 0.0, 0.0)
        uniform token[] xformOpOrder = ["xformOp:translate"]
    }
}
```

**A-Frame's HTML-based scenes** are the **most LLM-friendly format** by a significant margin. LLMs already excel at HTML generation due to massive training data, and A-Frame encodes a complete 3D scene in ~20 lines of highly readable markup. A box, sphere, cylinder, sky, and ground plane with lighting fit in a block smaller than most configuration files. The hierarchical nesting maps naturally to scene graphs, and component attributes as key-value pairs are simple and declarative. However, A-Frame lacks composition, layering, variants, and is tied to the Three.js web runtime.

**Godot's .tscn format** achieves remarkable compactness through parent-path hierarchy and only storing non-default properties. Its external resource references (`ext_resource`) and scene inheritance (creating derived scenes that override specific properties) map well to the WorldSpec needs. The INI-like syntax is easy to parse and generate.

**Bevy's current .scn.ron format is unsuitable** for WorldSpec. It serializes fully qualified type names (`bevy_transform::components::transform::Transform`), uses flat entity lists without hierarchy, includes computed values and defaults, and produces 150+ lines for a single empty UI entity. The upcoming **BSN (Bevy Scene Notation)**, targeting Bevy 0.18, dramatically improves this with hierarchical nesting via square brackets, scene inheritance with patches, and a terse Rust-like syntax. BSN is still in draft (PR #20158) and not yet stable.

**The recommended approach for WorldSpec is a custom format** that compiles to standard formats. Key design principles derived from analysis:

- **Semantic primitives** rather than raw geometry — `Terrain`, `Building`, `Forest` instead of vertex buffers — enabling LLMs to describe intent at a natural abstraction level
- **Hierarchical multi-LOD natively** — each entity specifies intent at high, medium, and detailed levels: `Building(style: "gothic", floors: 3)` at high level, facade rules at medium, window placements at detail level
- **Two-arc composition** (simpler than USD's six) — layering for non-destructive overrides and references for reusing world fragments
- **Variant system** for generating multiple versions from the same base specification
- **Minimal boilerplate** — no UUIDs, no binary blobs, no index-based cross-references
- **Transpilation targets** — the custom format compiles to USD (for interchange) or directly to Bevy scene data (for runtime)

---

## Procedural generation as the execution engine for specifications

The WorldSpec format defines what a world should be; the generation engine determines how to build it. Every major procedural generation system implements the same architectural pattern: **specification artifact → generation engine → concrete output**. The specification style varies — WFC uses tilesets with adjacency rules, Houdini uses node graphs with parameters, CityEngine uses CGA shape grammars, terrain generators use erosion parameters and seeds — but all achieve **deterministic regeneration through seed control**.

**CityEngine's CGA (Computer Generated Architecture)** provides the closest analogy to hierarchical WorldSpec generation. CGA rules decompose shapes top-down: `Lot → extrude → Envelope → split(y) → Floor → split(x) → Tile → Window`. Each level adds detail, and the entire derivation is controlled by attributes (`attr height = 30`, `attr tile_width = 4`) that can be overridden per-building or per-selection. A CGA rule file is compact text that generates complete 3D buildings with textures. This hierarchical refinement pattern — high-level intent progressively resolved into concrete geometry — maps directly to WorldSpec's three-tier specification model.

**Wave Function Collapse (WFC)** demonstrates how constraint-based generation separates rules from output. Given a tileset with adjacency constraints and a random seed, WFC deterministically fills a grid by iteratively collapsing the cell with lowest entropy and propagating constraints to neighbors. The specification is small (tiles + adjacency rules + weights), the output is a complete tilemap, and regeneration from the same seed is identical. WFC excels at filling interiors and decorating spaces — it's used in Bad North for island generation and Townscaper for building assembly. **Hierarchical Semantic WFC** (2023) organizes tilesets into meta-tiles for two-pass generation: first assign abstract types ("castle wing"), then fill with detail tiles.

**Houdini's approach** is the most sophisticated: the `.hip` file IS the recipe, not the content. Houdini Digital Assets (HDAs) package procedural node networks with curated parameter interfaces, and PDG (Procedural Dependency Graph) distributes work items with varying attribute values through the pipeline. The key insight: **every node that uses randomness accepts a seed parameter**, and a single centralized seed attribute referenced by all downstream nodes ensures full reproducibility. This is the model LocalGPT Gen should follow — the WorldSpec contains seeds at each generation stage, ensuring the same spec always produces the same world.

For **constraint-based generation**, Answer Set Programming (ASP) offers a powerful declarative approach where the design space is modeled as logic rules plus integrity constraints, and an off-the-shelf solver enumerates valid solutions. WFC has been implemented as an ASP program (Karth & Smith, 2017), demonstrating that constraint solving is the fundamental mechanism underlying most procedural generation. This enables adding global constraints like "a path must exist from entrance to exit" or "all biomes must have water access" that procedural rule systems cannot easily express.

---

## The dual-artifact pattern: why "source code for worlds" works

The WorldSpec/WorldInstance duality draws on proven patterns across software engineering. Analysis of seven dual-artifact systems reveals consistent design principles.

The most instructive analogy is **source code → compiled binary**. What makes source code valuable for creators — human-readable text, version-controllable with line-oriented diffs, composable via modules and libraries, editable with simple tools — is precisely what WorldSpec must achieve. What makes binaries valuable for consumers — no tooling beyond the OS needed, performance-optimized for the target platform, self-contained — defines WorldInstance requirements. The **LLVM intermediate representation** demonstrates a crucial "third artifact" pattern: a normalized, optimizable form between source and output that enables N frontends × M backends. A WorldSpec pipeline benefits similarly from an **intermediate scene graph** — fully resolved, validated, all references expanded — before platform-specific export.

**Terraform's plan/apply duality** is directly relevant to live world editing. Before applying specification changes to a running world, generate a preview showing what will be created, modified, or destroyed. The state file pattern — tracking the mapping between declared specification and instantiated world — enables **drift detection** (when a live world diverges from its spec), **incremental updates** (changing only what's different), and **review workflows** (teammates approve changes before application). This maps to a `worldspec plan` → `worldspec apply` workflow.

**Docker's layer model** informs how world updates should work. Changes can be represented as incremental diffs (new layer) rather than full rebuilds. Content-addressable layers (identified by SHA256 hash) enable deduplication across world versions and shared caching. The image/container separation — immutable layers for distribution, writable layer for runtime state — maps to WorldInstance bundles (immutable, distributable) versus live world sessions (with player state, runtime modifications).

**Game engine build pipelines** demonstrate platform-specific "cooking." Unreal's BuildCookRun strips editor-only data, byte-swaps for target architecture, converts textures to platform-native compression (ASTC for mobile, BC7 for desktop), excludes unreferenced assets, and creates seek-free packages. Unity's AssetBundles are platform-specific binaries — an iOS bundle is incompatible with Android. WorldInstance export must similarly cook for each target: desktop native, mobile, web/WASM, with different texture formats, mesh budgets, and compression strategies per platform.

The key design decisions that make dual-artifact systems successful:

- **Text-based source format** enables version control, diffing, merging, collaboration, and AI generation (every successful source format except Figma's is text-based)
- **Composability mechanism** is non-negotiable: LaTeX packages, SVG `<defs>/<use>`, Figma components, Docker `FROM`, Terraform modules, code libraries
- **Reproducible builds** with deterministic transformation from spec to output (same WorldSpec + same build config = identical WorldInstance)
- **Incremental updates** reduce rebuild cost: Docker layers rebuild only from the first changed instruction; Terraform skips unchanged resources; Unity's SBP uses cache files
- **Information loss is intentional**: the output strips editability, abstraction, metadata, and debug info in exchange for performance, portability, and self-containment

---

## Multi-modal inputs: text, images, sketches, and assets as world seeds

LocalGPT Gen's input pipeline should support a **text → image → multi-view → 3D mesh** cascade, with each stage accepting direct input. This is the dominant pipeline architecture in 2025, and it allows sketch or image injection at the appropriate stage.

For **text-to-3D objects**, the current best options are Meta 3D Gen (full PBR meshes in under 1 minute, combining AssetGen for geometry and TextureGen for materials), Hunyuan3D 2.0 (open-source, flow-based diffusion transformer with PBR baking), and InstantMesh (multi-view generation via Zero123++ followed by FlexiCubes mesh extraction in ~10 seconds). The original Score Distillation Sampling approach from DreamFusion (using a frozen 2D diffusion model as a 3D optimization loss) is now largely superseded by feed-forward models for speed, though ProlificDreamer's Variational Score Distillation remains state-of-the-art for quality when time permits.

For **scene-level generation** (not just single objects), Meta WorldGen's architecture is the most practical template: LLM planning → procedural blockout → diffusion-based reconstruction → scene decomposition → enhancement. The blockout establishes spatial structure (room shapes, corridors, terrain); the reconstruction pass fills in visual detail; decomposition separates individual objects for editing; enhancement adds high-resolution textures and geometric refinement. This produces **50×50 meter scenes** in ~5 minutes with standard trimesh output and navigation mesh.

**Sketch-to-3D** has matured significantly. Commercial tools like Meshy, Tripo3D, and 3D AI Studio accept sketch uploads and produce textured models in seconds. MIT's VideoCAD (2025) takes a radically different approach: instead of directly predicting 3D, it trains an AI agent to operate CAD software based on sketch input, learning from 41,000+ videos of human designers. For world generation, sketches serve as **structural constraints** — overall composition, spatial layout, proportions — combined with text prompts for semantic detail.

For **asset-conditioned generation** (generating worlds around existing 3D assets), ThemeStation produces stylistically consistent new objects from few exemplars, while RefAny3D uses multi-view renders of existing assets as conditioning signals for generation. Meta's TextureGen can retexture any existing mesh with new text-described materials in ~20 seconds. This enables a workflow where creators provide key hero assets and the system generates complementary environment elements in a matching style.

The recommended pipeline for LocalGPT Gen:
1. LLM generates WorldSpec from text/image/sketch input
2. WorldSpec defines spatial layout, object list, style constraints, and generation parameters
3. Per-object generation dispatches to appropriate models (InstantMesh for speed, AssetGen for PBR quality)
4. Scene assembly places objects per layout with texture harmonization
5. Post-processing generates LODs, collision meshes, and navigation data
6. Export cooks for target platform

---

## Export pipeline: from Bevy scene to self-contained distributable world

The WorldInstance format must target three outputs: native Bevy binary (highest fidelity), self-contained GLB (universal interchange), and web-playable WASM+WebGPU bundle.

**glTF/GLB serves as the universal interchange layer** but is insufficient alone for full game worlds. GLB bundles geometry, textures, materials, animations, and scene hierarchy into a single binary file with excellent tool support across every major engine and web framework. Critical extensions include **KHR_draco_mesh_compression** (95% geometry reduction), **KHR_texture_basisu** (KTX2/Basis Universal GPU textures that stay compressed in VRAM), **EXT_mesh_gpu_instancing** (essential for foliage and repeated architecture), and **KHR_lights_punctual** (directional, point, and spot lights). The glTF-Transform toolkit provides a comprehensive optimization pipeline: prune → dedup → instance → simplify → draco → KTX2 → partition, achieving **80–90% file size reduction**. However, glTF lacks scripting, physics, audio, particle systems, navigation meshes, terrain systems, and streaming metadata. A higher-level format wrapping multiple GLBs is necessary for full worlds.

**WebGPU reached universal browser support in mid-2025** — Chrome since v113 (April 2023), Safari since Safari 26 (June 2025), Firefox since v141 (July 2025). This transforms web 3D: compute shaders, modern low-level GPU access, and the WGSL shading language are now available everywhere. Bevy compiles to WASM targeting `wasm32-unknown-unknown` and supports both WebGL2 and WebGPU backends. The build pipeline is `cargo build --target wasm32-unknown-unknown` → `wasm-bindgen` for JS glue → `wasm-opt` for binary optimization. Bevy WASM binaries are **15–30MB after optimization** — significantly larger than Three.js (<1MB) or PlayCanvas (~170KB), making progressive loading and asset streaming essential. The critical limitation: **WASM is single-threaded**, meaning Bevy's multithreaded ECS executor cannot parallelize on web.

For self-contained world bundles, the recommended structure separates manifest, geometry, textures, and custom data:

| Data Type | Format | Compression | Typical Savings |
|-----------|--------|-------------|-----------------|
| Geometry | GLB | Draco (quantization 11–14 bits) | ~95% for meshes >1MB |
| Color textures | KTX2 | ETC1S (Basis Universal) | Smaller than JPEG, GPU-compressed |
| Normal/roughness | KTX2 | UASTC (Basis Universal) | Higher quality for data textures |
| Lightmaps | KTX2 | UASTC + Zstd supercompression | Preserves lighting detail |
| Audio | Opus/Vorbis | Native codec compression | Standard |
| Navigation | Custom binary | Zstd | Compact spatial data |

For large worlds, **Cesium's 3D Tiles format** provides a proven spatial hierarchy architecture: a tileset JSON describes a tree of tiles with bounding volumes and geometric error thresholds, and the runtime streams only visible tiles at appropriate LOD based on screen-space error. This pattern — tile-based streaming with HLOD refinement — has been validated at planetary scale and maps well to generated worlds. Each tile is an independent GLB with metadata, enabling progressive loading: skybox + low-res terrain first (0–2 seconds), visible chunks at LOD2 (2–5 seconds), then progressive quality upgrades in the background.

Platform-specific export profiles should define texture formats, mesh budgets, and compression:

| Target | Textures | Mesh Budget | Initial Bundle |
|--------|----------|-------------|----------------|
| Desktop native | BC7 via KTX2 UASTC | 1M+ tris/frame | No limit |
| Mobile native | ASTC/ETC2 via KTX2 | 200K tris/frame | <500MB |
| Web desktop | Basis → BC7 transcode | 500K tris/frame | <15MB WASM + streamed |
| Web mobile | Basis → ETC2 transcode | 100K tris/frame | <10MB initial |

---

## Bevy's ECS architecture as the foundation for dynamic world construction

Bevy (latest stable: **v0.18**, released late 2025) provides a strong foundation for LocalGPT Gen, with critical capabilities in ECS composition, reflection, and asset management — but significant gaps in scene serialization and web performance.

**Dynamic world construction is natural in Bevy's ECS.** Entities are spawned with arbitrary component bundles via `commands.spawn()`, hierarchies are built with `ChildOf` relationships and the `children!` macro (0.16+), and components can be inserted or removed at runtime. The reflection system (`bevy_reflect`) enables runtime type introspection and dynamic component insertion from serialized data — essential for constructing worlds from specification data rather than compiled Rust types. Function reflection (0.15+) even allows registering generation functions that can be invoked by name at runtime.

**Required Components** (shipped in 0.15) dramatically simplify entity templates for world generation. A component like `Button` declared with `#[require(Node, UiImage)]` automatically spawns all dependent components transitively. For world generation, this means defining `Tree` as requiring `Transform`, `Mesh3d`, `MeshMaterial3d`, and `CollisionShape` — spawning a `Tree` component handles all the plumbing.

The **asset system** uses `AssetServer` with `Handle<T>` references and supports glTF 2.0 as the primary 3D format, PNG/JPEG/KTX2/WebP/HDR textures, OGG/WAV/MP3 audio, and WGSL/GLSL shaders. The asset processor (0.12+) provides a build pipeline with `.meta` files configuring per-asset loading and processing. Hot reloading via filesystem watching enables live iteration on world specifications. The `bevy_asset_loader` crate provides structured loading with `AssetCollection` derive macros and loading states.

**Rendering capabilities** are increasingly competitive. Built on wgpu (Vulkan/DX12/Metal/WebGPU/WebGL2), Bevy provides full PBR materials (metallic-roughness, clearcoat, transmission, anisotropy), cascade shadow maps, environment map lighting, irradiance volumes, lightmaps, clustered forward rendering for many lights, and experimental real-time raytracing via **Bevy Solari** (0.17). GPU-driven rendering (0.16+) offloads visibility and culling to the GPU. DLSS support arrived in 0.17.

**Headless operation** is supported through multiple approaches: `MinimalPlugins` for pure ECS processing, `DefaultPlugins` with rendering disabled for server-side scene processing, or selective feature flags in Cargo.toml. An official `headless_renderer` example renders to offscreen buffers and saves images to disk — suitable for thumbnail and preview generation in a world generation pipeline.

The **critical gap is scene serialization**. The current `.scn.ron` format is verbose, flat, and designed for machine serialization rather than human authoring. BSN (Bevy Scene Notation) will address this with hierarchical nesting, scene inheritance, template system, and both code macro and asset file workflows — but it remains in draft (PR #20158) and is not yet stable. For LocalGPT Gen, this means the WorldSpec format must implement its own serialization and scene construction pipeline rather than relying on Bevy's native scene system, at least until BSN matures.

Key ecosystem crates for the project: **Avian** (avian3d) for ECS-native physics with rigid bodies, colliders, and raycasting; **bevy_hanabi** for GPU particle systems; **oxidized_navigation** for runtime navmesh generation (Recast-based); **bevy_egui** and **bevy_inspector_egui** for editor UIs; and **bevy_asset_loader** for structured asset loading with loading states.

---

## Conclusion: architectural blueprint for LocalGPT Gen

The research converges on a clear architectural direction. LocalGPT Gen should adopt the **LLM-agent procedural approach** (demonstrated by 3D-GPT and SceneCraft) rather than the pixel-prediction approach (Genie) or pure neural generation (World Labs). This means the LLM generates a WorldSpec — human-readable, version-controllable text — that drives a procedural generation pipeline producing concrete 3D assets.

The WorldSpec format should borrow composition from USD (layering and variants, simplified to two arcs), conciseness from A-Frame (HTML-like readability, attribute-based properties), and compactness from Godot (store only non-defaults, parent-path hierarchy). Semantic primitives (`Terrain`, `Building`, `Forest`) rather than raw geometry allow LLMs to describe worlds at natural abstraction levels while a generation engine resolves these into meshes.

The generation pipeline should follow Meta WorldGen's modular architecture: LLM planning → procedural blockout (WFC for interiors, CGA-style grammars for architecture, noise-based terrain) → per-object AI generation (InstantMesh or Hunyuan3D 2.0 for speed/quality) → scene assembly → platform-specific cooking. Full determinism requires centralized seed control at every randomized stage, following Houdini's model.

For export, a 3D Tiles-inspired spatial hierarchy wrapping per-tile GLB files with KTX2 textures and Draco geometry provides streaming for large worlds across all targets. Bevy's WebGPU backend enables web deployment, though the 15–30MB WASM binary and single-threaded limitation demand progressive loading strategies and careful performance budgeting. The dual-artifact contract is simple: **WorldSpec is for creators** (editable, composable, version-controllable), **WorldInstance is for consumers** (self-contained, optimized, no tooling required). The transformation between them must be reproducible and incremental — same spec plus same config yields identical output, and changes rebuild only affected tiles.