#!/usr/bin/env bash
# Builds and starts Akshara Office on this desktop (Linux, macOS, or Windows
# under Git Bash). Extra arguments are passed to the app, e.g. files to open:
#
#   ./run.sh                 # start the app
#   ./run.sh report.odt      # open a document
#   AKSHARA_DEBUG=1 ./run.sh # debug build (faster to compile)
#
# Note: this project is Rust + Flutter, not Python, so instead of uv the
# script checks for cargo and flutter (see README "Prerequisites").
set -e
cd "$(dirname "${BASH_SOURCE[0]}")"

# Optional machine-specific setup (PATH to toolchains etc.), not in git.
if [ -f scripts/local-env.sh ]; then
    # shellcheck disable=SC1091
    source scripts/local-env.sh
fi

if ! command -v cargo &> /dev/null; then
    echo "cargo (Rust) is not installed. Install it without admin rights: curl -sSf https://sh.rustup.rs | sh" >&2
    exit 1
fi
if ! command -v flutter &> /dev/null; then
    echo "flutter is not installed. Download the SDK from https://docs.flutter.dev/get-started/install and add flutter/bin to PATH." >&2
    exit 1
fi

mode="release"
[ -n "${AKSHARA_DEBUG:-}" ] && mode="debug"

case "$(uname -s)" in
    Linux*)  platform="linux" ;;
    Darwin*) platform="macos" ;;
    MINGW*|MSYS*|CYGWIN*) platform="windows" ;;
    *) echo "Unsupported desktop: $(uname -s)" >&2; exit 1 ;;
esac

if [ "$platform" = "macos" ]; then
    scripts/build-native.sh macos $([ "$mode" = debug ] && echo --debug)
fi

cd app
flutter pub get --offline > /dev/null || flutter pub get
flutter build "$platform" --"$mode"

case "$platform" in
    linux)   exec "build/linux/x64/$mode/bundle/akshara" "$@" ;;
    macos)   exec "build/macos/Build/Products/$( [ "$mode" = debug ] && echo Debug || echo Release )/Akshara.app/Contents/MacOS/Akshara" "$@" ;;
    windows) exec "build/windows/x64/runner/$( [ "$mode" = debug ] && echo Debug || echo Release )/akshara.exe" "$@" ;;
esac
