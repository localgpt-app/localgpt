# The harness goes: what 0.5.0 keeps, ships and installs

**Status:** proposal, 2026-10-26. Nothing built against it. Supersedes §3,
§4 and *What to stop* of [the-cli-and-the-world.md](the-cli-and-the-world.md);
its §1 and §2, `localgpt world` and the shim, stand and are what 0.5.0 keeps.
Builds on [agents-outside-the-window.md](agents-outside-the-window.md), whose
tier 0 is built (the herdr pane, `crates/gen/src/gen3d/herdr.rs`), and on
`docs/world-strategy.md` §13.6 (surface reduction) and §13.3 (the generation
tiers, which this does not touch).

## The problem

The 2026-10-04 decision stopped `chat` and `tui` as product surfaces and kept
the harness under them for two reasons: memory, heartbeat and cron pointed at
worlds (its §3), and eleven providers kept as plumbing (its §4). Three weeks
on, neither reason holds.

- **§3 was never built**, and the coding agents now carry that list
  themselves: Claude Code has persistent memory, lifecycle hooks and scheduled
  runs; Codex runs tools in a sandbox. "No coding agent does scheduled
  unattended work" was true in April and is not now. The moat the section
  hoped for is a feature of the agents the app already defers to.
- **§4's plumbing is where the maintenance goes.** The last Security entry in
  `CHANGELOG.md` — JWT signing moved to AWS-LC, the `rsa` and ECDSA
  advisories, `tokio`, `rustls-webpki`, `webpki-roots` — was entirely
  `localgpt-core`'s dependency tree, carried for a Vertex AI provider the
  product does not need. `deny.toml:77` ignores an unmaintained `daemonize`
  for the daemon. ONNX Runtime is installed in three CI jobs for local
  embeddings. None of this serves a world.
- **The app no longer wants it.** The desktop app must not manage agent
  configuration — no model menu, no keys, no `default_model` — because agents
  run outside and the app spawns, shows, sandboxes and kills them
  (*The line to hold*, agents-outside-the-window.md). The prompt panel and
  the model menu are the harness's front; with them gone, `localgpt-core`
  has no caller in the app that is not the loop.

The config said it in April (`claude-cli/opus` as the default model: the
harness shells out to the agent it competes with). The honest end of that
sentence is to stop shipping the harness, not to keep it as plumbing.

**Decision: the agent harness is removed in 0.5.0.** `localgpt` becomes the
world's command line, the shim, and the setup for a machine whose agents are
someone else's. The desktop app ships as a release, not as a source build.

## What the CLI is

Four commands. Two exist.

| Command | | What it does |
|---|---|---|
| `localgpt world …` | built | `init`, `submit` (`--at` forks), `undo`, `log`, `tips`, `verify`, `history`: the headless authority, `world-agent::headless`, one implementation of commit with the app's |
| `localgpt world mcp [dir]` | built | the shim: a stdio MCP server the agent starts, eight tools over the open world's API (`crates/cli/src/cli/world_mcp.rs`) |
| `localgpt doctor` | new | what this machine has: the app and its version; which of `claude`, `codex`, `gemini`, `kimi`, `agy`, `copilot`, `opencode` are on `PATH`, with versions; whether the shim is registered in each one's MCP configuration |
| `localgpt setup [agent…]` | new | registers the shim with each agent found — `claude mcp add world -- localgpt world mcp` for Claude Code, the `mcp_servers` table in Codex's `config.toml`, `mcpServers` in Gemini CLI's `settings.json` — and prints the maker's install line for one that is missing |

`setup` is the one install-shaped job nobody else does: it is what makes
"agents outside the window" work on a fresh machine. Everything else named
"install" belongs to a package manager, below.

### What it does not do

- **It does not install the desktop app.** Today there is nothing to install:
  the repository has tags to v0.4.1 and no GitHub Release, and `LocalGPT.app`
  has only ever been built locally with `apps/app-desktop/macos/build-app.sh`.
  A CLI one must `cargo install` in order to download an app is backwards —
  the people who want the app do not have cargo. The app installs through a
  release pipeline (next section), and once it exists a cask or a DMG link is
  the install.
