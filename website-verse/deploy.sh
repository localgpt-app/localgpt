#!/usr/bin/env bash
# Deploy website/ as the "localgpt-verse" Cloudflare Worker (static assets),
# served at verse.localgpt.app.
set -euo pipefail
cd "$(dirname "$0")"

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

npx --yes wrangler deploy
