#!/usr/bin/env bash
# Deploy website-gen/ as the "localgpt-gen" Cloudflare Worker (static assets).
# Serves at https://localgpt-gen.<account-subdomain>.workers.dev until
# gen.localgpt.app is attached as a custom domain in the Cloudflare dashboard.
set -euo pipefail
cd "$(dirname "$0")"

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

npx --yes wrangler deploy
