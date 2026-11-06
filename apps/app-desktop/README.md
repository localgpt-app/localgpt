# LocalGPT desktop app

The one LocalGPT desktop app (`crates/app`, binary `localgpt-app`): Gen's
world in a Bevy viewport with the app's own panels around it — egui, drawn
in Rust (`crates/app/src/shell`) — where documents, songs and worlds open as
places and the person's own coding agents run in terminals beside them. A
paired phone gets a web page of its own (`crates/app/ui`), which the release
binary carries.
`docs/world-strategy.md` §13.1 records why it is one app. It replaces the
`LocalGPT Gen.app` bundle; `localgpt-gen` itself stays a binary (the MCP
server and relay, headless generation, `--host`/`--join`).

- **Open…** at the top of the window takes a Markdown document, a song or a
  world. A document opens with an editor pane beside the world: it saves and
  the world rebuilds as you type, and scenery a model authored comes from
  MD's sidecar beside the file, written by `localgpt-md --generate` or by an
  agent. A song opens as the world Verse builds for it, playing, with the
  world performing it. A `.world` package opens as a live canvas: the window
  holds its authority, agents edit it through its API, and the history panel
  draws the branches. The same from the command line: `--md notes.md`,
  `--song track.mp3`, `--world place.world`.
- **Terminals.** Each tab is your login shell, started in the folder of what
  is open with `LOCALGPT_WORLD_DIR` set, so `localgpt world …` and
  `localgpt world mcp` find the world. Run whichever agent you like in it;
  the app runs no model, holds no key, knows no agent by name and reads no
  agent's configuration.
- **The login shell's `PATH`.** Opened from Finder, the app reads your login
  shell's `PATH`, so programs installed with Homebrew, npm or into
  `~/.local/bin` are found.
- **Collaborative sessions** are hosted and joined from `localgpt-gen` with
  `--host` and `--join`; the app has no controls for them.
- **Logs** go to `~/.local/state/localgpt/logs/localgpt-app.log` when there
  is no terminal.

While a terminal or the editor has focus, keys don't reach the world; click
the viewport and WASD moves you again.

## Build the macOS app

```bash
apps/app-desktop/macos/build-app.sh            # release build → dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-<version>.dmg
apps/app-desktop/macos/build-app.sh --debug    # quick bundle of the debug build
open apps/app-desktop/dist/LocalGPT.app
```

The release build uses the workspace's fat LTO profile, so the first one is
slow. It builds the app with no optional features; `APP_FEATURES` passes a
feature list to cargo. The script renders the icon from `website/static/logo/localgpt-icon.svg`
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

The CC0 asset pack is not in the bundle; `scripts/fetch-assets.sh` fetches it
once per machine for every LocalGPT app. The four CC0 starter tracks are, in
`Resources/starter`, so a first launch opens a song: the script takes them
from the asset root `LOCALGPT_STARTER_MUSIC` names, a `localgpt-world-assets`
checkout beside the repository, or the pack, and warns when it finds none.

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
  yet. The panels run there as on macOS, though they are new and less tried.
- **A proper app icon.** QuickLook flattens the SVG onto white; a real macOS
  icon wants a designed rounded-square tile.
