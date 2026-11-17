# Akshara Office: Flutter app

The user interface of [Akshara Office](../README.md). The document engines
live in the Rust workspace one level up and are reached through `dart:ffi`
(`lib/core/native.dart`).

```
lib/main.dart        shell: home screen, tabs, file commands, language, theme
lib/writer/          word processor UI
lib/calc/            spreadsheet UI
lib/math/            formula editor and renderer
lib/core/            FFI binding, file dialog, paths/settings, fonts, document model
lib/l10n/            UI translations (9 languages)
assets/fonts/        bundled Google Noto fonts (SIL OFL 1.1)
test/                widget and protocol tests (run against the real Rust core)
```

Run from the repository root with `./run.sh`, or:

```bash
cargo build -p akshara-ffi
cd app && flutter test && flutter run -d linux   # or windows / macos
```

See [../docs/building.md](../docs/building.md) for every platform.
