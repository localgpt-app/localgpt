#!/bin/bash
set -e

# Create target directories
mkdir -p ../localgpt-rs/content
mkdir -p ../localgpt-rs/static/img

# Move the image
cp website/static/img/desert-pyramids-ufo-screenshot.png ../localgpt-rs/static/img/

# File 1
cat << 'FILE1' > ../localgpt-rs/content/2026-02-07-why-i-built-localgpt-in-4-nights.md
+++
title = "Why I Built LocalGPT in 4 Nights"
description = "I'm a data engineer by day. At night, I've been building side projects. I kept running into the same problem: context. Every time I started a new AI chat session, I had to re-explain my projects, my preferences, my decisions. The AI had amnesia. Then I discovered OpenClaw."
date = 2026-02-07
draft = false
authors = ["Yi"]

[taxonomies]
tags = ["rust", "ai", "open-source", "development", "openclaw"]
projects = ["localgpt"]
+++
I'm a data engineer by day. At night, I've been building side projects — apps, games, the usual fun stuff. I kept running into the same problem: context. Every time I started a new AI chat session, I had to re-explain my projects, my preferences, my decisions. The AI had amnesia.

Then I discovered OpenClaw.

### The OpenClaw Pattern

OpenClaw pioneered something elegant: a set of markdown files that give an AI assistant persistent context.

- **SOUL** — personality and behavioral guidance
- **MEMORY** — long-term knowledge that persists across sessions
- **HEARTBEAT** — a task queue the assistant checks autonomously

It's brilliant in its simplicity. No databases to manage. No complex state machines. Just markdown files that both humans and AI can read and write.

But OpenClaw is a full platform — ~460k lines of TypeScript across ~2,500 files, with a gateway, mobile apps, multi-channel integrations, and Docker deployments. I wanted something smaller.

### 4 Nights in Rust

I wondered: what does this architecture look like as a single Rust binary? So I paired up with Claude and here's what happened.

**Night 1 (Feb 1, 2 commits)**: The foundation. Core agent system with the full module architecture (agent, CLI, daemon, memory, server, heartbeat), streaming responses, and the initial project structure. By midnight: a working skeleton. ~3,000 lines.

**Night 2 (Feb 2-3, 34 commits — the marathon)**: This is where it came alive. Claude CLI as the primary LLM provider, OpenClaw-compatible file structure, SOUL persona support, system prompt with identity and safety, skills system, readline CLI with arrow keys and history, SQLite memory index with FTS5, embeddings and vector search, streaming tool execution, WebSocket API, session persistence, Anthropic streaming, model selection, and memory management — and by 2am, a working end-to-end system. The most productive night by far.

**Night 3 (Feb 3, 27 commits)**: Woke up and kept going. Local embeddings with fastembed for semantic search, multi-session HTTP API with 6+ endpoints, token tracking, file and image attachments, tool approval mode, an embedded web UI served from the binary, daemon mode running heartbeat and server concurrently, configurable memory indexing, session management, and config API endpoints.

By the afternoon of Feb 3, I couldn't resist — I got this running on my work machine, tweaked the configuration, and started integrating it into my actual workflow. I set up heartbeat tasks to autonomously discover and summarize data pipeline improvements for my team. It went from side project to daily tool in two days.

**Night 4 (Feb 4-5, 18 commits)**: Polish and hardening. First-run experience improvements, configurable embedding providers with multilingual model support, heartbeat task tracking with git integration, a daemon logs panel and session viewer in the web UI, date-based rolling logs, prompt injection defenses (a full 381-line sanitization module), cached MemoryManager, GGUF embedding support via llama.cpp, and the egui-based desktop GUI app.

After that, a few days of bug fixes and documentation.

The result: ~15k lines of Rust (+ ~1,400 lines of HTML/CSS/JS for the web UI), a ~27MB binary, and `cargo install localgpt` just works. No Node.js. No Docker. No Python. (The embedding model cache may require a few hundred MB of disk space, depending on the model you choose.)

