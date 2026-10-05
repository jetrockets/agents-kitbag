#!/bin/bash
# Put "Agents Kitbag.app" in a disk image with a link to Applications.
#
#   packaging/macos/dmg.sh <Agents Kitbag.app> <output.dmg>
#
# The name fastframe-update looks for is
# agents-kitbag-v<version>-macos-universal.dmg.
set -euo pipefail

app="$1"
output="$2"

[ -e "$output" ] && { echo "output already exists: $output" >&2; exit 1; }
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "Agents Kitbag" -srcfolder "$staging" -format UDZO "$output" >/dev/null
hdiutil verify "$output" >/dev/null

echo "$output"
