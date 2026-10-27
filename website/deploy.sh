#!/usr/bin/env bash
# Build the site with Zola and deploy public/ as the "localgpt-app" Cloudflare Worker.
set -euo pipefail
cd "$(dirname "$0")"

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

zola build
npx --yes wrangler deploy "$@"
