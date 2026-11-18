#!/usr/bin/env bash
# Bumps the Akshara version everywhere it is recorded. Run before every
# commit (see CLAUDE.md):
#
#   scripts/bump-version.sh          # 0.1.0 -> 0.1.1 (patch, the default)
#   scripts/bump-version.sh minor    # 0.1.1 -> 0.2.0
#   scripts/bump-version.sh major    # 0.2.0 -> 1.0.0
#
# Updates Cargo.toml (workspace version, shown in About and used for the
# package file names), Cargo.lock, app/pubspec.yaml (version and build
# number, +1 on every bump), the Inno Setup default and the file names in
# docs/installing.md.
set -e
cd "$(dirname "${BASH_SOURCE[0]}")/.."

part="${1:-patch}"
old="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
IFS=. read -r major minor patch <<< "$old"
case "$part" in
    patch) patch=$((patch + 1)) ;;
    minor) minor=$((minor + 1)); patch=0 ;;
    major) major=$((major + 1)); minor=0; patch=0 ;;
    *) echo "usage: $0 [patch|minor|major]" >&2; exit 1 ;;
esac
new="$major.$minor.$patch"

build="$(grep -m1 '^version:' app/pubspec.yaml | sed -E 's/.*\+([0-9]+).*/\1/')"
build=$((build + 1))

sed -i -E "0,/^version = \"$old\"/s//version = \"$new\"/" Cargo.toml
sed -i -E "s/^version: .*/version: $new+$build/" app/pubspec.yaml
sed -i -E "s/(#define AppVersion )\"$old\"/\1\"$new\"/" packaging/windows/akshara.iss
sed -i "s/akshara-$old-/akshara-$new-/g" docs/installing.md

# Refresh the workspace crates' versions in the lock file (no downloads).
if command -v cargo &> /dev/null; then
    cargo update --workspace --offline --quiet
else
    sed -i -E "/^name = \"(office-[a-z]+|akshara-ffi|icongen)\"$/{n;s/^version = \"$old\"/version = \"$new\"/}" Cargo.lock
fi

echo "Version $old -> $new (build $build)"
