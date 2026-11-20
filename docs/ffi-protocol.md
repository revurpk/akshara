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
| `new` | `kind`: `writer`/`calc`/`impress`/`draw`/`math`, `source?` (math) | `{doc, kind}` |
| `new_envelope` | `width, height, margin` (points), `sender?, recipient?` (lines separated by `\n`) | `{doc, kind}` |
| `new_index_cards` | `width, height, margin, count?` (1–500, one card per page) | `{doc, kind}` |
| `new_labels` | `sheet` (id from `paper_sizes`), `text?`, `same?` (default true: every label gets `text`; false: one label per blank-line separated entry) | `{doc, kind}` |
| `paper_sizes` | | `{papers: [{id, name, kind: sheet/envelope/card, width, height}], labels: [{id, name, paper, cols, rows, width, height, top, left, h_pitch, v_pitch}]}` |
| `pdf_from_pngs` | `pngs` (page image paths), `width, height` (page size in points), `path`, `title?` | page count; writes a PDF with one image per page (used for printing) |
| `open` | `path` (kind from the extension: odt ott txt docx docm dotx / ods ots csv tsv xlsx xlsm xltx / odp otp / odg otg / odf otf mml; `.doc`/`.xls`/`.ppt`/`.pptx` give an `unsupported` error) | `{doc, kind}` |
| `save` | `doc`, `path` (format from the extension: writer odt docx txt html md / calc ods xlsx csv tsv / impress odp / draw odg / impress and draw also svg png txt / math odf mml), `sheet?`, `sanitize?` (CSV), `page?` (svg, png) | info; odt, docx, ods, xlsx, odp and odg clear "modified" |
| `close` | `doc` | `true` if it was open |
| `info` | `doc` | `{kind, modified, can_undo, can_redo, title}` |
| `undo` / `redo` | `doc` | `true` if something changed |
| `set_title` | `doc`, `title` | |

Only formats that keep everything the model holds clear the "modified"
flag.

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
| `calc.print_area` | `doc, sheet` | `{range, repeat_rows}`: e.g. `"A1:D20"`, `"1:2"` (1-based rows), or `null` |
| `calc.set_print_area` | `doc, sheet, range?, repeat_rows?` | print range (`null`/empty = used area) and rows repeated on every page (`1`, `1:2`; `null`/empty = none); undoable |
| `calc.page` | `doc` | `{width, height, margin, scale, fit_width, grid, header_footer, paper}` (points; `scale` in percent; `paper`: matching paper size id or `null`) |
| `calc.set_page` | `doc, width, height, margin, scale?, fit_width?, grid?, header_footer?` | page setup for printing, all sheets (undoable) |

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
| `writer.insert_image` | `doc, at, path` | inserts a picture block (natural size, at most the text width) |
| `writer.insert_diagram` | `doc, at, source` | inserts a Mermaid diagram block |
| `writer.insert_drawing` | `doc, at, width?, height?` | inserts an empty drawing canvas (points; default: text width × half) |
| `writer.frame` | `doc, block, width?, height?, align?, description?, source?` | resizes (pictures keep their aspect without `height`; diagrams always), aligns, sets a picture's alternative text or a diagram's source |

Blocks are serialised as `{"type": "paragraph", "kind": "heading",
"level": 2, "align": "left", "indent": 36, "space_before": 12, "runs":
[{"text": "...", "bold": true}]}` (`indent` and `space_before` omitted when
zero), `{"type": "table", "rows": [[[paragraph, ...], ...], ...],
"col_widths": [...], "row_heights": [...], "borders": false}` (sizes in
points, omitted when automatic; `borders` omitted when true) and
`{"type": "page_break"}`, `{"type": "image", "image": id, "format":
"png", "width", "height", "align", "description"}` (fetch the picture with
`image.data`), `{"type": "diagram", "source", "width", "height", "align",
"figure", "errors"}` and `{"type": "drawing", "width", "height", "align",
"shapes": [...]}`. Run attributes: `bold italic underline strike code
superscript subscript color highlight size link field` (`field`:
`page_number` / `page_count`, the text being its last known value); false
and null values are omitted. `header` and `footer` are lists of paragraphs
(empty when absent).

