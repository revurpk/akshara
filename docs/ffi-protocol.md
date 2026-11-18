# FFI protocol

The Flutter UI talks to the Rust core through three C functions exported by
`libakshara` (`crates/ffi`):

```c
uint8_t *akshara_alloc(size_t len);                  /* buffer for a request */
void     akshara_free(uint8_t *ptr, size_t len);     /* free any buffer      */
uint8_t *akshara_call(const uint8_t *req, size_t len, size_t *out_len);
```

`akshara_call` takes a UTF-8 JSON object and returns a newly allocated UTF-8
JSON response. Its length is written to `*out_len`, and the caller releases
it with `akshara_free(ptr, *out_len)`. Every response is either

```json
{"ok": true, "result": ...}
{"ok": false, "error": {"kind": "invalid", "message": "human readable"}}
```

Error kinds are `io`, `zip`, `xml`, `limit`, `invalid`, `unsupported` and
`internal` (a caught panic). Requests larger than 64 MiB are refused.

Documents are addressed by the integer handle returned from `new` or `open`
(`"doc": 3`). Cells use A1 notation (`"B7"`, `"A1:C10"`). Writer positions
are UTF-16 offsets inside a paragraph, and paragraphs are addressed as
`{"block": 4}` or, inside a table, `{"block": 4, "cell": [row, col, para]}`.
Colours are `"#rrggbb"` strings, or `null` to reset.

## General operations

| op | arguments | result |
|---|---|---|
| `version` | | `{name, version, odf}` |
| `new` | `kind`: `writer`/`calc`/`math`, `source?` (math) | `{doc, kind}` |
| `new_envelope` | `width, height, margin` (points), `sender?, recipient?` (lines separated by `\n`) | `{doc, kind}` |
| `new_index_cards` | `width, height, margin, count?` (1–500, one card per page) | `{doc, kind}` |
| `new_labels` | `sheet` (id from `paper_sizes`), `text?`, `same?` (default true: every label gets `text`; false: one label per blank-line separated entry) | `{doc, kind}` |
| `paper_sizes` | | `{papers: [{id, name, kind: sheet/envelope/card, width, height}], labels: [{id, name, paper, cols, rows, width, height, top, left, h_pitch, v_pitch}]}` |
| `pdf_from_pngs` | `pngs` (page image paths), `width, height` (page size in points), `path`, `title?` | page count; writes a PDF with one image per page (used for printing) |
| `open` | `path` (kind from the extension: odt ott txt / ods ots csv tsv / odf otf mml) | `{doc, kind}` |
| `save` | `doc`, `path` (format from the extension), `sheet?`, `sanitize?` (CSV) | info |
| `close` | `doc` | `true` if it was open |
| `info` | `doc` | `{kind, modified, can_undo, can_redo, title}` |
| `undo` / `redo` | `doc` | `true` if something changed |
| `set_title` | `doc`, `title` | |

Save formats: Writer `odt`, `txt`, `html`, `md`; Calc `ods`, `csv`, `tsv`;
Math `odf`, `mml`. Only native formats clear the "modified" flag.

## Calc

| op | arguments | result |
|---|---|---|
| `calc.sheets` | `doc` | `[{name, used}]` |
| `calc.view` | `doc, sheet, row, col, rows ≤ 500, cols ≤ 200` | `{cells: [{r, c, t, v, s}], styles: {id: style}, widths: {col: pt}, used: {rows, cols}}` |
| `calc.cell` | `doc, sheet, cell` | `{input, display, type, style}` |
| `calc.set_input` | `doc, sheet, cell, text` | text is interpreted like typing (`=` formula, `'` literal, numbers, dates, `₹`, `%`...) |
| `calc.paste` | `doc, sheet, cell, rows` (array of string arrays, or TSV text) | |
| `calc.copy_text` | `doc, sheet, range` | TSV of displayed values |
| `calc.clear` | `doc, sheet, range` | |
| `calc.style` | `doc, sheet, range, patch` | |
| `calc.insert` / `calc.delete` | `doc, sheet, axis: row/col, at, count` | |
| `calc.sort` | `doc, sheet, range, col, ascending?` | |
| `calc.fill` | `doc, sheet, src, dst` | |
| `calc.copy` | `doc, sheet, src, dst_sheet, dst` | |
| `calc.add_sheet` / `calc.rename_sheet` / `calc.remove_sheet` | `doc, name? / sheet, name / sheet` | |
| `calc.col_width` | `doc, sheet, col, width` (points, `null` = default) | |
| `calc.functions` | | list of function names |

`v` is the value type: `e` empty, `n` number, `s` text, `b` boolean,
`x` error. A style patch may contain `bold`, `italic`, `underline`, `align`
(`general`/`left`/`center`/`right`), `color`, `background` and `format`.
Formats: `{"kind": "general" | "number" (decimals, thousands) | "indian"
(decimals) | "percent" (decimals) | "currency" (decimals, symbol) |
"scientific" (decimals) | "date" | "date_dmy" | "time" | "datetime" | "text"}`.

