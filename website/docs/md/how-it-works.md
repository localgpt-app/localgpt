---
description: "How LocalGPT MD compiles a Markdown file into a world: the modules, the per-section hashes, the shared world format, and how to develop it."
---

# How it works

```text
FILE.md ──► doc.rs ──────► draft.rs ─────────► scene.rs ──► Bevy
            sections +     WorldManifest       entities,
            BLAKE3 hashes  (localgpt-world-    lights, fog
                            types)                  ▲
   ▲                                                │
watch.rs: poll the file, recompile on save     tour.rs: camera + caption
```

- **`doc.rs`**: Markdown → title, front matter (`genre: …`), and one section per place — a `##` heading (`world`) or a `---`-separated slide (`deck`). Each section carries a hash of its text (plus any `world` fence).
- **`draft.rs`**: sections → a `WorldManifest` with one region per section — a winding path (`world`) or a straight presentation path (`deck`) — and a tour with one stop per section. Deterministic, with entity ids that are stable per section, so editing one section leaves the other regions untouched. Per section the first of these wins: a `world` fence's exact entities, the sidecar's cached agent build, the cached recipe — else the rules.
- **`agent.rs` / `assets.rs`** (`llm` feature): the tool-calling agent (ported from Verse) and the pack's manifest/kind/span math. A pure interpreter applies the model's tool calls to platform-local entities; the pack's real CC0 models are placed by semantic kind.
- **`recipe.rs` / `sidecar.rs`** (pure): the clamped per-region recipe type with its lenient LLM-reply parse, and the hash-keyed `<doc>.world.json` v2 cache (builds + recipes). Compiled in every build; only the authoring is feature-gated.
- **`llm.rs` / `generation.rs`** (`llm` feature): the model (ported from Verse) and the worker thread running the tier chain — agent build → recipe → draft — in the background while the draft is already on screen.
- **`scene.rs`**: spawns the manifest in Bevy using the same mapping as LocalGPT Gen, and loads placed GLBs.
- **`tour.rs`** and **`watch.rs`**: navigation and hot reload.

Worlds use [`localgpt-world-types`](https://crates.io/crates/localgpt-world-types), the format LocalGPT Gen saves as `world.ron`, and pass Gen's own validation. `--print-ron` prints exactly that file.

## Develop {#develop}

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

LocalGPT MD is open source under Apache-2.0; the code is at [github.com/localgpt-app/localgpt-md](https://github.com/localgpt-app/localgpt-md).
