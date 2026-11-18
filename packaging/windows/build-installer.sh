#!/usr/bin/env bash
# Builds the Windows release and the per-user installer. Run in Git Bash on
# Windows with Rust, Flutter and Inno Setup 6 (iscc) installed; all three can
# be installed without administrator rights.
# build-installer.ps1 (PowerShell) does the same and also fetches missing tools.
# Outputs: dist/akshara-<version>-windows-x64-setup.exe and a portable zip.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
(cd app && flutter pub get --offline >/dev/null && flutter build windows --release)
mkdir -p dist
iscc_bin="$(command -v iscc || echo "$LOCALAPPDATA/Programs/Inno Setup 6/ISCC.exe")"
"$iscc_bin" "//DAppVersion=$version" packaging/windows/akshara.iss
# Portable zip: unzip anywhere and run akshara.exe.
(cd app/build/windows/x64/runner/Release && powershell -NoProfile -Command \
    "Compress-Archive -Force -Path * -DestinationPath ../../../../../../dist/akshara-$version-windows-x64-portable.zip")
echo "Created dist/akshara-$version-windows-x64-setup.exe and the portable zip."
