# 3D Collaborative World Engine: Architecture Specification

This document outlines the technical architecture for a natural-language-driven 3D world-building engine, scaling from a localized prototype accommodating on-device mobile AI generation to a massive cloud-based persistent world.

---

## 1. Prototype & Session-Based Architecture

Designed for room-scale collaboration, local LAN networking, or single-user mobile sandbox environments. This architecture directly supports the ability to run 3D generation AI models locally on mobile devices, including Android and iPad hardware.

### Networking Topology (Listen Server)

A single desktop application acts as both the authoritative host and a rendering client. The host broadcasts its session locally via Multicast DNS (mDNS), allowing mobile or secondary desktop clients to discover and connect without manual IP configuration.

### State Synchronization

Utilizes a flat broadcast replication model (e.g., via the lightyear Bevy crate). The host maintains the master ECS (Entity Component System) state and blasts all entity mutations (position changes, spawns, despawns) to every connected client over reliable UDP.

### AI Inference Routing

- **Desktop Host**: Embeds an LLM inference engine (llama-cpp-rs or candle) natively into a background thread. It leverages the host's GPU to process chat commands and output structured JSON tool calls.
- **Standalone Mobile**: For offline or single-player usage, mobile apps run quantized Small Language Models (SLMs) directly on-device using native NPUs/GPUs, processing user input natively without cloud dependencies.

### Client Rendering

- **Desktop**: Powered by Bevy's native wgpu rendering context.
- **Mobile**: Operates via a Shared Rust Core wrapped by UniFFI. The native UI (SwiftUI or Jetpack Compose) handles touch inputs, while rendering is offloaded to native cross-platform engines like Filament or Metal, ensuring glTF 2.0 PBR visual parity with the desktop host.

## 2. Massive Scale Architecture (Cloud MMO)

Designed to support thousands of concurrent users, massive persistent geographic maps, and unbounded entity generation without bottlenecking client hardware or network bandwidth.

### Spatial Interest Management (AoI)

The flat broadcast model is replaced by a Spatial Subscription Gateway. The 3D world is partitioned into a spatial grid or Octree. Clients only open network subscriptions to their immediate surrounding chunks, reducing incoming packet ingestion from the total server entity count down to just the local area.

### Asynchronous AI Inference Pool

Synchronous local inference is replaced by an asynchronous distributed queue (e.g., Redis Streams, Kafka).

1. When a user prompts the AI, the client spawns a zero-latency translucent "scaffold" mesh.
2. The prompt enters a queue and is processed by a dedicated fleet of backend GPU worker nodes.
3. The worker outputs the validated mutation, commits it to the database, and the scaffold is replaced by replicated server geometry.

### Distributed State Engine

A single host ECS is replaced by a geo-partitioned headless server mesh or a database engine like SpacetimeDB. Spatial worker nodes manage collision and state for specific regions, dynamically subdividing and transferring authority if player density spikes in a single location.

### Asset Streaming & Memory Management

- **Mesh Baking**: To prevent mobile clients from crashing due to Out-Of-Memory (OOM) errors or draw-call saturation, the server periodically bakes static, AI-generated primitives into combined optimized meshes.
- **Hierarchical LODs (HLOD)**: Distant chunks stream low-poly impostors. High-fidelity glTF assets and KTX2 compressed textures are fetched on-demand from an Edge CDN rather than being pre-bundled into the mobile application binaries.

---

## Implementation Status

Tracked against `localgpt-gen` (`crates/gen/src/net/`, see
[docs/gen/multiplayer.md](../../gen/multiplayer.md)) and the SpacetimeDB module
(`crates/spacetime`).

| Spec item | Status | Where |
|-----------|--------|-------|
| §1 Listen server + mDNS discovery | Done | `net/host.rs`, `net/mdns.rs` (`--host` / `--join`) |
| §1 Flat broadcast replication (lightyear) | Done, superseded by AoI | `net/protocol.rs`, `net/host.rs` |
| §1 Desktop host inference | Via configured providers (incl. local Ollama); no embedded llama.cpp/candle | `main.rs` agent loop |
| §1 Standalone mobile SLM inference | Not started | — |
| §1 Desktop rendering (Bevy wgpu) | Done | `net/client.rs` |
| §1 Mobile rendering (UniFFI + Filament/Metal) | Not started | — |
| §2 Spatial interest management | Done (64-unit chunk windows, per-link visibility) | `net/interest.rs`, `net/host.rs`; `chunk_subscription` in `crates/spacetime` |
| §2 Async inference queue + scaffolds | Done in-process (host queue, replicated scaffolds, client prediction + hand-off); cloud queue in SpacetimeDB (worker registry, claim/heartbeat/complete, stale requeue) | `net/jobs.rs`, `net/host.rs`, `net/client.rs`; `crates/spacetime/src/jobs.rs` |
| §2 Distributed state engine | SpacetimeDB module holds world state + queue; no geo-partitioned authority transfer | `crates/spacetime` |
| §2 Mesh baking | Done client-side (per chunk × material, quiet-period, auto un-bake) | `net/bake.rs`, `net/client_lod.rs` |
| §2 HLOD impostors | Done (per-chunk summary boxes outside the view window) | `net/interest.rs`, `net/client_lod.rs` |
| §2 On-demand asset streaming | Done for custom meshes (content-addressed HTTP + digest-verified disk cache); glTF/KTX2 not yet | `net/assets.rs` |

---

## Related Documents

This is the **umbrella architecture spec** for the collaborative world engine, covering the full scaling path from listen-server prototype to cloud MMO. The documents below are subsystem deep-dives that expand on individual sections:

- [SpacetimeDB Integration Design](../../architecture/spacetimedb-integration-design.md) — deep-dive on §2 Distributed State Engine: database-backed collaborative world state, reducers, and subscriptions
- [Massively Multiplayer Persistent World](massively-multiplayer-persistent-world.md) — deep-dive on §2: schema, spatial ownership, and governance for the MMO tier
- [Massively Multiplayer Co-Creation](massively-multiplayer-co-creation.md) — deep-dive on §2 Spatial Interest Management and §2 Asynchronous AI Inference Pool: spatial sharding, multi-stage AI pipeline, and conflict resolution
- [Multi-Scale 3D Universe](multi-scale-3d-universe.md) — deep-dive on §2: coordinate systems and spatial partitioning at planetary scale
