#!/usr/bin/env python3
"""The world-in-the-page measurement's numbers, as Markdown tables.

usage: analyze.py <out-dir>

The page's frames are its requestAnimationFrame intervals over the measured
window: with `web` the world is drawn in them; with `native` the hole is
empty (headless) or Bevy draws the world behind it (in the app). Bevy's
frames are its own log, cut to the same window, and exist only for runs in
the app (run.sh). A frame is late when it took more than 1.5 of its
source's display intervals; a hitch is any frame over 50 ms.
"""
import glob
import json
import os
import statistics
import sys

OUT = sys.argv[1]
ORDER = {"idle": 0, "agent": 1, "build": 2, "flood": 3, "deluge": 4}


def pct(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p / 100
    f = int(k)
    c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def stats(deltas, window_s):
    med = statistics.median(deltas)
    interval = 1000 / 120 if med < 12 else 1000 / 60
    return {
        "fps": len(deltas) / window_s,
        "hz": round(1000 / interval),
        "p50": med,
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


rows = []
for path in glob.glob(os.path.join(OUT, "*-page.json")):
    rep = json.load(open(path))
    window_s = (rep["stop"] - rep["start"]) / 1000
    run = rep["run"]
    engine = run.split("-")[0] if run.split("-")[0] in ("webkit", "chromium") else "app"
    bevy, notes = None, []
    log = os.path.join(OUT, f"{run}-bevy.csv")
    if os.path.exists(log):
        frames = []
        for line in open(log):
            if line.startswith("#"):
                notes.append(line.strip("# \n"))
                continue
            t, dt = line.split(",")
            if rep["start"] <= float(t) <= rep["stop"]:
                frames.append(float(dt))
        if frames:
            bevy = stats(frames, window_s)
    rows.append((engine, rep, stats(rep["deltas"], window_s), bevy, notes))

rows.sort(key=lambda r: (r[0] != "app", r[0], ORDER.get(r[1]["load"], 9), r[1]["mode"] != "native"))

print("The page's frames")
print()
print("| engine | terminals | world | fps | ms p50 / p95 / p99 | late | hitches | worst ms | one world frame, ms (with GPU wait) | terminal KB/s |")
print("|---|---|---|---|---|---|---|---|---|---|")
for engine, rep, page, _, _ in rows:
    native = rep["mode"] == "native"
    world = ("Bevy, behind the page" if engine == "app" else "none (empty hole)") if native else "three.js, in the page"
    cost = f"{rep['tickMs']:.1f} ({rep['tickSyncMs']:.1f})" if rep.get("tickMs") is not None else ""
    print(f"| {engine} | {rep['load']} | {world} | {cells(page)} | {cost} | {rep['termKBps']:.0f} |")

app = [r for r in rows if r[3] is not None]
if app:
    print()
    print("Bevy's frames (in the app)")
    print()
    print("| terminals | world | fps | ms p50 / p95 / p99 | late | hitches | worst ms |")
    print("|---|---|---|---|---|---|---|")
    for engine, rep, _, bevy, _ in app:
        world = "Bevy draws it" if rep["mode"] == "native" else "the page draws it; Bevy's camera is off"
        print(f"| {rep['load']} | {world} | {cells(bevy)} |")

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
