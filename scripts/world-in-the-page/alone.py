#!/usr/bin/env python3
"""The app with no page: Bevy alone draws the song's world in the whole
window while the song plays (muted), and logs its frames. On Windows this is
the app's whole half for now, since there is no web view host there yet; on
macOS it is the control, Bevy without the page over it. Runs anywhere Python
and the app run.

usage: alone.py <song> <out-dir> [secs]
env:   BIN (the app: target/debug/localgpt-app, .exe on Windows),
       PLAY (seconds before the song plays: the world loaded, its shaders
       built; 15), LOCALGPT_WORLD_ASSETS (the CC0 pack, when it is not a
       sibling checkout of localgpt-world-assets)
then:  python analyze.py <out-dir>

The app's data, state and cache go to <out-dir>/env, so nothing reaches the
real workspace; the song's world is written there as manifest.json too, which
headless.mjs can serve. The log is <out-dir>/alone-idle-bevy.csv.
"""
import os
import subprocess
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
root = here.parent.parent
song = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
secs = float(sys.argv[3]) if len(sys.argv) > 3 else 30.0
play = float(os.environ.get("PLAY", 15))
exe = "localgpt-app.exe" if os.name == "nt" else "localgpt-app"
target = Path(os.environ.get("CARGO_TARGET_DIR", root / "target"))
app = Path(os.environ.get("BIN", target / "debug" / exe))
if not app.exists():
    sys.exit(f"no app at {app}: build it (cargo build -p localgpt-app) or set BIN")

envdir = out / "env"
for d in ("data", "state", "cache"):
    (envdir / d).mkdir(parents=True, exist_ok=True)
env = dict(
    os.environ,
    LOCALGPT_DATA_DIR=str(envdir / "data"),
    LOCALGPT_STATE_DIR=str(envdir / "state"),
    LOCALGPT_CACHE_DIR=str(envdir / "cache"),
    LOCALGPT_APP_UI="none",
    LOCALGPT_SPIKE_FRAMES=str(out / "alone-idle-bevy.csv"),
    LOCALGPT_SPIKE_MANIFEST="1",
    LOCALGPT_SPIKE_PLAY=str(play),
    LOCALGPT_SPIKE_EXIT=str(play + 4 + secs + 2),
)
print(f"alone-idle: the app draws for about {play + secs + 6:.0f} s; leave its window in front")
with open(out / "alone-idle-app.log", "w") as log:
    # From the repository's root: the app finds a sibling checkout of
    # localgpt-world-assets (the pack) relative to where it runs.
    code = subprocess.call(
        [str(app), "--song", str(song)],
        cwd=str(root),
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        timeout=play + secs + 600,
    )
print(f"alone-idle: the app exited ({code}); frames in {out / 'alone-idle-bevy.csv'}")
