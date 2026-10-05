#!/bin/sh
# Install Claude Toolkit for this user: the programs in a folder that stays,
# and an entry in the applications menu.
#
#   ./install.sh
#
# The folder matters: a server whose token is in the Secret Service is
# started through claude-toolkit-runner, and Claude Desktop's config names it
# by its path here.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
target="$data/claude-toolkit"

mkdir -p "$target" "$data/applications" "$data/icons/hicolor/512x512/apps"
for file in claude-toolkit claude-toolkit-runner claude-toolkit-portable.txt; do
    cp "$here/$file" "$target/$file"
done
chmod 755 "$target/claude-toolkit" "$target/claude-toolkit-runner"
cp "$here/claude-toolkit.png" "$data/icons/hicolor/512x512/apps/claude-toolkit.png"

cat > "$data/applications/claude-toolkit.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Claude Toolkit
Comment=MCP servers for Claude Desktop
Exec="$target/claude-toolkit"
Icon=claude-toolkit
Terminal=false
Categories=Development;Utility;
DESKTOP

echo "Installed to $target"
echo "Start it from the applications menu, or run: $target/claude-toolkit"
