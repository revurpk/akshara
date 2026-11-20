# Akshara Office: project notes

- Rust workspace (crates/odf, writer, calc, draw, math, indic, voice, ai,
  ffi; tools/icongen, tools/fatlib) and a Flutter app in app/. crates/draw
  holds Impress/Draw shapes, pictures, the Mermaid engine and ODP/ODG I/O;
  crates/voice is offline Whisper speech-to-text (whisper.cpp ggml model
  files, cpal microphone capture); crates/ai is the Ollama client.
- This is not a Python project: run.sh checks for cargo and flutter instead
  of uv (deliberate exception to the global rule).
- The macOS/iOS Rust core cross-compiles on Linux:
  `scripts/build-native.sh macos` (static libs + tools/fatlib); the app
  bundle and .dmg need a Mac or the macos-dmg GitHub workflow.
- License: Apache-2.0 (Cargo `license.workspace`), fonts are SIL OFL.
- Check everything with:
  `cargo test --workspace && cargo clippy --workspace --all-targets`
  `cargo build -p akshara-ffi && (cd app && flutter analyze && flutter test)`
- Core crates forbid `unsafe`; only crates/ffi/src/lib.rs may use it.
- third_party/teltools is vendored upstream code (excluded from the
  workspace): keep changes minimal, list them in PATCHES.md, and run
  `(cd third_party/teltools && cargo test)` to check upstream vectors.
- UI strings go through `t('English text')`; add every new key to all
  tables in app/lib/l10n (test/l10n_test.dart enforces this).
- The app must not use the network or need admin rights; do not add
  plugins/packages that do. Sole exception: the opt-in AI chat talks to
  Ollama over HTTP on loopback only (office-ai refuses any other host).
- Linux builds need the ALSA development files (`libasound2-dev`) for
  microphone capture. Real-model voice test: set AKSHARA_WHISPER_MODEL and
  AKSHARA_WHISPER_WAV (e.g. whisper.cpp's ggml-tiny.bin and jfk.wav).
- Every new commit bumps the version: run `scripts/bump-version.sh`
  (patch by default; `minor` / `major` when asked) before committing and
  include its changes in the commit. It updates Cargo.toml, Cargo.lock,
  app/pubspec.yaml (+1 build number), the Inno Setup default and
  docs/installing.md. Amending a commit does not bump again.