For perspective, when this post is written:

```bash
# OpenClaw: ~460k lines of TypeScript across ~2,500 files
$ find openclaw/src -name "*.ts" | wc -l
    2598
$ find openclaw/src -name "*.ts" | xargs wc -l | tail -1
  463649 total

# LocalGPT: ~15k lines of Rust across 43 files
$ find localgpt/src -name "*.rs" | wc -l
      43
$ find localgpt/src -name "*.rs" | xargs wc -l | tail -1
   15031 total
```

To be fair, this isn't an apples-to-apples comparison. OpenClaw is a full platform — it includes a gateway, mobile apps, multi-channel integrations, and much more that LocalGPT simply doesn't have now. LocalGPT currently is a focused, single-user tool. The point isn't that less code is better — it's that you can get surprisingly far with a narrow scope and the right language.

### How I Actually Use It

LocalGPT runs as a daemon on my machine — `localgpt daemon start`. Every heartbeat interval, it checks HEARTBEAT for tasks.

**Knowledge accumulator** — It remembers my project context across sessions. My TODO list, my decisions. I never re-explain.

**Research assistant** — "What embedding models support multilingual search?" It researches, summarizes, and saves the answer to my knowledge bank for next time.

**Autonomous worker** — I use `localgpt chat` and add tasks to HEARTBEAT and walk away. It organizes my knowledge files, researches topics, drafts content, and reminds me of approaching deadlines.

**Memory that compounds** — Every session makes the next one better. It builds a structured knowledge bank across domains — finance, legal, tech — all on my personal computer — that grows over time.

### The Technical Bits

For those interested in the stack:

- **Tokio** async runtime for concurrent operations
- **Axum** for the HTTP API
- **SQLite** with FTS5 for full-text search and sqlite-vec for semantic search
- **fastembed** for local embeddings (no API key needed)
- **eframe** for the desktop GUI
- **rusqlite** with bundled SQLite (zero system dependencies)

The architecture is intentionally simple. Markdown files are the source of truth. SQLite is the index. The binary is the runtime. That's it.

### Open Source

LocalGPT is open source under the Apache 2.0 license. Fork it, extend it, build something better — that's the point.

```bash
cargo install localgpt
localgpt config init
localgpt chat
```

If you're building with AI assistants — whether you're in the OpenClaw community or rolling your own — I'd love to hear what patterns are working for you.

GitHub: https://github.com/localgpt-app/localgpt
Website: https://localgpt.app

### One more thing...

I don't have access to Project Genie 3, so I made my own (extremely minified) version.

**LocalGPT Gen** is a built-in 3D scene generation mode. You type natural language, and the AI composes 3D scenes from geometric primitives — spheres, cubes, cylinders, tori — with full material control, lighting, and camera positioning. All inside the same single Rust binary, powered by Bevy.

```bash
# Enable the gen feature and try it
cargo install localgpt --features gen
localgpt gen "create a heart outline with spheres and cubes"
```

The agent gets 11 tools — `gen_spawn_primitive`, `gen_modify_entity`, `gen_set_camera`, `gen_set_light`, `gen_spawn_mesh`, and more — and iteratively builds scenes by spawning shapes, adjusting materials, and positioning the camera. It can see what it built via `gen_screenshot` and course-correct.

Here's a quick demo — "create a heart outline with spheres and cubes" using LocalGPT Gen:

<iframe width="100%" height="400" src="https://www.youtube.com/embed/n18qnSDmBK0" title="LocalGPT Gen — Heart Outline Demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture" allowfullscreen></iframe>

**Fair warning:** this is early and I haven't fully validated the results myself. Things work... probably. The architecture is solid — Bevy runs on the main thread, the agent loop runs on a background tokio runtime, and they talk through async mpsc channels — but the actual scene output quality depends heavily on the LLM's spatial reasoning. Consider it a proof of concept and a fun experiment.

---

### Appendix: Complete Commit Log

