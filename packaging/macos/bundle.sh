#!/bin/bash
# Build "Agents Kitbag.app" from the two binaries on macOS.
#
#   packaging/macos/bundle.sh <app binary> <runner binary> <output.app> <version>
#
# The runner sits beside the app in Contents/MacOS: the config written for a
# server with a stored token names it by that path, and an update swaps the
# whole bundle, so the path stays valid.
#
# Set CODESIGN_IDENTITY to sign with a certificate in the keychain. Otherwise
# the bundle gets the ad-hoc signature arm64 requires.
set -euo pipefail

binary="$1"
runner="$2"
app="$3"
version="$4"
numeric_version="${version%%-*}"
here="$(cd "$(dirname "$0")" && pwd)"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/agents-kitbag"
cp "$runner" "$app/Contents/MacOS/agents-kitbag-runner"
chmod 755 "$app/Contents/MacOS/agents-kitbag" "$app/Contents/MacOS/agents-kitbag-runner"
sed "s/__VERSION__/$numeric_version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

iconset="$(mktemp -d)/agents-kitbag.iconset"
mkdir -p "$iconset"
# iconutil reads these base sizes and optional @2x versions. It ignores 64x64.
for size in 16 32 128 256 512; do
    sips -z $size $size "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/agents-kitbag.icns"

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    # Apple's timestamp service only answers for its own certificates.
    case "$CODESIGN_IDENTITY" in
        "Developer ID"*) timestamp=--timestamp ;;
        *) timestamp=--timestamp=none ;;
    esac
    codesign --force "$timestamp" --options runtime --sign "$CODESIGN_IDENTITY" \
        "$app/Contents/MacOS/agents-kitbag-runner"
    codesign --force "$timestamp" --options runtime --sign "$CODESIGN_IDENTITY" "$app"
else
    codesign --force --sign - "$app/Contents/MacOS/agents-kitbag-runner"
    codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"

echo "$app"
