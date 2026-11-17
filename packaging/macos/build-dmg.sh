#!/usr/bin/env bash
# Builds Akshara.app and a disk image. Run on macOS with Xcode, Rust and
# Flutter. Users install by dragging the app to the "Applications" folder in
# their home directory (~/Applications), which needs no administrator rights,
# or by double-clicking "Install for me.command" inside the image.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
scripts/build-native.sh macos
(cd app && flutter pub get --offline >/dev/null && flutter build macos --release)
stage="dist/dmg"
rm -rf "$stage" && mkdir -p "$stage"
cp -R app/build/macos/Build/Products/Release/Akshara.app "$stage/"
cp packaging/macos/install-for-me.command "$stage/Install for me.command"
chmod +x "$stage/Install for me.command"
cp LICENSE NOTICE "$stage/"
# Ad-hoc signature so Gatekeeper can verify integrity; replace with a
# Developer ID signature and notarization for public distribution.
codesign --force --deep --sign - "$stage/Akshara.app"
hdiutil create -volname "Akshara Office" -srcfolder "$stage" -ov -format UDZO "dist/akshara-$version-macos.dmg"
echo "Created dist/akshara-$version-macos.dmg"