- **It does not install the agents.** hatcheck's `docs/agents.md` puts the
  rule well: "hatcheck installs none of them: each comes from its maker."
  Nine `curl | bash` lines run from inside this binary would make LocalGPT
  the supply chain for nine vendors, the table is dated ("as of 2026-10-24")
  and goes stale, and every agent has a login flow an installer cannot finish.
  `doctor` detects, prints the maker's own command for what is missing, and
  links [hatcheck.dev/agents](https://hatcheck.dev/agents). The table lives
  there; this repository links to it and does not copy it.
- **It does not run anything in the background**, hold a login, or stand
  between an agent and its model. The shim is started by the agent, as MCP
  servers are, and exits with it.

## Shipping

### The desktop app, as a release

`build-app.sh` already signs with `APPLE_SIGNING_IDENTITY` and notarizes
through a `notarytool` keychain profile (`build-app.sh:12-18`); it has
never run in CI. hatcheck has the workflow this needs, and it is the template:

| | hatcheck | here |
|---|---|---|
| CLI binaries, installers, Homebrew formula, GitHub Release | `dist-workspace.toml` → cargo-dist writes `v-release.yml` from it; a `v0.2.0` tag releases; shell and PowerShell installers into `~/.local/bin` (where Claude Code's installer puts `claude`); formula to a tap | the same file for `localgpt`, `tag-namespace = "v"`, and the formula to a `localgpt-app/homebrew-tap`. `release.yml` keeps publishing the crates on the same tag |
| the desktop app | `desktop.yml`: a `desktop-v*` tag builds macOS (both architectures), Windows and Linux, signs and notarizes when the secrets are set, makes a draft release with the installers | `desktop.yml` wrapping `build-app.sh --dmg` on the macOS runner, `APPLE_*` secrets in Actions, a cask in the same tap. Linux (`apps/app-desktop/linux/localgpt.desktop` exists) and Windows follow when someone asks |
| the page | hatcheck.dev's install section | an **Install** page on localgpt.app: the app (cask or DMG), the CLI (one curl line), the agents (a link to hatcheck.dev/agents) |

A cask that installs the app from the release's DMG is zero lines of Rust and
is the install tool. The same tap can carry the CLI's formula, so
`brew install localgpt-app/tap/localgpt` and `brew install --cask localgpt`
are the two lines a Mac needs.

### Versions

| Version | What | When |
|---|---|---|
| **0.4.2** | what is in *Unreleased* now (Open World Format 0.4.0, the AWS-LC move), plus a deprecation line in `README.md` and `CHANGELOG.md`: 0.5.0 removes the assistant; `cargo install localgpt --version 0.4.2` keeps it. The crate has 62 downloads in the last 90 days: nobody should be surprised twice | now |
| **release pipeline** | `desktop.yml` for the app, the cask, the tap, the Install page: independent of the removal, and the first release of the app ever. cargo-dist for the CLI waits for 0.5.0's CLI — the 0.4.2 binary needs ONNX Runtime and eframe's system libraries on each of six targets, work the removal makes moot | two to three days |
| **0.5.0** | the removal (next section). The CLI is `world`, `world mcp`, `doctor`, `setup`. `localgpt-gen mcp-server` stays and is marked deprecated | about two weeks, most of it the app's panel and the docs |
| **0.6.0** | Gen's tools re-fronted over the ops API through the shim; the `localgpt-gen` binary's agent half retired; one MCP entry point | one to two weeks |

## What goes, what moves, what stays

Lines are `*.rs` under `crates/` and `bridges/` on 2026-10-26; the workspace
is 180,317.

| Crate or part | Lines | Fate |
|---|---|---|
| `core` | 37,950 | **goes**, after about 1,300 lines move out (below) |
| `cli` | 8,766 | `world.rs` (375) and `world_mcp.rs` (433) **stay**; `chat`, `tui`, `ask`, `daemon`, `memory`, `cron`, `hooks`, `bridge`, `session`, `cert`, `policy`, `sandbox`, `init`, `config`, `search`, `tool`, `mcp-server`, `desktop`, `gen3d`, `completion` and the `desktop` feature (eframe) **go**; `doctor` is rewritten |
| `server` (and `apps/web`, its UI) | 4,337 | **goes** |
| `cli-tools` (including `pty.rs`, 742) | 1,785 | **goes** — tier 2 of agents-outside-the-window was never built and herdr is the pane |
| `sandbox` | 1,542 | **goes** — see *What this gives up* |
| `bridge`, `bridges/cli` | 1,261 | **go** |
| `mobile-ffi` | 511 | **goes** — see *Mobile* |
| `gen/src/agent_loop.rs`, `local_llm.rs` | 2,231 | **go** — the conversation, slash commands, streaming, the in-process GGUF loader |
| `gen/src/mcp/*` | 6,932 | **stays, re-fronted** (0.6.0): the `Tool` impls are shells over `GenBridge` commands, and the commands are the world's vocabulary. `worldgen_tools.rs` and `terrain_tools.rs` are generators that would be rebuilt the next day if deleted with the loop |
| `gen/src/mcp_server.rs` | 251 | **folds into the shim** — same endpoints, over the live API, as agents-outside-the-window planned |
| `app/src/desktop/panel.rs`, `chat.rs`, `client_panel.rs`, the model menu, `mcp-server --connect`, the `local-llm*` features, `--prompt` | about 1,500 | **go** — two to three days to delete, then one to five days per mode to make prompt, document and song worlds `.world` packages on the live authority |
| `world-*`, `previs`, `previs-web`, `md`, `verse`, `relay` | | **unchanged**; none depends on `core` |
| `spacetime` | 1,437 | **unchanged**; its inference queue is a multiplayer question, not this one |

About 57k lines go: a third of the workspace.

### What moves out of `core` first

The shim and Gen's tool server stand on four things in `core` that are not
the harness. They move to `world-agent`, the crate every app and the CLI
already depend on and the one that owns the agent-facing side of a world:

| From `core` | Lines | To |
|---|---|---|
| `mcp/server.rs`, `transport.rs`, `tools.rs`, `mod.rs` — the stdio MCP server; `world_mcp.rs:64` calls `run_mcp_stdio_server` | ~680 | `world-agent::mcp` |
| the `Tool` trait and `ToolSchema` (from `agent/tools/mod.rs`; the safe tools around them go) | ~150 | `world-agent::mcp` |
| `herdr.rs` — the self-report state the pane reads | 416 | `world-agent::herdr` |
| `text.rs` (`ellipsize_chars`, `prefix_chars_with_ellipsis`) | 100 | `app` |
| the two `LOCALGPT_GEN_*` names in `env.rs` that the external-services tools read | | `gen` |

Everything else Gen and the app import from `core` — `Config` (25 sites),
`MemoryManager`, `providers`, `Role`, `skills`, `Agent`, `memory_tools`,
`local_embeddings_cached`, `commands` — is the loop's and goes with it.

### One MCP entry point

Three exist today and the docs name all three: `localgpt-gen mcp-server`
(`website/content/docs/gen/mcp-server.md`, with snippets for Claude Desktop,
Claude Code, Codex, Gemini CLI, Zed and Cursor), `localgpt-app mcp-server
--connect` (the relay the prompt panel's CLI backend used), and `localgpt
world mcp`. After 0.6.0 there is one: the shim, with Gen's vocabulary behind
it. Until then the shim's eight tools (`submit`, `undo`, `log`, `tips`,
`goto`, `verify`, `screenshot`, `selection`) mean an agent writes ops by hand from the world's
`AGENTS.md` — workable, and the reason the re-fronting is sequenced before
the retirement rather than after.

### Mobile

`mobile-ffi`, `apps/apple` (7,864 Swift lines) and `apps/android` are the
harness on phones: `LocalGPTClient` wraps `AgentHandle`. The Apple app also
holds the on-device authoring §13.3 counts as built
(`AppleFoundationModelsService.swift`, the `World*Service.swift` files) and
the parallel world format §13.6 already marks as debt. worldwalk is the
format's viewer on Apple and Android and adds no semantics of its own.

This RFC removes the FFI crate and proposes that the two app trees leave this
repository with it: the viewer is worldwalk's, and on-device authoring, if it
is wanted, is a worldwalk question. That is a decision, listed under *Not
decided* because it is not this document's to make alone.

### Docs and CI

- `website/content/docs/`: `claw`, `cli-ask`, `cli-chat`, `cli-daemon`,
  `cli-memory`, `configuration`, `heartbeat`, `http-api`, `memory-system`,
  `openclaw-compatibility`, `policy`, `sandbox`, `skills`, `tools`,
  `upgrade-v0.2` are assistant docs and are archived, not ported.
  `installation`, `quick-start`, `intro`, `architecture`, `cli-commands` are
  rewritten; `app`, `worlds`, `ecosystem`, `gen/`, `md/`, `verse/` stay.
  `README.md` opens with "An AI agent that lives where your data lives",
  which becomes false; the second half of that sentence is the product.
- `.github/workflows/ci.yml`: the ONNX Runtime action leaves all three jobs;
  the mobile cross-compile checks go; the `features` job keeps `md llm`,
  `verse llm`, `gen feathers` and `verse ml` (§13.3's local tier is not the
  harness) and loses `gen local-llm`. `deny.toml` loses the `daemonize`
  ignore.
- crates.io: `localgpt-core`, `-server`, `-sandbox`, `-cli-tools` and
  `-bridge` publish no further versions. Nothing is yanked.

## The command's name

`lg` as a second name for `localgpt` is proposed and **declined for now**:

- lazygit's README tells its users to add `alias lg='lazygit'`, and a shell
  alias shadows a binary on `PATH`. The one path a short name serves — a
  person typing — is the one it breaks for those users. MCP configurations
  and scripts resolve `PATH`, not aliases, and gain nothing from two letters.
- `bin/lg` has been this repository's build-and-release router since
  2026-10-26 (`bin/lg build app`, `bin/lg web deploy world`). One name for
  the developers' router and the users' command is a confusion the CLAUDE.md
  would have to explain on every line.
- After 0.5.0 the command is typed rarely: `setup` once, `doctor` sometimes,
  `world …` mostly from scripts and agents. Brevity is worth little here.
- It is reversible in one file — hatcheck ships `hat` as
  `crates/hatcheck/src/bin/hat.rs` beside `hatcheck` — while taking a
  two-letter name back from users is not.

## What this gives up

- **Memory, heartbeat, cron, dreaming, the wiki, active recall.** The 10-04
  RFC's §3 moat. Unbuilt against worlds; the agents carry memory and
  schedules now.
- **The kernel sandbox** (Landlock, Seatbelt). The one piece with no exact
  outside equivalent on macOS, and the one agents-outside-the-window flagged
  as "an invitation to run arbitrary commands from a 3D editor". It can come
  back as a crate the day an outside agent asks for it; nothing in the world
  path calls it.
- **`localgpt mcp-server`**, memory and web tools for other agents. The
  agents have both.
- **The daemon, the HTTP and OpenAI-compatible APIs, the Telegram and other
  bridges, the outbox, eleven providers, OpenClaw compatibility.** The
  assistant. 0.4.2 is its last release, and it stays installable by version.
- **The mobile apps**, unless worldwalk takes their authoring half.

## Not decided

- Mobile: whether `apps/apple` and `apps/android` leave with `mobile-ffi`, or
  one of them is kept as a viewer here rather than in worldwalk.
- How a document's regions are authored once there is no model menu. The
  app's document mode is the one that most wants a model (§13.3), and
  `crates/app/src/authoring.rs` runs the app's own; whether that becomes an
  outside agent's job through the shim, or is dropped, is open. It is the one
  place the rule that the app never manages agent configuration conflicts
  with something the app does today.
- Whether the `localgpt-gen` crate stays on crates.io as a library once its
  binary's agent half is gone, or the app's shell stops being a published
  crate.
- Whether `setup` writes a *per-world* registration (an agent's config names
  the folder) or one `localgpt world mcp` that finds the open world — the
  10-04 RFC's open question, now the only thing `setup` has to decide.

## Status

- [ ] 0.4.2 released with the deprecation line (the lines are in `README.md`
      and `CHANGELOG.md` since 2026-10-27; the tag is the release)
- [x] `.github/workflows/desktop.yml` and `packaging/homebrew/cask.sh`,
      2026-10-27 — written, not yet run; the first `desktop-v` tag is the
      first run
- [ ] the first release of `LocalGPT.app`; the tap; the Install page
- [ ] cargo-dist for the 0.5.0 CLI
- [ ] 0.5.0: the removal; `doctor` and `setup`
- [ ] 0.6.0: Gen's tools behind the shim; one MCP entry point
