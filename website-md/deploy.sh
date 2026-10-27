#!/usr/bin/env bash
# Deploy this directory as the "localgpt-md" Cloudflare Worker (static
# assets), serving localgpt.md — drop a screenplay, get a previs board.
#
# Assembles first: the samples from crates/previs and the WASM draft from
# crates/previs-web via wasm-pack. See scripts/assemble.sh.
set -euo pipefail
cd "$(dirname "$0")"

./scripts/assemble.sh

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

npx --yes wrangler deploy "$@"
