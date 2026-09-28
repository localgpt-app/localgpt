#!/usr/bin/env bash
# Assemble the localgpt.md page: the compiler, the viewer and the samples.
#
# Like website-world's assemble: nothing below is tracked except the page and
# this script — the viewer comes from the crate that owns it (so it cannot
# drift from the apps), three.js is vendored from website-world's copy (the
# no-third-party-requests rule), and the samples are the app's own.
# The WASM is built by wasm-pack; rebuilt here only when cargo changes.
set -euo pipefail
cd "$(dirname "$0")/.."
root="$(cd .. && pwd)"

# 1. The one web renderer, from the crate that owns it.
mkdir -p viewer
cp "$root/crates/world-export/js/world-viewer.js" viewer/world-viewer.js

# 2. three.js, shared with website-world's vendored copy.
if [ -d "$root/website-world/vendor/three" ]; then
  mkdir -p vendor
  cp -R "$root/website-world/vendor/three" vendor/three
fi

# 3. The samples the page offers.
mkdir -p samples
cp "$root/crates/md/samples/hello.md" samples/hello.md
cp "$root/crates/md/samples/deck.md" samples/deck.md

# 4. The compiler, if stale (wasm-pack skips when up to date).
if ! command -v wasm-pack >/dev/null; then
  echo "wasm-pack not found — wasm/ left as is (CI builds it separately)" >&2
  exit 0
fi
(cd "$root" && wasm-pack build crates/md-web --target web --out-dir ../../website-md/wasm)
echo "assembled"
