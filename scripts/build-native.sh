#!/usr/bin/env bash
# Builds the Akshara Rust core (libakshara) for a target platform and puts it
# where the Flutter platform project expects it.
#
#   scripts/build-native.sh linux|windows|macos|ios|android [--debug]
#
# Desktop builds (linux, windows) also happen automatically from CMake during
# `flutter build`; this script is mainly for macOS, iOS and Android.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

target="${1:-}"
profile="release"
flags=(--release)
if [[ "${2:-}" == "--debug" ]]; then profile="debug"; flags=(); fi

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' is required: $2" >&2; exit 1; }
}
need cargo "install Rust from https://rustup.rs (no admin rights needed)"
[[ -f scripts/local-env.sh ]] && source scripts/local-env.sh

case "$target" in
    linux|windows)
        cargo build "${flags[@]}" -p akshara-ffi
        ;;
    macos)
        need lipo "run this on macOS with the Xcode command line tools"
        rustup target add aarch64-apple-darwin x86_64-apple-darwin
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            cargo build "${flags[@]}" -p akshara-ffi --target "$t"
        done
        out="target/universal-apple-darwin/$profile"
        mkdir -p "$out"
        lipo -create -output "$out/libakshara.a" \
            "target/aarch64-apple-darwin/$profile/libakshara.a" \
            "target/x86_64-apple-darwin/$profile/libakshara.a"
        echo "universal static library: $out/libakshara.a"
        ;;
    ios)
        need xcrun "run this on macOS with Xcode installed"
        rustup target add aarch64-apple-ios aarch64-apple-ios-sim
        for t in aarch64-apple-ios aarch64-apple-ios-sim; do
            cargo build "${flags[@]}" -p akshara-ffi --target "$t"
        done
        ;;
    android)
        need cargo-ndk "cargo install cargo-ndk, and install the Android NDK (ANDROID_NDK_HOME)"
        rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
        cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
            -o app/android/app/src/main/jniLibs build "${flags[@]}" -p akshara-ffi
        ;;
    *)
        echo "usage: $0 linux|windows|macos|ios|android [--debug]" >&2
        exit 2
        ;;
esac
echo "Akshara core built for $target ($profile)."
