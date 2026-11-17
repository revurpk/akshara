# Architecture

Akshara has two halves: a **Rust core** that owns every document and file
format, and a **Flutter UI** that renders and edits. They talk through a tiny
C ABI carrying JSON.

```
┌──────────────────────── Flutter app (Dart) ────────────────────────┐
│ main.dart: shell, tabs, home screen, language & theme              │
│ writer/    paragraph editors (one TextField per paragraph)         │
│ calc/      custom-painted virtual grid, formula bar, sheet tabs    │
│ math/      StarMath editor + widget-based formula renderer         │
│ core/      native.dart (dart:ffi), file dialog, paths, fonts       │
│ l10n/      9 UI languages                                          │
└───────────────┬────────────────────────────────────────────────────┘
                │ akshara_call(JSON) -> JSON   (crates/ffi, C ABI)
┌───────────────┴──────────────────── Rust core ─────────────────────┐
│ ffi     registry of open documents, request dispatcher             │
│ writer  Document model, editing + undo, ODT, HTML/Markdown/text    │
│ calc    Workbook, formula parser/evaluator, dependency graph, ODS  │
│ math    StarMath lexer/parser, AST, MathML in/out, .odf            │
│ indic   transliteration (7 scripts) on vendored teltools           │
│ odf     hardened ZIP package + strict XML parser/writer, limits    │
└────────────────────────────────────────────────────────────────────┘
```

## Why this split

- **One source of truth.** All document state lives in Rust. The UI keeps
  only what it shows, so every platform behaves identically and the core is
  testable without a GUI.
- **Memory safety where the risk is.** Parsing untrusted files is the riskiest
  thing an office suite does, and it happens entirely in safe Rust
  (`unsafe_code = "forbid"` in every core crate).
- **No code generators, no plugins.** The FFI is three C functions. The app
  depends only on the Flutter SDK and the first-party `material_ui` package,
  so it builds offline and has no third-party native code.

## The core crates

### `office-odf`
- `Package`: reads ZIP containers with entry-count, size, total-size and
  compression-ratio limits enforced while inflating; rejects encrypted
  packages, unsafe names and duplicates; writes deterministic packages with
  `mimetype` first and stored, as ODF requires.
- `xml`: a strict, **iterative** (non-recursive) XML parser. It accepts no
  DTDs, does no entity expansion and accepts only UTF-8. Every qualified name
  is normalised to the conventional ODF prefix (`text:p`), whatever prefix the
  file used. `XmlWriter` always produces well-formed output and drops
  characters that are illegal in XML.
- `Limits`: one struct with every resource limit, passed to every reader.
- `Metadata` (`meta.xml`), atomic file writes, UTF-16 offset helpers.

### `office-writer`
- Flat model: `Block` = paragraph | table | page break. A `Paragraph` has a
  kind (body, title, heading 1-6, bullet/numbered list item with level,
  quote, code), an alignment and attributed `Run`s.
- `Document` editing operations are addressed in **UTF-16 offsets**, which is
  what Flutter uses. They include set/replace text with attribute
  inheritance, apply attributes, split/merge paragraphs, kinds, indentation,
  tables and find/replace. Every operation validates its input and is
  transactional: it works on a copy, so a failure changes nothing.
- Undo/redo stores whole-block snapshots and coalesces consecutive typing in
  one paragraph into a single step.
- ODT import resolves style inheritance. Run attributes come only from
  *automatic* styles, while named styles determine the paragraph kind. Lists
  are nested and can mix bullet and numbered levels.

### `office-calc`
- `formula`: a precedence-climbing parser for both the UI syntax
  (`=SUM(A1:B2;Sheet2.C3)`) and OpenFormula (`of:=SUM([.A1:.B2])`).
  Unparseable input is kept verbatim and shows `Err:509`.
- `eval`: a pure evaluator with 131 functions. It reads cells only through
  the `Host` trait and has no I/O. Text results are capped so that formulas
  cannot exhaust memory.
- `deps`: a reverse dependency graph. Ranges are indexed in 256-row stripes,
  with a separate list for very large ranges.
