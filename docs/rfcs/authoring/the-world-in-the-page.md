# The world in the page: what Bevy buys

**Status:** measured in part, not decided (2026-10-31). The page's side is
measured, headless; the app's side (Bevy's frames, and WKWebView's own
inside the app) waits on an unlocked screen (*The app's half*). Asks of
[the-page.md](the-page.md)'s "the world stays native" what
[mobile-remote.md](mobile-remote.md) raises: the phone is to draw a live
canvas's world with the format's web viewer (its first step sends this
window's frames instead), so this same page would hold both renderers. The spike is in
the worktree `target/spike/wt`, on the branch `spike/world-in-the-page`
(`spike/`, `crates/app/src/spike.rs`, `crates/app/ui/src/spike.ts`); none of
it is meant for main.

## The question

The page is web and the world is Bevy, behind a hole the page leaves.
Should the world move into the page, drawn by `world-viewer.js`, the
renderer that localgpt.world and localgpt.md use, and the phone will for a
live canvas?

"Pure web" cannot mean a browser tab here. The agents are processes in PTYs
(`crates/app/src/webui/term.rs`), so the native host stays, and with it the
files, the local model on Metal, Verse's analysis, kira and FunDSP, the live
API and the rooms. None of those is Bevy's. The question is only who draws
the world: Bevy, or three.js in the page.

## What only Bevy gives

1. **Frames on a thread of their own.** Bevy draws into its own Metal layer
   and Core Animation lays the page over it, so nothing the page does can
   cost the world a frame. In the page, three.js runs on the web view's main
   thread with React, the timeline, CodeMirror and every terminal's parser:
   the panels this app is built around. (Measured below: it pays only under
   an overload.)
2. **The scene in the process of everything that touches it.** Gen
   (63,743 lines) works on the ECS: its tools, avian behind
   `gen_set_physics` and `gen_add_collider`, the canvas's gestures
   (`canvas_edit.rs`), the shots' thumbnails rendered off-screen
   (`offscreen.rs`), `GET /screenshot` (`live.rs`), and the song's clock in
   the frame it is heard. In the page each becomes a message or a second
   copy in Rust. This is mostly the cost of moving, and it shrinks: the
   outliner and the inspector read the document, not the scene, and
   `LiveWorld::submit` needs no Bevy.
3. **Perhaps 120 Hz.** Bevy presents at the display's rate. Safari holds
   `requestAnimationFrame` near 60 on a ProMotion display by default;
   whether WKWebView does in this app is the app's half's first question.

## What looks like Bevy's and is not

- **Rendering headroom.** A `.world` must draw in three.js, RealityKit and
  on Android, so what only Bevy can draw cannot be in the format;
  `crates/world-export/js/world-viewer.js:44` already calibrates Bevy's
  physical light units down to three's. Headroom buys a desktop that looks
  better than its share link, which in an authoring tool is closer to a bug.
- **Physics.** `ext-physics` has a JS reference (`openworldformat/physics`).

## What the page gives

1. **One renderer.** localgpt.world and localgpt.md draw the world with
   `world-viewer.js`, and the phone's live canvas will. Nothing checks that
   Bevy agrees with it
   ([world-strategy.md](../../world-strategy.md):198), every format feature
   is written twice in two repositories, and audio already is: FunDSP, and
   WebAudio oscillators in the viewer.
2. **No hole.** Each platform's trick for it goes (`webui/mac.rs`'s
   hit-test view; WebView2 cut with `SetWindowRgn`; webkit2gtk's child
   views, which nest on X11 only, so Wayland comes back), so does "input
   belongs to whichever side was clicked last", and
   gizmos, a drag from the outliner into the world and a context menu over
   it become React instead of `bevy_ui` and messages.
3. **The app and the site are one program**, which settles the-page.md's
   open question about the panels on localgpt.world.

## The test

**The world** is the song world the app makes of the starter track *Amber
Drift* (`localgpt-app --song`): 178 entities, 40 of them models from the
CC0 pack, 15 modulated by the song's curves, 25 with behaviors. three.js
draws it in 96 draw calls and 5.1 million triangles, most of them the pack's
scanned rocks and plants.

**The page** is the production build (`vite build`) in the song's layout:
the world in the hole (`?spike=web`) or the hole left empty, three shell
tabs open with one in view, and the timeline playing.

**The load** is `spike/load.py` in each tab, delivered as the app delivers
it: per tab, 120 times a second, through `window.__lg.recv`.

| load | what it is | per tab |
|---|---|---|
| agent | an Ink-style live region, 14 lines redrawn ten times a second over a streamed answer: Claude Code or Codex at work | ~7 KB/s |
| build | compiler lines, with a burst of 400 every 5 s: a shell tab running a build | ~14 KB/s |
| flood | coloured log lines: `cat` of a big log, a verbose test run | ~0.9 MB/s |
| deluge | the flood's lines, four times as fast: an overload, to find where the page gives way | ~3.5 MB/s |

**Measured:** `requestAnimationFrame`'s intervals for 30 s, after 14 s of
warm-up; then one world frame, timed on the page's thread back to back,
alone and with a one-pixel read so the GPU's work is waited for.

**Where:** an M2 Max (12 CPU cores, 30 GPU cores, 32 GB) on macOS 26.6.
Headless WebKit 27.2 (Playwright's build), the macOS host's engine, and
headless Chromium 156, the engine of WebView2 and so of the Windows host,
both drawing on the GPU (WebKit's "Apple GPU", Chromium's ANGLE on Metal).
The page is 1440 × 860 at 2×, the app's window; the hole is 960 × 595.

## What the page's side showed

Each row is 30 s of the page's frames. "World" is whether three.js drew
the world in the hole; the last column is one frame of it on the page's
thread, and in brackets the same frame waiting on the GPU.

| engine | terminals, all three | world | fps | frame ms p50 / p95 / p99 | worst ms | one world frame, ms |
|---|---|---|---|---|---|---|
| WebKit | idle | no | 60 of 60 | 17 / 18 / 18 | 19 | |
| WebKit | idle | yes | 60 of 60 | 17 / 18 / 18 | 19 | 0.3 (12.9) |
| WebKit | agent, 21 KB/s | no | 60 of 60 | 17 / 19 / 19 | 19 | |
| WebKit | agent, 21 KB/s | yes | 60 of 60 | 17 / 18 / 19 | 25 | 0.3 (12.6) |
| WebKit | build, 44 KB/s | no | 60 of 60 | 17 / 18 / 19 | 19 | |
| WebKit | build, 44 KB/s | yes | 60 of 60 | 17 / 19 / 19 | 20 | 0.3 (11.7) |
| WebKit | flood, 2.6 MB/s | no | 60 of 60 | 17 / 19 / 21 | 51 | |
| WebKit | flood, 2.6 MB/s | yes | 60 of 60 | 17 / 19 / 20 | 22 | 0.3 (6.0) |
| WebKit | overload, 10.5 MB/s | no | 60 of 60, 10.7% late | 17 / 31 / 37 | 44 | |
| WebKit | overload, 10.5 MB/s | yes | 60 of 60, 4.3% late | 17 / 25 / 30 | 34 | 0.3 (6.4) |
| Chromium | idle | no | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | |
| Chromium | idle | yes | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | 0.4 (13.4) |
| Chromium | agent, 21 KB/s | no | 60 of 60 | 16.7 / 16.8 / 16.8 | 17 | |
| Chromium | agent, 21 KB/s | yes | 60 of 60 | 16.7 / 16.8 / 16.8 | 17 | 0.7 (8.8) |
| Chromium | build, 44 KB/s | no | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | |
| Chromium | build, 44 KB/s | yes | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | 0.7 (9.3) |
| Chromium | flood, 2.6 MB/s | no | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | |
| Chromium | flood, 2.6 MB/s | yes | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | 0.5 (14.9) |
| Chromium | overload, 10.5 MB/s | no | 60 of 60 | 16.7 / 16.7 / 16.8 | 33 | |
| Chromium | overload, 10.5 MB/s | yes | 60 of 60 | 16.7 / 16.7 / 16.8 | 17 | 0.5 (14.0) |

WebKit's clock is coarsened to whole milliseconds, so its 16.7 ms frames
read as 17, and its p95 of 18 or 19 is that rounding and a millisecond of
jitter, not lateness.

- **The page never dropped the world's frames.** In both engines, under
  the agents, the build and the flood, the page held 60 of 60 with the
  world in it, as it did without. The worst frame with the world was 25 ms,
  once in 1,800, in WebKit.
- **The world costs the page's thread almost nothing.** One frame of it
  (behaviors, modulations, matrices, 96 draw calls) takes 0.3 to 0.7 ms of
  a 16.7 ms frame. The WebGL work itself runs in the browser's GPU process,
  so the world does not wait behind a terminal's parser, nor the parser
  behind the world.
- **It costs the GPU 6 to 15 ms a frame**, depending on what the tour has in
  view: 5.1 million triangles, soft shadows, 4× MSAA at 2×. That fits 60 Hz
  with room and does not reliably fit 120 Hz (8.3 ms) on this GPU. Bevy draws the same
  triangles; whether it fits 120 is for the app's half to say.
- **WebKit gives way at about 10 MB/s.** The overload rows are no agent:
  `spike/load.py deluge`, five hundred times what three agents write. There
  WebKit's frames went late with the world and without it (10.7% of them
  with the hole empty, 4.3% with the world in it); Chromium still held.
  This is where Bevy's thread of its own pays: behind the page, the world
  would have kept its frames while the terminals lagged; in the page it
  lags with them.
- **Nothing paces a terminal.** Each tab's reader reads 64 KB at a time
  into an unbounded channel, and every frame sends all of it to the page
  (`webui/term.rs`, `pump_terms`). An agent that writes faster than the page
  parses grows a backlog without bound, whoever draws the world. xterm.js's
  own advice is watermarks: stop reading the PTY while the page is behind.

So of what only Bevy gives, the first, frames on a thread of their own, is
real, and on this machine it showed only under an overload no agent makes.
What is left is 120 Hz, unmeasured, and the scene in-process, which is the
cost of moving rather than a reason to stay.

## What this suggests

If the app's half agrees, the world can move into the page. Frames do not
decide it; the question becomes one renderer against the cost of moving,
and one renderer is worth more to this product: what the author sees is
what the link and the previs page show, every format feature is
written once, and the hole goes with its three platforms' tricks. The cost
of moving:

- **The scene's tools.** Gen's tools act on the ECS through its command
  channel. The app's agents already go through the live API's ops, which
  need no Bevy; Gen's own binary would keep its window or follow.
- **The clock.** Bevy reads the song's position in the frame it is heard.
  The page would draw at the app's clock, extrapolated between messages as
  the timeline already does, or play the song itself through WebAudio; a
  beat must land within a frame either way.
- **Screenshots, the shots' thumbnails and the phone's frames** (which
  mobile-remote.md's first step reads back from Bevy's window) come from
  the page's canvas in the app, and from the viewer in a headless browser
  without it; the `viewer` CI job already renders that way.
- **The world's own interface** (HUD, labels, signs, dialogue, the
  selection box, all `bevy_ui` today) becomes DOM over the canvas.
