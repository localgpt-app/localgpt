# LocalGPT desktop app

The one LocalGPT desktop app (`crates/app`, binary `localgpt-app`): a world
viewport with Gen's prompt panel, where documents, songs and worlds open as
places. `docs/world-strategy.md` §13.1 records why it is one app.

- **Open…** at the top of the window takes a Markdown document, a song or a
  world. A document opens with an editor pane beside the world: it saves and
  the world rebuilds as you type, and the app's model authors each section's
  place in the background. A song opens as the world Verse builds for it,
  playing, with the world performing it. The same from the command line:
  `--md notes.md`, `--song track.mp3`, `--world place.ron`.
- **Model.** The prompt panel's model menu, shared with Gen
  (`~/.local/state/localgpt/gen-settings.json`); document authoring uses the
  same model. No config file is read or written.
- **Logs** go to `~/.local/state/localgpt/logs/localgpt-app.log` when there
  is no terminal.

Keys: **F2** the prompt panel, **F3** the document pane, **F1** Gen's
inspector.

## Build the macOS app

```bash
apps/app-desktop/macos/build-app.sh            # release build → dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-<version>.dmg
apps/app-desktop/macos/build-app.sh --debug    # quick bundle of the debug build
open apps/app-desktop/dist/LocalGPT.app
```

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
