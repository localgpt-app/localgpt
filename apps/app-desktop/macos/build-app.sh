#!/usr/bin/env bash
# Package localgpt-app — the one LocalGPT desktop app — as "LocalGPT.app",
# and optionally a .dmg.
#
# Usage: apps/app-desktop/macos/build-app.sh [--debug] [--dmg]
#   --debug  bundle the debug build (quick, for trying the bundle locally)
#   --dmg    also create dist/LocalGPT-<version>.dmg
#
# Signing. Ad-hoc by default: the app runs on this Mac and nowhere else
# without a Gatekeeper override. For a download other people can open:
#
#   APPLE_SIGNING_IDENTITY="Developer ID Application: <name> (<team id>)"
#       signs the app (and the .dmg) with that identity, the hardened runtime
#       and a secure timestamp — what notarization requires;
#   APPLE_NOTARY_PROFILE=<profile>
#       a keychain profile made once with `xcrun notarytool
#       store-credentials`; implies --dmg, submits the .dmg for notarization,
#       waits, and staples the ticket to it.
#
# Local models. On Apple Silicon the app is built with `local-llm-metal`, so
# the bundle runs a GGUF from the shared model folder in-process on the GPU
# (docs/world-strategy.md §13.3, the third tier). Intel Macs build without it:
# the CPU path can't hold the ~5 GB model beside the renderer. APP_FEATURES
# overrides the feature list (comma-separated; empty for none).
#
# Output goes to apps/app-desktop/dist/.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
PROFILE=release
MAKE_DMG=0
for arg in "$@"; do
  case "$arg" in
    --debug) PROFILE=debug ;;
    --dmg) MAKE_DMG=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done
IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
NOTARY_PROFILE="${APPLE_NOTARY_PROFILE:-}"
if [ -n "$NOTARY_PROFILE" ]; then
  [ -n "$IDENTITY" ] || { echo "APPLE_NOTARY_PROFILE needs APPLE_SIGNING_IDENTITY" >&2; exit 2; }
  MAKE_DMG=1
fi

VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
OUT="$ROOT/apps/app-desktop/dist"
APP="$OUT/LocalGPT.app"
ICON_SVG="$ROOT/website/static/logo/localgpt-icon.svg"

if [ -z "${APP_FEATURES+set}" ]; then
  if [ "$(uname -m)" = arm64 ]; then APP_FEATURES=local-llm-metal; else APP_FEATURES=""; fi
fi
# One word, so it expands safely unquoted (macOS bash 3.2 has no safe empty
# arrays under `set -u`).
FEATURE_FLAG=""
[ -z "$APP_FEATURES" ] || FEATURE_FLAG="--features=$APP_FEATURES"

echo "==> cargo build ($PROFILE${APP_FEATURES:+, features: $APP_FEATURES})"
cd "$ROOT"
if [ "$PROFILE" = release ]; then
  cargo build --release -p localgpt-app $FEATURE_FLAG
else
  cargo build -p localgpt-app $FEATURE_FLAG
fi
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/$PROFILE/localgpt-app"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/localgpt-app"

# App icon from the family mark: QuickLook renders the SVG, sips makes the
# sizes, iconutil packs them. Best effort; the app works without an icon.
ICON_KEY=""
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
if qlmanage -t -s 1024 -o "$WORK" "$ICON_SVG" >/dev/null 2>&1 \
  && [ -f "$WORK/$(basename "$ICON_SVG").png" ]; then
  MASTER="$WORK/$(basename "$ICON_SVG").png"
  ICONSET="$WORK/AppIcon.iconset"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$MASTER" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$MASTER" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  done
  if iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"; then
    ICON_KEY="<key>CFBundleIconFile</key><string>AppIcon</string>"
  fi
fi
[ -n "$ICON_KEY" ] || echo "    warn: couldn't render the app icon; bundling without one"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LocalGPT</string>
  <key>CFBundleDisplayName</key><string>LocalGPT</string>
  <key>CFBundleIdentifier</key><string>app.localgpt.desktop</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>localgpt-app</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  ${ICON_KEY}
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>LocalGPT uses your local network to host and join collaborative world-building sessions.</string>
  <key>NSBonjourServices</key>
  <array><string>_localgpt-world._udp</string></array>
</dict>
</plist>
PLIST

if [ -n "$IDENTITY" ]; then
  echo "==> signing with $IDENTITY (hardened runtime)"
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
else
  echo "==> ad-hoc signing (this Mac only; see APPLE_SIGNING_IDENTITY)"
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"

if [ "$MAKE_DMG" = 1 ]; then
  DMG="$OUT/LocalGPT-$VERSION.dmg"
  echo "==> $DMG"
  STAGE="$WORK/dmg"
  mkdir -p "$STAGE"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG"
  hdiutil create -volname "LocalGPT" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
  if [ -n "$IDENTITY" ]; then
    codesign --force --timestamp --sign "$IDENTITY" "$DMG"
  fi
  if [ -n "$NOTARY_PROFILE" ]; then
    echo "==> notarizing (this waits for Apple)"
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
    spctl --assess --type open --context context:primary-signature --verbose "$DMG"
  fi
fi

du -sh "$APP" | sed 's/^/    /'
echo "done: open \"$APP\""