- **Physics** comes from `ext-physics`'s JS reference; Gen's avian tools are
  Gen's, not the format's.
- **A smaller GPU** pays the world's GPU cost several times over. That is
  true of both renderers, but three.js has fewer ways to cut it built in:
  no automatic batching or instancing, no culling on the GPU.

## The app's half

The same runs inside the app (`spike/run.sh`) measure what headless runs
cannot: Bevy's frames under the same loads, WKWebView's own inside the
app's window, with the app's real messages (the clock, the scene, the
history), and the rate each keeps on a ProMotion display. The runner opens
the app once per run with `--song`. Its data, state and cache go to the run's
own folder, so nothing it does reaches the real workspace or what the app
reopens. The page comes from a local server (`LOCALGPT_APP_UI_URL`), the
song is muted at its first view, and the page posts its frames back while
Bevy logs its own (`LOCALGPT_SPIKE_FRAMES`). In `web` runs Bevy's world
camera is off and its loop is held to 120 Hz, so the app's own systems cost
what they cost when Bevy draws.

It did not run on 2026-10-31: the Mac had locked. A locked session hides
every window, so the page reported `visibilityState: hidden` and
`requestAnimationFrame` never fired; Bevy kept logging frames, but with no
screen to show them on they say nothing about smoothness. Headless browsers
are exempt, which is why the page's side above is headless. To run it, at
an unlocked screen and with the window left in front for about ten minutes,
from the spike's worktree (the binary, `target/debug/localgpt-app-spike`,
and the page are built):

```bash
spike/run.sh ../../../../localgpt-world-assets/music/amber-drift.mp3 ../out-app \
  native:idle web:idle native:agent web:agent native:build web:build \
  native:flood web:flood native:deluge web:deluge
python3 spike/analyze.py ../out-app
```

## Not decided

- Who draws the world on the desktop.
- Whether 120 Hz matters to authoring and previs at all.
- Whether the phone layout's viewer component (mobile-remote.md, step 1)
  is written so the desktop could take it unchanged; it costs nothing
  either way and keeps the door open.

## Status

- [x] The spike: the viewer in the hole, Bevy's frame log, the load, the
      driver (`spike/run.sh`) and the headless stand-in (`spike/headless.mjs`)
- [x] The page's side, headless, in WebKit and Chromium
- [ ] The app's side on an unlocked screen: Bevy's frames, WKWebView's own
      and their rate on ProMotion
- [ ] A screenshot of each renderer from the same camera, for the parity
      the format promises
- [ ] Watermark flow control for the terminals, wanted whoever draws the
      world
- [ ] The decision
