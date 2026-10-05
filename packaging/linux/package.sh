#!/bin/bash
# Put the two programs, the update marker, the icon and the installer in the
# portable archive fastframe-update looks for:
#
#   claude-toolkit-v<version>-x86_64-unknown-linux-gnu.tar.gz
#     claude-toolkit-v<version>-x86_64-unknown-linux-gnu/
#       claude-toolkit
#       claude-toolkit-runner
#       claude-toolkit-portable.txt
#       claude-toolkit.png
#       install.sh
#
#   packaging/linux/package.sh <bin dir> <version> <out dir>
#
# The marker is what lets a copy replace itself. As on Windows, a self-update
# replaces claude-toolkit only; the runner's command line does not change
# between versions.
set -euo pipefail

bin="$1"
version="$2"
out="$3"
here="$(cd "$(dirname "$0")" && pwd)"
name="claude-toolkit-v$version-x86_64-unknown-linux-gnu"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/$name" "$out"
cp "$bin/claude-toolkit" "$bin/claude-toolkit-runner" "$staging/$name/"
cp "$here/claude-toolkit.png" "$here/install.sh" "$staging/$name/"
printf '%s' 'claude-toolkit-portable-v1' > "$staging/$name/claude-toolkit-portable.txt"
chmod 755 "$staging/$name/claude-toolkit" "$staging/$name/claude-toolkit-runner" "$staging/$name/install.sh"

tar -C "$staging" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
