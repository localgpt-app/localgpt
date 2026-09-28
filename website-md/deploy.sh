#!/usr/bin/env bash
# Deploy this directory as the "localgpt-md" Cloudflare Worker (static
# assets), serving localgpt.md — the drop-a-Markdown-file page.
#
# Assembles first: the viewer from crates/world-export, three.js and the
# samples from their homes in this repository, and the WASM compiler from
# crates/md-web via wasm-pack. See scripts/assemble.sh.
set -euo pipefail
cd "$(dirname "$0")"

./scripts/assemble.sh

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

npx --yes wrangler deploy
