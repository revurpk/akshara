#!/usr/bin/env bash
# Builds the Akshara Rust core (libakshara) for a target platform and puts it
# where the Flutter platform project expects it.
#
#   scripts/build-native.sh linux|windows|macos|ios|android [--debug]
#
# Desktop builds (linux, windows) also happen automatically from CMake during
# `flutter build`; this script is mainly for macOS, iOS and Android.
#
# The Apple libraries are static (`.a`), so building them needs no Apple
# linker: `macos` and `ios` also work on Linux and Windows (Git Bash) with
# just rustup. The universal macOS library is merged with `lipo` when it is
# available and with tools/fatlib otherwise. Building the app bundle and the
# disk image still needs a Mac (packaging/macos/build-dmg.sh --prebuilt-core).
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
        need rustup "install Rust with rustup (https://rustup.rs) to add the macOS targets"
        rustup target add aarch64-apple-darwin x86_64-apple-darwin
        for t in aarch64-apple-darwin x86_64-apple-darwin; do
            # Only the static library: no Apple linker needed.
            cargo rustc "${flags[@]}" -p akshara-ffi --target "$t" --crate-type staticlib
        done
        out="target/universal-apple-darwin/$profile"
        mkdir -p "$out"
        slices=("target/aarch64-apple-darwin/$profile/libakshara.a" "target/x86_64-apple-darwin/$profile/libakshara.a")
        if command -v lipo >/dev/null 2>&1 && lipo -info "${slices[0]}" >/dev/null 2>&1; then
            lipo -create -output "$out/libakshara.a" "${slices[@]}"
        else
            cargo run --quiet --release -p fatlib -- -o "$out/libakshara.a" "${slices[@]}"
        fi
        echo "universal static library: $out/libakshara.a"
        ;;
    ios)
        need rustup "install Rust with rustup (https://rustup.rs) to add the iOS targets"
        rustup target add aarch64-apple-ios aarch64-apple-ios-sim
        for t in aarch64-apple-ios aarch64-apple-ios-sim; do
            cargo rustc "${flags[@]}" -p akshara-ffi --target "$t" --crate-type staticlib
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
