#!/bin/bash
# Deploy the family's sites: each a Cloudflare Worker of static assets, each
# with a deploy.sh of its own that assembles what it needs first.
#
# Usage:
#   bin/web/deploy.sh <site…> [--dry-run]     site: app gen verse md world, or all
#   bin/lg web deploy …
#
#   app    website/         localgpt.app         Zola build
#   gen    website-gen/     gen.localgpt.app
#   verse  website-verse/   verse.localgpt.app
#   md     website-md/      localgpt.md          wasm-pack (crates/previs-web) + the samples
#   world  website-world/   localgpt.world       the viewer and conformance scenes from crates/,
#                                                the curated worlds from $LOCALGPT_WORLD_ASSETS
#
# --dry-run assembles and resolves each Worker's config without publishing.
# There is one environment and it is production, so a deploy of more than
# one site prints the plan and asks once before the first upload.
# `all` deploys in the family strip's order: app, gen, verse, md, world.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
cd "$LG_ROOT"

site_dir() { # site_dir <site> -> directory, or "" for an unknown site
  case "$1" in
    app)   echo "website" ;;
    gen)   echo "website-gen" ;;
    verse) echo "website-verse" ;;
    md)    echo "website-md" ;;
    world) echo "website-world" ;;
    *)     echo "" ;;
  esac
}

site_host() { # site_host <site>
  case "$1" in
    app)   echo "https://localgpt.app" ;;
    gen)   echo "https://gen.localgpt.app" ;;
    verse) echo "https://verse.localgpt.app" ;;
    md)    echo "https://localgpt.md" ;;
    world) echo "https://localgpt.world" ;;
  esac
}

sites=""
dry_run=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    -h|--help) lg_usage_header "$0"; exit 0 ;;
    all) sites="app gen verse md world" ;;
    -*) echo "Error: unexpected option '$1'." >&2; exit 2 ;;
    *)
      [ -n "$(site_dir "$1")" ] || { echo "Error: unknown site '$1' (app gen verse md world, or all)." >&2; exit 2; }
      sites="$sites $1"
      ;;
  esac
  shift
done
[ -n "$sites" ] || { lg_usage_header "$0" >&2; exit 2; }

lg_need_tool npx "install node 22"
if [ "$dry_run" = 0 ] && ! npx --yes wrangler whoami 2>&1 | grep -q "Account Name"; then
  echo "Not logged in to Cloudflare. Run: npx wrangler login" >&2
  exit 1
fi

echo "Deploying:"
for s in $sites; do
  printf '  %-6s %-15s -> %s\n' "$s" "$(site_dir "$s")/" "$(site_host "$s")"
done
[ "$dry_run" = 1 ] && echo "  mode:  DRY RUN (assembles, publishes nothing)"
echo ""

count=$(echo "$sites" | wc -w | tr -d ' ')
if [ "$dry_run" = 0 ] && [ "$count" -gt 1 ] && [ -t 0 ]; then
  printf 'Publish these %s sites now? [y/N] ' "$count"
  read -r answer
  case "$answer" in y|Y|yes) ;; *) echo "Aborted."; exit 1 ;; esac
  echo ""
fi

for s in $sites; do
  dir="$(site_dir "$s")"
  echo "==> $s ($dir)"
  if [ "$dry_run" = 1 ]; then
    "$dir/deploy.sh" --dry-run
  else
    "$dir/deploy.sh"
  fi
  echo ""
done

if [ "$dry_run" = 0 ]; then
  echo "Deployed. Verify:"
  for s in $sites; do echo "  curl -sI $(site_host "$s") | head -1"; done
fi
