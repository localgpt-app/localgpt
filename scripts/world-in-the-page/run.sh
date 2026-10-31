#!/bin/bash
# The world-in-the-page measurement in the app
# (docs/rfcs/authoring/the-world-in-the-page.md): one app launch per run. The
# page measures its frames (ui/src/spike.ts) and Bevy logs its own
# (src/spike.rs) while the song's clock runs and the three terminals stream;
# the run ends when the page posts its report. It needs an unlocked screen
# with the window in front: a locked Mac hides every window, and the page
# then never draws.
#
# usage: scripts/world-in-the-page/run.sh <song> <out-dir> <mode:load>...
#   mode: native (Bevy draws the world) | web (the page draws it)
#   load: idle | agent | build | flood | deluge (load.py)
# env:  BIN (the app, target/debug/localgpt-app), SECS (measured seconds,
#       30), PORT (8765); and passed to the app (src/spike.rs):
#       LOCALGPT_SPIKE_MONITOR=fastest (the window on the fastest display, a
#       ProMotion screen), LOCALGPT_SPIKE_CASCADES=<n> (Bevy's shadow
#       cascades; four by default)
# also: python3 scripts/world-in-the-page/gpu.py <out-dir> & (macOS: each
#       process's share of the GPU's time, beside the frames)
# then: HZ=<the display's rate> python3 scripts/world-in-the-page/analyze.py <out-dir>
#
# Build the page and the app first. The app's data, state and cache go to
# <out-dir>/env, so the runs never touch the real workspace or what the app
# reopens; the CC0 pack is read where it is.
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SONG=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
mkdir -p "$2"
OUT=$(cd "$2" && pwd)
shift 2
BIN=${BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/localgpt-app}
PORT=${PORT:-8765}
SECS=${SECS:-30}
PACK=${LOCALGPT_WORLD_ASSETS:-$HOME/.local/share/localgpt/models/pack}
ENVDIR=$OUT/env
mkdir -p "$ENVDIR/data" "$ENVDIR/state" "$ENVDIR/cache"

python3 "$HERE/serve.py" "$ROOT/crates/app/ui/dist" "$ENVDIR/data/gen-workspace/songs" "$OUT" "$PORT" &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT

for spec in "$@"; do
  mode=${spec%%:*}
  load=${spec#*:}
  id="$mode-$load"
  rm -f "$OUT/$id-page.json" "$OUT/$id-bevy.csv"
  web=0
  [ "$mode" = web ] && web=1
  url="http://127.0.0.1:$PORT/?spike=$mode&load=$load&run=$id&secs=$SECS&open=shell,shell,shell&loadpy=$HERE/load.py"
  LOCALGPT_DATA_DIR="$ENVDIR/data" LOCALGPT_STATE_DIR="$ENVDIR/state" LOCALGPT_CACHE_DIR="$ENVDIR/cache" \
    LOCALGPT_WORLD_ASSETS="$PACK" LOCALGPT_APP_UI_URL="$url" \
    LOCALGPT_SPIKE_FRAMES="$OUT/$id-bevy.csv" LOCALGPT_SPIKE_MANIFEST=1 LOCALGPT_SPIKE_WEB_WORLD=$web \
    "$BIN" --song "$SONG" >"$OUT/$id-app.log" 2>&1 &
  app=$!
  for _ in $(seq 1 $((SECS + 90))); do
    [ -s "$OUT/$id-page.json" ] && break
    kill -0 $app 2>/dev/null || break
    sleep 1
  done
  sleep 1
  kill $app 2>/dev/null
  wait $app 2>/dev/null
  pkill -f "$HERE/load.py" 2>/dev/null
  if [ -s "$OUT/$id-page.json" ]; then echo "$id: measured"; else echo "$id: no report (see $OUT/$id-app.log)"; fi
  sleep 3
done
