#!/usr/bin/env bash
# Fetch the CC0 3D asset pack that MD, Verse and Gen share.
#
# The pack's source of truth is the localgpt-world-assets repo (formerly
# localgpt-verse-assets): its models/ ships manifest.json plus the packed .glb
# files via Git LFS, all Poly Haven CC0. This script copies the manifest and
# every referenced GLB into the ONE directory every app reads,
# ~/.local/share/localgpt/models/pack, so a 522 MB pack is not duplicated per
# app. Idempotent; re-run after pulling new assets upstream.
#
# You don't strictly need this copy: world_pack_dir also finds a sibling
# checkout of the assets repo directly, under either name. Run this for a
# machine-wide pack, a self-contained tree, or a packaged build.
#
# $LOCALGPT_WORLD_ASSETS overrides where it lands; $XDG_DATA_HOME moves the
# base. Keep in step with localgpt-world-agent's `paths::world_pack_dir`.
set -euo pipefail
cd "$(dirname "$0")/.."

# First sibling checkout that has the pack wins; the former repo name is still
# accepted so a rename mid-flight does not break a working tree.
SRC="${LOCALGPT_WORLD_ASSETS_SRC:-}"
if [ -z "$SRC" ]; then
  for cand in ../localgpt-world-assets ../localgpt-verse-assets; do
    if [ -f "$cand/models/manifest.json" ]; then SRC="$cand"; break; fi
  done
fi
OUT="${LOCALGPT_WORLD_ASSETS:-${XDG_DATA_HOME:-$HOME/.local/share}/localgpt/models/pack}/models"

if [ -z "$SRC" ] || [ ! -f "$SRC/models/manifest.json" ]; then
  echo "no pack found — clone localgpt-world-assets next to this repo and run"
  echo "'git lfs pull' there first (or point LOCALGPT_WORLD_ASSETS_SRC at it)"
  exit 1
fi

mkdir -p "$OUT"
cp "$SRC/models/manifest.json" "$OUT/manifest.json"

# Copy every GLB the manifest references (a JSON list of files), skipping
# ones already present. A one-line python walk beats shell-JSON parsing.
python3 - "$SRC/models/manifest.json" "$OUT" <<'EOF'
import json, shutil, sys
from pathlib import Path

manifest, out = Path(sys.argv[1]), Path(sys.argv[2])
files = sorted({a["file"] for a in json.loads(manifest.read_text())["assets"]})
copied = 0
for file in files:
    src, dst = manifest.parent / file, out / file
    if not src.is_file():
        print(f"  missing {file} (run 'git lfs pull' in the assets repo)")
        continue
    if dst.is_file():
        continue
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)
    copied += 1
print(f"  {len(files)} assets referenced, {copied} copied")
EOF

# Link the pack into each Bevy app's asset root. Bevy takes ONE asset base
# path (AssetPlugin.file_path), so models/ has to sit beside fonts/, ml/ and
# music/ rather than being resolved separately — and before these apps joined
# the workspace each carried its own 522 MB copy. A symlink gives one root per
# app with one copy on disk.
PACK="$(cd "$(dirname "$OUT")" && pwd)/models"
for crate in crates/verse crates/md; do
  link="$crate/assets/models"
  if [ -L "$link" ] || [ ! -e "$link" ]; then
    mkdir -p "$crate/assets"
    rm -f "$link"
    ln -s "$PACK" "$link"
    echo "linked $link -> $PACK"
  else
    echo "  $link exists and is not a symlink — left alone"
  fi
done

# The four CC0 "Starter Worlds" tracks, beside the pack. Two megabytes
# against the pack's 522, and they are what the desktop app opens on a cold
# start — a song is the one input whose model-free world is a finished world
# (docs/world-strategy.md §13.3), so a build without them has no first frame.
# Resolved by localgpt-world-agent's `paths::starter_music_dir`, which looks
# for music/music.json beside models/manifest.json.
MUSIC_SRC="$SRC/music"
MUSIC_OUT="$(dirname "$OUT")/music"
if [ -d "$MUSIC_SRC" ]; then
  mkdir -p "$MUSIC_OUT"
  copied=0
  for f in "$MUSIC_SRC"/*; do
    [ -f "$f" ] || continue
    dst="$MUSIC_OUT/$(basename "$f")"
    if [ ! -f "$dst" ]; then
      cp "$f" "$dst"
      copied=$((copied + 1))
    fi
  done
  echo "starter music -> $MUSIC_OUT ($copied copied)"
else
  echo "  no music/ in the assets repo — the app will have no cold-start song"
fi

echo "done -> $OUT"
echo "license: the pack is Poly Haven CC0 (see the assets repo's LICENSE);"
echo "         the starter tracks are CC0 originals (see music/NOTICE)"