## Writer

| op | arguments | result |
|---|---|---|
| `writer.get` | `doc` | `{blocks, header, footer, page, paper, stats}` (`paper`: id of the matching paper size or `null`) |
| `writer.para` | `doc, para` | one paragraph |
| `writer.stats` | `doc` | `{words, chars, chars_no_spaces, paragraphs, tables}` |
| `writer.set_text` | `doc, para, text` | the core diffs and keeps formatting |
| `writer.replace` | `doc, para, start, end, text` | |
| `writer.attrs` | `doc, para, start, end, patch` | |
| `writer.split` | `doc, para, at` | address of the new paragraph |
| `writer.merge` | `doc, para` | `{para, at}` or `null` |
| `writer.kind` | `doc, para, kind: {kind, level?}` | |
| `writer.align` | `doc, para, align` | |
| `writer.indent` | `doc, para, delta` | list level for list items, else the left indent in steps of 36 pt |
| `writer.spacing` | `doc, para, indent, space_before` (points) | |
| `writer.field` | `doc, para, at, field`: `page_number` / `page_count` | |
| `writer.page` | `doc, width, height, margin` (points) | page size and margin (undoable) |
| `writer.header` / `writer.footer` | `doc, on` | turns the header / footer on (one empty paragraph) or off |
| `writer.letterhead` | `doc, name?, details?, contact?, align?, color?` | replaces header (name, address) and footer (contact) |
| `writer.insert_table` / `writer.insert_page_break` / `writer.remove_block` | `doc, at, rows?, cols?` | |
| `writer.table` | `doc, block, rows: bool, at, count` (negative deletes) | |
| `writer.find` | `doc, needle, match_case?` | `[{para, start, end}]` |
| `writer.replace_all` | `doc, needle, replacement, match_case?` | count |
| `writer.break_undo` | `doc` | ends the current typing undo group |
| `writer.export_html` | `doc` | HTML string |

Blocks are serialised as `{"type": "paragraph", "kind": "heading",
"level": 2, "align": "left", "indent": 36, "space_before": 12, "runs":
[{"text": "...", "bold": true}]}` (`indent` and `space_before` omitted when
zero), `{"type": "table", "rows": [[[paragraph, ...], ...], ...],
"col_widths": [...], "row_heights": [...], "borders": false}` (sizes in
points, omitted when automatic; `borders` omitted when true) and
`{"type": "page_break"}`. Run attributes: `bold italic underline strike code
superscript subscript color highlight size link field` (`field`:
`page_number` / `page_count`, the text being its last known value); false
and null values are omitted. `header` and `footer` are lists of paragraphs
(empty when absent).

A paragraph address `para` is `{"block": n}` for a top-level paragraph,
`{"block": n, "cell": [row, column, paragraph]}` inside a table, and
`{"area": "header" | "footer", "block": n}` for header and footer
paragraphs.

## Transliteration (teltools)

| op | arguments | result |
|---|---|---|
| `indic.scripts` | | `[{code, name, native}]`: `deva telu taml knda mlym orya beng` |
| `indic.translit` | `text`, `mode`, `script`, `native_digits?` | converted text |
| `indic.detect` | `text` | most-used script code, or `null` |
| `calc.transliterate` | `doc, sheet, range, mode, script, native_digits?` | number of text cells changed (one undo step) |

Modes: `roman` (teltools RTS/ITRANS → script, the default), `script` (any
supported Indic script → `script`), `to_iast` (`script` ignored),
`from_iast`. Texts larger than 4 MiB are refused.

## Math

| op | arguments | result |
|---|---|---|
| `math.get` | `doc` | `{source, tree, errors}` |
| `math.set` | `doc, source` | `{tree, errors}` |
| `math.parse` | `source` | `{tree, errors}` (stateless preview) |
| `math.mathml` | `doc` | MathML string |
| `math.symbols` | | `[{name, char}]` Greek letters |

Tree nodes are tagged with `"t"`: `row, lines, ident, number, op, text, func,
big_op, frac, script, under_over, sqrt, root, fenced, matrix, accent, styled,
phantom, brace, space, placeholder`. See `crates/math/src/ast.rs`.

## Example (Python, for testing)

```python
import ctypes, json
lib = ctypes.CDLL("target/release/libakshara.so")
lib.akshara_call.restype = ctypes.POINTER(ctypes.c_uint8)
lib.akshara_call.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
lib.akshara_free.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]

def call(**req):
    data = json.dumps(req).encode(); n = ctypes.c_size_t()
    out = lib.akshara_call(data, len(data), ctypes.byref(n))
    resp = json.loads(ctypes.string_at(out, n.value)); lib.akshara_free(out, n.value)
    return resp

doc = call(op="new", kind="calc")["result"]["doc"]
call(op="calc.set_input", doc=doc, sheet=0, cell="A1", text="₹1,50,000")
print(call(op="calc.cell", doc=doc, sheet=0, cell="A1")["result"]["display"])  # ₹1,50,000.00
```
