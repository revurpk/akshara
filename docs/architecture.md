# Architecture

Akshara has two halves: a **Rust core** that owns every document and file
format, and a **Flutter UI** that renders and edits. They talk through a tiny
C ABI carrying JSON.

```
┌──────────────────────── Flutter app (Dart) ────────────────────────┐
│ main.dart: shell, tabs, home screen, language & theme              │
│ writer/    paragraph editors (one TextField per paragraph)         │
│ calc/      custom-painted virtual grid, formula bar, sheet tabs    │
│ draw/      Impress & Draw: shape editor, slide pane, slide show,   │
│            shape/figure painter, Mermaid editor (also in Writer)   │
│ math/      StarMath editor + widget-based formula renderer         │
│ assist/    voice input button, AI chat panel, their settings       │
│ core/      native.dart (dart:ffi), file dialog, paths, fonts, look │
│ l10n/      9 UI languages                                          │
└───────────────┬────────────────────────────────────────────────────┘
                │ akshara_call(JSON) -> JSON   (crates/ffi, C ABI)
┌───────────────┴──────────────────── Rust core ─────────────────────┐
│ ffi     registry of open documents, request dispatcher             │
│ writer  Document model, editing + undo, ODT, DOCX, HTML/Markdown   │
│ calc    Workbook, formula parser/evaluator, dependency graph, ODS  │
│ draw    shapes, slides/pages + undo, pictures, Mermaid, ODP/ODG,   │
│         SVG and PNG (resvg)                                        │
│ math    StarMath lexer/parser, AST, MathML in/out, .odf            │
│ indic   transliteration (7 scripts) on vendored teltools           │
│ voice   offline Whisper speech-to-text (candle), microphone (cpal) │
│ ai      Ollama chat client, loopback only                          │
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
  depends only on the Flutter SDK and the first-party `material_ui` and
  `cupertino_ui` packages,
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

- Pictures, Mermaid diagrams and drawing canvases are blocks of their own
  (`Block::Image`, `Block::Diagram`, `Block::Drawing`), between paragraphs.
  In ODT they are `draw:frame`s: pictures in `Pictures/`, diagrams as SVG
  pictures with the Mermaid source in `svg:desc`, and canvases as embedded
  OpenDocument Graphics objects (`Object N/`). DOCX gets DrawingML pictures
  with an SVG blip and a PNG fallback rendered by resvg.

### `office-draw`
- `model`: a `Shape` is a box (or a line from `(x, y)` to `(x+w, y+h)`) with
  a kind (rectangle, rounded rectangle, ellipse, triangle, diamond, polygon,
  text box, picture, Mermaid diagram), rotation, fill, outline, arrowheads,
  text and one text style, and an optional presentation placeholder role.
  `ops` validates and edits shape lists; it is shared by `Drawing` pages and
  Writer canvases, each wrapping it in its own undo transaction.
- `Drawing`: pages (slides) with names, backgrounds and notes, slide
  layouts, and the same snapshot undo as Writer; repeated edits with a key
  (typing in a shape, nudging) coalesce into one step.
- `Image`: the original bytes behind an `Arc` (undo snapshots stay cheap);
  the format is sniffed from the content and the size read from the header.
- `mermaid`: line-oriented, total parsers for flowcharts, sequence, class and
  state diagrams and pie charts, and a layered (Sugiyama) layout: cycle
  removal, longest-path ranking, dummy nodes for long edges and labels,
  barycentric crossing reduction, iterative coordinate placement, cluster
  boxes. Every phase is iterative with explicit budgets, so hostile input
  cannot exhaust the stack or take long. The output is a `Figure`: a flat
  list of primitives (rectangles, ellipses, polylines with arrowheads,
  wedges, text) that the Flutter app paints with its own fonts and the core
  turns into SVG.
- `odg`: ODP/ODG import and export with style resolution (graphic,
  presentation, paragraph and text families, parents, defaults), master page
  decorations, transforms, custom shapes, paths (flattened), notes. The
  shape reader is reused for shapes anchored in Writer documents.
- `render`: SVG to PNG through resvg/tiny-skia (pure Rust) with the app's
  bundled fonts; external files referenced by SVGs are never read.

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

### `office-voice`
- Reads whisper.cpp model files (`ggml-*.bin`: header, mel filters,
  vocabulary and weights in one file, f16/f32 or quantized) with a
  bounds-checked reader, and runs the Whisper encoder and decoder on
  [candle](https://github.com/huggingface/candle)'s CPU tensors. Quantized
  matrices stay quantized.
- Log-mel spectrogram, language detection restricted to the offered
  languages (English, Telugu, Hindi, Sanskrit), greedy decoding with a
  repetition guard, silence skipping, 30-second windows.
- `Recorder` captures the default microphone with cpal on its own thread and
  resamples to 16 kHz mono. Android and iOS get a stub (no permission flow
  yet).
- The other engine, the operating system's recogniser, lives in the Flutter
  runners rather than the core: `assist/system_speech.dart` talks to the
  `akshara/speech` method channel, which macOS and iOS answer from
  `Runner/SystemSpeech.swift` (SFSpeechRecognizer, on-device only), Android
  from `MainActivity.kt` (`createOnDeviceSpeechRecognizer`, API 31 and later)
  and Windows from `runner/system_speech.cpp` (SAPI dictation). Its status
  replies have the same shape as `voice.status`, so the microphone button
  polls both engines the same way.

### `office-ai`
- A minimal HTTP/1.1 client (`std::net::TcpStream`, chunked decoding) for
  Ollama's `/api/tags` and streamed `/api/chat`. `Endpoint::parse` accepts
  only loopback addresses. A system prompt asks for documents in Markdown,
  with diagrams as ```` ```mermaid ```` blocks, which
  `office_writer::markdown` turns into Writer blocks.

### `akshara-ffi`
- `akshara_alloc`, `akshara_free`, `akshara_call`: the only `unsafe` code in
  the project, about 20 lines with documented safety contracts.
- `api::Registry` holds the open documents by handle and dispatches about 60
  operations (see [ffi-protocol.md](ffi-protocol.md)). Panics are caught and
  returned as errors, so they never unwind into Dart.
- `assist` runs voice sessions and chat requests on background threads; the
  UI starts them, polls (`voice.status`, `ai.poll`) and stops or cancels
  them, so no call blocks the UI. The Whisper model is loaded once per path,
  while the user is still speaking.

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
- **Impress and Draw** (`draw/`): the core sends each page as JSON shapes
  (diagrams carry their laid-out figure). `shape_paint.dart` paints shapes,
  figures and pictures (decoded once per document in `CoreImages`);
  `shape_editor.dart` handles selection, moving, resizing (rotated shapes
  keep their anchor), drawing, in-place text editing and the keyboard, and
  sends one `shapes.update` per gesture. The same editor edits Writer
  drawings. The slide show and printing use the read-only painter.
- **Math**: the parser's JSON tree is rendered with nested widgets: stacked
  fractions, shrunken scripts, painted radicals, and stretchy brackets painted
  to the height of their content.
- **Look and feel** (`core/look.dart`): `buildTheme` builds one theme per
  platform (Fluent-style Windows, macOS/iOS, GNOME/Adwaita Linux, Material 3
  Android) from first-party Material components, plus a single Material look.
  The native look is the default. `showChoice` shows Cupertino alerts on
  Apple systems and orders buttons the way each platform does. Only
  first-party libraries are used (`material_ui`, `cupertino_ui`). Documents
  keep the bundled fonts in every look.
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