85 commits across 4 nights + follow-up. Every commit listed.

**Night 1 — Feb 1 (2 commits)**

| # | Hash | Time | Message |
|---|------|------|---------|
| 1 | `55599d3` | 12:33 | first commit |
| 2 | `c7e92ed` | 22:55 | feat: implement LocalGPT core application |

**Night 2 — Feb 2 afternoon → Feb 3 ~2am (34 commits)**

| # | Hash | Time | Message |
|---|------|------|---------|
| 3 | `cf9cb6a` | Feb 2 16:27 | feat: add thread safety and SSE streaming support |
| 4 | `e143f73` | Feb 2 18:03 | feat: add Claude CLI as primary LLM provider |
| 5 | `e549dd2` | Feb 2 19:29 | feat: add OpenClaw-compatible file structure for session storage |
| 6 | `a2b8bc8` | Feb 2 19:31 | feat: add SOUL.md support for persona/tone guidance |
| 7 | `3734aea` | Feb 2 19:37 | feat: add system prompt with identity, safety, and workspace info |
| 8 | `9568159` | Feb 2 19:51 | feat: add tool call style, time, skills system, and /new command |
| 9 | `9520850` | Feb 2 19:53 | feat: add OpenClaw config migration support |
| 10 | `74505a0` | Feb 2 19:53 | feat: add memory recall guidance to system prompt |
| 11 | `2aebfae` | Feb 2 19:54 | docs: update CLAUDE.md with skills, CLI commands, and migration info |
| 12 | `6cf4b39` | Feb 2 20:04 | feat: add streaming output in CLI chat + clean up dead code |
| 13 | `a22f96e` | Feb 2 20:17 | feat: auto-create workspace templates and .gitignore on first run |
| 14 | `5779539` | Feb 2 20:21 | fix: move daemon.pid from workspace to state directory |
| 15 | `e0fbc7f` | Feb 2 20:24 | feat: add line editing with rustyline for arrow keys and history |
| 16 | `dbe9670` | Feb 2 23:23 | feat: move SQLite to OpenClaw-compatible location |
| 17 | `e05d050` | Feb 2 23:37 | feat: add embeddings and vector search support |
| 18 | `637f611` | Feb 2 23:39 | feat: add streaming with tool support |
| 19 | `d18ebd8` | Feb 2 23:41 | feat: add session persistence across restarts |
| 20 | `39449af` | Feb 2 23:42 | feat: add WebSocket API for real-time chat |
| 21 | `38fdf39` | Feb 2 23:49 | fix: reorder schema to create index after migration |
| 22 | `3283d17` | Feb 3 00:01 | feat(memory): align SQLite schema with OpenClaw |
| 23 | `40de884` | Feb 3 00:15 | feat: add OpenClaw workspace compatibility and multi-agent support |
| 24 | `b82c188` | Feb 3 00:37 | feat: add Anthropic streaming and fix provider selection |
| 25 | `005d896` | Feb 3 00:45 | feat: align model naming with OpenClaw format |
| 26 | `5454349` | Feb 3 00:51 | fix: correct Anthropic model ID to claude-sonnet-4-20250514 |
| 27 | `23dd9c9` | Feb 3 00:53 | feat: use Claude Opus 4.5 as default model |
| 28 | `702b7b5` | Feb 3 00:54 | fix: use correct Anthropic model IDs from official docs |
| 29 | `fa0354e` | Feb 3 00:55 | refactor: only use Claude 4.5 models, remove legacy mappings |
| 30 | `693fd5d` | Feb 3 01:08 | fix: pass tools to streaming API to prevent XML tool output |
| 31 | `919a006` | Feb 3 01:13 | feat: implement streaming tool execution |
| 32 | `8fd048e` | Feb 3 01:19 | fix: memory_append now supports MEMORY.md for persistent facts |
| 33 | `bd30e61` | Feb 3 01:20 | chore: add user data files to gitignore |
| 34 | `f469cce` | Feb 3 01:27 | feat: OpenClaw-style memory management |
| 35 | `66fd879` | Feb 3 01:44 | feat: show file paths and details in tool execution display |
| 36 | `6ff42ac` | Feb 3 01:48 | feat: improve memory search UX |

