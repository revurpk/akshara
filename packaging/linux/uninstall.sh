#!/usr/bin/env bash
# Removes a per-user Akshara Office installation. Your documents and
# settings (~/.config/akshara) are kept unless you pass --purge.
set -euo pipefail
prefix="${XDG_DATA_HOME:-$HOME/.local/share}"
rm -rf "$prefix/akshara"
rm -f "$HOME/.local/bin/akshara" "$prefix/applications/org.akshara.Office.desktop"
find "$prefix/icons/hicolor" -name org.akshara.Office.png -delete 2>/dev/null || true
if [[ "${1:-}" == "--purge" ]]; then
    rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/akshara"
fi
echo "Akshara Office has been removed."
