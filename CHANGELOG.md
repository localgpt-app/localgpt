# Changelog

All notable changes to LocalGPT are documented in this file.

## [Unreleased]

### Added

- **A person edits the live canvas (`crates/gen/src/gen3d/canvas_edit.rs`, `crates/gen/src/inspector/`)** — the inspector's detail panel edits every field of the selected entity (name, transform, shape, material, light, behaviors and their targets, audio, mesh, parent), the outliner reparents by drag and offers duplicate, delete, unparent, group and focus from its context menu, and the viewport has handles: three arrows to move along an axis, three rings to turn about one (red X, green Y, blue Z), a cube for uniform scale, with grid and angle snapping (`Y` toggles, Cmd/Ctrl held inverts), key nudges and turns, Shift- or Cmd/Ctrl-click multi-selection, Cmd+G grouping, Cmd+D duplication and Delete. Every gesture is one batch through the same authority the agents use, so it lands in the history as one entry, is undone with Cmd+Z, and is rolled back when the authority refuses it. `--review` shows an agent's batch on the canvas before it commits (Enter accepts, Esc rejects, or the bar's buttons), a batch may carry an `expected_revision` and gets a receipt back, and the bottom bar keeps camera bookmarks as a `bookmarks` tour in the document. The fly camera's descend key moved from Shift to Q, freeing Shift for selection. `L` switches the handles to the entity's own axes, an Alt-drag of an arrow scales along that axis alone, a drag from empty space (or a Shift-drag from anywhere) box-selects what it encloses, and Alt + right-drag orbits the camera around the selection. Undo is per author: Cmd+Z takes back the person's own newest change (Cmd+Alt+Z anyone's), Cmd+Shift+Z redoes it, and the API gains `POST /redo` and an `own` flag on `/undo`; an own undo that a later batch built on is refused naming that entry. A direct write to `manifest.json` is put back as before and now noted in `.live/refusals.jsonl` (`GET /refusals`) and on the bottom bar. A `git checkout` under the open app is followed within a guard tick, a replay loads each commit's own assets, and committed audio, mesh-asset, modulation and camera ops reach the open scene. `scripts/live-smoke.py` drives all of it with a synthesized hand (`LOCALGPT_SYNTHETIC_INPUT=1`, `POST /input`), on a `--git` world for the checkout and replay checks. Rides `openworldformat` 0.3.6, whose validation warns about a behavior whose target is gone, so the canvas and the inspectors show it.
- **Screenplay PDFs draft (`crates/previs/src/pdf.rs`)** — the file that gets sent around reads on localgpt.md and in `localgpt-previs`: the PDF's text layer (`pdf-extract`, pure Rust, WASM too) is read back into elements by where each line sits on the page — the columns learned per document, since every app sets them a few points apart — with scene numbers from the margins, page furniture dropped, and a one-column print of a Fountain file handed to the Fountain parser as it is. The page's lines are the script's lines, so the page-time estimate reads a PDF the way a page is read. A scan is refused as a picture of a script, a `.pdf` that isn't one in its own words, and the page caps PDFs at 300 pages. Tested against a corpus of one script through five generators (`crates/previs/samples/pdf/`): the page's PDF sample drafts to the same board as the Fountain, Final Draft and Markdown samples, byte for byte. The page now reads every file as bytes (UTF-16 by its byte-order mark too) and reloads its WASM after a trap, since a PDF beyond the reader can take the module with it.
- **Final Draft files draft (`crates/previs/src/fdx.rs`)** — a `.fdx` is read as Final Draft saved it: the typed paragraphs (scene headings with their scene numbers, characters, parentheticals, dialogue and dual dialogue, transitions, action; acts as sections, script notes as notes) map to Fountain and draft through the same parser, so the file a director actually has goes onto the localgpt.md page and into `localgpt-previs` without an export step — and XML at the head is Final Draft whatever the file is called, while XML that isn't is refused in its own words. Action is re-wrapped at a screenplay page's 61-character column before timing, so the same screenplay as `.fdx`, `.fountain` and `.md` drafts to the same board, byte for byte (the three samples, checked in the crate and on the page). The Fountain parser gains the `!` (forced action) and `@` (forced cue) escapes the mapping leans on.
- **localgpt.md is the previs page (`website-md/`)** — drop a screenplay, get the board in the tab: `crates/previs-web` is `localgpt_previs::draft` as WASM (the same function the CLI writes to disk, factored out of `main.rs`), so a director with script pages gets a set, stand-ins and coverage per scene, the shot list and the printable board with nothing to install and nothing uploaded; the page shows the board and offers it, the shot list and a zip of the `.world` packages as downloads. It reads Markdown script pages as well as Fountain (`crates/previs/src/markdown.rs`: a heading that reads `INT.`/`EXT.` is the scene, `**MAYA:**` opens dialogue, front matter is the title page, Fountain inside passes through), and the same screenplay both ways drafts to the same board, byte for byte; `localgpt-previs` takes `.md` too. Per `docs/world-strategy.md` §15 it replaces the drop-a-Markdown-file-walk-it page at that domain: that page and `crates/md-web` are removed, and walking a document stays in the desktop app. Checked headlessly in the `viewer` CI job.
- **Browser guests for Gen sessions (`--web`)** — a hosting Gen serves a join page and a WebSocket endpoint on the session port and prints an invite link; anyone on the network opens it in a browser (nothing to install), sees the same world live, walks with a labeled avatar, chats, and prompts the room's AI through the same job queue native clients use. Every change to the world — tool calls, inspector edits, undo/redo, world loads — streams to guests as world-types `EditOp`s, because the host projects its scene into the shared document and diffs it ~4 times a second.
- **`localgpt-world-sync`** — the collaboration core with no Bevy and no I/O: the shared world document (atomic, validated `EditOp` application with subtree deletes), the scene→ops diff, the versioned JSON wire protocol, and the room authority (peers, roles, revisions, presence, chat, prompt queue). `crates/world-sync`.
- **`EntityPatch.modulations`** — entity modulation stacks can now be patched like every other field, so live sync and diffs cover them.

### Fixed

- **An orbit or look-at bound by id circled the origin or did nothing** (`crates/gen/src/gen3d/behaviors.rs`) — the behavior tick knew its targets by name only, while every committed reference converts to an id key (the format's log carries ids), and entities with no behaviors of their own were not in its map at all; so in every world loaded from disk an orbit around an entity circled the world origin and a look-at never turned. Every entity is now registered under its name and its id. Found by the second cold agent run, whose moon circled the origin instead of the lantern.
- **Live API: a behavior target sent by name committed as the name** (`crates/world-agent`) — the authority's intake bound only an op's `id` and `parent`, so `{"LookAt": {"target": "bench"}}` was logged with the string, the fold bound it, and `GET /verify` reported the entity as differing from its history. Every marked reference (behavior targets and centers, a camera's `aim_entity` and `focus_entity`, the avatar's model) now binds at intake through the format crate's one list, and a name nothing has refuses the batch, naming it. Found by a cold run of a fresh agent on `AGENTS.md` alone, which also drove a dozen additions to that guide, `GET /world?tip=current`, `goto {"tip": "head"}`, `/world` printing f32 fields as `manifest.json` does, and a check the committer runs before every write: the entry as it will be logged must fold back to the world it made, or the batch is refused naming the entity.
- **A package drafted elsewhere opens live with its guide** — `LiveWorld::open` now furnishes any package it opens with `AGENTS.md` and `assets/` when they are missing, not only the ones it made itself: a previs draft (four files from `localgpt-previs` or the localgpt.md zip) opened with `localgpt-gen --live` printed "see AGENTS.md" and had none for the agent to read.

### Changed

- **`openworldformat` 0.3.5, viewer 0.3.4 — cameras track actors by entity** — `ext-cinematography` 0.3's `aim_entity` and `focus_entity` arrive: the drafted singles aim and focus at their speaker's head by entity, so "move Maya a step left" keeps 1B on her; the shot list, the board, Gen's look-through and thumbnails resolve a shot's view in the world (`shot_camera_in`, origins through parents); and the agent prompt knows the two fields ("stay on her").
- **`openworldformat` 0.3.4, viewer 0.3.3** — the workspace pins the crate at 0.3.4 and `world-types`' copy of the `ext-cinematography` math is gone: `wt::cinematography` is the crate's module (cameras, frames, the shot list, the conformance outcome runner) plus the writer-side helpers LocalGPT needs (`default_camera`, `camera_json`, `view_from`). The vendored viewer is openworldformat 0.3.3, which looks through a shot (`opts.shot`, letterboxed to its frame) and imports `./cinematography.js` for the math — so `scripts/sync-viewer.sh` now vendors both files, the session servers serve `/cinematography.js` next to `/world-viewer.js`, exported pages and the previs board map the relative import to an inline `data:` URL, and both websites assemble both files.
- **Previs boards read at a glance** — each stand-in has a pale face on the front of its head, so eyelines read; a single is a proper over: the camera 0.8 m behind the partner and 0.6 m to the side, their head on the near edge and the speaker's head on the far third; interiors are lit flat and bright.
- **The web renderer moves upstream** — `js/world-viewer.js` is now a verbatim vendored snapshot of the [`openworldformat`](https://www.npmjs.com/package/openworldformat) npm package (0.2.0), synced by `scripts/sync-viewer.sh` from the registry tarball and drift-gated in the `viewer` CI job. Renderer logic changes ship in the openworldformat repository, release on npm, then flow back — this repo stops being the place the renderer is edited. The 0.2.0 sync brings: `mesh_asset.fallback` placeholders that render the declared shape with the entity's material while the mesh loads (and keep it on failure), a punctual-light budget (16, least-priority hidden, ties in document order), and null guards on fog density and audio bins. Embedded HTML grows ~11 kB, almost entirely JSDoc.
- **The collaborative spec is v2** — `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` now describes the op-based document model (sync the world format, one authority per room, WebSocket baseline, browser guests) with phases through relay, persistence and guest editing, replacing the ECS-replication-first plan.

- **Cloud rooms in the SpacetimeDB module** — `submit_ops` and `undo` reducers apply committed ops to the `world_entity` table through the same `localgpt-world-sync` document and validation as a LAN host, with an op log (`world_op` table) and per-author undo stacks. Native-tested and wasm-verified; client SDK wiring is next.
- **Internet invite links (`--relay`)** — `localgpt-relay` (new small binary) forwards WebSocket frames between a NAT'd host and its guests by room code without ever holding world state; `--host --relay <url>` prints a public invite link, and relay and LAN guests share the room.
- **Guest editing (`--web-edit`)** — browser guests can join as editors: click to select, drag to move, `Q`/`E` rotate, `+`/`-` scale, `Delete` remove — validated by the room authority, visible in the host's window and to every guest, and undoable per author.
- **Session history, per-user undo and time-lapse replay** — every web session appends committed batches to `<workspace>/sessions/<name>/ops.jsonl`. Guests undo their own builds (including ones the AI made for them — prompt builds are attributed to the asker) with `/undo` in chat; `--host --web --resume <session>` restores a room from its log; `localgpt-gen --replay <ops.jsonl>` rebuilds a world batch by batch in the window. Undo, guest edits and resume rebuilds apply to the host's live scene through a new ops→scene applier (`gen3d/ops_apply.rs`), so the projection no longer "heals" them back.
- **Fully offline web sessions** — the join page's three.js (r170) is vendored and served by the host (`/vendor/…`), so a guest needs nothing beyond the link: no CDN, no internet. Verified with all external requests blocked in headless Chromium.
- **One world format for Gen, MD and Verse** — `localgpt-world-types` gains `ModulationDef` (an entity parameter bound to a runtime signal: soundtrack energy, beat, bass/highs, a stem, an oscillator or a constant) and `SoundtrackDef` (the song a world performs to, as analysis curves with an optional audio file), `validate_manifest`, a `schema` feature that writes `world.schema.json`, and reference worlds under `crates/world-types/conformance/` covering every shape, material feature, light, behavior, tour and modulation.
- **`localgpt-world-bevy`** — the one Bevy mapping of the format (meshes, materials, lights, environment, camera), used by Gen and by LocalGPT MD. Every parametric shape is centered and fits `Shape::local_aabb_half`, checked by tests.
- **`localgpt-world-export`** — the web side of the format with no Bevy dependency: `js/world-viewer.js`, a data-driven three.js renderer of manifest JSON (behaviors, procedural audio, tours, modulations, soundtrack playback, glTF mesh assets, an embed API), plus `generate_html` (Gen's `gen_export_html`, MD's `--export x.html`) and `to_json`. localgpt.world serves the same viewer.

### Changed

- **Gen renders through the shared mapping** — `gen_export_html` now emits the shared viewer with the manifest as JSON instead of generated per-entity JavaScript; pyramids are centered like every other shape (their base was at the origin), the torus no longer swaps its radii, and the wedge is a ramp (a right-triangle profile extruded along Z) in every renderer. Colour fields are documented as they were always read: `color`, light and environment colours are sRGB-encoded, `emissive` is linear.
- **Policy file renamed `LocalGPT.md` → `POLICY.md`** — the workspace security-policy / standing-instructions file is now `POLICY.md`, avoiding the name collision with the LocalGPT MD app (md.localgpt.app). Existing `LocalGPT.md` files keep loading when no `POLICY.md` exists and stay on the agent write-deny list; new workspaces get a `POLICY.md` template. The CLI subcommand is now `localgpt policy` (alias `md`), and the mobile FFI exposes `get_policy`/`set_policy` (with legacy-name fallback).
- **Gen upgraded to Bevy 0.19** — `bevy` 0.18→0.19, `bevy_egui` 0.39→0.40 (egui 0.34), and `avian3d` 0.6→0.7. The `physics` feature now provides avian3d rigid-body physics only; the bevy-tnua floating character controller moved to a new off-by-default `character-controller` feature, pending a Bevy 0.19-compatible `bevy-tnua` release (the player falls back to a transform-based controller until then).
- **Workspace dependency upgrades** — major-version bumps across the workspace: `rusqlite` 0.38→0.40, `ron` 0.9→0.12, `jsonwebtoken` 9→10 (now on the `rust_crypto` backend), `hmac` 0.12→0.13 with `sha2` 0.10→0.11, `tiktoken-rs` 0.9→0.12, `croner` 2→3, `cpal` 0.17→0.18, `rustyline` 17→18, `splines` 4.4→5.0, `eframe` 0.33→0.34, `interprocess` 1→2, and `tokio-tungstenite` 0.26→0.29, plus all semver-compatible updates via `cargo update`. The bridge IPC layer migrated to the `interprocess` 2.x `Name`/`ListenerOptions` API with built-in cross-platform peer-credential lookup.

## [0.3.6] - 2026-04-12

### Added

- **Memory Wiki** — structured knowledge management with claims, evidence chains, and staleness tracking.
- **Active memory recall** — agent automatically recalls relevant memories before replying.
- **Dreaming memory consolidation** — background process consolidates and deduplicates memories, wired into heartbeat runner.
- **Compaction checkpoints** — session restore points created during context compaction.
- **Model cost tracking** — per-session token usage and cost tracking.
- **Elevated permission mode** — approval gate for dangerous tools with `StreamEvent::ApprovalRequired`, HTTP approve endpoint, and loopback auto-approval.
- **TLS for HTTP server** — auto-certificate generation, `cert` CLI commands, and `--no-tls` flag.
- **Durable outbound message queue** — SQLite-backed outbox with retry, wired into Telegram bridge send path.
- **Claude CLI effort configuration** — configurable effort level (`low`/`medium`/`high`/`max`) with `/effort` slash command for runtime switching.
- **Gen MCP multimodal input (AI3)** — image-guided world generation tools.
- **Gen MCP HTTP transport** — compliance metadata, starter templates, and AGENTS.md.
- **Gen MCP tool annotations** — Dockerfile, Smithery config, and privacy policy for distribution.
- **MCP conformance tests** — test suite, case study docs, and composable agent guide.
- **Heartbeat gen experiment dispatch (H2.5)** — localgpt-gen subprocess for automated scene generation.
- **SSRF protection** — server-side request forgery guard, stricter CORS, NPC brain loop, gallery thumbnails.
- **Per-job MCP server filtering** — cron jobs can specify which MCP servers to use.
- **Telegram heartbeat alerts** — heartbeat alerts routed to Telegram forum topics.
- **Gen conditional MCP relay** — relay only starts when using CLI backends.

### Fixed

- **Config show** — resolved paths displayed correctly, secrets redacted in output.
- **Clippy and cargo-deny** — warnings and stale advisory ignores resolved.

## [0.3.5] - 2026-03-22

### Added

- **WorldGen pipeline (WG1–WG7)** — procedural world-building: blockout layout planning (`gen_plan_layout`, `gen_apply_blockout`), navmesh with A* pathfinding, hierarchical entity placement (hero/medium/decorative), screenshot evaluation loop, blockout editing with incremental regeneration, scene decomposition with semantic roles, depth map rendering, and styled 2D preview generation.
- **Gen gameplay runtime (P1–P5)** — NPC dialogue UI panel, teleport fade and collectible pickup effects, physics constraints, interaction triggers, and terrain runtime systems.
- **Gen AI infrastructure** — AI1 local 3D asset generation scaffolding, AI2 AI NPC intelligence with dialogue and behaviors.
- **Gen world inspector** — egui overlay panel with 3D viewport picking, WebSocket protocol for remote inspection, scene detection, transform streaming, entity focus. Native clients: SwiftUI (iOS/macOS), Compose (Android), GLB scene snapshot export.
- **Gen headless mode** — headless generation pipeline with experiment queue for CI/batch workflows. CLI relay routes MCP tool calls to an existing Bevy window.
- **Gen world forking** — `gen_fork_world` tool for world remix with attribution tracking.
- **Gen noclip mode** — N key toggles noclip, V key toggles first/third-person POV.
- **Gen physics** — avian3d 0.6 / bevy-tnua 0.30 physics integration with new physics tools.
- **Gen gameplay triggers** — PlaySoundAction in triggers, key collection in collectibles.
- **Gen multi-file worlds** — multi-file world generation with drift detection.
- **Gen mesh segmentation** — optional mesh segmentation on glTF loading (WG6.3).
- **Plugin system** — plugin CLI with `localgpt plugin enable/disable/list`, daemon-level plugin filtering.
- **Pluggable memory backends** — `sqlite`, `markdown`, and `none` backends selectable via config.
- **Post-compaction audit logging** — Merkle hash chain for tamper-evident audit trail after session compaction.
- **Encryption at rest** — encrypted sessions and config secrets, `localgpt encrypt` CLI commands.
- **Slack bridge** — Socket Mode integration for Slack workspaces.
- **Telegram forum topics** — support for Telegram forum/topic-based conversations.
- **Telegram reactions** — acknowledgment reactions on incoming user messages.
- **Session branching** — fork conversation sessions into parallel branches.
- **Self-repair and stuck recovery** — agent detects and recovers from stuck states automatically.
- **LLM-based query expansion** — memory search queries expanded via LLM for better recall.
- **Memory citation format** — search results include source citations.
- **Multimodal embeddings** — Gemini embedding provider with multimodal support.
- **Gemini embedding provider** — `gemini` as a new embedding backend.
- **Session transcript indexing** — past session content indexed for memory search.
- **Browser automation tool** — Chrome DevTools Protocol integration for web interaction.
- **Docker/Podman sandbox** — container-based sandbox backend alongside Seatbelt/Landlock.
- **Web UI settings panel** — config editing via `POST /api/config` and web UI.
- **Channels status API** — `/api/channels/status` endpoint and web UI panel.
- **`localgpt tool` CLI** — MCP server management from the command line.
- **Webhook signature verification** — HMAC-SHA256 verification for incoming webhooks.
- **Media processing cache** — file-hash-based cache for document/audio processing.
- **Image optimization** — automatic image resizing/optimization for vision models.
- **OpenRouter alias** — `openrouter/*` provider prefix for convenience.
- **Streaming abort preservation** — partial output preserved when streaming is aborted.
- **Post-compaction context injection** — AGENTS.md content injected after session compaction.
- **Document and audio tools** — `document_load` and `transcribe_audio` wired as agent tools.
- **Gemini CLI streaming** — gemini-cli tool extraction support in streaming UI.

### Changed

- **Legacy avatar system removed** — unified to Player + FreeFly camera modes.
- **gen_package_world removed** — skill directories are now the sole distribution unit.

### Fixed

- **Gen physics** — Tnua `initiate_action_feeding` called every frame (not just on jump), physics engine plugins registered correctly, SpatialQuery made optional in camera follow system.
- **Gen display** — player mesh hidden in first-person mode, all keyboard controls shown in startup help, 4 P0 creator workflow blockers resolved.
- **Gen stability** — wgpu surface crash on Wayland avoided, audio buffer underruns fixed, MCP relay and GPU lock hardened, CLI backend MCP tool calls routed to existing Bevy window.
- **Windows** — HANDLE type casting and BOOL API mismatch fixed for windows-sys 0.59, all compiler warnings resolved.
- **CLI** — short flags removed from `tool add` to avoid clap conflict.

## [0.3.4] - 2026-03-15

### Added

- **3D World Templates** — website section for browsable world templates.

### Fixed

- **Gen ECS** — resolved Bevy ECS query conflict B0001 in interaction systems.

## [0.3.3] - 2026-03-14

### Added

- **Gen P1–P5 MCP tool specs** — NPC dialogue, interaction triggers, terrain, collectibles, portals, and physics constraint tools.
- **Gen click triggers, portals, and sky lighting** — click-based interaction triggers, portal visuals, and sky lighting system.
- **Gen 3D UI enhancements** — improved interaction effects with Apple SceneKit animation fixes.
- **iPad/RealityKit** — RealityKit world generation with MCP tools on iPad, with macOS conditional compilation.
- **Strict CORS** — enforced strict CORS origins for the HTTP server.
- **Session management enhancements** — previews and history playback in the CLI.

### Changed

- **Physics made optional** — avian3d physics behind a feature gate for lighter builds.
- **Interaction params** — replaced string-typed interaction parameters with enums and added terrain features.

### Fixed

- **Gen NPC idle** — avoided query conflict in npc idle system.
- **Core clippy** — addressed clippy warnings across codebase.
- **iPad** — Swift warnings for RealityKit resolved, macOS conditional compilation added.

## [0.3.2] - 2026-03-08

### Added

- **Gen MCP server** — MCP server mode for external CLI backends with core tools, memory-only mode with write tools, and Ctrl+C handling.
- **Gen new primitives** — `pyramid`, `tetrahedron`, `icosahedron`, and `wedge` parametric shapes.
- **Gen HTML export** — export worlds as interactive browser-based experiences with Three.js viewer.
- **Gen avatar movement and camera control** — WASD/arrow key movement, mouse look, first-person/third-person camera modes.
- **CLI TUI slash commands and line editing** — `/help`, `/clear`, `/exit`, `/model`, `/session` commands with readline-style editing (Ctrl+A/E, arrow keys, history).
- **Gen streaming chat** — real-time tool call display and streaming responses in Gen mode for better visibility into scene building.
- **Gen batch entity operations** — `gen_spawn_entities` tool for efficient multi-entity creation in a single call.
- **Gen on-demand export** — `gen_export_world` tool with mesh asset localization for portable scene exports.
- **Gen human-readable filenames** — export filenames now use `YYYY-MM-DD_HH-MM-SS` format instead of timestamps.

### Changed

- **Gen mode architecture** — delegated to `localgpt-gen` binary via subprocess for cleaner separation.
- **Gen glTF export format** — switched to JSON format with version compatibility for better interoperability.

### Fixed

- **Core provider error messages** — API errors now show actual provider name instead of generic label for easier debugging.
- **Agent streaming** — correct tool message ordering in streaming path. ([#88](https://github.com/localgpt-app/localgpt/pull/88))
- **Gen compiler warnings** — silenced warnings in plugin.rs.

### Contributors

Thanks to **[@itripn](https://github.com/itripn)** (Ron Forrester) for fixing tool message ordering in the streaming path!

## [0.3.1] - 2026-03-03

### Added

- **Gen undo/redo system** — full undo/redo support with stable entity IDs, LLM tools (`gen_undo`, `gen_redo`), and persistence via `history.jsonl`. Covers entities, lights, behaviors, camera, and environment.
- **Gen audio undo/redo** — audio emitter commands now support full undo/redo with `gen_audio_emitter` and `gen_modify_audio` operations reversible.
- **Gen behavior system** — declarative entity animations: `orbit`, `spin`, `bob`, `look_at`, `pulse`, `path_follow`, `bounce`. Behaviors stack and persist through save/load.
- **Gen world save/load** — complete worlds serialized as skills (`SKILL.md` + `world.ron`). Includes `gen_save_world`, `gen_load_world`, and `gen_clear_scene` tools.
- **Gen avatar and tours** — avatar and tours sections in `world.ron` for user presence and guided waypoint sequences with descriptions and movement modes.
- **Gen parametric shapes** — unified world data model with `shape` field on entities. Supported: `box`, `sphere`, `cylinder`, `capsule`, `plane`, `torus`.
- **Gen material properties** — full PBR material support: `alpha_mode`, `unlit`, `double_sided`, `reflectance`, `emissive`. Exposed in spawn and modify tools.
- **Gen light properties** — `range`, `outer_angle`, `inner_angle` for spot lights; `direction` for directional/spot lights. All persisted and exposed in `entity_info`.
- **Gen entity info enrichment** — `entity_info` now includes shape type, emissive, light data (direction, range, angles), audio emitter type, and behavior info.
- **Gen glTF tracking** — source path tracked through save/load cycle for asset provenance.
- **Document loaders** — new module for loading documents (PDF, DOCX, etc.) for RAG workflows.
- **Audio transcription** — new module for transcribing audio files via Whisper-compatible APIs.
- **TTS module** — text-to-speech integration for voice output.
- **MMR re-ranking** — Maximal Marginal Relevance re-ranking for diverse memory search results.
- **CLI completion** — shell completion generation for bash, zsh, fish via `localgpt completion`.
- **CLI cron command** — manage cron jobs from CLI (`localgpt cron list/add/remove`).
- **CLI hooks command** — manage lifecycle hooks from CLI.
- **SpacetimeDB multiplayer** — web app for multiplayer 3D world collaboration.
- **Mobile workspace editor** — iOS/Android UI for editing workspace files with security hardening.

### Changed

- **Mobile apps restructured** — `apps/ios` renamed to `apps/apple` for multi-platform support (iOS + macOS).
- **OAuth providers removed** — all OAuth provider implementations removed for simplified authentication.

### Fixed

- **Gen visibility persistence** — visibility state now persists correctly through save/load and undo snapshots.
- **Gen camera FOV** — saves actual camera FOV instead of hardcoded 45 degrees.
- **Gen light saving** — light components save on any entity type, not just `GenEntityType::Light`.
- **Gen loop detection** — raised threshold and fixed command channel race condition.
- **Gen world load** — undo stack cleared when loading world without saved history.
- **Codex CLI provider** — updated for new CLI interface.

## [0.3.0] - 2026-02-27

A major release bringing the agent platform to production readiness with MCP tool integration, OpenAI-compatible API, cron scheduling, security hardening, mobile apps, and multi-agent orchestration.

### Added

- **MCP client support** — connect to external MCP tool servers via stdio or HTTP/SSE transports. Tools are auto-discovered and namespaced as `mcp_{server}_{tool}`. Configure in `[mcp]` config section.
- **OpenAI-compatible HTTP API** — `/v1/chat/completions` (streaming + non-streaming) and `/v1/models` endpoints. Enables integration with Cursor, Continue, Open WebUI, and the Python `openai` library.
- **Cron job scheduling** — run prompts on cron expressions (`0 */6 * * *`) or simple intervals (`every 30m`). Each job gets a fresh agent session with overlap prevention and configurable timeout.
- **Per-IP rate limiting** — token bucket rate limiter on all API routes. Configurable `requests_per_minute` and `burst` in `[server.rate_limit]`.
- **Oversized payload guard** — `RequestBodyLimitLayer` prevents OOM from large POST bodies (default: 10MB, configurable via `server.max_request_body`).
- **Configuration hot-reload** — daemon watches `config.toml` for changes and propagates updates to running services without restart. Also responds to SIGHUP on Unix.
- **Session pruning** — auto-cleanup of old session files at daemon startup and hourly. Configurable `session_max_age` (default: 30 days) and `session_max_count` (default: 500).
- **`localgpt doctor` command** — diagnostics that validate config, check provider reachability, test memory database, verify MCP connections, and report disk space. Supports `--fix` and `--json` flags.
- **Multi-agent orchestration** with `spawn_agent` tool for hierarchical delegation to specialist subagents.
- **OpenAI-compatible provider** for third-party APIs (OpenRouter, DeepSeek, Groq, vLLM, LiteLLM).
- **Multi-provider failover** with automatic retry across configured fallback models.
- **Lifecycle hook system** — `beforeToolCall`, `onMessage`, `onSessionStart` extensibility points.
- **Stuck loop detection** — prevents infinite tool-call loops by detecting repeated identical calls (configurable `max_tool_repeats`).
- **Bearer token authentication** for all HTTP API routes via `server.auth_token`.
- **Session file permissions** hardened to `0o600`.
- **Codex CLI provider** integration (`codex/*` models).
- **Apple Foundation Models** integration for on-device AI on iOS.
- **iOS app** with SwiftUI, MVVM architecture, and UniFFI bindings.
- **Android app** initial project structure.
- **Profile isolation** via `-p/--profile` CLI option for complete XDG path separation.
- **Hybrid web search** with configurable providers (`searxng`, `brave`, `tavily`, `perplexity`) and native-search passthrough.
- **xAI provider support** (`xai/*`, `grok-*`) with native `web_search` tool passthrough.
- **Vertex AI provider** — access Claude and Gemini models via Google Cloud with service account authentication (`vertex/*` models).
- **Gemini API key provider** — simple `GEMINI_API_KEY` authentication without OAuth (`gemini/*` models).
- **Bridge health monitoring** — automatic health status tracking (healthy/degraded/unhealthy) for all connected bridges.
- **CLI bridge** (`localgpt-bridge-cli`) — interactive terminal that connects to a running daemon via bridge IPC socket.
- **Skill routing rules** — `useWhen` and `dontUseWhen` conditions for context-aware skill activation.
- **Temporal decay for memory search** — optional scoring penalty for older memories (`temporal_decay_lambda` config).
- **LLM reasoning text preservation** — thinking/reasoning content emitted alongside tool calls is now preserved in responses.

### Changed

- **Actor-based agent execution** with `Arc<MemoryManager>` for improved thread safety.
- **`web_fetch` extraction upgraded** to use the `readability` crate with fallback text sanitization.
- **BridgeManager moved** from core to server crate for cleaner dependency graph.
- **Model routing updated** to support Claude 4.6 models.
- Replaced unsafe string byte-slicing with `floor_char_boundary` to prevent UTF-8 panics.
- Config templates expanded with `providers.xai`, `[tools.web_search]`, `[cron]`, and `[mcp]` examples.

### Fixed

- Mobile init EPERM by passing Config to MemoryManager.
- iOS XCFramework library identifiers, actor isolation, and C++ linking.
- Silent `NO_REPLY` tokens filtered from user-facing chat responses.
- Daemon foreground mode logging level.

### Contributors

Thanks to all contributors who helped shape this release! Special thanks to **[@jcorbin](https://github.com/jcorbin)** for generalizing daemon process handles, improving heartbeat reliability, incremental session saves, provider tooling improvements, and web search summaries; **[@TranscriptionFactory](https://github.com/TranscriptionFactory)** for tool filter infrastructure.

## [0.2.0] - 2026-02-14

A milestone release introducing LocalGPT Gen for 3D scene generation, XDG Base Directory compliance, Docker Compose support, and workspace restructuring.

### Added

- **LocalGPT Gen** — a new `localgpt-gen` subcrate for AI-driven 3D scene generation (Phase 1+2). ([55aa127](https://github.com/localgpt-app/localgpt/commit/55aa127))
- **Secure Docker Compose setup** for running LocalGPT in containers. ([#2](https://github.com/localgpt-app/localgpt/pull/2))
- **XDG Base Directory layout** for all paths, following platform conventions on Linux, macOS, and Windows. ([#18](https://github.com/localgpt-app/localgpt/issues/18))
- **Local server config guidance** for OpenAI-compatible server setups. ([#25](https://github.com/localgpt-app/localgpt/pull/25))
- Security section added to README covering sandbox, signed policy, and injection defenses.

### Changed

- Extracted `localgpt-gen` into its own workspace member and bumped to v0.2.0.
- Replaced OpenClaw auto-migration with a detection notice (no longer silently migrates config).
- Configured cargo-release for joint crates.io publishing.

### Fixed

- Added glibc compatibility shim for `ort-sys` on glibc < 2.38 (fixes builds on older Linux distros).

### Contributors

Thanks to **[@ttulttul](https://github.com/ttulttul)** (Ken Simpson) for the secure Docker Compose setup, and **[@cnaples79](https://github.com/cnaples79)** (Chase Naples) for documenting local server configuration!

## [0.1.3] - 2026-02-12

A major release focused on security hardening, new provider support, and the Telegram bot interface.

### Added

- **Telegram bot interface** with one-time pairing auth, slash commands, streaming responses with debounced edits, and full tool support. Runs as a background task inside the daemon. ([#15](https://github.com/localgpt-app/localgpt/pull/15))
- **Telegram HTML rendering** for agent responses with markdown-to-HTML conversion. ([#16](https://github.com/localgpt-app/localgpt/pull/16))
- **GLM (Z.AI) provider** support, adding Z.AI's GLM models as a new LLM backend. ([#21](https://github.com/localgpt-app/localgpt/pull/21))
- **Security policy module** with HMAC signing and tamper-detecting audit chain.
- **Kernel-enforced shell sandbox** for LLM-issued commands using macOS Seatbelt and Linux Landlock/seccomp.
- **Prompt injection defenses** with per-turn injection, suspicious-content warnings surfaced to users, and configurable strict policy mode.
- **Windows build support** by gating Unix-only sandbox and nix APIs behind `cfg(unix)`.

### Changed

- Renamed `localgpt security` CLI subcommand to `localgpt md`.
- Updated `LocalGPT.md` init template to match standing-instructions framing.
- Upgraded all dependencies to latest versions.

### Fixed

- Security block incorrectly sent as a prompt to Claude CLI instead of as a user message.
- Clippy warnings for Rust 1.93.
- Landlock/seccompiler API usage updated for latest crate versions.

### Contributors

Thanks to **[@haraldh](https://github.com/haraldh)** for building the Telegram bot interface and HTML rendering, and **[@austingreisman](https://github.com/austingreisman)** for adding GLM (Z.AI) provider support!

## [0.1.2] - 2026-02-09

This release enables tool calling for Ollama and OpenAI-compatible providers, and improves memory search quality.

### Added

- **Ollama tool calling support**, allowing Ollama models to execute agent tools. ([#14](https://github.com/localgpt-app/localgpt/pull/14))
- **Desktop feature flag** for headless builds (compile without GUI dependencies).
- GitHub Actions CI workflow with license audit via `cargo-deny`.

### Fixed

- **OpenAI provider tools** were silently dropped during streaming — the default `chat_stream` fallback now forwards tools and handles `ToolCalls` responses correctly. ([#11](https://github.com/localgpt-app/localgpt/pull/11))
- **Memory search** improved with token AND matching and rank-based scoring for more relevant results. ([#10](https://github.com/localgpt-app/localgpt/pull/10))
- Linux desktop builds now include x11 and Wayland features.

### Contributors

Thanks to **[@JarvisDeLaAri](https://github.com/JarvisDeLaAri)** for enabling Ollama tool calling, and **[@Ax73](https://github.com/Ax73)** for fixing OpenAI provider tool support!

## [0.1.1] - 2026-02-07

Introduces the desktop GUI, GGUF embedding support, and workspace concurrency safety.

### Added

- **Desktop GUI** built with egui, providing a native app experience.
- **GGUF embedding support** via llama.cpp for fully local semantic search.
- **Streaming tool details and slash commands** in the egui and web UIs.
- **Concurrency protections** for workspace file access.

### Fixed

- UTF-8 boundary panics in memory search snippets resolved; indexing simplified.

## [0.1.0] - 2026-02-04

Initial release of LocalGPT — a local-only AI assistant with persistent markdown-based memory.

### Added

- **Interactive CLI chat** with streaming responses and tool execution.
- **Multi-provider LLM support**: Anthropic, OpenAI, Ollama, and Claude CLI.
- **Markdown-based memory system** with `MEMORY.md`, daily logs, and `HEARTBEAT.md`.
- **Semantic search** using SQLite FTS5 and local embeddings via fastembed.
- **Autonomous heartbeat** runner for background task execution on configurable intervals.
- **HTTP/WebSocket API** with REST endpoints and real-time chat.
- **Embedded Web UI** for browser-based interaction.
- **OpenClaw compatibility** for workspace files, session format, and skills system.
- **Agent tools**: bash, read_file, write_file, edit_file, memory_search, memory_get, web_fetch.
- **Session management** with persistence, compaction, search, and export.
- **Image attachment support** for multimodal LLMs.
- **Tool approval mode** for dangerous operations.
- **Zero-config startup** defaulting to `claude-cli/opus`.
- **Auto-migration** from OpenClaw config if present.

[Unreleased]: https://github.com/localgpt-app/localgpt/compare/v0.3.5...HEAD
[0.3.5]: https://github.com/localgpt-app/localgpt/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/localgpt-app/localgpt/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/localgpt-app/localgpt/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/localgpt-app/localgpt/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/localgpt-app/localgpt/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/localgpt-app/localgpt/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/localgpt-app/localgpt/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/localgpt-app/localgpt/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/localgpt-app/localgpt/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/localgpt-app/localgpt/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/localgpt-app/localgpt/releases/tag/v0.1.0