**Night 3 — Feb 3 morning → midnight (27 commits)**

| # | Hash | Time | Message |
|---|------|------|---------|
| 37 | `8706ca5` | 09:47 | feat: add local embeddings with fastembed for semantic search |
| 38 | `bd302cf` | 14:02 | chore: change server port to 31327 and fix license to Apache-2.0 |
| 39 | `db86b8a` | 14:05 | feat: default to claude-cli/opus for zero-config startup |
| 40 | `2143e43` | 15:35 | fix: use correct db_path in MemoryWatcher |
| 41 | `dc11dc1` | 16:52 | feat: add configurable memory index paths with glob support |
| 42 | `cb2d27e` | 17:09 | feat: add multi-session HTTP API, token tracking, and daemon improvements |
| 43 | `c0d8134` | 17:16 | feat: add configurable tools and agent parameters |
| 44 | `718d10b` | 17:19 | test: add tests for token tracking and LLM response handling |
| 45 | `88b9783` | 17:21 | docs: update /help to mention API token usage in /status |
| 46 | `b7948f1` | 17:41 | feat: use config chunk_size and chunk_overlap in memory index |
| 47 | `2f1d6a6` | 17:52 | feat: add session management endpoints and /model command |
| 48 | `68545bf` | 18:08 | feat: enhance CLI status and add memory reindex API |
| 49 | `45bdd23` | 18:09 | feat: add session management API endpoints |
| 50 | `f190a62` | 18:10 | feat: add config and saved sessions API endpoints |
| 51 | `b2c3b80` | 18:14 | chore: fix clippy warnings and formatting |
| 52 | `327ea18` | 18:16 | feat: add context usage and session export commands |
| 53 | `041d070` | 18:21 | feat: add session search across all saved sessions |
| 54 | `03595d9` | 18:23 | feat: add file attachments to chat messages |
| 55 | `75405cc` | 18:36 | feat: add tool approval mode for dangerous operations |
| 56 | `0bf0f7a` | 18:41 | feat: add persistent HTTP sessions |
| 57 | `f057232` | 18:50 | feat: add image attachment support for multimodal LLMs |
| 58 | `294c9d7` | 20:23 | feat: add embedded Web UI for browser-based chat |
| 59 | `4299ed9` | 20:35 | fix: daemonize before starting Tokio runtime on macOS |
| 60 | `2d7771f` | 21:00 | fix: cleanup deleted files from memory index during reindex |
| 61 | `d01e61c` | 22:08 | feat: run heartbeat and server concurrently in daemon |
| 62 | `c8bf9c2` | 22:13 | chore: add commented active_hours example to config templates |
| 63 | `9afdd22` | 23:53 | feat: add OpenClaw compatibility for sessions, skills, and workspace files |

**Night 4 — Feb 4 afternoon → Feb 5 ~1am (18 commits)**

