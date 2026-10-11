#!/usr/bin/env bash
# Assemble the previs page: the sample screenplay and the WASM draft.
#
# Like website-md's assemble: nothing below is tracked except the page and
# this script — the sample is the crate's own, and the WASM is built by
# wasm-pack from crates/previs-web (rebuilt here only when cargo changes).
# The page needs no viewer of its own: the board it shows is the
# self-contained board.html the draft produces, in an iframe.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$(cd .. && pwd)"

# 1. The sample the page offers.
mkdir -p samples
cp "$root/crates/previs/samples/cold-eggs.fountain" samples/cold-eggs.fountain

# 2. The page's icon, shared with the other landing pages.
cp "$root/website-md/localgpt-icon.svg" localgpt-icon.svg

# 3. The draft, if stale (wasm-pack skips when up to date).
if ! command -v wasm-pack >/dev/null; then
  echo "wasm-pack not found — wasm/ left as is (CI builds it separately)" >&2
  exit 0
fi
(cd "$root" && wasm-pack build crates/previs-web --target web --out-dir ../../website-previs/wasm)
echo "assembled"
