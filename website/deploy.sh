#!/usr/bin/env bash
# Build the site with Zola and deploy public/ as the "localgpt-app" Cloudflare Worker.
# Serves at https://localgpt-app.<account-subdomain>.workers.dev until
# localgpt.app is attached as a custom domain in the Cloudflare dashboard.
set -euo pipefail
cd "$(dirname "$0")"

if ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login"
  exit 1
fi

zola build
npx --yes wrangler deploy