| # | Hash | Time | Message |
|---|------|------|---------|
| 64 | `530bc13` | Feb 4 17:10 | feat: improve first-run experience and Claude CLI provider |
| 65 | `fca8290` | Feb 4 18:49 | feat: add configurable embedding provider for memory search |
| 66 | `2525330` | Feb 4 19:06 | feat(heartbeat): improve task tracking and git integration |
| 67 | `e746275` | Feb 4 19:07 | fix: correct indentation in MemoryManager constructor |
| 68 | `0773d78` | Feb 4 19:22 | fix: remove session memory truncation, add configurable limits |
| 69 | `b4b0532` | Feb 4 19:47 | feat(ui): add daemon logs panel and session viewer |
| 70 | `beb82d2` | Feb 4 21:23 | fix(ui): sessions panel reads from correct agent directory |
| 71 | `4b23b3e` | Feb 4 21:59 | feat(security): add prompt injection defenses |
| 72 | `4f5718d` | Feb 4 22:02 | fix(heartbeat): cache MemoryManager to avoid reinitializing embedding provider |
| 73 | `fe36213` | Feb 4 22:08 | fix(daemon): use date-based rolling logs with configurable retention |
| 74 | `34ce89c` | Feb 4 22:20 | fix(server): update logs endpoint to use date-based log path |
| 75 | `7eb8156` | Feb 4 22:27 | fix(server): share MemoryManager to avoid reinitializing embedding provider |
| 76 | `a7fc787` | Feb 4 22:28 | fix(daemon): disable ANSI color codes in log files |
| 77 | `cbc2f67` | Feb 4 23:01 | chore: suppress unused function warning for test helper |
| 78 | `ff86421` | Feb 4 23:16 | feat(memory): add multilingual embedding models |
| 79 | `a3d440f` | Feb 4 23:35 | feat(memory): add configurable embedding model cache directory |
| 80 | `77486d3` | Feb 5 00:22 | feat(memory): add GGUF embedding support via llama.cpp |
| 81 | `c313178` | Feb 5 00:40 | feat(desktop): add egui-based desktop GUI app |

**Follow-up — Feb 6-7 (4 commits)**

| # | Hash | Time | Message |
|---|------|------|---------|
| 82 | `f573039` | Feb 6 00:40 | feat: add concurrency protections for workspace files |
| 83 | `6bd21a3` | Feb 6 02:10 | feat: add streaming tool details and slash commands to egui/web UIs |
| 84 | `9c91931` | Feb 7 15:36 | fix: resolve UTF-8 boundary panics in memory search snippets and simplify indexing |
| 85 | `6f4ea95` | Feb 7 21:06 | docs: rewrite README and update crate metadata for v0.1.2 |
FILE1

# File 2
cat << 'FILE2' > ../localgpt-rs/content/2026-03-09-world-as-skill.md
+++
title = "Explorable World as Agent Skill"
description = "LocalGPT Gen v0.3.2 introduces a new concept: treating complete 3D worlds as reusable skills that agents can save, load, and share."
date = 2026-03-09
draft = false
authors = ["Yi"]

[taxonomies]
tags = ["3d", "architecture", "skills", "world"]
projects = ["localgpt", "gen"]
+++
![desert-pyramids-ufo-screenshot.png](/img/desert-pyramids-ufo-screenshot.png)

**LocalGPT Gen** v0.3.2 introduces a new concept: treating complete 3D worlds as reusable skills that agents can save, load, and share.

`localgpt-gen` now saves complete worlds as skill directories containing:

*   **Scene geometry** — All entities, meshes, and transforms
*   **Behaviors** — Animations like orbit, spin, bob, path following
*   **Audio configuration** — Ambient soundscapes and spatial emitters

The skill folder structure:

```
skills/my-world/
├── SKILL.md          # Description and usage
├── world.ron         # World manifest with entities preserves parametric shapes
└── export/
    └── scene.glb     # glTF export (generated on demand)
```

This is inspired by [blender-mcp](https://github.com/ahujasid/blender-mcp) and [bevy_brp](https://github.com/natepiano/bevy_brp), and may shrink the software supply chain from intent to result a little bit more.

LocalGPT Gen is closer to the domain of tools for [Explorable World](https://localgpt.app/docs/worlds) like Genie 3, SIMA 2, Marble, Intangible and Artcraft.

**Showcases:**
- [localgpt-gen-workspace](https://github.com/localgpt-app/localgpt-gen-workspace) — "World as skill" examples: complete explorable worlds saved as reusable, shareable skills
- [proofof.video](https://proofof.video/) — Video gallery comparing world generations across different models using the same or similar prompts

Many players are advancing the [Claw Ecosystem](https://localgpt.app/docs/claw) so the agent memory and orchestration will improve automatically.
FILE2

# Remove blog files from website
rm -rf website/blog

