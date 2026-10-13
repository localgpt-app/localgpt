#!/usr/bin/env bash
# Assemble the localgpt.md page: the sample screenplays and the WASM draft.
#
# Like website-world's assemble: nothing below is tracked except the page
# and this script — the samples are the crate's own, and the WASM is built
# by wasm-pack from crates/previs-web (rebuilt here only when cargo
# changes). The page needs no viewer of its own: the board it shows is the
# self-contained board.html the draft produces, in an iframe.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$(cd .. && pwd)"

# 1. The samples the page offers: the same screenplay as Fountain and as
# Markdown script pages (they draft to the same board, byte for byte).
mkdir -p samples
cp "$root/crates/previs/samples/cold-eggs.fountain" samples/cold-eggs.fountain
cp "$root/crates/previs/samples/cold-eggs.md" samples/cold-eggs.md

# 2. The draft, if stale (wasm-pack skips when up to date).
if ! command -v wasm-pack >/dev/null; then
  echo "wasm-pack not found — wasm/ left as is (CI builds it separately)" >&2
  exit 0
fi
(cd "$root" && wasm-pack build crates/previs-web --target web --out-dir ../../website-md/wasm)
echo "assembled"
