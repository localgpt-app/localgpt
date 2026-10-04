# Not Yet Specced (and Why)

Items identified in the AI world generation research (no longer in this repository) that were deliberately not turned into TODO specs.

---

## Text-to-Skeleton Animation (HY-Motion)

HY-Motion 1.0 (Tencent, December 2025) is the first billion-parameter text-to-motion model producing standard skeleton animations, with a Lite variant for reduced VRAM. The PARC framework (SIGGRAPH 2025) and PDP (SIGGRAPH Asia 2024) offer complementary physics-based character animation approaches.

**Why not specced:** Bevy's skeleton animation support is still maturing. The current gen mode has no humanoid rigs — all entities are parametric primitives or imported meshes without skeletal structure. Speccing text-to-motion integration before P1 avatar system ships and skeleton infrastructure exists would be premature. Revisit after P1 is implemented and Bevy's animation system stabilizes.

## Procedural World Streaming

The research describes procedural world expansion where new areas generate seamlessly as players explore, combining TRELLISWorld's tile-based approach with navmesh conditioning from WorldGen.

**Why not specced:** This is Phase 4 of the 12-month research roadmap (months 9–12). It depends on WG1 (procedural blockout), WG2 (navmesh), WG3 (hierarchical placement), and P1 (avatar for exploration). None of these prerequisites have shipped yet. Speccing infinite-world streaming before single-scene generation is robust would produce a spec disconnected from reality. Revisit after WG1–WG3 are implemented.

## Bevy MCP Server Extraction

The research notes that MCP servers exist for Unity (35+ tools), Unreal, Godot, and Blender — but no MCP server for Bevy exists. LocalGPT Gen's MCP server could be extracted into a standalone community crate, representing a first-mover opportunity in the Bevy ecosystem.

**Why not specced:** This is a strategic/organizational decision, not a feature. The current MCP server (`mcp_server.rs`) is tightly coupled to LocalGPT Gen's command protocol and entity registry. Extracting it into a general-purpose Bevy MCP crate requires design decisions about what "generic Bevy MCP" means (ECS entity CRUD? Scene management? Asset loading?) that go beyond a TODO spec. Better suited as an RFC or project decision after the current MCP surface area stabilizes.
