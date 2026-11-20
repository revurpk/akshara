#!/usr/bin/env bash
# Builds an iOS archive. Apple requires every app to be signed: set your
# development team in Xcode (Runner > Signing & Capabilities) first. Personal
# installs work with a free Apple ID (7-day provisioning); distribution needs
# TestFlight / the App Store or enterprise/ad-hoc provisioning.
#
#   packaging/ios/build-ipa.sh                   # signed archive (needs a team)
#   packaging/ios/build-ipa.sh --unsigned        # dist/akshara-<ver>-ios-unsigned.ipa
#   ... --prebuilt-core                          # use target/aarch64-apple-ios/
#                                                # release/libakshara.a built
#                                                # elsewhere (Linux works)
#
# The unsigned IPA cannot be installed as is; sign it with your own
# certificate and provisioning profile (e.g. with a re-signing tool).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
unsigned=false prebuilt=false
for arg in "$@"; do
    case "$arg" in
        --unsigned) unsigned=true ;;
        --prebuilt-core) prebuilt=true ;;
        *) echo "usage: $0 [--unsigned] [--prebuilt-core]" >&2; exit 2 ;;
    esac
done
version="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
if $prebuilt; then
    core="target/aarch64-apple-ios/release/libakshara.a"
    [[ -f "$core" ]] || { echo "error: $core is missing (build it with scripts/build-native.sh ios)" >&2; exit 1; }
else
    scripts/build-native.sh ios
fi
cd app
{ flutter pub get --offline >/dev/null || flutter pub get; }
if $unsigned; then
    flutter build ios --release --no-codesign
    rm -rf build/ios/unsigned && mkdir -p build/ios/unsigned/Payload ../dist
    cp -R build/ios/iphoneos/Runner.app build/ios/unsigned/Payload/
    (cd build/ios/unsigned && zip -qr "../../../../dist/akshara-$version-ios-unsigned.ipa" Payload)
    echo "Created dist/akshara-$version-ios-unsigned.ipa (unsigned)"
else
    flutter build ipa --release
    echo "Archive: app/build/ios/archive/Runner.xcarchive; IPA (if exported): app/build/ios/ipa/"
fi
