#!/bin/bash
# Build the mobile library (localgpt-mobile-ffi) and its UniFFI bindings.
#
# Usage:
#   bin/build/mobile.sh ios|android
#   bin/lg build mobile ios|android
#
#   ios      apps/apple/scripts/build_apple.sh      XCFramework + Swift (iOS, macOS)
#   android  apps/android/scripts/build_android.sh  cargo-ndk + Kotlin
#
# The scripts live with the apps they feed; this only routes to them, so a
# build from Xcode or Android Studio and one from here are the same build.

set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
cd "$LG_ROOT"

case "${1:-}" in
  ios)     exec apps/apple/scripts/build_apple.sh ;;
  android) exec apps/android/scripts/build_android.sh ;;
  -h|--help) lg_usage_header "$0"; exit 0 ;;
  *) lg_usage_header "$0" >&2; exit 2 ;;
esac
