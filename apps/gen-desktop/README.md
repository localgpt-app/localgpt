# LocalGPT Gen desktop app

LocalGPT Gen packaged as an app you double-click: no terminal, no
`cargo install`. It's the same `localgpt-gen` binary. When it starts without a
terminal (or with `--desktop`), it runs in **desktop mode**:

- **Prompt panel.** Prompts are typed in a panel on the right of the window
  instead of the terminal REPL. The panel streams the model's replies, shows
  each tool call as it runs, queues prompts while the agent is busy, and
  suggests a few starter worlds. It lives in `crates/gen/src/desktop/`.
- **Model menu.** Lists the models this machine can use right now: installed
  CLI backends (Claude CLI, Gemini CLI, Codex) and models pulled into a local
  Ollama. Switching lasts for the session; `agent.default_model` in the
  config file sets the default.
- **First-run help.** If the configured model is a CLI backend that isn't
  installed, the panel says so and points at the model menu instead of failing
  silently. Startup errors (a missing API key, say) appear in the panel, and
  the window stays open so you can read them.
- **Login-shell PATH.** Apps opened from Finder get launchd's minimal `PATH`,
  which hides `claude`, `gemini`, and `codex` when they're installed with
  Homebrew, npm, or into `~/.local/bin`. Desktop mode asks your login shell for
  its `PATH` first, so those CLIs are found.
- **Collaborate.** A section under the model menu hosts or joins a
  collaborative session without command-line flags. *Host* picks a name,
  port, and PIN or open session, then shows the PIN and how many guests are
  connected. *Join* browses the LAN (or takes an address), pairs with the
  PIN, and opens a viewer window with its own prompt panel for asking the
  host's agent. Guests' prompts run on a scene-only agent, as with `--host`.
- **Logs** go to `~/.local/state/localgpt/logs/gen-desktop.log`.

Keys: **F2** shows or hides the panel, **Enter** in the 3D view jumps to the
prompt box, **Esc** leaves it so WASD moves you again. While you type, keys
don't reach the world. Terminal mode gets the same panel, hidden until F2.

## Build the macOS app

```bash
apps/gen-desktop/macos/build-app.sh            # release build → dist/LocalGPT Gen.app
apps/gen-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-Gen-<version>.dmg
apps/gen-desktop/macos/build-app.sh --debug    # quick bundle of the debug build
open "apps/gen-desktop/dist/LocalGPT Gen.app"
```

The release build uses the workspace's fat LTO profile, so the first one is
slow. The script renders the icon from `website/static/logo/localgpt-icon.svg`,
writes `Info.plist` (bundle id `app.localgpt.gen`, plus the local-network
usage string and Bonjour service that collaborative sessions need), and
ad-hoc signs the bundle.

To try desktop mode without bundling: `cargo run -p localgpt-gen -- --desktop`.

## Linux launcher

`linux/localgpt-gen.desktop` adds Gen to the app menu, starting it in desktop
mode. It hasn't been tested on a Linux desktop yet.

```bash
cargo install localgpt-gen   # puts localgpt-gen in ~/.cargo/bin; make sure that's on PATH
install -Dm644 apps/gen-desktop/linux/localgpt-gen.desktop \
  ~/.local/share/applications/localgpt-gen.desktop
install -Dm644 website/static/logo/localgpt-icon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/localgpt-gen.svg
```

## Not done yet

- **Signing and notarization.** The bundle is ad-hoc signed, so other Macs
  will quarantine a downloaded copy. Shipping it needs a Developer ID
  Application certificate (`codesign --options runtime --sign "Developer ID
  Application: …"`), `xcrun notarytool submit … --wait`, and
  `xcrun stapler staple`.
- **Windows and Linux packages.** Desktop mode itself works on both, but
  there's no Windows installer or Linux AppImage yet. On Windows,
  `localgpt-gen` is a console program, so launching it from Explorer opens a
  console window and stays in terminal mode. Use `--desktop` in a shortcut, or
  build a GUI-subsystem variant.
- **A proper app icon.** QuickLook flattens the SVG onto white; a real macOS
  icon wants a designed rounded-square tile.
- **Tool calls from CLI backends.** With Claude CLI, Gemini CLI, or Codex, tools
  run through the MCP relay, so the panel shows the streamed text but not
  each tool call.
