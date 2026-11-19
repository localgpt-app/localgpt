# The iPad player spike

The native half of the mobile question raised on 2026-11-17: can a song
world play well on an iPad when it is drawn and sounded by the desktop app's
own crates, `localgpt-world-bevy` and `localgpt-world-audio`, rather than by
a browser or by a second implementation in Swift?

The player opens one `.world` package from its bundle. The default is
`verse-amber-drift`: 178 entities, 40 glTF models and a 58-second song. It
draws the package through `world-bevy` with the desktop's camera: HDR,
TonyMcMapface, bloom, 4× MSAA, and the world's shadowed sun and fog. It runs
the world's spin and bob behaviors and its song-driven modulations, and it
plays the song through `world-audio` (kira on cpal) in the playback audio
session. Nothing from Gen, the shell or the terminals is in it.

## What it asks

Each question passes or fails on what the screen shows, the log, or
`Documents/player-metrics.csv`:

1. **Frame rate.** Does it hold steady for ten minutes at the iPad's native
   resolution, with the camera drifting? Measure it in Full and in Lite (no
   bloom, MSAA or shadows).
2. **Heat.** Does the thermal state stay nominal or fair over those ten
   minutes, and how much battery do they use?
3. **First launch.** How long until the first frame, until every glTF has
   loaded, and until every pipeline has compiled? The desktop app's first
   launch took 10–16 s to draw its world.
4. **The audio session.** Does the song play with the silent switch on, keep
   playing with the screen locked, come back after Siri or a call, and
   survive AirPods connecting and disconnecting?
5. **Touch.** Does one finger turn, two fingers pinch and pan, and a double
   tap reset? Does the layout hold in both orientations, in Split View and
   in Stage Manager?

## Running it

It needs Xcode, the `aarch64-apple-ios` and `aarch64-apple-ios-sim` Rust
targets, and the `localgpt-world-assets` checkout beside this repository
(or `LOCALGPT_WORLD_ASSETS`) for the world's files.

```bash
cd spikes/ipad-player
cargo run -- sim                   # the iPad simulator nearest a 12.9-inch iPad Pro
cargo run -- device --provision    # a paired iPad, the first time
cargo run -- device --console      # after that; the player's output here
```

`src/bin/bundle.rs` builds the player, puts it in `Player.app` with the
world, signs it and starts it. On a device it signs with an Apple
Development certificate of the team that holds the `app.localgpt` App ID,
never another team's. `--provision` has Xcode register the iPad and make the
certificate and profile when they are missing. `--console` keeps the
player's output in this terminal, on either; the simulator would not write
it to a file. To fetch the device's metrics:

```bash
xcrun devicectl device copy from --device <udid> --domain-type appDataContainer \
  --domain-identifier app.localgpt.spike.player \
  --source Documents/player-metrics.csv --destination player-metrics.csv
```

The buttons along the bottom are Pause, Lite (or Full) and Reset view. The
readout in the top left shows the frame rate and the worst frame of the last
half second, the thermal state and the battery, the song's position and
session, and the first draw's steps.

## Results

Not yet on the iPad, whose run waits for its signing. On the simulator
(iPad Pro 13-inch (M5), iOS 27.0):

- **iOS 27 will not launch winit 0.30 as built.** An app linked against the
  iOS 27 SDK must use scenes ("UIScene life cycle is required for apps
  built with this SDK"), and winit 0.30, Bevy 0.20's, has none. `assemble`
  marks the binary as linked against the iOS 26 SDK (`vtool`), and iOS 27
  launches it. That does for a spike; an app that ships needs a winit with
  scenes before uploads must use the iOS 27 SDK.
- **The window has to ask for the screen.** Without
  `WindowMode::BorderlessFullscreen`, winit's window was 1280×720 points in
  a corner; with it, the world fills the screen, 2064×2752 pixels at 2×.
- **First launch.** 0.7 s to the first frame, 3.0 s until the 40 glTF
  models had loaded, 8.2 s until all 33 pipelines had compiled. With the
  shader cache warm: 1.6 s, 2.3 s and 2.4 s.
- **Sound.** The playback session was granted and the song played through
  kira. The silent switch, the lock screen, Siri, calls and AirPods wait for
  the iPad.
- **Frame rate.** 7–15 fps, which says nothing about the iPad's own GPU.
- **Size.** The player is 158 MB with line tables, 79 MB stripped and about
  26 MB compressed; the world adds 19 MB.
- **Bevy 0.20's buttons.** `Button` is a picking widget now, so the buttons
  are nodes with the deprecated `Interaction`, which touches still update.
