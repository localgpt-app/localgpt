# LocalGPT desktop app

The one LocalGPT desktop app (`crates/app`, binary `localgpt-app`): a world
viewport with Gen's prompt panel, where documents, songs and worlds open as
places. `docs/world-strategy.md` §13.1 records why it is one app. It replaces
the `LocalGPT Gen.app` bundle; `localgpt-gen` itself stays a binary (the MCP
server, headless generation, `--host`/`--join`).

- **Open…** at the top of the window takes a Markdown document, a song or a
  world. A document opens with an editor pane beside the world: it saves and
  the world rebuilds as you type, and the app's model authors each section's
  place in the background. A song opens as the world Verse builds for it,
  playing, with the world performing it. The same from the command line:
  `--md notes.md`, `--song track.mp3`, `--world place.ron`.
- **Prompt panel.** Prompts are typed in a panel on the right: replies stream
  in, each tool call shows as it runs, and prompts sent while the agent is
  busy wait in a queue. The toolbelt is the `core` profile unless `--tools`
  or Gen's settings pick `standard` or `full`.
- **Model menu.** Lists the models this machine can use right now: installed
  CLI backends (Claude CLI, Gemini CLI, Codex), models pulled into a local
  Ollama, and every GGUF in the model folder LocalGPT's apps share
  (`~/.local/share/localgpt/models/llm`), which the app runs in-process as
  `gguf/<name>` when built with `local-llm-metal` (the macOS bundle is) or
  `local-llm`. The choice is remembered in Gen's settings
  (`~/.local/state/localgpt/gen-settings.json`), and document authoring uses
  the same model. No config file is read or written.
- **First-run help.** If the chosen model is a CLI backend that isn't
  installed, the panel says so and points at the model menu. Opened from
  Finder, the app reads your login shell's `PATH`, so `claude`, `gemini` and
  `codex` installed with Homebrew, npm or into `~/.local/bin` are found.
- **Collaborate.** A section under the model menu hosts or joins a
  collaborative session without command-line flags: *Host* picks a name,
  port, and PIN or open session, then shows the PIN and how many guests are
  connected; *Join* browses the LAN (or takes an address) and pairs with the
  PIN. Guests' prompts run on a scene-only agent, as with `localgpt-gen
  --host`. With a CLI backend, the app also serves an MCP relay so the
  backend's tool calls reach the open window (`localgpt-app mcp-server
  --connect`).
- **Logs** go to `~/.local/state/localgpt/logs/localgpt-app.log` when there
  is no terminal.

Keys: **F2** the prompt panel, **F3** the document pane, **F1** Gen's
inspector. **Enter** in the 3D view jumps to the prompt box and **Esc** leaves
it, so WASD moves you again; while you type, keys don't reach the world.

## Build the macOS app

```bash
apps/app-desktop/macos/build-app.sh            # release build → dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-<version>.dmg
apps/app-desktop/macos/build-app.sh --debug    # quick bundle of the debug build
open apps/app-desktop/dist/LocalGPT.app
```

The release build uses the workspace's fat LTO profile, so the first one is
slow. On Apple Silicon it builds with `local-llm-metal`, so the bundle can run
a local GGUF on the GPU; `APP_FEATURES` overrides the feature list (empty for
none). The script renders the icon from `website/static/logo/localgpt-icon.svg`
and writes `Info.plist` (bundle id `app.localgpt.desktop`, with the
local-network usage string and Bonjour service collaborative sessions need).

Without credentials the bundle is ad-hoc signed and runs on the Mac that
built it. For a download anyone can open, sign with a Developer ID and
notarize — the script does both when given them:

```bash
xcrun notarytool store-credentials localgpt-notary \
  --apple-id <apple id> --team-id <team id> --password <app-specific password>

APPLE_SIGNING_IDENTITY="Developer ID Application: <name> (<team id>)" \
APPLE_NOTARY_PROFILE=localgpt-notary \
  apps/app-desktop/macos/build-app.sh
```

The shared downloads — the CC0 asset pack and, for local models, the GGUF —
are not in the bundle; `scripts/fetch-assets.sh` and `scripts/fetch-model.sh`
fetch them once per machine for every LocalGPT app.

## Release

`.github/workflows/desktop.yml` builds the bundle on GitHub's macOS runners,
Apple silicon and Intel, with this script (`--dmg`), so a release and a
bundle made on a Mac are the same build. A `desktop-v<version>` tag — the
workspace's version, as `Cargo.toml` has it — makes a draft release holding
`LocalGPT-<version>-arm64.dmg`, `LocalGPT-<version>-x86_64.dmg` and
`localgpt.rb`, the Homebrew cask rendered with their checksums
(`packaging/homebrew/cask.sh`). A tag with a hyphen after the version
(`desktop-v0.5.0-beta.1`) is a pre-release. Run by hand, the workflow keeps
the files as artifacts and makes no release.

```bash
git tag -s desktop-v0.4.2 -m desktop-v0.4.2 && git push origin desktop-v0.4.2
```

Then, by hand: check the draft and publish it, and put `localgpt.rb` in the
tap (`localgpt-app/homebrew-tap`, as `Casks/localgpt.rb`), after which
`brew install --cask localgpt-app/tap/localgpt` installs the app. The cask's
URLs are the release's, so the release is published first.

Signing and notarization run when the secrets are set in the repository
(Settings > Secrets and variables > Actions); without them the release is
ad-hoc signed and opens only with a Gatekeeper override:

| Secret | |
|---|---|
| `APPLE_CERTIFICATE` | the Developer ID Application certificate as a base64 `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | its password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: <name> (<team id>)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | the Apple ID, an app-specific password and the team, for notarization; all three or none |

The workflow imports the certificate into a keychain of its own, makes the
`notarytool` profile from the three notarization secrets, and runs this
script as the section above does.

## Linux launcher

`linux/localgpt.desktop` adds the app to the app menu. It hasn't been tested
on a Linux desktop yet.

```bash
cargo install --path crates/app   # puts localgpt-app in ~/.cargo/bin; make sure that's on PATH
install -Dm644 apps/app-desktop/linux/localgpt.desktop \
  ~/.local/share/applications/localgpt.desktop
install -Dm644 website/static/logo/localgpt-icon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/localgpt.svg
```

## Not done yet

- **The signing path has not been run** — no Developer ID is set up where
  it was written — and neither has the workflow: the first `desktop-v` tag
  is its first run.
- **Windows and Linux packages.** No Windows installer or Linux AppImage
  yet, and `local-llm-metal` is macOS-only: those builds need `local-llm`
  (CPU) or a GPU backend chosen per platform.
- **A proper app icon.** QuickLook flattens the SVG onto white; a real macOS
  icon wants a designed rounded-square tile.
- **Tool calls from CLI backends.** With Claude CLI, Gemini CLI or Codex,
  tools run through the MCP relay, so the panel shows the streamed text but
  not each tool call.
