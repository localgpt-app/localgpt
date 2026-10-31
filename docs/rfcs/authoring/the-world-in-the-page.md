# The world in the page: what Bevy buys

**Status:** measured on a Mac, not decided (2026-10-31). Headless and in
the app on an M2 Max; on Windows, handed over
([`scripts/world-in-the-page/WINDOWS.md`](../../../scripts/world-in-the-page/WINDOWS.md)).
Asks of [the-page.md](the-page.md)'s "the world stays native" what
[mobile-remote.md](mobile-remote.md) raises: the phone is to draw a live
canvas's world with the format's web viewer (its first step sends this
window's frames instead), so this same page would hold both renderers. The
harness is in the tree — `crates/app/src/spike.rs`,
`crates/app/ui/src/spike.ts`, `scripts/world-in-the-page/` — and nothing in
it runs unless its variable or `?spike=` asks.

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
   the panels this app is built around. (Measured below: headless it paid
   only under an overload; in the app it did not hold, for the page's
   frames followed Bevy's.)
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
   `requestAnimationFrame` near 60 on a ProMotion display by default.
   (Measured below: in the app neither renderer reached 60 on this Mac, on
   the ProMotion display or the 60 Hz one.)

## What looks like Bevy's and is not

- **Rendering headroom.** A `.world` must draw in three.js, RealityKit and
  on Android, so what only Bevy can draw cannot be in the format;
  `crates/world-export/js/world-viewer.js:44` already calibrates Bevy's
  physical light units down to three's. Headroom buys a desktop that looks
  better than its share link, which in an authoring tool is closer to a bug.
  In the app it ran the other way: with its default shadow settings Bevy
  spent nearly three times three.js's GPU time on the same world.
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
CC0 pack, 15 modulated by the song's curves, 25 with behaviors, and one
shadow-casting light. three.js draws it in 93–96 draw calls and about 5
million triangles, most of them the pack's scanned rocks and plants.

**The page** is the production build (`vite build`) in the song's layout:
the world in the hole (`?spike=web`) or the hole left to Bevy, or empty
headless (`?spike=native`), three shell tabs open with one in view, and the
timeline playing.

**The load** is `scripts/world-in-the-page/load.py` in each tab: in the app
through the tab's real PTY, headless delivered as the app delivers it, per
tab, 120 times a second, through `window.__lg.recv`.

| load | what it is | per tab |
|---|---|---|
| agent | an Ink-style live region, 14 lines redrawn ten times a second over a streamed answer: Claude Code or Codex at work | ~7 KB/s |
| build | compiler lines, with a burst of 400 every 5 s: a shell tab running a build | ~14 KB/s |
| flood | coloured log lines: `cat` of a big log, a verbose test run | ~0.9 MB/s |
| deluge | the flood's lines, four times as fast: an overload, to find where the page gives way | ~3.5 MB/s |

**Measured:** `requestAnimationFrame`'s intervals for 30 s, after 14 s of
warm-up; Bevy's own frames in the same 30 s; one world frame, timed on the
page's thread back to back, alone and with a one-pixel read so the GPU's
work is waited for; and, in the app, each process's share of the GPU's time
(`gpu.py`, from each GPU client's `accumulatedGPUTime`).

**Where:** an M2 Max (12 CPU cores, 30 GPU cores, 32 GB) on macOS 26.6.
Headless: WebKit 27.2 (Playwright's build), the macOS host's engine, and
Chromium 156, the engine of WebView2 and so of the Windows host, both
drawing on the GPU (WebKit's "Apple GPU", Chromium's ANGLE on Metal), with
the screen locked. In the app: its debug build (the dev profile is
optimized), with three displays awake — a 60 Hz 4K Dell U2720QM, the main
one, where the window opened; a second 4K Dell; and the built-in 120 Hz
XDR — and a browser and an Electron app busy behind it. The page is
1440 × 860 at 2×, the app's window; the hole is 960 × 626 (595 headless).

## What the page's side showed

Each row is 30 s of the page's frames, headless. "World" is whether
three.js drew the world in the hole; the last column is one frame of it on
the page's thread, and in brackets the same frame waiting on the GPU.

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
  once in 1,800, in WebKit. Run again with the displays awake, idle, it
  held 59.5 of 60 (p95 26 ms), and the world frame waited 21 ms on the GPU
  instead of 13.
- **The world costs the page's thread almost nothing.** One frame of it
  (behaviors, modulations, matrices, 96 draw calls) takes 0.3 to 0.7 ms of
  a 16.7 ms frame. The WebGL work itself runs in the browser's GPU process,
  so the world does not wait behind a terminal's parser, nor the parser
  behind the world.
- **It costs the GPU 6 to 15 ms a frame** with the displays asleep,
  depending on what the tour has in view: 5 million triangles, soft
  shadows, 4× MSAA at 2×.
- **WebKit gives way at about 10 MB/s.** The overload rows are no agent:
  `load.py deluge`, five hundred times what three agents write. There
  WebKit's frames went late with the world and without it (10.7% of them
  with the hole empty, 4.3% with the world in it); Chromium still held.
- **Nothing paces a terminal.** Each tab's reader reads 64 KB at a time
  into an unbounded channel, and every frame the app parses all of it on
  its main thread (`vt100`, since the phone remote, so a page that attaches
  late gets the screen) and sends all of it to the page (`webui/term.rs`,
  `Terms::drain`; `pump_terms`). An agent that writes faster than the page
  parses grows a backlog without bound, whoever draws the world. xterm.js's
  own advice is watermarks: stop reading the PTY while the page is behind.

## What the app's half showed

Each row is 30 s in the app (`scripts/world-in-the-page/run.sh`), the page
and Bevy measured together. "Bevy fps" with the page drawing the world is
Bevy's loop with its camera off, held to a 120 Hz tick.

On the 60 Hz main display:

| terminals, all three | world drawn by | page fps | page frame ms p50 / p95 | page worst ms | Bevy fps |
|---|---|---|---|---|---|
| idle | Bevy, behind the page | 30.6 | 33 / 49 | 174 | 30.6 |
| idle | three.js, in the page | 50.9 | 21 / 27 | 35 | 120 |
| agent, 22 KB/s | Bevy | 31.7 | 33 / 44 | 67 | 31.8 |
| agent, 22 KB/s | three.js | 50.5 | 21 / 27 | 61 | 120 |
| build, 45 KB/s | Bevy | 31.9 | 33 / 44 | 61 | 32.0 |
| build, 45 KB/s | three.js | 49.3 | 18 / 30 | 158 | 113 |
| flood, 2.7 MB/s | Bevy | 31.3 | 33 / 44 | 90 | 31.3 |
| flood, 2.7 MB/s | three.js | 50.0 | 19 / 28 | 78 | 120 |
| overload, 10.9 MB/s | Bevy | 29.6 | 33 / 50 | 75 | 30.1 |
| overload, 10.9 MB/s | three.js | 45.0 | 22 / 34 | 50 | 120 |
| none: no page at all | Bevy, alone, the whole window | | | | 28.8 |

On the built-in 120 Hz display (`LOCALGPT_SPIKE_MONITOR=fastest`):

| terminals | world drawn by | page fps | page frame ms p50 / p95 | page worst ms | Bevy fps |
|---|---|---|---|---|---|
| idle | Bevy | 31.7 | 32 / 46 | 64 | 31.7 |
| idle | three.js | 49.3 | 19 / 29 | 36 | 120 |
| agent, 22 KB/s | Bevy | 31.5 | 33 / 43 | 85 | 31.6 |
| agent, 22 KB/s | three.js | 51.0 | 19 / 27 | 45 | 120 |
| none: no page at all | Bevy, alone | | | | 29.4 |

Each process's share of the GPU's time over a run:

| run | Bevy | the web view's GPU process | the window server |
|---|---|---|---|
| Bevy draws, 60 Hz, agent | 44% | 12% | 19% |
| the page draws, 60 Hz, idle | 2% | 25% | 22% |
| Bevy alone, 60 Hz | 46% | | 13% |
| Bevy draws, 120 Hz, idle / agent | 42 / 41% | 12 / 14% | 23 / 20% |
| the page draws, 120 Hz, idle / agent | 2 / 2% | 22 / 22% | 26 / 24% |
| Bevy alone, 120 Hz | 48% | | 13% |
| headless, the world in the page, no app window | | 16% | 7% |

- **Behind the page, Bevy held the page to its own rate.** Bevy drew at
  30–32 fps under every load, idle included, at 60 Hz and at 120 Hz, and
  the page's frames ran at the same rate: the agents' terminals, the
  timeline and every panel updated at 30. Bevy's frames do not depend on
  the page — alone, with no page at all, it drew at 29 — but the page's
  depend on Bevy's. The likeliest reason is that WKWebView's side of the
  page and Bevy's loop share the app's main thread; that is not isolated.
  So the first thing only Bevy gives did not hold in the app: instead of
  the world staying smooth while the page lagged, the page lagged with the
  world.
- **Drawn in the page, the world ran at 45–51 fps, and the panels with it**,
  while Bevy's loop, with nothing to draw, kept its 120 Hz tick at 2% of
  the GPU. Not 60: headless, with the same displays awake, the same page
  held 59.5. Inside the app it loses about ten frames a second to something
  a browser does not have — Bevy's tick on the shared main thread, or the
  window's compositing — not yet found.
- **The GPU sets Bevy's 30, and the shadow sets the GPU's.** Per frame,
  Bevy spent 13–14 ms of GPU time on the world in the hole (16 alone, in
  the whole window); the web view's process 4–5 ms for the world and the
  page together. The world's one shadow-casting light is drawn by
  three.js into one 2048² map and by Bevy, by default, into four cascades,
  so Bevy draws the 5 million triangles five times a frame to three.js's
  two. With one cascade (`LOCALGPT_SPIKE_CASCADES=1`), Bevy's world frame
  took 7 ms of GPU instead of 14. That pair ran with the screen locked,
  where frames are paced at 60 whatever they cost, so it is GPU time, not a
  frame rate: 14 ms and the window server's share do not fit a 16.7 ms
  frame, and 7 should.
- **120 Hz changed nothing**, because nothing reached 60. What WKWebView
  does on ProMotion with a page that keeps up is still unmeasured.
- **The displays take a fifth of the GPU.** With the app's window on
  screen, the window server used 13–26% of the GPU's time compositing three
  displays and what the apps behind it redrew; with no app window on
  screen, 6–7%; with the displays asleep, none. It slows both renderers —
  it is why a 14 ms Bevy frame misses 60 here — and it will differ on every
  desk.
- **The terminals moved little.** At 10.9 MB/s the page drawing the world
  fell from about 50 fps to 45; Bevy behind the page stayed at 30, though
  it now parses every byte on its main thread.

## What this suggests

If the Windows runs agree, the world can move into the page. Frames do not
decide it, and where they lean, they lean to the page: headless it held 60
with the world in it under every load an agent makes, and in the app the
page drawing the world beat Bevy behind the page, 45–51 fps against 30,
for the world and for the panels, because behind the page Bevy set the
page's rate as well as its own. Bevy with one shadow cascade may win the
world's frames back, since its GPU cost halves; even then its frames do not
stay its own, and what is left of Bevy's case is the scene in-process,
which is the cost of moving rather than a reason to stay.

One renderer is worth more to this product: what the author sees is what
the link and the previs page show, every format feature is written once,
and the hole goes with its three platforms' tricks. The cost of moving:

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

## How it runs

In the app, at an unlocked screen with the window left in front (a locked
Mac hides every window: the page then never draws, and Bevy's frames are
paced at 60 whatever they cost). Build the page and the app first; each run
opens the app once with `--song`, and its data, state and cache go to the
run's own folder, so nothing reaches the real workspace or what the app
reopens. The page comes from a local server (`LOCALGPT_APP_UI_URL`), the
song is muted at its first view, and the page posts its frames back while
Bevy logs its own (`LOCALGPT_SPIKE_FRAMES`). In `web` runs Bevy's world
camera is off and its loop is held to 120 Hz, so the app's own systems
cost what they cost when Bevy draws.

```bash
W=scripts/world-in-the-page SONG=../localgpt-world-assets/music/amber-drift.mp3
python3 $W/gpu.py target/spike/out-app &        # macOS: GPU time per process
$W/run.sh $SONG target/spike/out-app \
  native:idle web:idle native:agent web:agent native:build web:build \
  native:flood web:flood native:deluge web:deluge
python3 $W/alone.py $SONG target/spike/out-app   # Bevy with no page (LOCALGPT_APP_UI=none)
kill %1; HZ=60 python3 $W/analyze.py target/spike/out-app
```

`LOCALGPT_SPIKE_MONITOR=fastest` moves the window to the fastest display (a
ProMotion screen; analyze with `HZ=120`), and `LOCALGPT_SPIKE_CASCADES=1`
gives Bevy's shadow one cascade. `headless.mjs` drives the same page in a
headless browser (`webkit`, `chromium` or `msedge`; `HEADED=1` for a window
on the real display), standing in for the app. On Windows, which has no
web view host yet, the page's side runs in Edge and Chromium and Bevy runs
alone: [`WINDOWS.md`](../../../scripts/world-in-the-page/WINDOWS.md) is
the handover.

## Not decided

- Who draws the world on the desktop.
- Whether 120 Hz matters to authoring and previs at all.
- Whether the phone layout's viewer component (mobile-remote.md, step 1)
  is written so the desktop could take it unchanged; it costs nothing
  either way and keeps the door open.

## Status

- [x] The harness: the viewer in the hole, Bevy's frame log, the loads,
      the drivers (`run.sh`, `alone.py`, `headless.mjs`), GPU time per
      process (`gpu.py`)
- [x] The page's side, headless, in WebKit and Chromium
- [x] The app's side on a Mac: Bevy's frames and WKWebView's, at 60 Hz and
      on ProMotion, Bevy alone, GPU time per process
- [ ] Bevy with one shadow cascade, on screen: behind the page and alone,
      at 60 and 120 Hz
- [ ] Why the page loses ten frames a second inside the app
- [ ] Windows: Edge and Chromium, headless and in a window, and Bevy alone
      ([`WINDOWS.md`](../../../scripts/world-in-the-page/WINDOWS.md))
- [ ] A screenshot of each renderer from the same camera, for the parity
      the format promises
- [ ] Watermark flow control for the terminals, wanted whoever draws the
      world
- [ ] The decision
