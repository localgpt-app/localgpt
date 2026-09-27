# LocalGPT MD

Open a Markdown file and walk through it as a 3D world. Every `##` section
becomes a place, and saving the file rebuilds the world while you watch.

Built with [Bevy](https://bevyengine.org/). Part of [LocalGPT](https://localgpt.app).

**Status: M3.** Worlds start from a fast rule-based draft, then a local LLM
(feature `llm`) takes over per section: an **agent** composes each place
through tool calls — primitives, lights, and real CC0 assets from the
shared Poly Haven pack — with a one-JSON recipe tier as fallback, and a
```` ```world ```` fence for exact hand-authored overrides. Everything is
cached per section in a sidecar next to the document, so nothing is
generated twice and the no-model build renders cached worlds identically. A
Markdown deck (`genre: deck`) presents as a 3D talk. The on-device model
and inference path are ported from LocalGPT Verse (Bonsai-8B via
mistral.rs; see [PLAN.md](PLAN.md)).

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
[screenshot mode](https://localgpt.app/docs/md#screenshot),
[the LLM tier](https://localgpt.app/docs/md/llm) and
[how it works](https://localgpt.app/docs/md/how-it-works).

## Develop

```bash
cargo test -p localgpt-md
cargo clippy -p localgpt-md --all-targets -- -D warnings
cargo fmt --check
```

## Website

[`website-md/`](../../website-md/) — at the workspace root, beside
`website-gen/` — is the landing page for
[localgpt.md](https://localgpt.md/): static HTML, no build step.
`website-md/deploy.sh` publishes it to Cloudflare as the `localgpt-md` Worker.
The docs live on localgpt.app, in this repository's `website/docs/md/`.

## License

Apache-2.0. See [LICENSE](LICENSE).
