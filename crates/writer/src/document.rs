//! The editable document: editing operations, undo/redo, search.
//!
//! Positions inside a paragraph are **UTF-16 code unit offsets**, the unit
//! the Flutter text widgets use. Every operation validates its block index
//! and offsets and fails with [`OdfError::Invalid`] instead of panicking.

use crate::model::*;
use office_odf::util::{byte_to_utf16, utf16_len, utf16_to_byte};
use office_odf::{Metadata, OdfError, Result};

/// Largest number of undo steps kept.
pub const MAX_UNDO: usize = 200;
/// Largest number of blocks a document may hold.
pub const MAX_BLOCKS: usize = 1_000_000;
/// Longest paragraph in bytes.
pub const MAX_PARAGRAPH_LEN: usize = 4 << 20;

/// Address of a paragraph: a top-level block, or a paragraph inside a table
/// cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParaRef {
    /// Top-level block index.
    pub block: usize,
    /// For tables: `(row, column, paragraph index in the cell)`.
    pub cell: Option<(usize, usize, usize)>,
}

impl ParaRef {
    /// A top-level paragraph.
    pub const fn top(block: usize) -> ParaRef {
        ParaRef { block, cell: None }
    }

    /// A paragraph inside a table cell.
    pub const fn in_cell(block: usize, row: usize, col: usize, para: usize) -> ParaRef {
        ParaRef { block, cell: Some((row, col, para)) }
    }
}

/// A search hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// Paragraph containing the hit.
    pub para: ParaRef,
    /// Start offset (UTF-16).
    pub start: usize,
    /// End offset (UTF-16, exclusive).
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing(ParaRef),
    Other,
}

/// A text document with editing history.
#[derive(Debug, Clone)]
pub struct Document {
    blocks: Vec<Block>,
    /// Document metadata.
    pub meta: Metadata,
    /// Page setup.
    pub page: PageSetup,
    undo: Vec<(Vec<Block>, EditKind)>,
    redo: Vec<Vec<Block>>,
    last_edit: EditKind,
    modified: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

/// Splits the runs of `p` at UTF-16 offset `at` so a run boundary falls there.
/// Returns the index of the first run at or after the offset.
fn split_runs_at(p: &mut Paragraph, at: usize) -> Result<usize> {
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < p.runs.len() {
        let len = utf16_len(&p.runs[i].text);
        if at == pos {
            return Ok(i);
        }
        if at < pos + len {
            let byte = utf16_to_byte(&p.runs[i].text, at - pos)?;
            let tail = p.runs[i].text.split_off(byte);
            let attrs = p.runs[i].attrs.clone();
            p.runs.insert(i + 1, Run { text: tail, attrs });
            return Ok(i + 1);
        }
        pos += len;
        i += 1;
    }
    if at == pos {
        Ok(p.runs.len())
    } else {
        Err(bad(format!("offset {at} is past the end of the paragraph ({pos})")))
    }
}

/// Attributes in effect at an offset (those of the character before it).
fn attrs_at(p: &Paragraph, at: usize) -> TextAttrs {
    let mut pos = 0;
    let mut current = p.runs.first().map(|r| r.attrs.clone()).unwrap_or_default();
    for run in &p.runs {
        let len = utf16_len(&run.text);
        if len > 0 && pos < at {
            current = run.attrs.clone();
        }
        pos += len;
        if pos >= at {
            break;
        }
    }
    current
}

impl Document {
    /// An empty document with one empty paragraph.
    pub fn new() -> Self {
        Self::from_blocks(Vec::new())
    }

    /// A document with the given blocks (normalised; never empty).
    pub fn from_blocks(mut blocks: Vec<Block>) -> Self {
        for block in &mut blocks {
            match block {
                Block::Paragraph(p) => {
                    p.kind = p.kind.normalized();
                    p.normalize();
                }
                Block::Table(t) => t.normalize(),
                Block::PageBreak => {}
            }
        }
        if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
            // Always end with a paragraph so the cursor has somewhere to go.
            blocks.push(Block::Paragraph(Paragraph::plain("")));
        }
        Document {
            blocks,
            meta: Metadata::default(),
            page: PageSetup::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::Other,
            modified: false,
        }
    }

    /// All blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Whether the document changed since it was created, loaded or saved.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Marks the current state as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// The plain text of the whole document, paragraphs separated by `\n`.
    pub fn plain_text(&self) -> String {
        let mut out = Vec::new();
        for block in &self.blocks {
            match block {
                Block::Paragraph(p) => out.push(p.text()),
                Block::Table(t) => {
                    for row in &t.rows {
                        let cells: Vec<String> = row
                            .iter()
                            .map(|cell| cell.iter().map(Paragraph::text).collect::<Vec<_>>().join(" "))
                            .collect();
                        out.push(cells.join("\t"));
                    }
                }
                Block::PageBreak => out.push("\u{c}".into()),
            }
        }
        out.join("\n")
    }

