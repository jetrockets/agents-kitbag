#!/bin/bash
# Draw the app icon's PNGs from packaging/icon.svg. Needs rsvg-convert
# (librsvg). Run it after the SVG changes and commit what it writes.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
rsvg-convert -w 1024 -h 1024 "$here/icon.svg" -o "$here/macos/icon-1024.png"
rsvg-convert -w 512 -h 512 "$here/icon.svg" -o "$here/linux/agents-kitbag.png"
