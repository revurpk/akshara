#!/usr/bin/env bash
# Builds an iOS archive. Apple requires every app to be signed: set your
# development team in Xcode (Runner > Signing & Capabilities) first. Personal
# installs work with a free Apple ID (7-day provisioning); distribution needs
# TestFlight / the App Store or enterprise/ad-hoc provisioning.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
scripts/build-native.sh ios
(cd app && flutter pub get --offline >/dev/null && flutter build ipa --release)
echo "Archive: app/build/ios/archive/Runner.xcarchive; IPA (if exported): app/build/ios/ipa/"
