#!/usr/bin/env bash
# Vendor the web renderer from the openworldformat npm package.
#
# crates/world-export/js/world-viewer.js and cinematography.js are verbatim
# snapshots of openworldformat@$OWF_VERSION (its src/render.js and
# src/cinematography.js) — never hand-edited. They are the one copy the
# apps use: html.rs embeds them into exported HTML (include_str!), the
# session servers serve them at /world-viewer.js and /cinematography.js, and
# both websites assemble from them. The renderer imports
# `./cinematography.js` (the ext-cinematography math behind its shot view),
# so the two travel together and every import map keys that relative
# specifier. The renderer's upstream is the openworldformat repository;
# changes ship there, get released on npm, and flow back through this
# script.
#
# The source is always the registry tarball, never a sibling checkout: what
# the apps embed is exactly what npm shipped. Bumping is a deliberate
# ritual: raise OWF_VERSION below (or pass OWF_VERSION=x.y.z), run this
# script, then cargo test -p localgpt-world-export and the website checks.
# CI runs `--check` on every push and PR, so a hand edit or a forgotten
# sync fails the build instead of drifting.
#
# This script flows npm -> crate, replacing the old repo->repo sync-viewer
# that localgpt.world's assemble replaced in 2026-09.
set -euo pipefail
cd "$(dirname "$0")/.."

OWF_VERSION="${OWF_VERSION:-0.3.5}"
DEST_DIR="crates/world-export/js"
# upstream file -> vendored name
FILES="render.js:world-viewer.js cinematography.js:cinematography.js"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

npm pack "openworldformat@$OWF_VERSION" --silent --pack-destination "$tmp" >/dev/null
tar -xzf "$tmp"/openworldformat-*.tgz -C "$tmp" package/src/render.js package/src/cinematography.js

# The embed invariants html.rs asserts — refuse a bad upstream before writing.
grep -q 'export function createWorldViewer' "$tmp/package/src/render.js"
grep -q "from './cinematography.js'" "$tmp/package/src/render.js" || {
  echo "refusing: upstream render.js no longer imports './cinematography.js' — the import maps key that specifier" >&2
  exit 1
}
for pair in $FILES; do
  if grep -q '</script' "$tmp/package/src/${pair%%:*}"; then
    echo "refusing: upstream ${pair%%:*} contains '</script' — it would break the inline embed" >&2
    exit 1
  fi
done

if [ "${1:-}" = "--check" ]; then
  drifted=0
  for pair in $FILES; do
    src="$tmp/package/src/${pair%%:*}"
    dest="$DEST_DIR/${pair##*:}"
    if ! cmp -s "$src" "$dest"; then
      diff -u "$dest" "$src" || true
      drifted=1
    fi
  done
  if [ "$drifted" = 0 ]; then
    echo "in sync with openworldformat@$OWF_VERSION"
    exit 0
  fi
  echo "drifted from openworldformat@$OWF_VERSION — run ./scripts/sync-viewer.sh" >&2
  exit 1
fi

for pair in $FILES; do
  cp "$tmp/package/src/${pair%%:*}" "$DEST_DIR/${pair##*:}"
  echo "vendored openworldformat@$OWF_VERSION src/${pair%%:*} -> $DEST_DIR/${pair##*:}"
done