    /// Word and character counts.
    pub fn stats(&self) -> Stats {
        let mut stats = Stats::default();
        let mut count = |p: &Paragraph| {
            let text = p.text();
            if !text.trim().is_empty() {
                stats.paragraphs += 1;
            }
            stats.words += text.split_whitespace().count();
            stats.chars += text.chars().count();
            stats.chars_no_spaces += text.chars().filter(|c| !c.is_whitespace()).count();
        };
        for block in &self.blocks {
            match block {
                Block::Paragraph(p) => count(p),
                Block::Table(t) => {
                    for p in t.rows.iter().flatten().flatten() {
                        count(p);
                    }
                }
                Block::PageBreak => {}
            }
        }
        stats.tables = self.blocks.iter().filter(|b| matches!(b, Block::Table(_))).count();
        stats
    }

    /// The paragraph at `r`.
    pub fn paragraph(&self, r: ParaRef) -> Result<&Paragraph> {
        let block = self.blocks.get(r.block).ok_or_else(|| bad(format!("no block {}", r.block)))?;
        match (block, r.cell) {
            (Block::Paragraph(p), None) => Ok(p),
            (Block::Table(t), Some((row, col, para))) => t
                .rows
                .get(row)
                .and_then(|cells| cells.get(col))
                .and_then(|cell| cell.get(para))
                .ok_or_else(|| bad("no such table cell paragraph")),
            _ => Err(bad("block is not a paragraph at that address")),
        }
    }

    fn paragraph_mut(blocks: &mut [Block], r: ParaRef) -> Result<&mut Paragraph> {
        let block = blocks.get_mut(r.block).ok_or_else(|| bad(format!("no block {}", r.block)))?;
        match (block, r.cell) {
            (Block::Paragraph(p), None) => Ok(p),
            (Block::Table(t), Some((row, col, para))) => t
                .rows
                .get_mut(row)
                .and_then(|cells| cells.get_mut(col))
                .and_then(|cell| cell.get_mut(para))
                .ok_or_else(|| bad("no such table cell paragraph")),
            _ => Err(bad("block is not a paragraph at that address")),
        }
    }

    // ---- history ----------------------------------------------------------------

