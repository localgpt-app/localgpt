# localgpt-world-agent

The world authority and agent runtime behind the [LocalGPT](https://github.com/localgpt-app/localgpt) apps,
with no Bevy in it.

- **The live authority** (`live::LiveWorld`): a `.world` folder an app keeps
  open while agents change it from outside, through batches of ops that are
  bound, checked and committed whole or refused, with undo, a guard on
  `manifest.json`, git commits and `verify`.
- **Packages** (`session`): read, fold, write and zip `.world` packages.
- **Headless commands** (`headless`): the same authority for scripts, CI
  and the terminal; the logic behind `localgpt world`.
- **The tool-call protocol** (`interpreter`): the commands an LLM issues to
  build a world, applied to `localgpt-world-types` entities. The mistral.rs
  tool-calling loop is behind the `llm` feature.
- **Asset packs** (`assets`): the manifest types and deterministic scatter
  math that placing and scattering assets speak.

The format itself is the [Open World Format](https://openworldformat.org),
through the `openworldformat` crate.

License: Apache-2.0.
