#!/usr/bin/env bash
# Builds dist/akshara-<version>-linux-x64.tar.gz: the release bundle plus a
# per-user installer. Users extract it and run ./install.sh (no sudo).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
[[ -f scripts/local-env.sh ]] && source scripts/local-env.sh
version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
(cd app && { flutter pub get --offline >/dev/null || flutter pub get; } && flutter build linux --release)
name="akshara-$version-linux-x64"
stage="dist/$name"
rm -rf "$stage" && mkdir -p "$stage"
cp -a app/build/linux/x64/release/bundle "$stage/bundle"
cp packaging/linux/install.sh packaging/linux/uninstall.sh packaging/linux/org.akshara.Office.desktop "$stage/"
cp -a packaging/linux/icons "$stage/icons"
cp LICENSE NOTICE "$stage/"
cp packaging/linux/README.txt "$stage/README.txt"
tar -C dist -czf "dist/$name.tar.gz" "$name"
echo "Created dist/$name.tar.gz"