    /// Runs `f` on a copy of the blocks; commits and records undo on success,
    /// leaves the document untouched on error.
    fn edit(&mut self, kind: EditKind, f: impl FnOnce(&mut Vec<Block>) -> Result<()>) -> Result<()> {
        let mut blocks = self.blocks.clone();
        f(&mut blocks)?;
        if blocks.len() > MAX_BLOCKS {
            return Err(OdfError::limit("document has too many blocks"));
        }
        if blocks == self.blocks {
            return Ok(());
        }
        let coalesce = matches!(kind, EditKind::Typing(_)) && kind == self.last_edit && !self.undo.is_empty();
        let before = std::mem::replace(&mut self.blocks, blocks);
        if !coalesce {
            self.undo.push((before, kind));
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = kind;
        self.modified = true;
        Ok(())
    }

    /// Whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts the last change (consecutive typing in one paragraph counts as
    /// one change).
    pub fn undo(&mut self) -> bool {
        let Some((before, _)) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.blocks, before));
        self.last_edit = EditKind::Other;
        self.modified = true;
        true
    }

    /// Re-applies the last undone change.
    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.blocks, after), EditKind::Other));
        self.last_edit = EditKind::Other;
        self.modified = true;
        true
    }

    /// Ends the current typing group so the next keystroke starts a new undo
    /// step (call when the cursor moves to another place).
    pub fn break_undo_group(&mut self) {
        self.last_edit = EditKind::Other;
    }

    // ---- text editing --------------------------------------------------------------

    /// Replaces the UTF-16 range `start..end` of a paragraph with `text`.
    /// Inserted text takes the attributes of the character before `start`
    /// (or `attrs` when given).
    pub fn replace_text(
        &mut self,
        r: ParaRef,
        start: usize,
        end: usize,
        text: &str,
        attrs: Option<&TextAttrs>,
    ) -> Result<()> {
        if start > end {
            return Err(bad("range start after end"));
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.edit(EditKind::Typing(r), |blocks| {
            let p = Self::paragraph_mut(blocks, r)?;
            if p.runs.iter().map(|r| r.text.len()).sum::<usize>() + text.len() > MAX_PARAGRAPH_LEN {
                return Err(OdfError::limit("paragraph is too long"));
            }
            let inherit = attrs.cloned().unwrap_or_else(|| attrs_at(p, start));
            let first = split_runs_at(p, start)?;
            let last = split_runs_at(p, end)?;
            p.runs.drain(first..last);
            if !text.is_empty() {
                p.runs.insert(first, Run { text, attrs: inherit });
            }
            p.normalize();
            Ok(())
        })
    }

    /// Sets the whole text of a paragraph, preserving attributes of the
    /// unchanged prefix and suffix (what the editor calls after each
    /// keystroke).
    pub fn set_text(&mut self, r: ParaRef, text: &str) -> Result<()> {
        let old = self.paragraph(r)?.text();
        if old == text {
            return Ok(());
        }
        // Common prefix / suffix in chars, then convert to UTF-16 offsets.
        let old_chars: Vec<char> = old.chars().collect();
        let new_chars: Vec<char> = text.chars().collect();
        let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
        let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
        let suffix = old_chars
            .iter()
            .rev()
            .zip(new_chars.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        let u16 = |chars: &[char]| chars.iter().map(|c| c.len_utf16()).sum::<usize>();
        let start = u16(&old_chars[..prefix]);
        let end = u16(&old_chars[..old_chars.len() - suffix]);
        let inserted: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
        self.replace_text(r, start, end, &inserted, None)
    }

    /// Applies an attribute patch to the UTF-16 range `start..end`.
    pub fn apply_attrs(&mut self, r: ParaRef, start: usize, end: usize, patch: &AttrPatch) -> Result<()> {
        if start > end {
            return Err(bad("range start after end"));
        }
        if let Some(Some(link)) = &patch.link {
            if sanitize_link(link).is_none() {
                return Err(bad("only http, https and mailto links are allowed"));
            }
        }
        self.edit(EditKind::Other, |blocks| {
            let p = Self::paragraph_mut(blocks, r)?;
            let first = split_runs_at(p, start)?;
            let last = split_runs_at(p, end)?;
            for run in &mut p.runs[first..last] {
                run.attrs = patch.apply(&run.attrs);
            }
            if start == end && utf16_len(&p.text()) == 0 {
                // Formatting an empty paragraph sets the style for new text.
                if let Some(run) = p.runs.first_mut() {
                    run.attrs = patch.apply(&run.attrs);
                }
            }
            p.normalize();
            Ok(())
        })
    }

    /// Splits a paragraph at `at` (Enter key). The new paragraph keeps the
    /// kind, except that headings and the title are followed by body text.
    /// Returns the address of the new paragraph.
    pub fn split_paragraph(&mut self, r: ParaRef, at: usize) -> Result<ParaRef> {
        let mut new_ref = r;
        self.edit(EditKind::Other, |blocks| {
            let p = Self::paragraph_mut(blocks, r)?;
            let idx = split_runs_at(p, at)?;
            let tail_runs = p.runs.split_off(idx);
            let tail_attrs = tail_runs.first().map(|r| r.attrs.clone()).unwrap_or_else(|| attrs_at(p, at));
            let next_kind = match p.kind {
                ParaKind::Heading { .. } | ParaKind::Title => ParaKind::Normal,
                other => other,
            };
            let mut tail = Paragraph { kind: next_kind, align: p.align, runs: tail_runs };
            if tail.runs.is_empty() {
                tail.runs.push(Run { text: String::new(), attrs: tail_attrs });
            }
            p.normalize();
            tail.normalize();
            match r.cell {
                None => {
                    blocks.insert(r.block + 1, Block::Paragraph(tail));
                    new_ref = ParaRef::top(r.block + 1);
                }
                Some((row, col, para)) => {
                    if let Block::Table(t) = &mut blocks[r.block] {
                        t.rows[row][col].insert(para + 1, tail);
                    }
                    new_ref = ParaRef::in_cell(r.block, row, col, para + 1);
                }
            }
            Ok(())
        })?;
        Ok(new_ref)
    }

    /// Joins a paragraph with the one before it (Backspace at the start).
    /// Returns the address of the joined paragraph and the UTF-16 offset of
    /// the join point, or `None` when there is nothing to join with.
    pub fn merge_with_previous(&mut self, r: ParaRef) -> Result<Option<(ParaRef, usize)>> {
        let prev = match r.cell {
            None => {
                if r.block == 0 || !matches!(self.blocks.get(r.block - 1), Some(Block::Paragraph(_))) {
                    // Backspace after a table or page break removes a page break,
                    // otherwise does nothing.
                    if r.block > 0 && matches!(self.blocks.get(r.block - 1), Some(Block::PageBreak)) {
                        self.edit(EditKind::Other, |blocks| {
                            blocks.remove(r.block - 1);
                            Ok(())
                        })?;
                        return Ok(Some((ParaRef::top(r.block - 1), 0)));
                    }
                    self.paragraph(r)?;
                    return Ok(None);
                }
                ParaRef::top(r.block - 1)
            }
            Some((row, col, para)) => {
                if para == 0 {
                    return Ok(None);
                }
                ParaRef::in_cell(r.block, row, col, para - 1)
            }
        };
        let join_at = utf16_len(&self.paragraph(prev)?.text());
        self.paragraph(r)?;
        self.edit(EditKind::Other, |blocks| {
            let current = Self::paragraph_mut(blocks, r)?.clone();
            let p = Self::paragraph_mut(blocks, prev)?;
            p.runs.extend(current.runs);
            p.normalize();
            match r.cell {
                None => {
                    blocks.remove(r.block);
                }
                Some((row, col, para)) => {
                    if let Block::Table(t) = &mut blocks[r.block] {
                        t.rows[row][col].remove(para);
                    }
                }
            }
            Ok(())
        })?;
        Ok(Some((prev, join_at)))
    }

    /// Changes the kind of a paragraph.
    pub fn set_kind(&mut self, r: ParaRef, kind: ParaKind) -> Result<()> {
        self.edit(EditKind::Other, |blocks| {
            Self::paragraph_mut(blocks, r)?.kind = kind.normalized();
            Ok(())
        })
    }

    /// Changes the alignment of a paragraph.
    pub fn set_align(&mut self, r: ParaRef, align: Align) -> Result<()> {
        self.edit(EditKind::Other, |blocks| {
            Self::paragraph_mut(blocks, r)?.align = align;
            Ok(())
        })
    }

    /// Changes the list level of a list paragraph by `delta` (Tab /
    /// Shift+Tab). Non-list paragraphs are unchanged.
    pub fn indent(&mut self, r: ParaRef, delta: i32) -> Result<()> {
        self.edit(EditKind::Other, |blocks| {
            let p = Self::paragraph_mut(blocks, r)?;
            let shift = |level: u8| (i32::from(level) + delta).clamp(0, 9) as u8;
            p.kind = match p.kind {
                ParaKind::Bullet { level } => ParaKind::Bullet { level: shift(level) },
                ParaKind::Numbered { level } => ParaKind::Numbered { level: shift(level) },
                other => other,
            };
            Ok(())
        })
    }

    // ---- block editing ----------------------------------------------------------------

    /// Inserts a block before index `at` (`at == len` appends).
    pub fn insert_block(&mut self, at: usize, block: Block) -> Result<()> {
        if at > self.blocks.len() {
            return Err(bad("block index out of range"));
        }
        let block = match block {
            Block::Table(mut t) => {
                t.normalize();
                Block::Table(t)
            }
            Block::Paragraph(mut p) => {
                p.kind = p.kind.normalized();
                p.normalize();
                Block::Paragraph(p)
            }
            other => other,
        };
        self.edit(EditKind::Other, |blocks| {
            blocks.insert(at, block);
            if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
                blocks.push(Block::Paragraph(Paragraph::plain("")));
            }
            Ok(())
        })
    }

    /// Removes a block. The document always keeps a final paragraph.
    pub fn remove_block(&mut self, at: usize) -> Result<()> {
        if at >= self.blocks.len() {
            return Err(bad("block index out of range"));
        }
        self.edit(EditKind::Other, |blocks| {
            blocks.remove(at);
            if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
                blocks.push(Block::Paragraph(Paragraph::plain("")));
            }
            Ok(())
        })
    }

    /// Inserts or deletes a table row or column. `count` may be negative to
    /// delete. A table that loses its last row or column is removed.
    pub fn resize_table(&mut self, block: usize, rows: bool, at: usize, count: i32) -> Result<()> {
        self.edit(EditKind::Other, |blocks| {
            let Some(Block::Table(t)) = blocks.get_mut(block) else {
                return Err(bad("block is not a table"));
            };
            let (len, cols) = (t.rows.len(), t.cols());
            let span = if rows { len } else { cols };
            if at > span || (count < 0 && at + count.unsigned_abs() as usize > span) {
                return Err(bad("row or column out of range"));
            }
            let n = count.unsigned_abs() as usize;
            if count > 0 {
                if (rows && len + n > 10_000) || (!rows && cols + n > 64) {
                    return Err(OdfError::limit("table is too large"));
                }
                for _ in 0..n {
                    if rows {
                        t.rows.insert(at, vec![vec![Paragraph::plain("")]; cols]);
                    } else {
                        for row in &mut t.rows {
                            row.insert(at, vec![Paragraph::plain("")]);
                        }
                    }
                }
            } else if rows {
                t.rows.drain(at..at + n);
            } else {
                for row in &mut t.rows {
                    row.drain(at..at + n);
                }
            }
            if t.rows.is_empty() || t.cols() == 0 {
                blocks.remove(block);
                if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
                    blocks.push(Block::Paragraph(Paragraph::plain("")));
                }
            }
            Ok(())
        })
    }

    // ---- search ---------------------------------------------------------------------

    fn each_paragraph(&self, f: &mut dyn FnMut(ParaRef, &Paragraph)) {
        for (b, block) in self.blocks.iter().enumerate() {
            match block {
                Block::Paragraph(p) => f(ParaRef::top(b), p),
                Block::Table(t) => {
                    for (ri, row) in t.rows.iter().enumerate() {
                        for (ci, cell) in row.iter().enumerate() {
                            for (pi, p) in cell.iter().enumerate() {
                                f(ParaRef::in_cell(b, ri, ci, pi), p);
                            }
                        }
                    }
                }
                Block::PageBreak => {}
            }
        }
    }

    /// Finds all occurrences of `needle` (plain text, optionally
    /// case-insensitive). At most `limit` matches are returned.
    pub fn find(&self, needle: &str, match_case: bool, limit: usize) -> Vec<Match> {
        let mut out = Vec::new();
        if needle.is_empty() {
            return out;
        }
        let fold = |s: &str| -> String {
            if match_case { s.to_owned() } else { s.chars().flat_map(char::to_lowercase).collect() }
        };
        let needle_folded = fold(needle);
        self.each_paragraph(&mut |r, p| {
            if out.len() >= limit {
                return;
            }
            let text = p.text();
            // Case folding can change lengths, so search char-wise on a
            // folded copy only when it preserves the char count.
            let folded = fold(&text);
            let (hay, ndl) = if folded.chars().count() == text.chars().count() {
                (folded, needle_folded.clone())
            } else {
                (text.clone(), needle.to_owned())
            };
            let hay_chars: Vec<char> = hay.chars().collect();
            let text_chars: Vec<char> = text.chars().collect();
            let ndl_chars: Vec<char> = ndl.chars().collect();
            if ndl_chars.len() > hay_chars.len() {
                return;
            }
            let mut i = 0;
            while i + ndl_chars.len() <= hay_chars.len() && out.len() < limit {
                if hay_chars[i..i + ndl_chars.len()] == ndl_chars[..] {
                    let start: usize = text_chars[..i].iter().map(|c| c.len_utf16()).sum();
                    let len: usize = text_chars[i..i + ndl_chars.len()].iter().map(|c| c.len_utf16()).sum();
                    out.push(Match { para: r, start, end: start + len });
                    i += ndl_chars.len();
                } else {
                    i += 1;
                }
            }
        });
        out
    }

    /// Replaces every occurrence of `needle` with `replacement` as one undo
    /// step. Returns the number of replacements.
    pub fn replace_all(&mut self, needle: &str, replacement: &str, match_case: bool) -> Result<usize> {
        let matches = self.find(needle, match_case, usize::MAX);
        if matches.is_empty() {
            return Ok(0);
        }
        let count = matches.len();
        self.edit(EditKind::Other, |blocks| {
            // Replace back to front so earlier offsets stay valid.
            for m in matches.iter().rev() {
                let p = Self::paragraph_mut(blocks, m.para)?;
                let inherit = attrs_at(p, m.start + 1);
                let first = split_runs_at(p, m.start)?;
                let last = split_runs_at(p, m.end)?;
                p.runs.drain(first..last);
                if !replacement.is_empty() {
                    p.runs.insert(first, Run { text: replacement.to_owned(), attrs: inherit });
                }
                p.normalize();
                if p.runs.iter().map(|r| r.text.len()).sum::<usize>() > MAX_PARAGRAPH_LEN {
                    return Err(OdfError::limit("paragraph is too long"));
                }
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Converts a byte offset in a paragraph's text to UTF-16 (helper for
    /// callers working with Rust string indices).
    pub fn utf16_offset(&self, r: ParaRef, byte: usize) -> Result<usize> {
        let text = self.paragraph(r)?.text();
        if !text.is_char_boundary(byte.min(text.len())) {
            return Err(bad("offset is not on a character boundary"));
        }
        Ok(byte_to_utf16(&text, byte))
    }
}
