#!/bin/sh
# Install Agents Kitbag for this user: the programs in a folder that stays,
# and an entry in the applications menu.
#
#   ./install.sh
#
# The folder matters: a server whose token is in the Secret Service is
# started through agents-kitbag-runner, and Claude Desktop's config names it
# by its path here.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
target="$data/agents-kitbag"

mkdir -p "$target" "$data/applications" "$data/icons/hicolor/512x512/apps"
for file in agents-kitbag agents-kitbag-runner agents-kitbag-portable.txt; do
    cp "$here/$file" "$target/$file"
done
chmod 755 "$target/agents-kitbag" "$target/agents-kitbag-runner"
cp "$here/agents-kitbag.png" "$data/icons/hicolor/512x512/apps/agents-kitbag.png"

cat > "$data/applications/agents-kitbag.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Agents Kitbag
Comment=MCP servers for Claude Desktop
Exec="$target/agents-kitbag"
Icon=agents-kitbag
Terminal=false
Categories=Development;Utility;
DESKTOP

echo "Installed to $target"
echo "Start it from the applications menu, or run: $target/agents-kitbag"
