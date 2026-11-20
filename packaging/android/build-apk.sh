#!/usr/bin/env bash
# Builds release APKs (one per CPU architecture) for sideloading. Needs Rust,
# cargo-ndk, the Android SDK/NDK and Flutter; none require admin rights.
# Installing an APK needs no root: enable "Install unknown apps" for your file
# manager, or use `adb install`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
scripts/build-native.sh android
(cd app && { flutter pub get --offline >/dev/null || flutter pub get; } && flutter build apk --release --split-per-abi -PskipRustCore)
version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
mkdir -p dist
for apk in app/build/app/outputs/flutter-apk/app-*-release.apk; do
    abi="$(basename "$apk" | sed -E 's/^app-(.*)-release\.apk$/\1/')"
    cp "$apk" "dist/akshara-$version-android-$abi.apk"
done
echo "APKs copied to dist/. For Google Play use: flutter build appbundle"
