---
sidebar_position: 9.2
---

# Explorable World Systems

A comparison of AI systems that generate or interact with 3D explorable worlds.

## High-Level Comparison

| System | Input | Output | Runtime | Primary Focus |
|--------|-------|--------|---------|---------------|
| [Genie 3](https://deepmind.google/discover/blog/genie-2-a-large-scale-foundation-world-model/) | Text, Image | Interactive 3D | Cloud | Game world generation |
| [SIMA 2](https://deepmind.google/discover/blog/sima-2-next-gen-game-ai/) | Gameplay video | Agent behavior | Cloud | Game-playing AI agent |
| [WorldGen](https://www.meta.com/blog/worldgen-3d-world-generation-reality-labs-generative-ai-research/) | Text | 3D Worlds (USD) | Cloud | Structured 3D world generation |
| [Marble](https://www.worldlabs.ai/) | Text, Image, Video, 3D | Gaussian Splats, 3D | Cloud | World model for 3D generation |
| [Artcraft](https://artcraft.ai/) | Text | Images, Video | Cloud | Creative IDE for AI media |
| [Intangible](https://www.intangible.ai/) | Text | 3D Scenes | Cloud | Camera-centric scene composition |
| [SceneCraft](https://www.scenecraft.org/) | Text | Interactive narratives | Cloud | AI storytelling for education |
| [Unity AI Beta](https://discussions.unity.com/t/unity-ai-beta-2026-is-here/1703625) | Text | 3D Scenes | Cloud + Local | AI-assisted game development |
| [Roblox AI](https://www.developer-tech.com/news/roblox-brings-ai-generated-game-objects-to-its-developer-tools/) | Text | Game Objects | Cloud | AI-generated game objects |
| [Moonlake Reverie](https://moonlakeai.com/) | Text | Interactive 2D/3D | Cloud | Generative game engine |
| **LocalGPT Gen** | Text | Interactive 3D (glTF) | Local | Open-source world building |

## Feature Comparison

| Feature | Genie 3 | SIMA 2 | WorldGen | Marble | Artcraft | Intangible | SceneCraft | Unity AI | Roblox AI | Moonlake | LocalGPT Gen |
|---------|---------|--------|----------|--------|----------|------------|------------|----------|-----------|----------|--------------|
| Text-to-3D | ✓ | — | ✓ | ✓ | — | ✓ | — | ✓ | ✓ | ✓ | ✓ |
| Image-to-3D | ✓ | — | — | ✓ | — | — | — | — | — | — | — |
| Interactive playback | ✓ | ✓ | — | — | — | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Real-time simulation | ✓ | ✓ | — | — | — | — | — | ✓ | ✓ | ✓ | ✓ |
| Structured generation | — | — | ✓ | — | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| Local execution | — | — | — | — | — | — | — | ✓ | — | — | ✓ |
| Open source | — | — | — | — | — | — | — | — | — | — | ✓ |
| Procedural audio | — | — | — | — | — | — | — | — | — | — | ✓ |
| glTF/USD export | — | — | ✓ | ✓ | — | ✓ | — | ✓ | — | — | ✓ |
| Agent control | — | ✓ | — | — | — | — | — | — | — | ✓ | ✓ |

## System Highlights

### Genie 3 (DeepMind)

Foundation world model that generates interactive 3D environments from a single text prompt or image. Designed for rapid game prototyping and synthetic data generation.

### SIMA 2 (DeepMind)

Gemini-powered agent that learns to play 3D games by watching gameplay video. Self-improving through experience, it reasons about game objectives and adapts to new environments.

### WorldGen (Meta Reality Labs)

Structured 3D world generation from Meta Research. Uses a multi-stage pipeline — LLM generates high-level layout parameters, then procedural systems handle actual geometry placement. Outputs USD scenes with terrain, structures, vegetation, and props. Key insight: LLMs should generate *parameters*, not geometry directly. LocalGPT Gen's [WorldGen pipeline](/docs/gen/tools) implements a similar blockout-first architecture locally.

### Marble (World Labs)

Multimodal world model that creates 3D scenes from text, images, video, or 3D layouts. Exports as Gaussian splats for high-fidelity rendering.

### Artcraft

IDE for AI-assisted creative work. Combines image generation, video creation, 3D compositing, character posing, and scene blocking in a unified interface.

### Intangible

Spatial intelligence platform focused on camera-centric 3D composition. Designed for creative industries needing precise camera control and scene layout.

### SceneCraft (EngageAI Institute)

AI-powered storytelling platform that generates interactive, narrative-based learning experiences from natural language prompts. Developed by the [EngageAI Institute](https://engageai.org/) (NSF AI Institute for Engaged Learning, award DRL-2112635) across NC State, UNC, Indiana University, Vanderbilt, and Digital Promise. Teachers input prompts describing desired story foundations; SceneCraft generates scenes, characters, and dialogue that educators can fully customize to align with instructional goals.

- [scenecraft.org](https://www.scenecraft.org/)
- [Authoring tool](https://scenecraft.csc.ncsu.edu/scenecraft/home)
- [EngageAI publications](https://engageai.org/publications)

### Unity AI Beta

Unity's 2026 AI Beta integrates AI-powered tools directly into the Unity Editor for generating and modifying game objects, scenes, and assets from natural language prompts. Combines cloud AI services with local editor execution.

- [Unity AI Beta 2026 announcement](https://discussions.unity.com/t/unity-ai-beta-2026-is-here/1703625)

### Roblox AI

Roblox brings AI-generated game objects to its developer tools, enabling creators to generate 3D objects, textures, and game assets from text descriptions directly within Roblox Studio.

- [Roblox AI-generated game objects](https://www.developer-tech.com/news/roblox-brings-ai-generated-game-objects-to-its-developer-tools/)

### Moonlake Reverie

Generative Game Engine (GGE) from Moonlake AI that transforms text descriptions into playable 2D and 3D interactive worlds. Founded by Fan-Yun Sun and Sharon Lee (Stanford AI Lab), backed by $28M seed from AIX Ventures, Threshold, and NVIDIA Ventures. Combines multimodal reasoning with program synthesis and simulation layers — spatial layout, physics, and agent behaviors are generated structurally, then a real-time diffusion model conditioned on 3D signals provides visual reskinning. Unlike video-only generation, Reverie maintains world state across interactions, enabling consistent interactive sessions.

- [moonlakeai.com](https://moonlakeai.com/)
- [Moonlake blog](https://moonlakeai.com/blog)
- [Latent Space podcast interview](https://www.latent.space/p/moonlake)

### LocalGPT Gen

Open-source, local-first 3D world generation powered by Bevy. Features procedural audio synthesis, data-driven behaviors, and full glTF export. Runs entirely on your machine without cloud dependencies.

**Showcases:**
- [localgpt-gen-workspace](https://github.com/localgpt-app/localgpt-gen-workspace) — "World as skill" examples: complete explorable worlds saved as reusable, shareable skills
- [proofof.video](https://proofof.video/) — Video gallery comparing world generations across different models using the same or similar prompts

See the [Gen documentation](/docs/gen) for details on LocalGPT's world generation capabilities.
