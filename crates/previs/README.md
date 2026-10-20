# localgpt-previs

Script to board. A screenplay goes in: Fountain, a Final Draft `.fdx`, a
PDF, or Markdown script pages. Out comes, per scene, a head-first `.world`
package with the set, the cast and the coverage, plus a printable
`board.html` and a `shotlist.csv`.

```bash
cargo install localgpt-previs
localgpt-previs script.fountain --out board/
```

It is deterministic: no model, no network, no renderer, so the same script
drafts to the same bytes. The same code runs as WebAssembly on
[localgpt.md](https://localgpt.md), where a script dropped on the page
becomes the board in the tab. The packages follow the
[Open World Format](https://openworldformat.org); cameras and shots ride
its `ext-cinematography` extension.

Part of [LocalGPT](https://github.com/localgpt-app/localgpt). License: Apache-2.0.
