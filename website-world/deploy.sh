#!/usr/bin/env bash
# Deploy this directory as the "localgpt-world" Cloudflare Worker (static
# assets), serving localgpt.world.
#
# Assembles first: the viewer and the conformance scenes come from crates/ in
# this same repository (so they cannot drift from the apps), and the curated
# worlds' shrunk GLBs, music and posters come from the assets checkout. A
# deploy always publishes the full set, so it needs that checkout with its LFS
# objects pulled — `$LOCALGPT_WORLD_ASSETS` points at it.
set -euo pipefail
cd "$(dirname "$0")"

node scripts/assemble.mjs --all

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

npx --yes wrangler deploy
