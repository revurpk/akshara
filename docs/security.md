# Security model

Office documents arrive from email, chat and downloads, so every document is
treated as **hostile input**. This page describes the threats Akshara
defends against and how.

## Threats and defences

| Threat | Defence |
|---|---|
| Memory corruption in parsers (the classic office-suite RCE) | All parsing is safe Rust; `unsafe_code = "forbid"` in every core crate. The only `unsafe` code is the 3-function C ABI (`crates/ffi/src/lib.rs`) with documented contracts. |
| XML entity expansion ("billion laughs"), external entities (XXE) | The XML parser rejects every `<!DOCTYPE>` and declaration; only the 5 predefined entities and numeric references exist. |
| Stack exhaustion from deep nesting | The XML parser is iterative. Element depth (256), node count, attributes, formula depth (64), formula length, and StarMath depth (96) are all bounded. Recalculation uses an explicit stack. |
| ZIP bombs | Archive size, entry count, per-entry and total uncompressed size, and compression ratio are checked against the directory **and enforced while inflating** (the declared sizes are never trusted). |
| Path traversal ("zip slip") | Nothing is ever extracted to disk. Entry names are still validated (`..`, absolute paths, drive letters and backslashes are rejected). |
| Spreadsheet "cell bombs" (`number-columns-repeated="4000000000"`) | Materialised cells are capped (4 million by default); style-only runs over 64 cells are not materialised; rows and columns beyond the sheet are dropped. |
| Formula denial of service (`REPT("x";1E9)`, self-doubling `&`) | Text results are capped at 1 MiB; bulk operations at 1 million cells. Circular references are detected (`Err:522`), never looped. |
| Hostile pictures | The format is sniffed from the content; size (32 MiB), sides (30,000 px) and raster pixel count (64 megapixels, so decoding cannot exhaust memory) are checked before a picture is accepted. SVG is parsed by usvg, which accepts a DTD, so SVG with `<!ENTITY` declarations is refused; SVG rendering never reads files or URLs referenced by the picture. Raster pictures are decoded by Flutter's engine. |
| Diagram denial of service | Mermaid source is limited to 64 KiB and 5,000 statements; the layout is iterative with budgets on dummy nodes and crossing counting, so pathological graphs finish quickly (tested with property tests). |
| Macros and scripts | Not implemented, by design. Macros are never loaded or run. |
| Formulas that reach outside (`WEBSERVICE`, `DDE`, `HYPERLINK`, `INDIRECT` to files) | Not implemented. The evaluator is pure: no I/O, no clock except `NOW()`/`TODAY()`, no network. |
| Malicious links (`javascript:`, `file:`, custom schemes) | Only `http`, `https` and `mailto` links are kept on import, accepted when editing, or emitted on export. |
| HTML export injection | All text and attributes are escaped. Exported pages carry `Content-Security-Policy: default-src 'none'` (pictures only as `data:` URLs) and contain no scripts; links get `rel="noopener noreferrer"`. |
| CSV injection into other spreadsheets | CSV export prefixes text starting with `= + - @ Tab CR` with `'` by default. |
| Password-protected (encrypted) documents | Refused with a clear message instead of being half-read. |
| Corrupted saves | Saves go to a temporary file that is `fsync`ed and renamed over the target, so a crash never leaves a truncated document. |
| Crashes taking down the app | Panics in the core are caught at the FFI boundary and reported as errors. |
| Privacy | No network code (verified: none of the Rust dependencies can open sockets, and the Dart code uses no HTTP or sockets). No telemetry or update checks. Android release builds request no permissions; macOS release builds are sandboxed with no network entitlement. |
| Privilege | Nothing needs administrator rights: all installers are per-user. |
| Third-party code | The vendored teltools has no dependencies. A panic it had on non-ASCII input (byte-wise string slicing) is fixed and covered by property tests; its upstream test vectors still pass byte for byte. Transliteration runs locally. |

## Limits

All limits are fields of `office_odf::Limits` (`crates/odf/src/limits.rs`):

| Limit | Default |
|---|---|
| Archive size | 256 MiB |
| ZIP entries | 20,000 |
| Uncompressed size per entry / total | 128 MiB / 512 MiB |
| Compression ratio (entries > 8 MiB) | 500 : 1 |
| XML depth / nodes / attributes per element | 256 / 4,000,000 / 1,024 |
| Spreadsheet cells loaded | 4,000,000 |
| Text blocks (paragraphs, table rows) | 2,000,000 |

## Verification

- Property-based tests (`proptest`) feed random bytes and random grammar
  fragments to every loader and parser and assert that nothing panics and
  that output is always well formed: `crates/*/tests`.
- Explicit hostile-input tests cover billion laughs, deep nesting, ZIP
  bombs, huge repeat counts, unsafe names, encrypted packages,
  `javascript:` links and CSV injection.
- `cargo clippy` is clean and `unwrap()` is linted in library code.

## Reporting a vulnerability

Please report security issues privately to the maintainers rather than in a
public issue, with a sample file if possible.
