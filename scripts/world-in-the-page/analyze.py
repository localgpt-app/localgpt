#!/usr/bin/env python3
"""The world-in-the-page measurement's numbers, as Markdown tables.

usage: analyze.py <out-dir>
env:   HZ    the display's refresh rate. Without it, a run whose median frame
             is under 12 ms is taken as 120 Hz and any other as 60, which
             is wrong for a 144 Hz monitor, or for 120 Hz frames that never
             kept up: set it then.
       SECS  the measured seconds of a run with no page (30)

The page's frames are its requestAnimationFrame intervals over the measured
window: with `web` the world is drawn in them; with `native` the hole is
empty (headless) or Bevy draws the world behind it (in the app). Bevy's
frames are its own log, cut to the same window, and exist only for runs in
the app (run.sh). A run of the app with no page (a platform with no web view
yet: `alone-*-bevy.csv`) is measured from 4 s after its `# play` mark. A
frame is late when it took more than 1.5 of the display's intervals; a
hitch is any frame over 50 ms. When `gpu.csv` is there (gpu.py, macOS: each
process's share of the GPU's time), each run's means are shown beside it.
"""
import glob
import json
import os
import statistics
import sys

OUT = sys.argv[1]
SECS = float(os.environ.get("SECS", 30))
ORDER = {"idle": 0, "agent": 1, "build": 2, "flood": 3, "deluge": 4}
ENGINES = ("webkit", "chromium", "msedge")


