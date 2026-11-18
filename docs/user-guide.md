# User guide

## Getting around

- **Home** lists the three applications, *Open…* and recent documents. Change
  the interface language with the chips at the top (shown in each language's
  own script) or with the translate button (文A) in the title bar. Your choice
  is remembered.
- Every open document is a **tab**. A dot (•) marks unsaved changes. Close a
  tab with its × button, the middle mouse button, or Ctrl+W.
- **Save** (Ctrl+S) writes the native format (`.odt`, `.ods`, `.odf`).
  **Save as / export** (Ctrl+Shift+S) can also export plain text, HTML,
  Markdown, CSV, TSV or MathML.
- The built-in **file browser** works the same on every platform. It shows
  only documents Akshara can open, has quick links to Documents, Home and
  Desktop, and can create folders.
- The **Appearance** button (sun/moon icon) in the title bar chooses:
  - **Theme:** system (the default), light or dark.
  - **Look and feel:** *Native* (the default) or *Material Design*.

### Native look and feel

By default Akshara looks like the system it runs on:

| System | Native look |
|---|---|
| Windows | Fluent style: Segoe UI, Windows blue accent, square-ish 4 px corners, Windows button order (*Save · Don't save · Cancel*) |
| macOS, iOS | Apple style: San Francisco font, system blue, Cupertino alerts and page transitions |
| Linux | GNOME/Adwaita style: the desktop's font (Adwaita Sans, Cantarell or Ubuntu), GNOME blue, rounded flat surfaces |
| Android | Material 3, Android's own design, with Roboto |

None of the native looks draw Material ink ripples, except Android's. *Material
Design* gives the same saffron-coloured Material look on every device
instead. Either way, Indian scripts in the interface still use the bundled
Noto fonts, and documents are unaffected: pages, cells and formulas look
the same everywhere.

## Typing in Indian languages

Akshara bundles fonts for every supported script, so text displays correctly
on any device. To *type*, use your system's keyboard layouts. All of these
are built in and free:

| System | How to add an Indian keyboard |
|---|---|
| Windows 10/11 | Settings → Time & language → Language & region → add Hindi, Tamil, … → Keyboard (*Phonetic* or *INSCRIPT*). Switch with Win+Space. |
| macOS | System Settings → Keyboard → Text Input → Edit → + → e.g. Hindi – Transliteration. Switch with Ctrl+Space. |
| Linux | Settings → Keyboard → Input Sources → add e.g. *Hindi (Bolnagri)* or *Tamil (InScript)*; for transliteration install IBus with `m17n` (*hi-itrans*, *ta-itrans*…). |
| Android | Gboard → Settings → Languages → Add keyboard → choose language and *transliteration* or native layout. |
| iOS | Settings → General → Keyboard → Keyboards → Add New Keyboard. |

Sanskrit uses Devanagari: any Hindi or Marathi keyboard works.

### Built-in transliteration (no keyboard setup needed)

Akshara can also convert Roman letters as you type, using the scheme of
[teltools](https://github.com/revurpk/teltools) (RTS / ITRANS style). Click
the **abc** button in the title bar and pick a script: Devanagari (Hindi,
Sanskrit, Marathi), Telugu, Tamil, Kannada, Malayalam, Odia or Bengali. The
button then shows, for example, **a→అ**. **Ctrl+Shift+K** turns it on and off
(⌘⇧K on a Mac). The button's menu has a **Typing guide** with every key and a
box to try it out.

| You type | Telugu | Devanagari | Tamil |
|---|---|---|---|
| `namastE` | నమస్తే | नमस्ते | நமஸ்தே |
| `satyamEva jayatE` | సత్యమేవ జయతే | सत्यमेव जयते | ஸத்யமேவ ஜயதே |
| `kR^iShNa` | కృష్ణ | कृष्ण | க்ருஷ்ண |
| `vaNakkam` | వణక్కమ్ | वणक्कम् | வணக்கம் |

How it works:

- **Case matters:** `t` త/त, `T` ట/ट; `s` స, `S` or `sh` శ, `Sh` ష;
  `A`/`aa`, `I`/`ii`, `U`/`uu`, `E`/`ee` and `O`/`oo` are long vowels;
  `M` is anusvara (ం), `H` visarga (ః), `.N` candrabindu, `|` and `||` dandas,
  and `\om` is ॐ.
- **Consonants are bare unless a vowel follows:** `ram` → రమ్ / रम्, while
  `rAma` → రామ / राम. So type the final `a` (`kamala`).
- **e and o:** in Telugu, Tamil, Kannada and Malayalam, `e`/`o` are short and
  `E`/`O` long. Hindi, Sanskrit, Odia and Bengali have only one e and one o,
  so both give ए/ओ. Use `^e`/`^o` for ऍ/ऑ, as in `D^okTara` → डॉक्टर.
- **Scripts that lack a letter** use the nearest one. Tamil has no aspirated
  or voiced stops, so `kha`, `ga` and `gha` are all க (`bhArata` →
  பாரத), and `M` becomes ம். Bengali writes `va` as ব. Telugu-only ౘ/ౙ become
  च/ज elsewhere.
- **Digits stay 0-9** so numbers keep working in Calc; switch on *Native
  digits* in the menu for ౧౨౩ / १२३.
- A word is finished by Space or punctuation. **Backspace** removes the last
  Roman letter of the word you are typing (ఖ → ఖ్ → క్). Moving the caret
  starts a new word.
- In **Calc formulas** and in **Math**, only text between double quotes is
  converted, so function names and StarMath commands stay Latin
  (`=LEN("rAma")`, `"vargamUlaM" = sqrt{x}`).
- Quotation marks (`'` and `"`) stay quotation marks while typing; in the
  teltools scheme they are Vedic accent keys, which **Convert text**
  applies.

**Convert text** (⇄ in the Writer and Calc toolbars) converts the selection
(in Writer, the whole paragraph if nothing is selected; in Calc, every text
cell of the selection, as one undo step):

- *Roman letters → script*: the same conversion as typing.
- *Convert script to …*: any supported Indic script to another (for example
  a Telugu stotram to Devanagari or Kannada).
- *Romanize (IAST)*: सत्यमेव जयते → *satyameva jayate*.
- *IAST → script*: *kṛṣṇaṁ vande* → कृष्णं वन्दे.

Transliteration happens on your device; nothing is sent anywhere.

## Writer

Each paragraph is edited directly on the page.

| Action | How |
|---|---|
| New paragraph / line break | Enter / Shift+Enter |
| Join with previous paragraph | Backspace at the start (on a list item or quote, the first Backspace turns it back into body text) |
| Bold / italic / underline | Ctrl+B / Ctrl+I / Ctrl+U, or the toolbar |
| Superscript / subscript | Ctrl+Shift+P / Ctrl+Shift+B, or the toolbar |
| Strike-through, monospace, colours, highlight, size, link | toolbar |
| Paragraph style | the style menu: body text, title, headings 1–6, quotation, preformatted |
| Lists | bullet / numbered buttons; Tab and Shift+Tab change the level |
| Indent | the indent buttons move body text in half-inch steps (Tab in body text types a tab) |
| Alignment | left, centre, right, justify |
| Tables | *Insert table* (e.g. `3 x 4`); Tab moves to the next cell and adds a row at the end; right-click or long-press a cell for row and column commands |
| Page break | *Insert page break*; each page is shown as its own sheet |
| Page size, orientation, margins | *Page ▾* menu → *Page setup…* (A4, A5, A3, Letter, Legal, envelopes, index cards, or custom) |
| Header and footer | *Page ▾* → *Header* / *Footer*; type on the first page, other pages show a copy (click it to edit) |
| Page numbers | *Page ▾* → *Insert page number* / *Insert page count* (fields, shaded grey on screen) |
| Letterhead | *Page ▾* → *Letterhead…*: your name, address and contact details become the header and footer |
| Envelope | **+** → *Envelope…* (or *Page ▾* → *New envelope…*): pick DL, C6, C5, C4 or #10, type the addresses; opens as a new document |
| Labels | **+** → *Labels…* (or *Page ▾* → *New labels…*): pick an Avery sheet (L7160, L7163, L7173, L7651, 5160, 5163, 5167); the same text on every label, or one label per entry separated by blank lines |
| Index cards | **+** → *Index cards…* (or *Page ▾* → *New index cards…*): 3 × 5, 4 × 6, 5 × 8 in or A7, one card per page |
| Print | Ctrl+P or the printer button: a preview, then your system's print dialog |
| PDF | *Page ▾* → *Export as PDF…*, or *Export as PDF* in the print preview |
| Find & replace | Ctrl+F |
| Undo / redo | Ctrl+Z / Ctrl+Y (typing in one paragraph is undone as one step) |

The status bar shows word, character and paragraph counts, plus zoom. Links
can only use `http`, `https` or `mailto`.

**Printing.** Pages are drawn by Akshara itself, so every script prints
exactly as on screen. *Print…* in the preview opens your system's print
dialog, with its printers, copies, page range and printer preferences;
the document's paper size and orientation are preselected. On Windows the
pages go straight to the printer at its resolution (up to 300 dpi); on
Linux (GTK's print dialog) and macOS (the standard print panel) a 200 dpi
PDF goes to the chosen printer. Phones and tablets offer PDF export. Pages break only at page
breaks and where the text no longer fits, between paragraphs and table
rows; a single paragraph taller than a page is cut off. Label sheets print
at the exact label positions: check "actual size" (no scaling) in the
printer settings.

**Superscript and subscript.** Digits, `+ − = ( )` and a few letters are
drawn with the font's raised and lowered glyphs. Other characters are only
drawn smaller on screen; files and printouts in other programs show them
raised as usual.

## Calc

| Action | How |
|---|---|
| Enter data | Select a cell and type; Enter moves down, Tab moves right, Esc cancels. F2 or double-click edits the existing content. |
| Formula bar | Shows the formula of the current cell; edit and press Enter. |
| Go to a cell | Type `B7` or `A1:C10` in the box at the left of the formula bar. |
| Select | click and drag, Shift+click, Shift+arrows, Ctrl+A; click row or column headers |
| Move | arrows, Page Up/Down, Home, Ctrl+Home, Ctrl+End, Ctrl+arrows |
| Copy / cut / paste | Ctrl+C / Ctrl+X / Ctrl+V (tab-separated text, so it also works with other spreadsheets) |
| Clear | Delete |
| Formatting | bold, italic, underline, colours, alignment, number format menu |
| Column width | drag the border between column headers |
| AutoSum / insert function | Σ button / function list |
| Sort, fill down / right | toolbar (sort uses the current column) |
| Rows and columns | insert or delete from the toolbar or the right-click menu; formulas everywhere are adjusted |
| Sheets | + adds a sheet; right-click a sheet tab to rename or delete it |
| Page setup | *Page setup…* button: paper size, landscape, margin, scale (10–400 %) or *Fit columns to page width*, grid lines, and the header (sheet name) and footer (page number). Saved in the `.ods` file and used for all sheets. |
| Print / PDF | Ctrl+P or the printer button: every non-empty sheet, laid out by the page setup; columns that do not fit continue on the next pages. Then your system's print dialog, or *Export as PDF*. |

**What you type is recognised automatically:**

| You type | Becomes |
|---|---|
| `1234.5`, `-7`, `1e6` | number |
| `1,00,000` | number with Indian grouping (`1,00,000`) |
| `₹2,50,000` | rupee amount (`₹2,50,000.00`); `$`, `€`, `£`, `¥` also work |
| `12.5%` | 0.125 shown as a percentage |
| `15-08-2026`, `15/08/2026` | date, day first (DD-MM-YYYY) |
| `2026-08-15` | ISO date |
| `18:30` | time |
| `TRUE`, `FALSE` | logical value |
| `=A1*2` | formula |
| `'007` | text, kept exactly as typed |

**Number formats:** General, fixed decimals, thousands (`1,234.00`), **Indian
lakh/crore** (`12,34,567.00`), percent, currency (₹ uses Indian grouping),
scientific, DD-MM-YYYY or ISO dates, time, date and time, and text.

**Formulas:** use `;` or `,` between arguments. You can refer to other sheets
as `Sheet2.A1` or `'My sheet'.A1`; `$A$1` makes a reference absolute when
copying or filling. Errors use OpenOffice's codes: `#DIV/0!`, `#VALUE!`,
`#REF!`, `#NAME?`, `#N/A`, `#NUM!`, `Err:504` (wrong number of arguments),
`Err:509` (syntax error) and `Err:522` (circular reference).

**Functions (131):** ABS, ACOS, AND, ASIN, ATAN, ATAN2, AVERAGE, AVERAGEIF,
CEILING, CHAR, CHOOSE, CODE, COLUMN, COLUMNS, COMBIN, CONCAT, CONCATENATE, COS,
COSH, COUNT, COUNTA, COUNTBLANK, COUNTIF, DATE, DAY, DAYS, DEGREES, EDATE,
EOMONTH, EVEN, EXACT, EXP, FACT, FALSE, FIND, FLOOR, FV, GCD, GEOMEAN, HLOOKUP,
HOUR, IF, IFERROR, IFNA, INDEX, INT, ISBLANK, ISERR, ISERROR, ISEVEN, ISLOGICAL,
ISNA, ISNONTEXT, ISNUMBER, ISODD, ISTEXT, LARGE, LCM, LEFT, LEN, LN, LOG, LOG10,
LOWER, MATCH, MAX, MEDIAN, MID, MIN, MINUTE, MOD, MODE, MONTH, N, NA, NOT, NOW,
NPV, ODD, OR, PI, PMT, POWER, PRODUCT, PROPER, PV, QUOTIENT, RADIANS, RAND,
RANDBETWEEN, RANK, REPLACE, REPT, RIGHT, ROUND, ROUNDDOWN, ROUNDUP, ROW, ROWS,
SEARCH, SECOND, SIGN, SIN, SINH, SMALL, SQRT, STDEV, STDEVP, SUBSTITUTE, SUM,
SUMIF, SUMPRODUCT, SUMSQ, T, TAN, TANH, TEXT, TEXTJOIN, TIME, TODAY, TRIM, TRUE,
TRUNC, UPPER, VALUE, VAR, VARP, VLOOKUP, WEEKDAY, XOR, YEAR.

`SUMIF`, `COUNTIF`, `AVERAGEIF`, `MATCH` and `VLOOKUP` accept criteria such
as `">=10"` and the wildcards `*` and `?`. Text comparison ignores case.

## Math

Type a formula on the left in the StarMath language (the same language
OpenOffice Math uses); the rendering on the right updates as you type.
Problems are listed under the editor, and clicking one moves the cursor
there. The menus insert templates whose `<?>` placeholders you fill in;
**F4** jumps to the next placeholder. **Ctrl+P** (or the printer button
under the rendering) prints the formula at the preview size on an A4
page, through your system's print dialog, or exports it as PDF.

| You type | Shows |
|---|---|
| `a over b`, `{a+b} over 2` | fraction |
| `x^2`, `x_i`, `x_i^2` | super- and subscripts |
| `sqrt{x}`, `nroot 3 x` | roots |
| `sum from {i=1} to n i`, `int from 0 to 1 f(x) dx`, `lim from {x -> 0}` | large operators with limits |
| `left( a over b right)` | brackets that grow with their content |
| `matrix{a # b ## c # d}`, `stack{a # b}`, `binom n k` | matrices |
| `%alpha`, `%PI`, `infinity`, `partial`, `nabla` | symbols and Greek letters |
| `<=`, `<>`, `approx`, `in`, `subset`, `union`, `cdot`, `times`, `+-` | operators and relations |
| `hat a`, `vec v`, `overline{AB}`, `underline x` | accents |
| `bold x`, `ital x`, `color red x`, `size 20 x`, `font sans x` | formatting |
| `"वर्ग का क्षेत्रफल"` | text in any script, between quotes |
| `newline` | new line |

Formulas are saved as OpenDocument Formula (`.odf`) with the StarMath source
embedded, so OpenOffice and LibreOffice reopen them exactly. You can also
open and save standalone MathML (`.mml`).

## Limitations

Akshara is young. These features of OpenOffice are **not implemented yet**,
and content using them is simplified or dropped when a document is opened
and saved again:

- Writer: images and drawings, frames, footnotes and endnotes, comments,
  tracked changes, fields other than page number and page count, different
  headers and footers for first, left and right pages, sections and
  columns, custom fonts per run, line spacing, right and first-line
  indents, merged table cells and table styling, and spell checking.
- Calc: charts, images, cell comments, merged cells, conditional formatting,
  data validation, named ranges, pivot tables, row heights, custom number
  format codes (the format *category* is kept), and the remaining functions.
- Math: MathML-only features without a StarMath equivalent.
- Transliteration: Hindi-style dropping of the final inherent vowel
  (`kamal` → कमल) and the nukta letters ज़/फ़ (`z`, `f`) are not part of the
  teltools scheme; type `kamala`, or add a nukta with `^n`.
- Calc printing: print ranges, repeated title rows, page breaks, a
  different page setup per sheet, and custom header and footer text; cell
  borders are drawn as light grid lines.
- All: password-protected documents (they are refused), and Microsoft
  Office formats (`.docx`, `.xlsx`).

Keep an original copy when editing complex documents made elsewhere.