- `Workbook`: sparse sheets in `BTreeMap`s, interned styles and
  **incremental recalculation**. Only formulas that depend on a change (plus
  volatile ones like `RAND()`) are evaluated, in dependency order, by an
  iterative depth-first search that also marks every member of a cycle as
  `Err:522`. A chain of 200,000 dependent cells is handled without stack
  growth.
- Structural edits (inserting or deleting rows and columns, renaming sheets)
  rewrite references in every formula on every sheet, exactly like
  OpenOffice. Sort, fill and copy adjust relative references.

### `office-math`
- A lexer and a **total** recursive-descent parser for the StarMath 5
  language, with OpenOffice's precedence levels. It never fails: missing
  operands become placeholders and every problem is reported with a
  position, which allows a live preview while typing. Depth is bounded.
- MathML export embeds the StarMath source as an annotation (lossless round
  trip). MathML import translates foreign presentation MathML.

### `office-indic` and teltools
- `third_party/teltools` is Pradyumna Revur's
  [teltools](https://github.com/revurpk/teltools) (Apache-2.0), vendored at a
  pinned commit. Its RTS/ITRANS converter produces Telugu; it also converts
  Telugu ↔ Devanagari ↔ IAST. Two small local patches (`PATCHES.md`) add the
  "live" mode that upstream's own Keyman keyboard uses (no `om` → ॐ inside
  words, no tag skipping) and stop a panic on non-ASCII input. Upstream's
  test vectors still match byte for byte.
- `office-indic` maps teltools' Telugu onto Devanagari, Tamil, Kannada,
  Malayalam, Odia and Bengali through the shared ISCII-derived layout of the
  Unicode blocks. It uses Unicode 16 assignment tables and per-script
  fallbacks for missing letters, and never emits an unassigned code point.
  It also converts any supported script to any other, and to or from IAST.
- In the app, `core/translit.dart` is a `TextInputFormatter`: it keeps the
  Roman letters of the word being typed and re-runs teltools on every
  keystroke, so multi-letter keys (`kh`, `R^i`) resolve correctly. The same
  formatter works with hardware and soft keyboards and in every editor.

### `akshara-ffi`
- `akshara_alloc`, `akshara_free`, `akshara_call`: the only `unsafe` code in
  the project, about 20 lines with documented safety contracts.
- `api::Registry` holds the open documents by handle and dispatches about 60
  operations (see [ffi-protocol.md](ffi-protocol.md)). Panics are caught and
  returned as errors, so they never unwind into Dart.

## The Flutter app

- **native.dart** loads `libakshara` (or uses static linking on iOS and
  macOS), sends JSON, and runs slow calls (`open`, `save`) in a background
  isolate. The registry is process-wide, so isolates share documents.
- **Writer**: each paragraph is a `TextField` whose controller paints the
  attributed runs (`buildTextSpan`). Keystrokes call `writer.set_text`; the
  core computes the minimal edit and keeps the formatting around it. Enter,
  Backspace at the start, Tab and arrow keys at the edges are intercepted to
  split or merge paragraphs, indent, and move between paragraphs.
- **Calc**: a `CustomPainter` draws only the visible window of the 1,048,576
  × 1024 grid, fetched with `calc.view`. It supports text overflow into empty
  neighbours, `###` for numbers that don't fit, resizable columns, keyboard
  navigation, an in-cell editor, a formula bar and clipboard support (TSV).
- **Math**: the parser's JSON tree is rendered with nested widgets: stacked
  fractions, shrunken scripts, painted radicals, and stretchy brackets painted
  to the height of their content.
- **Fonts**: every text style uses the bundled Noto families with a fallback
  chain that covers all supported scripts (`core/fonts.dart`).
- **Localization**: `t('English text')` looks up the current language's table
  (`l10n/`). A test checks that every key used in the code is translated into
  every language.

## Data flow example: typing in Calc

1. The user types `=B2*C2` and presses Enter in the in-cell editor.
2. Dart sends `{"op":"calc.set_input","doc":1,"sheet":0,"cell":"D2","text":"=B2*C2"}`.
3. Rust parses the input, updates the dependency graph, recalculates D2 and
   everything that depends on it, and records an undo step.
4. Dart sends `calc.view` for the visible window and repaints.
