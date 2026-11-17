#!/usr/bin/env bash
# Installs Akshara Office for the current user only. No sudo, no network.
#
#   ./install.sh            install (or upgrade) into ~/.local
#   ./install.sh --prefix DIR   install somewhere else you own
#
# What it writes:
#   ~/.local/share/akshara/              the application
#   ~/.local/bin/akshara                 a launcher
#   ~/.local/share/applications/...      menu entry and file associations
#   ~/.local/share/icons/hicolor/...     icons
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

prefix="${XDG_DATA_HOME:-$HOME/.local/share}"
bindir="$HOME/.local/bin"
if [[ "${1:-}" == "--prefix" && -n "${2:-}" ]]; then
    prefix="$2/share"; bindir="$2/bin"
fi
if [[ $EUID -eq 0 ]]; then
    echo "Please run this installer as your normal user, not root: it installs for you only." >&2
    exit 1
fi

app="$prefix/akshara"
echo "Installing Akshara Office into $app"
mkdir -p "$prefix" "$bindir" "$prefix/applications"
rm -rf "$app.new"
cp -a "$here/bundle" "$app.new"
rm -rf "$app"
mv "$app.new" "$app"

cat > "$bindir/akshara" <<LAUNCHER
#!/bin/sh
exec "$app/akshara" "\$@"
LAUNCHER
chmod +x "$bindir/akshara"

sed "s|@BIN@|$app/akshara|" "$here/org.akshara.Office.desktop" > "$prefix/applications/org.akshara.Office.desktop"
for size_dir in "$here"/icons/hicolor/*; do
    size="$(basename "$size_dir")"
    mkdir -p "$prefix/icons/hicolor/$size/apps"
    cp "$size_dir/apps/org.akshara.Office.png" "$prefix/icons/hicolor/$size/apps/"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$prefix/icons/hicolor" 2>/dev/null || true

echo "Done. Start it from your applications menu, or run: akshara"
case ":$PATH:" in *":$bindir:"*) ;; *) echo "(Add $bindir to your PATH to use the 'akshara' command.)";; esac
