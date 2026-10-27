#!/bin/bash
# Shared helpers for the bin/ scripts. Bash 3.2 safe (macOS /bin/bash): no
# associative arrays, no mapfile.
#
# Source from any script — paths are computed from this file's own location:
#   source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
#
# Exports: LG_BIN (…/bin), LG_ROOT (the repository root).

LG_BIN="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LG_ROOT="$(cd "$LG_BIN/.." && pwd)"
export LG_ROOT

# Print a script's leading `#` comment block as its --help text (skips the
# shebang and the `# ` prefix). The one help idiom for bin/.
lg_usage_header() { # lg_usage_header <file>
  awk 'NR == 1 { next } /^#/ { print; next } { exit }' "$1" | sed 's/^# \{0,1\}//'
}

lg_need_tool() { # lg_need_tool <binary> <hint>
  command -v "$1" >/dev/null 2>&1 || {
    echo "Error: '$1' is not on PATH — $2" >&2
    exit 1
  }
}

# Run one named step in the current directory and stop at the first failure.
lg_stage() { # lg_stage <name> <cmd…>
  local name="$1"; shift
  echo ""
  echo "──────── $name ────────"
  if ! "$@"; then
    echo "" >&2
    echo "'$name' FAILED — nothing after it was run." >&2
    exit 1
  fi
}

# Install a site's packages when they are missing, as CI does with npm ci.
lg_need_modules() { # lg_need_modules <dir>
  [ ! -d "$1/node_modules" ] || return 0
  echo "==> $1: packages missing — npm ci"
  (cd "$1" && npm ci) || { echo "Error: npm ci failed in $1." >&2; exit 1; }
}