def pct(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def display_hz(deltas):
    if os.environ.get("HZ"):
        return float(os.environ["HZ"])
    return 120.0 if statistics.median(deltas) < 12 else 60.0


def stats(deltas, window_s):
    hz = display_hz(deltas)
    interval = 1000 / hz
    return {
        "fps": len(deltas) / window_s,
        "hz": round(hz),
        "p50": statistics.median(deltas),
        "p95": pct(deltas, 95),
        "p99": pct(deltas, 99),
        "max": max(deltas),
        "late": 100 * sum(d > 1.5 * interval for d in deltas) / len(deltas),
        "hitches": sum(d > 50 for d in deltas),
    }


def cells(s):
    return (
        f"{s['fps']:.1f} of {s['hz']} | {s['p50']:.1f} / {s['p95']:.1f} / {s['p99']:.1f} | "
        f"{s['late']:.1f}% | {s['hitches']} | {s['max']:.0f}"
    )


def read_bevy(path):
    """Frames as (unix_ms, dt_ms), and the log's comment lines."""
    frames, notes = [], []
    for line in open(path):
        if line.startswith("#"):
            notes.append(line.strip("# \n"))
            continue
        t, dt = line.split(",")
        frames.append((float(t), float(dt)))
    return frames, notes


GPU = []
if os.path.exists(os.path.join(OUT, "gpu.csv")):
    for line in open(os.path.join(OUT, "gpu.csv")).read().split("\n")[1:]:
        parts = line.split(",")
        if len(parts) == 3 and parts[0].isdigit():
            GPU.append((int(parts[0]) * 1000, parts[1], float(parts[2])))


def group(process):
    if process.startswith("localgpt-app"):
        return "app"
    if "WebKit" in process:
        return "web view"
    return "WindowServer" if process == "WindowServer" else "other"


def gpu_mean(start_ms, stop_ms):
    """Each one's mean share of the GPU's time over a run (gpu.py): the app,
    the web view's GPU process, the window server, everyone else."""
    samples = {}
    for t, process, share in GPU:
        if start_ms <= t <= stop_ms:
            by = samples.setdefault(t, {})
            by[group(process)] = by.get(group(process), 0.0) + share
    if not samples:
        return ""
    return " / ".join(
        f"{sum(s.get(g, 0.0) for s in samples.values()) / len(samples):.0f}"
        for g in ("app", "web view", "WindowServer", "other")
    )


def engine_of(run):
    first = run.split("-")[0]
    return first if first.split("+")[0] in ENGINES else "app"


rows = []
for path in glob.glob(os.path.join(OUT, "*-page.json")):
    rep = json.load(open(path))
    window_s = (rep["stop"] - rep["start"]) / 1000
    run = rep["run"]
    bevy, notes = None, []
    log = os.path.join(OUT, f"{run}-bevy.csv")
    if os.path.exists(log):
        frames, notes = read_bevy(log)
        cut = [dt for t, dt in frames if rep["start"] <= t <= rep["stop"]]
        if cut:
            bevy = stats(cut, window_s)
    rows.append((engine_of(run), rep, stats(rep["deltas"], window_s), bevy, notes))

rows.sort(key=lambda r: (r[0] != "app", r[0], ORDER.get(r[1]["load"], 9), r[1]["mode"] != "native"))

alone = []
for path in sorted(glob.glob(os.path.join(OUT, "alone-*-bevy.csv"))):
    frames, notes = read_bevy(path)
    play = next((float(n.split()[1]) for n in notes if n.startswith("play ")), None)
    if play is None or not frames:
        continue
    start = play + 4000
    stop = next((float(n.split()[1]) for n in notes if n.startswith("exit ")), frames[-1][0])
    stop = min(stop, start + SECS * 1000)
    cut = [dt for t, dt in frames if start <= t <= stop]
    if cut:
        name = os.path.basename(path)[: -len("-bevy.csv")]
        alone.append((name, stats(cut, (stop - start) / 1000), notes, gpu_mean(start, stop)))

if rows:
    print("The page's frames")
    print()
    print("| engine | terminals | world | fps | ms p50 / p95 / p99 | late | hitches | worst ms | one world frame, ms (with GPU wait) | terminal KB/s | GPU time %: app / web view / WindowServer / other |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for engine, rep, page, _, _ in rows:
        native = rep["mode"] == "native"
        world = ("Bevy, behind the page" if engine == "app" else "none (empty hole)") if native else "three.js, in the page"
        cost = f"{rep['tickMs']:.1f} ({rep['tickSyncMs']:.1f})" if rep.get("tickMs") is not None else ""
        busy = gpu_mean(rep["start"], rep["stop"])
        print(f"| {engine} | {rep['load']} | {world} | {cells(page)} | {cost} | {rep['termKBps']:.0f} | {busy} |")

app = [r for r in rows if r[3] is not None]
if app or alone:
    print()
    print("Bevy's frames (in the app)")
    print()
    print("| run | terminals | world | fps | ms p50 / p95 / p99 | late | hitches | worst ms | GPU time %: app / web view / WindowServer / other |")
    print("|---|---|---|---|---|---|---|---|---|")
    for engine, rep, _, bevy, _ in app:
        world = "Bevy draws it" if rep["mode"] == "native" else "the page draws it; Bevy's camera is off"
        print(f"| {rep['run']} | {rep['load']} | {world} | {cells(bevy)} | {gpu_mean(rep['start'], rep['stop'])} |")
    for name, s, _, busy in alone:
        print(f"| {name} | none | Bevy alone, no page | {cells(s)} | {busy} |")

print()
for engine, rep, page, bevy, notes in rows:
    extra = []
    if rep.get("hidden"):
        extra.append(f"page visibility changed: {rep['hidden']}")
    if rep.get("visibility") != "visible":
        extra.append(f"page ended {rep.get('visibility')}")
    if notes:
        extra.append("bevy: " + "; ".join(notes))
    if rep.get("calls") is not None:
        extra.append(f"three.js: {rep['calls']} draw calls, {rep['triangles']} triangles, {rep.get('entities')} entities")
    if rep.get("gl"):
        extra.append(f"gl {rep['gl']}")
    extra.append(f"clock messages {rep.get('clockHz', 0):.0f}/s, viewport {rep.get('viewport')}, dpr {rep.get('dpr')}")
    print(f"- {rep['run']}: " + " | ".join(extra))
for name, _, notes, _ in alone:
    print(f"- {name}: bevy: " + "; ".join(notes))
