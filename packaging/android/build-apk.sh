#!/usr/bin/env bash
# Builds release APKs (one per CPU architecture) for sideloading. Needs Rust,
# cargo-ndk, the Android SDK/NDK and Flutter; none require admin rights.
# Installing an APK needs no root: enable "Install unknown apps" for your file
# manager, or use `adb install`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
scripts/build-native.sh android
(cd app && flutter pub get --offline >/dev/null && flutter build apk --release --split-per-abi -PskipRustCore)
mkdir -p dist && cp app/build/app/outputs/flutter-apk/app-*-release.apk dist/
echo "APKs copied to dist/. For Google Play use: flutter build appbundle"
