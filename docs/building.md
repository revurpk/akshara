# Building Akshara Office

Everything below installs into your user account; no step needs administrator
rights, except where an operating system's own developer tools (Xcode,
Visual Studio) must be installed once.

## Prerequisites (all platforms)

1. **Rust** (stable, 1.85 or newer): `curl -sSf https://sh.rustup.rs | sh`
   (Windows: `rustup-init.exe`). It installs into `~/.cargo`.
2. **Flutter SDK** 3.50 or newer: download and unpack from
   <https://docs.flutter.dev/get-started/install>, and add `flutter/bin` to `PATH`.
3. Check with `cargo --version` and `flutter doctor`.

The app has no third-party Dart packages, only the Flutter SDK and the
first-party `material_ui` and `cupertino_ui` packages, so after the first `flutter pub get` it
builds offline (`flutter pub get --offline`).

## Rust core and tests

```bash
cargo build --release -p akshara-ffi      # target/release/libakshara.{so,dylib,dll}
cargo test --workspace                    # all Rust tests
cargo clippy --workspace --all-targets    # lints
cargo doc --no-deps --open                # API documentation
cargo run -p icongen --release            # regenerate all icons from branding/*.svg
```

## Linux desktop

Needs `clang`, `cmake`, `ninja`, `pkg-config` and the GTK 3 development
files (Debian/Ubuntu: `clang cmake ninja-build pkg-config libgtk-3-dev`).
Without root, you can unpack the same packages into a private sysroot and put
small wrapper scripts on `PATH`; this repository was developed that way (see
`scripts/local-env.sh` if present).

```bash
./run.sh                               # build and start
packaging/linux/build-package.sh       # dist/akshara-<ver>-linux-x64.tar.gz
```

`app/linux/CMakeLists.txt` builds the Rust core with Cargo automatically and
bundles `libakshara.so` into `bundle/lib/`. Set `AKSHARA_PREBUILT_LIB` to
use a prebuilt library instead.

## Windows desktop

