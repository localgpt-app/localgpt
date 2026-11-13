---
description: "Open a Markdown file and walk through it as a 3D world: run LocalGPT Document, the world and deck genres, navigation keys, exporting to the Open World Format, and screenshot mode."
title: "LocalGPT Document"
aliases: ["/docs/md/"]
---

# LocalGPT Document

**LocalGPT Document** opens a Markdown file and lets you walk through it as a 3D world. Every `##` section becomes a place, and saving the file rebuilds the world while you watch. It is built with [Bevy](https://bevyengine.org/), and it is a mode of the [LocalGPT desktop app](/docs/app/#document) — `localgpt-app --doc notes.md` opens the document with an editor pane beside the world, rebuilding in place as you type — as well as a window of its own, `localgpt-doc`.

Worlds start from a fast rule-based draft. What a section's place looks like beyond that comes from the **sidecar** next to the document (`<doc>.world.json`): per section, keyed by the section's content hash, a build (entities in place, with real [CC0 assets](../verse/index.md) from the shared Poly Haven pack) or a one-JSON recipe, and a `world` fence in the file itself for exact overrides. Document applies the sidecar and never writes it: no model runs in Document. The agent you already use writes it — put it on the file, or on the document's world through `localgpt world mcp` in the desktop app — and a section keeps what was authored for it until its text changes.

## Run {#run}

```bash
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt

# Open samples/hello.md
cargo run -p localgpt-doc
# Present a Marp-style deck as a 3D talk
cargo run -p localgpt-doc -- crates/doc/samples/deck.md
# Open any Markdown file
cargo run -p localgpt-doc -- path/to/notes.md
# Print the compiled world as RON and exit
cargo run -p localgpt-doc -- notes.md --print-ron
# Write a self-contained page with the web viewer
cargo run -p localgpt-doc -- notes.md --export notes.html
# Write the world as Open World Format JSON, for localgpt.world
cargo run -p localgpt-doc -- notes.md --export notes.json

# The same document in the desktop app, with the editor pane
cargo run -p localgpt-app -- --doc path/to/notes.md
```

## Genres {#genres}

Two genres: the default `world` makes every `##` section a place on a winding path; `genre: deck` (front matter) splits on `---` separators, Marp/Slidev style, and lays the slides out along a straight presentation path — arrow keys advance the talk. Keep the file open in your editor: every save rebuilds the world, and the camera stays on the slide you're looking at.

| Key | Action |
|---|---|
| → ↓ Space PageDown | Next section |
| ← ↑ PageUp | Previous section |
| Home / End | First / last section |

## Export {#export}

`--export` writes the compiled world in the [Open World Format](https://openworldformat.org), the one format every LocalGPT app reads and writes: `.json` (what the web viewer on [localgpt.world](https://localgpt.world) opens), `.ron` (Gen's `world.ron`, loadable with `gen_load_world`) or `.html` (a single page with the same viewer Gen's `gen_export_html` embeds). Only the document title, the first line of the intro and the section headings are in the manifest — body text stays in the `.md`.

## Screenshot mode {#screenshot}

`LOCALGPT_DOC_SCREENSHOT=out.png cargo run -p localgpt-doc` renders the first stop to a PNG and exits, for a quick check without a human. It renders offscreen with no window, so it also works over SSH or while the screen is locked.