A paragraph address `para` is `{"block": n}` for a top-level paragraph,
`{"block": n, "cell": [row, column, paragraph]}` inside a table, and
`{"area": "header" | "footer", "block": n}` for header and footer
paragraphs.

## Impress and Draw

| op | arguments | result |
|---|---|---|
| `draw.get` | `doc` | `{kind: impress/draw, width, height, pages: [page]}` |
| `draw.page` | `doc, page` | one page: `{name, title, background, notes, shapes}` (`title` is the name or "Slide n" / "Page n") |
| `draw.add_page` | `doc, at, layout?`: `title` / `content` / `two_content` / `title_only` / `blank` | |
| `draw.duplicate_page` / `draw.remove_page` | `doc, page` | (the last page cannot be removed) |
| `draw.move_page` | `doc, from, to` | |
| `draw.page_props` | `doc, page, name?, background?, notes?` | typing notes coalesces into one undo step |
| `draw.set_size` | `doc, width, height, scale?` | page size of all pages (points); `scale` scales the shapes too |
| `draw.break_undo` | `doc` | ends the current group of coalesced edits (also for Writer canvases) |

## Shapes

These operate on a slide or drawing page (`page`) or on a Writer drawing
canvas (`block`); each call is one undo step of that document.

| op | arguments | result |
|---|---|---|
| `shapes.add` | `doc, page/block, shapes: [{kind, x, y, w, h, ...patch}]` | new shape ids |
| `shapes.update` | `doc, page/block, updates: [{id, patch}], key?` | edits with the same `key` in a row are one undo step |
| `shapes.remove` | `doc, page/block, ids` | |
| `shapes.order` | `doc, page/block, id, to`: `front` / `back` / `forward` / `backward` | |
| `shapes.copy` | `doc, page/block, ids` | count; the shapes go to the core's clipboard |
| `shapes.paste` | `doc, page/block, dx?, dy?` | new ids (pasted shapes are offset, 12 pt by default) |
| `shapes.insert_image` | `doc, page/block, path, x?, y?, max_w?, max_h?` | id of the new picture shape |

Kinds: `rect`, `round_rect`, `ellipse`, `triangle`, `diamond`, `line` (from
`(x, y)` to `(x + w, y + h)`), `text_box`, `polygon` (`points`: `[x, y]`
fractions of the box, `closed?`), `image` (from `shapes.insert_image`) and
`diagram` (`source`). A patch may contain `x y w h rotation` (degrees,
clockwise), `fill stroke` (colour or null), `stroke_width dash arrow_start
arrow_end`, `text`, `size bold italic underline color align`
(`left`/`center`/`right`) `valign` (`top`/`middle`/`bottom`) `bullets`, and
`source` (diagrams). Shapes are serialised as `{id, kind, x, y, w, h,
rotation?, fill, stroke, stroke_width, dash?, arrow_start?, arrow_end?,
text, style: {size, bold?, italic?, underline?, color, align, valign,
bullets?}, placeholder?}` plus `image` and `format` for pictures and
`source` and `figure` for diagrams.

## Pictures and diagrams

| op | arguments | result |
|---|---|---|
| `image.data` | `doc, id, raster?` (default true) | `{format, width, height, data}`: base64 bytes; SVG pictures come as PNG unless `raster` is false |
| `mermaid.render` | `source` (≤ 64 KiB) | `{figure, kind, errors: [{line, message}]}` (stateless preview; never fails on bad syntax) |
| `mermaid.svg` | `source` | SVG text |
| `set_fonts` | `dir` | a folder of fonts for rendering text in PNG output (the app passes its bundled fonts) |

A figure is `{width, height, items}` in points; items are tagged with `t`:
`rect` (`x y w h r`), `ellipse` (`cx cy rx ry`), `poly` (`points closed?
smooth? start? end?`, arrowheads `triangle hollow open diamond
hollow_diamond circle cross`), `wedge` (`cx cy r start sweep`, degrees
clockwise from 12 o'clock), `text` (`x y text size bold? italic? anchor
color`, the text block centred on `y`), `image` (`x y w h id`) and `rotate`
(`cx cy angle items`). Paints are `fill`, `stroke`, `width`, `dash?`.

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
