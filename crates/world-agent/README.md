# localgpt-world-agent

The world authority behind the [LocalGPT](https://github.com/localgpt-app/localgpt) apps,
and what agents reach it through — with no Bevy in it and no model.

- **The live authority** (`live::LiveWorld`): a `.world` folder an app keeps
  open while agents change it from outside, through batches of ops that are
  bound, checked and committed whole or refused, with undo, a guard on
  `manifest.json`, git commits and `verify`.
- **Packages** (`session`): read, fold, write and zip `.world` packages.
- **Headless commands** (`headless`): the same authority for scripts, CI
  and the terminal; the logic behind `localgpt world`.
- **The MCP servers** (`mcp`, behind the `mcp` and `mcp-http` features): the
  `Tool` trait and the stdio and streamable-HTTP servers that `localgpt
  world mcp` and `localgpt-gen mcp-server` serve their tools through.
- **The agent's environment** (`agent_env`) and **the shared paths**
  (`paths`): which world is open and how to reach it; where the apps keep
  their files and the CC0 asset pack.

The format itself is the [Open World Format](https://openworldformat.org),
through the `openworldformat` crate.

License: Apache-2.0.
