#!/bin/bash
# Copies Akshara.app into ~/Applications (your own Applications folder).
# No administrator password is needed.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HOME/Applications"
rm -rf "$HOME/Applications/Akshara.app"
cp -R "$here/Akshara.app" "$HOME/Applications/"
xattr -dr com.apple.quarantine "$HOME/Applications/Akshara.app" 2>/dev/null || true
echo "Installed to ~/Applications/Akshara.app"
open "$HOME/Applications/Akshara.app"
