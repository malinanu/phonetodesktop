#!/bin/bash
# Build "Phone Remote.app" and a .dmg on macOS.
#   make-dmg.sh VERSION UNIVERSAL_BINARY [OUT_DIR]
# Optional signing and notarization when these environment variables are set (CI secrets):
#   MACOS_SIGN_IDENTITY   e.g. "Developer ID Application: Name (TEAMID)"; without it the app is ad-hoc signed
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD   notarize and staple the dmg
set -euo pipefail

VERSION=$1; BINARY=$2; OUT=${3:-dist}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
APP_NAME="Phone Remote"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/dmg/$APP_NAME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$OUT"

install -m 755 "$BINARY" "$APP/Contents/MacOS/phone-remote"

# The bundle's own executable is a tiny launcher: start the agent if needed and show the dashboard.
cat > "$APP/Contents/MacOS/PhoneRemote" <<'LAUNCH'
#!/bin/sh
exec "$(dirname "$0")/phone-remote" open
LAUNCH
chmod 755 "$APP/Contents/MacOS/PhoneRemote"

# Icon: build an .icns from the 512px PNG.
ICONSET="$WORK/phone-remote.iconset"
mkdir -p "$ICONSET"
for s in 16 32 64 128 256 512; do
  sips -z $s $s "$ROOT/installer/assets/phone-remote.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
done
cp "$ICONSET/icon_32x32.png"   "$ICONSET/icon_16x16@2x.png"
cp "$ICONSET/icon_64x64.png"   "$ICONSET/icon_32x32@2x.png"
cp "$ICONSET/icon_256x256.png" "$ICONSET/icon_128x128@2x.png"
cp "$ICONSET/icon_512x512.png" "$ICONSET/icon_256x256@2x.png"
cp "$ROOT/installer/assets/phone-remote.png" "$ICONSET/icon_512x512@2x.png"
rm "$ICONSET/icon_64x64.png"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/phone-remote.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>app.phoneremote.agent</string>
  <key>CFBundleExecutable</key><string>PhoneRemote</string>
  <key>CFBundleIconFile</key><string>phone-remote</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSLocalNetworkUsageDescription</key><string>Phone Remote talks to your phone over your local network.</string>
  <key>NSBonjourServices</key><array><string>_phoneremote._tcp</string></array>
</dict>
</plist>
PLIST

# Sign: Developer ID when available (a stable identity keeps the Accessibility permission across updates),
# otherwise ad-hoc so the binary at least runs on Apple silicon.
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP/Contents/MacOS/phone-remote"
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  codesign --force --sign - "$APP/Contents/MacOS/phone-remote"
  codesign --force --sign - "$APP"
fi

ln -s /Applications "$WORK/dmg/Applications"
DMG="$OUT/PhoneRemote-$VERSION-macos.dmg"
hdiutil create -volname "$APP_NAME" -srcfolder "$WORK/dmg" -ov -format UDZO "$DMG" >/dev/null

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "$DMG"
  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$DMG"
  fi
fi
echo "$DMG"
