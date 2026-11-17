# Akshara Office: project notes

- Rust workspace (crates/odf, writer, calc, math, ffi; tools/icongen) and a
  Flutter app in app/. This is not a Python project: run.sh checks for cargo
  and flutter instead of uv (deliberate exception to the global rule).
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
  plugins/packages that do.
