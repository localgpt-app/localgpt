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

echo "done -> $OUT"
echo "license: the pack is Poly Haven CC0 (see the assets repo's LICENSE)"
