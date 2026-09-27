#!/usr/bin/env bash
# Assemble a distributable LocalGPT Verse bundle (PLAN.md §5.1: assets are bundled at
# ship time from the separate verse-assets repo — no first-run download).
#
# Usage: scripts/bundle.sh
#   VERSE_ASSETS=<path>  override the assets checkout location (default: the
#                          first of ../../../localgpt-world-assets or
#                          ../../../localgpt-verse-assets — its former name —
#                          that exists, as a sibling of the localgpt repo)
#
# Produces dist/ with the release binary + assets/ beside it. The
# bundle is portable and launchable from anywhere: the binary resolves assets
# from its own directory, not the working directory (world_assets::asset_root).
# Platform packaging builds on this layout — a macOS .app instead puts assets/
# in Contents/Resources/, and a packager that needs a third layout (a Flatpak
# installing to /app/share) sets VERSE_ASSET_ROOT in the launcher.
set -euo pipefail
# This crate's directory, and the workspace root two levels up: since MD and
# Verse joined the localgpt workspace, cargo builds into the ROOT target/, not
# a per-crate one.
cd "$(dirname "$0")/.."
CRATE="$(pwd)"
WORKSPACE="$(cd ../.. && pwd)"

ASSETS_REPO="${VERSE_ASSETS:-}"
if [ -z "$ASSETS_REPO" ]; then
  for cand in "$WORKSPACE/../localgpt-world-assets" "$WORKSPACE/../localgpt-verse-assets"; do
    if [ -d "$cand" ]; then ASSETS_REPO="$cand"; break; fi
  done
  ASSETS_REPO="${ASSETS_REPO:-$WORKSPACE/../localgpt-world-assets}"
fi

echo "==> release build"
cargo build --release -p localgpt-verse --manifest-path "$WORKSPACE/Cargo.toml"

echo "==> assembling dist/"
rm -rf dist
mkdir -p dist/assets
cp "${CARGO_TARGET_DIR:-$WORKSPACE/target}/release/localgpt-verse" dist/
cp -R assets/fonts dist/assets/fonts

if [ -d "$ASSETS_REPO/models" ]; then
  # Ship only the manifest-referenced (normalized .glb) files, not the
  # original .gltf/.bin/texture trees (PLAN.md §1.3 offline normalization).
  python3 "$ASSETS_REPO/normalize.py" --sync "$(pwd)/dist/assets/models"
  echo "    bundled 3D pack from $ASSETS_REPO"
else
  echo "    warn: $ASSETS_REPO/models not found — bundling without the 3D pack (procedural worlds)"
fi

if [ -f "$ASSETS_REPO/music/music.json" ]; then
  # CC0 starter music so the app has something to play on first run.
  mkdir -p dist/assets/music
  cp "$ASSETS_REPO"/music/*.mp3 "$ASSETS_REPO"/music/music.json dist/assets/music/
  [ -f "$ASSETS_REPO/music/NOTICE" ] && cp "$ASSETS_REPO/music/NOTICE" dist/assets/music/
  echo "    bundled $(ls "$ASSETS_REPO"/music/*.mp3 | wc -l | tr -d ' ') CC0 starter tracks"
else
  echo "    warn: $ASSETS_REPO/music not found — shipping without starter music"
fi

du -sh dist
echo "done: $(pwd)/dist — run dist/localgpt-verse from anywhere"
