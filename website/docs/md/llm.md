---
description: "LocalGPT MD's optional LLM tier: fetch the model, style sections live or headless, and how the per-section sidecar cache keeps worlds reproducible."
---

# The LLM tier

*Optional: a local model restyles each section from what the prose says.*

```bash
./scripts/fetch-bonsai.sh                              # ~5.2 GB model, once
cargo run --features llm-metal --                      # styled live, in-app
cargo run --features llm-metal -- notes.md --generate  # headless: style all, exit
```

With the feature and a model present, a background worker styles every section that has no cached recipe, and each region upgrades in place as its recipe arrives. Results live in `notes.md` → `notes.world.json` — a lockfile keyed by each section's content hash: unchanged sections are never regenerated, and a `.md` plus its sidecar renders identically on any machine, even in the default (no-`llm`) build, which still applies cached recipes.

Every model-authored value is clamped on the Rust side, and any generation failure keeps the rule-based draft — the app is never broken by a missing or misbehaving model.

`llm-metal` is the macOS GPU path (the 5 GB Q4_K_M needs it to fit in memory); `llm` alone expects a smaller GGUF. Any standard GGUF plus a matching `tokenizer.json` dropped in `assets/llm/` (or `$LOCALGPT_MD_LLM`) is picked up — the sibling Verse checkout's model directory is searched too.
