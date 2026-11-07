# LocalGPT MD

Open a Markdown file and walk through it as a 3D world. Every `##` section
becomes a place, and saving the file rebuilds the world while you watch.

Built with [Bevy](https://bevyengine.org/). Part of [LocalGPT](https://localgpt.app).

Worlds start from a fast rule-based draft. What a section's place looks
like beyond that comes from the **sidecar** next to the document
(`<doc>.world.json`): per section, keyed by its content hash, a build —
entities in place, with real CC0 assets from the shared Poly Haven pack —
or a one-JSON recipe, and a ```` ```world ```` fence in the file itself for
exact overrides. MD applies the sidecar and never writes it: no model runs
here. The coding agent you already use writes it, working on the file, and
a section keeps what was authored for it until its text changes. A
Markdown deck (`genre: deck`) presents as a 3D talk.

## Run

From the `localgpt` workspace root:

```bash
cargo run -p localgpt-md                          # open samples/hello.md
cargo run -p localgpt-md -- crates/md/samples/deck.md   # a Marp-style deck as a 3D talk
cargo run -p localgpt-md -- path/to/notes.md      # open any Markdown file
```

The docs are on [localgpt.app/docs/md](https://localgpt.app/docs/md):
[genres and keys](https://localgpt.app/docs/md#genres),
[export](https://localgpt.app/docs/md#export) to the LocalGPT world format,
[screenshot mode](https://localgpt.app/docs/md#screenshot) and
[how it works](https://localgpt.app/docs/md/how-it-works).

## Develop

```bash
cargo test -p localgpt-md
cargo clippy -p localgpt-md --all-targets -- -D warnings
cargo fmt --check
```

## Website

MD has no landing page of its own. [`website-md/`](../../website-md/) at
the workspace root is [https://localgpt.md](https://localgpt.md/), the
**previs** page (`docs/world-strategy.md`): drop a screenplay, get a
board — it reads PDFs, Final Draft files and Markdown script pages as well
as Fountain, but it does not
walk a document; that is this crate's Bevy app. The docs live on
localgpt.app, in this repository's `website/docs/md/`.

## License

Apache-2.0. See [LICENSE](LICENSE).
