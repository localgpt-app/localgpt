---
description: "Every key in LocalGPT Verse: fly and drift cameras, the three-state HUD, queue, library, pause, comfort settings, and photo mode."
---

# Controls & HUD

*Chrome that rents, it doesn't own: one system, three states, and a single accent sampled from the world.*

## Keyboard & mouse {#keys}

| Input | Action |
|---|---|
| <kbd>W A S D</kbd> + mouse | Fly / look (Explore locks the pointer — full 360°, straight up and down) |
| <kbd>Space</kbd> / <kbd>Shift</kbd> | Fly up / down (Explore) |
| Scroll | Fly speed (Explore) |
| <kbd>F</kbd> / click tabs | Toggle Explore / Drift camera (Drift frees the cursor) |
| <kbd>E</kbd> | Send a pulse |
| <kbd>N</kbd> | Next track |
| <kbd>Tab</kbd> | Open / close the queue |
| <kbd>L</kbd> | Open / close the library (pick a world) |
| <kbd>Esc</kbd> | Pause (world time-dilates) · resume · close the top overlay |
| <kbd>H</kbd> | Hide the HUD now |
| <kbd>P</kbd> | Photo mode — hide the chrome and save a shot to `verse-photos/` |
| <kbd>←</kbd> / <kbd>→</kbd> | Adjust world intensity (while paused) |

## The HUD's three states {#hud}

The HUD follows the design spec's "one system, three states":

- **Visible** while you're active — now-playing cluster, beat-reactive progress with section notches, control hints, and the Explore/Drift toggle.
- **Minimized** to a hairline after four seconds idle.
- **Hidden** entirely when you stay away — or instantly, with <kbd>H</kbd>.

Any input wakes it. The one variable across the whole chrome is the accent, sampled from the current world's palette — the same rule [verse.localgpt.app](https://verse.localgpt.app/) follows.

## Overlays & panels {#overlays}

- **Pause** (<kbd>Esc</kbd>) — freezes into a time-dilated world and offers resume, settings, photo mode, and the **world intensity** slider (<kbd>←</kbd>/<kbd>→</kbd>).
- **Queue** (<kbd>Tab</kbd>) — a slide-in, now-playing-first view with ↑/↓ reorder buttons. Tracks carry a content-hash id, so imports dedupe and reordering never restarts the playing track.
- **Library** (<kbd>L</kbd>) — the world moods as selectable cards; pick one to jump worlds.
- **Settings** — the **Comfort** group works today; the remaining groups are display-only placeholders for now.
- **Credits & Licenses** — renders the asset manifest: every model's name, author, and license.
- **Onboarding** — the three-step first run (photosensitivity → controls → import); <kbd>Esc</kbd> closes the top overlay first, always.

## Comfort {#comfort}

Comfort is a cross-cutting gate, checked at the effect sites rather than in the UI — so every new effect inherits the discipline. Both options apply instantly:

- **Reduce flashing** — holds the world glow steady (no beat pulse) and caps UI pulses.
- **Gentler world motion** — damps the sway.

:::note

[verse.localgpt.app](https://verse.localgpt.app/) follows the same rule: palette cycling and reveal animations switch off entirely under your system's `prefers-reduced-motion` setting.

:::

## Photo mode {#photo}

Press <kbd>P</kbd> (or the pause menu's button) to clear the chrome and save a clean screenshot of the world to `verse-photos/`. If a capture reads back black — a known GPU readback flake on some machines — LocalGPT Verse retries automatically before writing the file.
