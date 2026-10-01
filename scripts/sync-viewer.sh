#!/usr/bin/env bash
# Vendor the web renderer from the openworldformat npm package.
#
# crates/world-export/js/world-viewer.js is a verbatim snapshot of
# openworldformat@$OWF_VERSION — never hand-edited. It is the one copy the
# apps use: html.rs embeds it into exported HTML (include_str!), the session
# servers serve it at /world-viewer.js, and both websites assemble from it.
# The renderer's upstream is the openworldformat repository; changes ship
# there, get released on npm, and flow back through this script.
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

OWF_VERSION="${OWF_VERSION:-0.2.0}"
DEST="crates/world-export/js/world-viewer.js"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

npm pack "openworldformat@$OWF_VERSION" --silent --pack-destination "$tmp" >/dev/null
tar -xzf "$tmp"/openworldformat-*.tgz -C "$tmp" package/src/render.js
src="$tmp/package/src/render.js"

# The embed invariants html.rs asserts — refuse a bad upstream before writing.
grep -q 'export function createWorldViewer' "$src"
if grep -q '</script' "$src"; then
  echo "refusing: upstream render.js contains '</script' — it would break the inline embed" >&2
  exit 1
fi

if [ "${1:-}" = "--check" ]; then
  if cmp -s "$src" "$DEST"; then
    echo "in sync with openworldformat@$OWF_VERSION"
    exit 0
  fi
  diff -u "$DEST" "$src" || true
  echo "drifted from openworldformat@$OWF_VERSION — run ./scripts/sync-viewer.sh" >&2
  exit 1
fi

cp "$src" "$DEST"
echo "vendored openworldformat@$OWF_VERSION -> $DEST"
