# localgpt-world-export

The distribution side of the LocalGPT world format, with no Bevy dependency:

- `js/world-viewer.js` — the one web renderer of a `WorldManifest` (three.js
  0.170). localgpt.world serves it as a module; Gen and MD embed it.
- `generate_html(&manifest)` — a self-contained page: viewer + manifest JSON.
- `to_json` / `to_json_pretty` — the manifest as the JSON the viewer reads
  (schema: `crates/world-types/world.schema.json`).

The viewer draws what `localgpt-world-bevy` draws: linear RGBA colours, XYZ
Euler degrees, lux for directional lights, lumens for point and spot lights.
The unit calibration lives in two constants at the top of the file
(`AMBIENT_SCALE`, `DIRECTIONAL_LUX`). Reference scenes:
`crates/world-types/conformance/`.

```bash
cargo test -p localgpt-world-export
```
