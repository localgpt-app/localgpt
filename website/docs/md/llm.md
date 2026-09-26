---
description: "LocalGPT MD's optional LLM tier: the agent composes each section's contents through tool calls — primitives, lights, and real CC0 assets — with a one-JSON recipe tier as fallback and a `world` fence for exact overrides, all cached per section."
---

# The LLM tier

*Optional: a local model authors each section from what the prose says.*

```bash
./scripts/fetch-bonsai.sh                              # ~5.2 GB model, once
./scripts/fetch-assets.sh                              # the CC0 3D asset pack (optional)
cargo run --features llm-metal --                      # authored live, in-app
cargo run --features llm-metal -- notes.md --generate  # headless: author all, exit
```

With the feature and a model present, a background worker authors every section that has no cached output, and each region upgrades in place as its result lands. Results live in `notes.md` → `notes.world.json` — a lockfile keyed by each section's content hash: unchanged sections are never regenerated, and a `.md` plus its sidecar renders identically on any machine, even in the default (no-`llm`) build, which still applies cached output.

Per section, the tier chain is:

1. **Agent build.** The model composes the place through tool calls — primitives, lights, and real assets from the shared [CC0 Poly Haven pack](#the-asset-pack) — emitting `localgpt-world-types` entities. This is what runs for most sections.
2. **Recipe** — one styled JSON (palette, landmark kind, props) — if the agent session yields nothing.
3. **Rule draft** — always the fallback, so the app is never broken by a missing or misbehaving model.

A ```` ```world ```` fence in a section is an exact override that needs no LLM at all: a JSON array of world entities in platform-local coordinates (ids optional). Edit only the fence and the cache re-keys.

Every model-authored value is clamped on the Rust side (position, colours, light intensity, entity count), and any failure keeps the rule-based draft.

`llm-metal` is the macOS GPU path (the 5 GB Q4_K_M needs it to fit in memory); `llm` alone expects a smaller GGUF. The model lives in `~/.local/share/localgpt/models/llm/` (set `$LOCALGPT_LLM_DIR` to move it), the folder [Verse](/docs/verse) and [Gen](/docs/gen) read too, so one download serves all three. Any standard GGUF plus a matching `tokenizer.json` dropped there is picked up. `$LOCALGPT_MD_LLM` points MD alone somewhere else, and the older `assets/llm/` folders are still searched.

## The asset pack

`place_asset` and `scatter_field` place real models from a 171-model CC0 Poly Haven pack (the same one Verse uses), grouped by semantic kind (`rock`, `tree`, `lamp`, …) so the model names a kind and the app picks and varies the concrete model. The pack resolves from `$LOCALGPT_MD_ASSETS` → `assets/` → the sibling [`localgpt-verse-assets`](https://github.com/localgpt-app/localgpt-verse-assets) checkout, and `scripts/fetch-assets.sh` copies it locally (about 520 MB). Without a pack the agent tier still works, with primitives only. Placed GLBs render in the app, and `--export *.html` copies every referenced model beside the page so the web viewer can load them.

## What gets cached

The sidecar holds two kinds of output per section-hash: **builds** (the agent's entities, plus the model's closing description and which model authored them) and **recipes** (the styled JSON). It's pruned to live sections on save and written atomically, so a half-written file never replaces a good one. Version 2 files (builds) supersede version 1 (recipes only); the loader warns and ignores older versions — regenerate with `--generate`.
