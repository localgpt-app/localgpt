---
description: "Open a Markdown file and walk through it as a 3D world: run LocalGPT MD, the world and deck genres, navigation keys, exporting to the LocalGPT world format, and screenshot mode."
sidebar_label: Getting started
---

# LocalGPT MD

**LocalGPT MD** opens a Markdown file and lets you walk through it as a 3D world. Every `##` section becomes a place, and saving the file rebuilds the world while you watch. It is built with [Bevy](https://bevyengine.org/); see [md.localgpt.app](https://md.localgpt.app/) for the overview.

Worlds start from a fast rule-based draft, then a local LLM (feature `llm`) restyles each region from what the prose actually says — palette, landmark, props — and caches the result per section in a sidecar next to the document, so nothing is generated twice. The on-device model and inference path are ported from [LocalGPT Verse](../verse/index.md) (Bonsai-8B via mistral.rs; see [PLAN.md](https://github.com/localgpt-app/localgpt-md/blob/main/PLAN.md)).

## Run {#run}

```bash
git clone https://github.com/localgpt-app/localgpt-md.git
cd localgpt-md

# Open samples/hello.md
cargo run
# Present a Marp-style deck as a 3D talk
cargo run -- samples/deck.md
# Open any Markdown file
cargo run -- path/to/notes.md
# Print the compiled world as RON and exit
cargo run -- notes.md --print-ron
# Write a self-contained page with the web viewer
cargo run -- notes.md --export notes.html
# Write the LocalGPT world format, for localgpt.world
cargo run -- notes.md --export notes.json
```

## Genres {#genres}

Two genres: the default `world` makes every `##` section a place on a winding path; `genre: deck` (front matter) splits on `---` separators, Marp/Slidev style, and lays the slides out along a straight presentation path — arrow keys advance the talk. Keep the file open in your editor: every save rebuilds the world, and the camera stays on the slide you're looking at.

| Key | Action |
|---|---|
| → ↓ Space PageDown | Next section |
| ← ↑ PageUp | Previous section |
| Home / End | First / last section |

## Export {#export}

`--export` writes the compiled world in LocalGPT's shared world format: `.json` (what the web viewer on [localgpt.world](https://localgpt.world) opens), `.ron` (Gen's `world.ron`, loadable with `gen_load_world`) or `.html` (a single page with the same viewer Gen's `gen_export_html` embeds). Only the document title, the first line of the intro and the section headings are in the manifest — body text stays in the `.md`.

## Screenshot mode {#screenshot}

`LOCALGPT_MD_SCREENSHOT=out.png cargo run` renders the first stop to a PNG and exits, for a quick check without a human. It renders offscreen with no window, so it also works over SSH or while the screen is locked.
