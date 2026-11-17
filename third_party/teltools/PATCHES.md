# Local patches (Akshara Office)

These modifications to upstream `src/lib.rs` were made by the Akshara
contributors, as required by section 4(b) of the Apache License 2.0.

1. **`trans_with(line, &TransOptions)`** added; `trans()` now calls it with
   the upstream behaviour (`TransOptions::BATCH`), so its output is
   unchanged.
   `TransOptions::LIVE` implements the "sensible mode" that upstream's own
   Keyman keyboard documents (`keyman/README.md`, "Differences from
   teltools.trans()"): `om`/`OM`/`AUM` no longer turn into ॐ anywhere in a
   word (type `\om` instead), and `<…>` tag skipping is off.
2. **No panic on non-ASCII input.** Upstream slices the input one *byte* at
   a time, which panics when a multi-byte character (for example an emoji
   or a Telugu letter) is reached. The loop now advances by whole
   characters; ASCII results are identical.
