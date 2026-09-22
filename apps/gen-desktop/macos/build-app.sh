#!/usr/bin/env bash
# Package localgpt-gen as "LocalGPT Gen.app" (and optionally a .dmg).
#
# The app is the same binary as `cargo install localgpt-gen`. Launched from
# Finder it has no terminal, so it starts in desktop mode: prompts come from
# the panel in the window (see crates/gen/src/desktop/).
#
# Usage: apps/gen-desktop/macos/build-app.sh [--debug] [--dmg]
#   --debug  bundle the debug build (quick, for trying the bundle locally)
#   --dmg    also create dist/LocalGPT-Gen-<version>.dmg
#
# Output goes to apps/gen-desktop/dist/. The app is ad-hoc signed only;
# distributing it needs a Developer ID signature and notarization (README.md).
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

VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
OUT="$ROOT/apps/gen-desktop/dist"
APP="$OUT/LocalGPT Gen.app"
ICON_SVG="$ROOT/website/static/logo/localgpt-icon.svg"

echo "==> cargo build ($PROFILE)"
cd "$ROOT"
if [ "$PROFILE" = release ]; then
  cargo build --release -p localgpt-gen
else
  cargo build -p localgpt-gen
fi
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/$PROFILE/localgpt-gen"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/localgpt-gen"

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
  <key>CFBundleName</key><string>LocalGPT Gen</string>
  <key>CFBundleDisplayName</key><string>LocalGPT Gen</string>
  <key>CFBundleIdentifier</key><string>app.localgpt.gen</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>localgpt-gen</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  ${ICON_KEY}
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>LocalGPT Gen uses your local network to host and join collaborative world-building sessions.</string>
  <key>NSBonjourServices</key>
  <array><string>_localgpt-world._udp</string></array>
</dict>
</plist>
PLIST

echo "==> ad-hoc signing"
codesign --force --sign - "$APP"

if [ "$MAKE_DMG" = 1 ]; then
  DMG="$OUT/LocalGPT-Gen-$VERSION.dmg"
  echo "==> $DMG"
  STAGE="$WORK/dmg"
  mkdir -p "$STAGE"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG"
  hdiutil create -volname "LocalGPT Gen" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
fi

du -sh "$APP" | sed 's/^/    /'
echo "done: open \"$APP\""
