#!/bin/bash
# Build LocalGPT's binaries with cargo, by name.
#
# Usage:
#   bin/build/target.sh <target…> [--release] [-- <cargo args…>]
#   bin/lg build …
#
#   cli       localgpt          the assistant: chat, tui, daemon, memory, `localgpt world`
#   app       localgpt-app      the one desktop app (not on crates.io)
#   gen       localgpt-gen      the world builder: MCP server, headless, sessions, --live
#   previs    localgpt-previs   a screenplay -> per-scene .world packages
#   md        localgpt-md       a Markdown file as a walkable world
#   verse     localgpt-verse    a 3D world for every song
#   headless  localgpt with no desktop GUI (--no-default-features), for servers
#   all       every workspace crate
#
# Everything after `--` goes to cargo: `-- --features local-llm-metal`, say.
# Binaries land in target/debug/ (or target/release/ with --release, which is
# LTO and stripped, so slow). Mobile and the macOS bundle are their own
# commands: bin/lg build mobile ios|android, bin/lg build bundle.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
cd "$LG_ROOT"

package() { # package <target> -> the cargo package, or "" for an unknown target
  case "$1" in
    cli)    echo "localgpt" ;;
    app)    echo "localgpt-app" ;;
    gen)    echo "localgpt-gen" ;;
    previs) echo "localgpt-previs" ;;
    md)     echo "localgpt-md" ;;
    verse)  echo "localgpt-verse" ;;
    *)      echo "" ;;
  esac
}

packages=""
headless=0
everything=0
profile=""
while [ $# -gt 0 ]; do
  case "$1" in
    --release) profile="--release" ;;
    -h|--help) lg_usage_header "$0"; exit 0 ;;
    --) shift; break ;;
    all) everything=1 ;;
    headless) headless=1 ;;
    -*) echo "Error: unexpected option '$1' (cargo args go after --)." >&2; exit 2 ;;
    *)
      p="$(package "$1")"
      [ -n "$p" ] || { echo "Error: unknown target '$1' (cli app gen previs md verse headless all)." >&2; exit 2; }
      packages="$packages -p $p"
      ;;
  esac
  shift
done
[ -n "$packages" ] || [ "$headless" = 1 ] || [ "$everything" = 1 ] || { lg_usage_header "$0" >&2; exit 2; }

lg_need_tool cargo "install Rust with rustup"

# One cargo invocation for the named packages (the workspace, when `all`),
# and a second for headless: --no-default-features would apply to every
# package selected beside it.
if [ "$everything" = 1 ]; then
  lg_stage "cargo build --workspace $profile $*" cargo build --workspace $profile "$@"
elif [ -n "$packages" ]; then
  # shellcheck disable=SC2086 # $packages is a list of -p flags, split on purpose
  lg_stage "cargo build$packages $profile $*" cargo build $packages $profile "$@"
fi
if [ "$headless" = 1 ]; then
  lg_stage "cargo build -p localgpt --no-default-features $profile $*" \
    cargo build -p localgpt --no-default-features $profile "$@"
fi

echo ""
echo "Built into target/${profile:+release}${profile:-debug}/"