Needs Visual Studio 2022 (or the Build Tools) with the "Desktop development
with C++" workload, Rust with the MSVC toolchain, Git, Flutter, and
[Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installer.

The easiest way to get a Windows installer is the PowerShell script. It
checks every dependency, fetches what is missing, and then builds:

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\build-installer.ps1
```

| Dependency | If missing |
|---|---|
| Visual Studio 2022 C++ tools + CMake | stops with instructions; `-InstallVsBuildTools` installs them (UAC prompt) |
| Rust (MSVC, 1.85+) | installs rustup per user (`~\.cargo`) |
| Git | unpacks MinGit into the tools folder |
| Flutter (Dart 3.14+) | unpacks the current stable SDK into the tools folder |
| Inno Setup 6 | installs it per user into the tools folder |

Fetched tools go to `%LOCALAPPDATA%\Akshara\build-tools` (change with
`-ToolsDir`) and are reused next time. Apart from the Visual Studio Build
Tools, nothing needs administrator rights. Every download is over HTTPS and
is verified before use: rustup and Flutter by their published SHA-256 sums,
MinGit and Inno Setup (from their official GitHub releases) by GitHub's
SHA-256 asset digests, and the installers also by their Authenticode
signatures. If GitHub's anonymous API limit (60 requests an hour) is hit,
set `GITHUB_TOKEN`.

| Option | Effect |
|---|---|
| `-CheckOnly` | report which dependencies are present, build nothing |
| `-Offline` | never download; stop if something is missing |
| `-InstallVsBuildTools` | install the Visual Studio C++ Build Tools if missing |
| `-Test` | run the Rust and Flutter tests before packaging |
| `-NoPortable` | skip the portable zip |
| `-ToolsDir <path>` | where fetched tools go |

It writes `dist\akshara-<ver>-windows-x64-setup.exe` and
`dist\akshara-<ver>-windows-x64-portable.zip`, and prints their SHA-256 sums.

With the tools already installed, Git Bash works too:

```bash
./run.sh                                    # build and start
packaging/windows/build-installer.sh        # per-user setup .exe + portable zip
```

CMake builds the Rust core and installs `akshara.dll` next to the
executable.

## macOS

Needs Xcode (with command-line tools), Rust and Flutter.

```bash
scripts/build-native.sh macos   # universal static library (arm64 + x86_64)
./run.sh                        # build and start
packaging/macos/build-dmg.sh    # dist/akshara-<ver>-macos.dmg
```

### Cross-compiling the core from Linux (or Windows)

The Rust core is linked into the app as a **static** library, and building a
static library needs no Apple linker or SDK. So the macOS core can be built
on Linux with just rustup (the targets are downloaded once):

```bash
scripts/build-native.sh macos
# -> target/universal-apple-darwin/release/libakshara.a (arm64 + x86_64)
```

The script builds `aarch64-apple-darwin` and `x86_64-apple-darwin` static
libraries with `cargo rustc --crate-type staticlib` and merges them with
`lipo` where it exists, otherwise with `tools/fatlib`, a small Rust tool that
writes the same universal ("fat") file format. `scripts/build-native.sh ios`
works the same way for the device and simulator libraries.

Flutter's macOS build (Dart AOT compilation for macOS, the Swift runner,
code signing) and `hdiutil` only exist on macOS, so the app bundle and the
disk image are made there, from the prebuilt core:

```bash
# on the Mac, with target/universal-apple-darwin/release/libakshara.a copied over
packaging/macos/build-dmg.sh --prebuilt-core
```

`.github/workflows/macos-dmg.yml` automates this: an Ubuntu job
cross-compiles the core, and a macOS job builds `Akshara.app` and the
`.dmg` from it (run it from the Actions tab, or push a `v*` tag).

The Rust core is linked statically through `macos/Flutter/Flutter-*.xcconfig`
(`-force_load` plus `-u` for the three entry points). Release builds are
sandboxed. For public distribution, sign with a Developer ID and notarize;
the script applies an ad-hoc signature only.

## Android

Needs the Android SDK and NDK (Android Studio, or the command-line tools),
Rust targets and `cargo-ndk`:

```bash
cargo install cargo-ndk
export ANDROID_NDK_HOME=/path/to/ndk
packaging/android/build-apk.sh      # dist/app-<abi>-release.apk
```

Gradle can also build the core itself: the `buildRustCore` task in
`app/android/app/build.gradle.kts` runs `cargo ndk` before every build (pass
`-PskipRustCore` to use libraries you built separately).

## iOS

Needs Xcode, Rust and Flutter, and an Apple developer team for signing.

```bash
scripts/build-native.sh ios     # static libraries for device and simulator
packaging/ios/build-ipa.sh
```

The static library is linked through `ios/Flutter/{Debug,Release}.xcconfig`.

## Flutter tests

```bash
cargo build -p akshara-ffi
cd app && flutter test       # finds ../target/debug/libakshara.* automatically
flutter analyze
```

Set `AKSHARA_LIB=/path/to/libakshara.so` to test against another build.

## Debug screenshots

Debug builds can render the Home screen and every open tab to PNG files and
then exit, which is useful for documentation and visual checks:

```bash
AKSHARA_SCREENSHOT_DIR=/tmp/shots app/build/linux/x64/debug/bundle/akshara doc.odt
```

This hook is compiled out of release builds.

## Verified in this repository

The macOS universal and iOS static core libraries were cross-compiled on
Linux and checked (`file` reports a Mach-O universal binary with x86_64 and
arm64 slices exporting the three entry points); they have not been linked
into an app here.


The Rust workspace (tests, clippy, rustdoc), the Flutter analyzer and tests,
and the **Linux** desktop release build and per-user installer were built and
exercised on Linux x86-64. The Windows, macOS, Android and iOS projects,
build scripts and installer definitions are complete, but they need those
platforms' toolchains to build and have not been compiled here.
