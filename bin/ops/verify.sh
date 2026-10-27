#!/bin/bash
# The CI checks, on this machine — what .github/workflows/ci.yml runs, in
# the same order. Run it before a release or a deploy; it stops at the first
# failure.
#
# Usage:
#   bin/ops/verify.sh [--rust] [--bevy] [--features] [--web] [--deps]   (default: all)
#   bin/lg verify …
#
#   --rust      fmt, clippy and tests for the workspace minus the Bevy crates,
#               and the world.schema.json snapshot
#   --bevy      clippy and tests for app, gen, md, verse, world-bevy, world-audio
#   --features  cargo check for the opt-in tiers (llm, local-llm, feathers, ml)
#   --web       the vendored viewer matches npm, localgpt.world's render check,
#               the localgpt.md page check (wasm-pack)
#   --deps      cargo-deny licenses, bans and sources; advisories too, which CI
#               runs on main and weekly, never on a pull request
#
# Needs what CI installs: the Rust toolchain, cargo-deny, node 22, wasm-pack
# with the wasm32-unknown-unknown target, and Chromium for Playwright. A
# missing node_modules is installed with `npm ci`, as CI does. Bash 3.2 safe.
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
cd "$LG_ROOT"

want_rust=0; want_bevy=0; want_features=0; want_web=0; want_deps=0
if [ $# -eq 0 ]; then want_rust=1; want_bevy=1; want_features=1; want_web=1; want_deps=1; fi
while [ $# -gt 0 ]; do
  case "$1" in
    --rust) want_rust=1 ;;
    --bevy) want_bevy=1 ;;
    --features) want_features=1 ;;
    --web) want_web=1 ;;
    --deps) want_deps=1 ;;
    -h|--help) lg_usage_header "$0"; exit 0 ;;
    *) echo "Error: unexpected argument '$1' (--rust|--bevy|--features|--web|--deps)." >&2; exit 2 ;;
  esac
  shift
done

started=$(date +%s)

# The crates the `bevy` job owns: native audio and windowing headers.
bevy_crates="-p localgpt-gen -p localgpt-world-bevy -p localgpt-md -p localgpt-verse -p localgpt-world-audio -p localgpt-app"
not_bevy="--exclude localgpt-gen --exclude localgpt-world-bevy --exclude localgpt-md --exclude localgpt-verse --exclude localgpt-world-audio --exclude localgpt-app"

if [ "$want_rust" = 1 ]; then
  lg_need_tool cargo "install Rust with rustup"
  lg_stage "cargo fmt --check" cargo fmt --all -- --check
  # shellcheck disable=SC2086 # the exclude list is one string on purpose
  lg_stage "cargo clippy --workspace (minus Bevy, -D warnings)" cargo clippy --workspace $not_bevy -- -D warnings
  # shellcheck disable=SC2086
  lg_stage "cargo test --workspace (minus Bevy)" cargo test --workspace $not_bevy
  lg_stage "world.schema.json snapshot" cargo test -p localgpt-world-types --features schema
fi

if [ "$want_bevy" = 1 ]; then
  lg_need_tool cargo "install Rust with rustup"
  # shellcheck disable=SC2086
  lg_stage "cargo clippy (app, gen, md, verse, world-bevy, world-audio)" cargo clippy $bevy_crates -- -D warnings
  # shellcheck disable=SC2086
  lg_stage "cargo test (app, gen, md, verse, world-bevy, world-audio)" cargo test $bevy_crates
fi

if [ "$want_features" = 1 ]; then
  lg_need_tool cargo "install Rust with rustup"
  lg_stage "cargo check -p localgpt-md --features llm" cargo check -p localgpt-md --features llm
  lg_stage "cargo check -p localgpt-verse --features llm" cargo check -p localgpt-verse --features llm
  lg_stage "cargo check -p localgpt-gen --features local-llm" cargo check -p localgpt-gen --features local-llm
  lg_stage "cargo check -p localgpt-gen --features feathers" cargo check -p localgpt-gen --features feathers
  lg_stage "cargo check -p localgpt-verse --features ml" cargo check -p localgpt-verse --features ml
fi

if [ "$want_web" = 1 ]; then
  lg_need_tool npm "install node 22"
  lg_need_tool wasm-pack "cargo install wasm-pack (and rustup target add wasm32-unknown-unknown)"
  lg_stage "viewer is the published openworldformat renderer" ./scripts/sync-viewer.sh --check
  lg_need_modules website-world
  lg_stage "localgpt.world: conformance render" bash -c "cd website-world && npm run check"
  lg_need_modules website-md
  lg_stage "localgpt.md: page check" bash -c "cd website-md && npm run assemble && npm run check"
fi

if [ "$want_deps" = 1 ]; then
  lg_need_tool cargo-deny "cargo install cargo-deny"
  lg_stage "cargo deny check licenses bans sources" cargo deny check licenses bans sources
  lg_stage "cargo deny check advisories" cargo deny check advisories
fi

echo ""
echo "verify: all selected checks passed in $(( $(date +%s) - started ))s."
