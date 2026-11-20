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
/// Most paragraphs the header and footer may hold together.
pub const MAX_AREA_PARAGRAPHS: usize = 1000;
/// Most labels [`Document::labels`] creates from a list.
pub const MAX_LABELS: usize = 10_000;
/// Indent step of [`Document::indent`] for non-list paragraphs (half an inch).
pub const INDENT_STEP: f32 = 36.0;

fn ensure_final_paragraph(blocks: &mut Vec<Block>) {
    // Always end with a paragraph so the cursor has somewhere to go.
    if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
        blocks.push(Block::Paragraph(Paragraph::plain("")));
    }
}

fn toggle_area(area: &mut Vec<Paragraph>, on: bool) {
    if !on {
        area.clear();
    } else if area.is_empty() {
        area.push(Paragraph::plain(""));
    }
}

fn normalized_area(mut paragraphs: Vec<Paragraph>) -> Vec<Paragraph> {
    paragraphs.truncate(MAX_AREA_PARAGRAPHS / 2);
    for p in &mut paragraphs {
        p.kind = p.kind.normalized();
        p.normalize();
    }
    paragraphs
}

fn paper_page(id: &str, margin: f32) -> PageSetup {
    let paper = paper_size(id).copied().unwrap_or(PAPER_SIZES[0]);
    PageSetup { width: paper.width, height: paper.height, margin }
}

/// Part of the document a paragraph belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Area {
    /// The main text.
    #[default]
    Body,
    /// The page header, repeated at the top of every page.
    Header,
    /// The page footer, repeated at the bottom of every page.
    Footer,
}

/// Address of a paragraph: a top-level block, a paragraph inside a table
/// cell, or a paragraph of the page header or footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParaRef {
    /// Which part of the document.
    pub area: Area,
    /// Top-level block index (body), or paragraph index (header, footer).
    pub block: usize,
    /// For tables: `(row, column, paragraph index in the cell)`.
    pub cell: Option<(usize, usize, usize)>,
}

impl ParaRef {
    /// A top-level paragraph.
    pub const fn top(block: usize) -> ParaRef {
        ParaRef { area: Area::Body, block, cell: None }
    }

    /// A paragraph inside a table cell.
    pub const fn in_cell(block: usize, row: usize, col: usize, para: usize) -> ParaRef {
        ParaRef { area: Area::Body, block, cell: Some((row, col, para)) }
    }

    /// A paragraph of the page header.
    pub const fn header(para: usize) -> ParaRef {
        ParaRef { area: Area::Header, block: para, cell: None }
    }

    /// A paragraph of the page footer.
    pub const fn footer(para: usize) -> ParaRef {
        ParaRef { area: Area::Footer, block: para, cell: None }
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
    /// Repeated edits of one canvas (moving, typing in a shape).
    Canvas(usize),
    Other,
}

/// Everything an edit can change (and undo restores).
#[derive(Debug, Clone, PartialEq, Default)]
struct Content {
    blocks: Vec<Block>,
    /// Header paragraphs; empty when the document has no header.
    header: Vec<Paragraph>,
    /// Footer paragraphs; empty when the document has no footer.
    footer: Vec<Paragraph>,
    page: PageSetup,
    /// Last shape id handed out in drawing canvases.
    next_shape_id: u32,
}

/// Gives every shape in every canvas a unique id; returns the last one.
fn renumber_shapes(blocks: &mut [Block]) -> u32 {
    let mut next = 0u32;
    for b in blocks.iter_mut() {
        if let Block::Drawing(c) = b {
            for s in &mut c.shapes {
                next += 1;
                s.id = next;
            }
        }
    }
    next
}

/// A text document with editing history.
#[derive(Debug, Clone)]
pub struct Document {
    c: Content,
    /// Document metadata.
    pub meta: Metadata,
    undo: Vec<(Content, EditKind)>,
    redo: Vec<Content>,
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

/// Attributes new text at an offset takes: those of the character before
/// it, without its field (typing next to a field makes plain text).
fn attrs_at(p: &Paragraph, at: usize) -> TextAttrs {
    let mut a = attrs_before(p, at);
    a.field = None;
    a
}

fn attrs_before(p: &Paragraph, at: usize) -> TextAttrs {
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
                Block::Image(p) => p.normalize(),
                Block::Diagram(d) => d.normalize(),
                Block::Drawing(c) => c.normalize(),
            }
        }
        let next_shape_id = renumber_shapes(&mut blocks);
        if !matches!(blocks.last(), Some(Block::Paragraph(_))) {
            // Always end with a paragraph so the cursor has somewhere to go.
            blocks.push(Block::Paragraph(Paragraph::plain("")));
        }
        Document {
            c: Content { blocks, next_shape_id, ..Default::default() },
            meta: Metadata::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::Other,
            modified: false,
        }
    }

    /// Sets the page and the header and footer paragraphs of a freshly built
    /// document, without recording an undo step. An invalid page is
    /// replaced by the default.
    pub fn with_page(mut self, page: PageSetup, header: Vec<Paragraph>, footer: Vec<Paragraph>) -> Self {
        self.c.page = if page.is_valid() { page } else { PageSetup::default() };
        self.c.header = normalized_area(header);
        self.c.footer = normalized_area(footer);
        self
    }

    /// A new envelope: the page is the envelope, the sender's address sits in
    /// the top-left corner and the recipient's address in the lower right
    /// part. Addresses are given one line per `\n`.
    pub fn envelope(page: PageSetup, sender: &str, recipient: &str) -> Self {
        let page = if page.is_valid() { page } else { paper_page("env_dl", 0.4 * 72.0) };
        let lines = |text: &str| -> Vec<String> {
            text.replace("\r\n", "\n")
                .split(['\n', '\r'])
                .map(|l| l.chars().filter(|c| !c.is_control() || *c == '\t').take(500).collect::<String>())
                .take(20)
                .collect()
        };
        let mut blocks = Vec::new();
        let sender_lines: Vec<String> = lines(sender).into_iter().filter(|l| !l.trim().is_empty()).collect();
        let small = TextAttrs { size: Some(10.0), ..Default::default() };
        for line in &sender_lines {
            blocks.push(Block::Paragraph(Paragraph {
                runs: vec![Run { text: line.clone(), attrs: small.clone() }],
                ..Default::default()
            }));
        }
        // The recipient block starts a little below the middle of the
        // envelope and a little left of its centre.
        let inner_h = page.height - 2.0 * page.margin;
        let used = sender_lines.len() as f32 * 14.0;
        let top = (page.height * 0.45 - page.margin - used).clamp(12.0, (inner_h - used).max(12.0));
        let indent = ((page.width * 0.45) - page.margin).max(0.0);
        let mut recipient_lines = lines(recipient);
        while recipient_lines.last().is_some_and(|l| l.trim().is_empty()) && recipient_lines.len() > 1 {
            recipient_lines.pop();
        }
        if recipient_lines.is_empty() {
            recipient_lines.push(String::new());
        }
        for (i, line) in recipient_lines.into_iter().enumerate() {
            blocks.push(Block::Paragraph(Paragraph {
                indent,
                space_before: if i == 0 { top } else { 0.0 },
                ..Paragraph::plain(line)
            }));
        }
        Document::from_blocks(blocks).with_page(page, Vec::new(), Vec::new())
    }

    /// A new set of `count` index cards (1..=500), one per page, each with a
    /// heading line for the card's title and a body paragraph.
    pub fn index_cards(page: PageSetup, count: usize) -> Self {
        let page = if page.is_valid() { page } else { paper_page("card_3x5", 0.25 * 72.0) };
        let mut blocks = Vec::new();
        for i in 0..count.clamp(1, 500) {
            if i > 0 {
                blocks.push(Block::PageBreak);
            }
            blocks.push(Block::Paragraph(Paragraph::with_kind(ParaKind::Heading { level: 2 }, "")));
            blocks.push(Block::Paragraph(Paragraph::plain("")));
        }
        Document::from_blocks(blocks).with_page(page, Vec::new(), Vec::new())
    }

    /// A new sheet (or sheets) of labels.
    ///
    /// `text` holds the label contents, one line per `\n`. With `same`, every
    /// label of one sheet gets the whole text; otherwise the text is a list of
    /// records separated by blank lines, one label each, using as many sheets
    /// as needed (at most [`MAX_LABELS`] labels).
    pub fn labels(sheet: &LabelSheet, text: &str, same: bool) -> Result<Self> {
        if !sheet.is_valid() {
            return Err(bad("the label sheet does not fit its paper"));
        }
        let paper = paper_size(sheet.paper).ok_or_else(|| bad("unknown paper size"))?;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let clean = |l: &str| l.chars().filter(|c| !c.is_control() || *c == '\t').take(500).collect::<String>();
        let records: Vec<Vec<String>> = if same {
            vec![text.trim_matches('\n').split('\n').map(clean).take(40).collect()]
        } else {
            text.split("\n\n")
                .map(|r| r.trim_matches('\n'))
                .filter(|r| !r.trim().is_empty())
                .map(|r| r.split('\n').map(clean).take(40).collect())
                .take(MAX_LABELS)
                .collect()
        };
        let per_sheet = sheet.per_sheet();
        let total = if same { per_sheet } else { records.len().max(1) };
        // The page margin is the smaller of the two offsets; the rest of the
        // offset and the gaps between labels become spacer rows / columns.
        let margin = sheet.top.min(sheet.left).min(paper.width.min(paper.height) / 3.0);
        let mut col_widths = Vec::new();
        let mut label_cols = Vec::new(); // column index of each label column
        if sheet.left - margin > 0.5 {
            col_widths.push(sheet.left - margin);
        }
        for c in 0..sheet.cols {
            label_cols.push(col_widths.len());
            col_widths.push(sheet.width);
            if c + 1 < sheet.cols && sheet.h_pitch - sheet.width > 0.5 {
                col_widths.push(sheet.h_pitch - sheet.width);
            }
        }
        let mut row_heights = Vec::new();
        let mut label_rows = Vec::new();
        if sheet.top - margin > 0.5 {
            row_heights.push(sheet.top - margin);
        }
        for r in 0..sheet.rows {
            label_rows.push(row_heights.len());
            row_heights.push(sheet.height);
            if r + 1 < sheet.rows && sheet.v_pitch - sheet.height > 0.5 {
                row_heights.push(sheet.v_pitch - sheet.height);
            }
        }
        let lines = records.iter().map(Vec::len).max().unwrap_or(1).max(1) as f32;
        let size = ((sheet.height - 8.0) / (lines * 1.4)).clamp(6.0, 11.0).floor();
        let attrs = TextAttrs { size: Some(size), ..Default::default() };
        // Empty lines stay plain: formatting of empty text is not saved.
        let line = |l: &String| match l.is_empty() {
            true => Paragraph::plain(""),
            false => Paragraph { runs: vec![Run { text: l.clone(), attrs: attrs.clone() }], ..Default::default() },
        };
        let label = |record: Option<&Vec<String>>| -> Vec<Paragraph> {
            match record {
                Some(lines) if !lines.is_empty() => lines.iter().map(line).collect(),
                _ => vec![Paragraph::plain("")],
            }
        };
        let mut blocks = Vec::new();
        let mut index = 0;
        while index < total {
            if !blocks.is_empty() {
                blocks.push(Block::PageBreak);
            }
            let mut rows = vec![vec![vec![Paragraph::plain("")]; col_widths.len()]; row_heights.len()];
            for &r in &label_rows {
                for &c in &label_cols {
                    if index < total {
                        rows[r][c] = label(if same { records.first() } else { records.get(index) });
                        index += 1;
                    }
                }
            }
            blocks.push(Block::Table(Table {
                rows,
                col_widths: col_widths.clone(),
                row_heights: row_heights.clone(),
                borders: false,
            }));
        }
        let page = PageSetup { width: paper.width, height: paper.height, margin };
        Ok(Document::from_blocks(blocks).with_page(page, Vec::new(), Vec::new()))
    }

    /// Replaces the header and footer with a letterhead (one undo step).
    pub fn set_letterhead(&mut self, l: &Letterhead) -> Result<()> {
        let lines = |text: &str| -> Vec<String> {
            text.replace("\r\n", "\n")
                .split(['\n', '\r'])
                .map(|x| x.chars().filter(|c| !c.is_control() || *c == '\t').take(500).collect::<String>())
                .filter(|x| !x.trim().is_empty())
                .take(10)
                .collect()
        };
        let para = |text: String, attrs: TextAttrs, align: Align| Paragraph {
            align,
            runs: vec![Run { text, attrs }],
            ..Default::default()
        };
        let mut header = Vec::new();
        let name = l.name.trim();
        if !name.is_empty() {
            let attrs = TextAttrs { bold: true, size: Some(20.0), color: l.color, ..Default::default() };
            header.push(para(name.chars().take(200).collect(), attrs, l.align));
        }
        let small = TextAttrs { size: Some(9.0), ..Default::default() };
        for line in lines(&l.details) {
            header.push(para(line, small.clone(), l.align));
        }
        let tiny = TextAttrs { size: Some(8.0), ..Default::default() };
        let footer: Vec<Paragraph> =
            lines(&l.contact).into_iter().map(|line| para(line, tiny.clone(), Align::Center)).collect();
        self.edit(EditKind::Other, |c| {
            c.header = normalized_area(header);
            c.footer = normalized_area(footer);
            Ok(())
        })
    }

    /// All blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.c.blocks
    }

    /// Page setup.
    pub fn page(&self) -> &PageSetup {
        &self.c.page
    }

    /// Header paragraphs (empty when there is no header).
    pub fn header(&self) -> &[Paragraph] {
        &self.c.header
    }

    /// Footer paragraphs (empty when there is no footer).
    pub fn footer(&self) -> &[Paragraph] {
        &self.c.footer
    }

    /// Number of pages as given by forced page breaks (the core does not lay
    /// out text, so overflowing pages are not counted).
    pub fn page_count(&self) -> usize {
        1 + self.c.blocks.iter().filter(|b| matches!(b, Block::PageBreak)).count()
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
        for block in &self.c.blocks {
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
                Block::Image(p) => out.push(p.description.clone()),
                Block::Diagram(d) => out.push(d.source.clone()),
                Block::Drawing(c) => out.extend(c.shapes.iter().filter(|s| !s.text.is_empty()).map(|s| s.text.clone())),
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
        for block in &self.c.blocks {
            match block {
                Block::Paragraph(p) => count(p),
                Block::Table(t) => {
                    for p in t.rows.iter().flatten().flatten() {
                        count(p);
                    }
                }
                Block::PageBreak | Block::Image(_) | Block::Diagram(_) | Block::Drawing(_) => {}
            }
        }
        stats.tables = self.c.blocks.iter().filter(|b| matches!(b, Block::Table(_))).count();
        stats
    }

    /// The paragraph at `r`.
    pub fn paragraph(&self, r: ParaRef) -> Result<&Paragraph> {
        match (r.area, r.cell) {
            (Area::Body, _) => {}
            (Area::Header, None) => return self.c.header.get(r.block).ok_or_else(|| bad("no such header paragraph")),
            (Area::Footer, None) => return self.c.footer.get(r.block).ok_or_else(|| bad("no such footer paragraph")),
            _ => return Err(bad("headers and footers have no tables")),
        }
        let block = self.c.blocks.get(r.block).ok_or_else(|| bad(format!("no block {}", r.block)))?;
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

    fn paragraph_mut(c: &mut Content, r: ParaRef) -> Result<&mut Paragraph> {
        match (r.area, r.cell) {
            (Area::Body, _) => {}
            (Area::Header, None) => return c.header.get_mut(r.block).ok_or_else(|| bad("no such header paragraph")),
            (Area::Footer, None) => return c.footer.get_mut(r.block).ok_or_else(|| bad("no such footer paragraph")),
            _ => return Err(bad("headers and footers have no tables")),
        }
        let block = c.blocks.get_mut(r.block).ok_or_else(|| bad(format!("no block {}", r.block)))?;
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

    /// Runs `f` on a copy of the content; commits and records undo on
    /// success, leaves the document untouched on error.
    fn edit(&mut self, kind: EditKind, f: impl FnOnce(&mut Content) -> Result<()>) -> Result<()> {
        let mut c = self.c.clone();
        f(&mut c)?;
        if c.blocks.len() > MAX_BLOCKS || c.header.len() + c.footer.len() > MAX_AREA_PARAGRAPHS {
            return Err(OdfError::limit("document has too many blocks"));
        }
        if c == self.c {
            return Ok(());
        }
        let coalesce = matches!(kind, EditKind::Typing(_) | EditKind::Canvas(_)) && kind == self.last_edit && !self.undo.is_empty();
        let before = std::mem::replace(&mut self.c, c);
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
        self.redo.push(std::mem::replace(&mut self.c, before));
        self.last_edit = EditKind::Other;
        self.modified = true;
        true
    }

    /// Re-applies the last undone change.
    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.c, after), EditKind::Other));
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
        self.edit(EditKind::Typing(r), |c| {
            let p = Self::paragraph_mut(c, r)?;
            if p.runs.iter().map(|r| r.text.len()).sum::<usize>() + text.len() > MAX_PARAGRAPH_LEN {
                return Err(OdfError::limit("paragraph is too long"));
            }
            let mut inherit = attrs.cloned().unwrap_or_else(|| attrs_at(p, start));
            inherit.field = None;
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
        self.edit(EditKind::Other, |c| {
            let p = Self::paragraph_mut(c, r)?;
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
        self.edit(EditKind::Other, |c| {
            let p = Self::paragraph_mut(c, r)?;
            let idx = split_runs_at(p, at)?;
            let tail_runs = p.runs.split_off(idx);
            let tail_attrs = tail_runs.first().map(|r| r.attrs.clone()).unwrap_or_else(|| attrs_at(p, at));
            let next_kind = match p.kind {
                ParaKind::Heading { .. } | ParaKind::Title => ParaKind::Normal,
                other => other,
            };
            let mut tail = Paragraph { kind: next_kind, align: p.align, indent: p.indent, runs: tail_runs, ..Default::default() };
            if tail.runs.is_empty() {
                tail.runs.push(Run { text: String::new(), attrs: tail_attrs });
            }
            p.normalize();
            tail.normalize();
            match (r.area, r.cell) {
                (Area::Header, _) => c.header.insert(r.block + 1, tail),
                (Area::Footer, _) => c.footer.insert(r.block + 1, tail),
                (Area::Body, None) => c.blocks.insert(r.block + 1, Block::Paragraph(tail)),
                (Area::Body, Some((row, col, para))) => {
                    if let Block::Table(t) = &mut c.blocks[r.block] {
                        t.rows[row][col].insert(para + 1, tail);
                    }
                }
            }
            new_ref = match r.cell {
                Some((row, col, para)) => ParaRef::in_cell(r.block, row, col, para + 1),
                None => ParaRef { block: r.block + 1, ..r },
            };
            Ok(())
        })?;
        Ok(new_ref)
    }

    /// Joins a paragraph with the one before it (Backspace at the start).
    /// Returns the address of the joined paragraph and the UTF-16 offset of
    /// the join point, or `None` when there is nothing to join with.
    pub fn merge_with_previous(&mut self, r: ParaRef) -> Result<Option<(ParaRef, usize)>> {
        let prev = match r.cell {
            None if r.area != Area::Body => {
                self.paragraph(r)?;
                if r.block == 0 {
                    return Ok(None);
                }
                ParaRef { block: r.block - 1, ..r }
            }
            None => {
                if r.block == 0 || !matches!(self.c.blocks.get(r.block - 1), Some(Block::Paragraph(_))) {
                    // Backspace after a table or page break removes a page break,
                    // otherwise does nothing.
                    if r.block > 0 && matches!(self.c.blocks.get(r.block - 1), Some(Block::PageBreak)) {
                        self.edit(EditKind::Other, |c| {
                            c.blocks.remove(r.block - 1);
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
        self.edit(EditKind::Other, |c| {
            let current = Self::paragraph_mut(c, r)?.clone();
            let p = Self::paragraph_mut(c, prev)?;
            p.runs.extend(current.runs);
            p.normalize();
            match (r.area, r.cell) {
                (Area::Header, _) => {
                    c.header.remove(r.block);
                }
                (Area::Footer, _) => {
                    c.footer.remove(r.block);
                }
                (Area::Body, None) => {
                    c.blocks.remove(r.block);
                }
                (Area::Body, Some((row, col, para))) => {
                    if let Block::Table(t) = &mut c.blocks[r.block] {
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
        self.edit(EditKind::Other, |c| {
            Self::paragraph_mut(c, r)?.kind = kind.normalized();
            Ok(())
        })
    }

    /// Changes the alignment of a paragraph.
    pub fn set_align(&mut self, r: ParaRef, align: Align) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            Self::paragraph_mut(c, r)?.align = align;
            Ok(())
        })
    }

    /// Changes the list level of a list paragraph by `delta` (Tab /
    /// Shift+Tab); other paragraphs move their left indent by `delta` steps
    /// of [`INDENT_STEP`] points.
    pub fn indent(&mut self, r: ParaRef, delta: i32) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            let p = Self::paragraph_mut(c, r)?;
            let shift = |level: u8| (i32::from(level) + delta).clamp(0, 9) as u8;
            match p.kind {
                ParaKind::Bullet { level } => p.kind = ParaKind::Bullet { level: shift(level) },
                ParaKind::Numbered { level } => p.kind = ParaKind::Numbered { level: shift(level) },
                _ => {
                    // Snap to the step grid so odd imported indents line up.
                    let steps = (p.indent / INDENT_STEP).round() as i32 + delta.clamp(-100, 100);
                    p.indent = (steps.max(0) as f32 * INDENT_STEP).min(MAX_PARA_SPACING);
                }
            }
            Ok(())
        })
    }

    /// Sets the left indent and the space above a paragraph, in points
    /// (clamped to 0..=[`MAX_PARA_SPACING`]).
    pub fn set_spacing(&mut self, r: ParaRef, indent: f32, space_before: f32) -> Result<()> {
        if !indent.is_finite() || !space_before.is_finite() {
            return Err(bad("spacing must be a finite number"));
        }
        self.edit(EditKind::Other, |c| {
            let p = Self::paragraph_mut(c, r)?;
            p.indent = indent;
            p.space_before = space_before;
            p.normalize();
            Ok(())
        })
    }

    /// Inserts a field at a UTF-16 offset. Its text is the value the field
    /// has on the first page; the editor computes the real value per page.
    pub fn insert_field(&mut self, r: ParaRef, at: usize, field: Field) -> Result<()> {
        let text = match field {
            Field::PageNumber => "1".to_owned(),
            Field::PageCount => self.page_count().to_string(),
        };
        self.edit(EditKind::Other, |c| {
            let p = Self::paragraph_mut(c, r)?;
            let attrs = TextAttrs { field: Some(field), ..attrs_at(p, at) };
            let idx = split_runs_at(p, at)?;
            p.runs.insert(idx, Run { text, attrs });
            p.normalize();
            Ok(())
        })
    }

    // ---- page setup, header and footer ---------------------------------------------

    /// Changes the page size and margins (one undo step).
    pub fn set_page(&mut self, page: PageSetup) -> Result<()> {
        if !page.is_valid() {
            return Err(bad("page size or margins are out of range"));
        }
        self.edit(EditKind::Other, |c| {
            c.page = page;
            Ok(())
        })
    }

    /// Turns the page header on (with one empty paragraph) or off (deleting
    /// its content).
    pub fn set_header(&mut self, on: bool) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            toggle_area(&mut c.header, on);
            Ok(())
        })
    }

    /// Turns the page footer on (with one empty paragraph) or off (deleting
    /// its content).
    pub fn set_footer(&mut self, on: bool) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            toggle_area(&mut c.footer, on);
            Ok(())
        })
    }

    // ---- block editing ----------------------------------------------------------------

    /// Inserts a block before index `at` (`at == len` appends).
    pub fn insert_block(&mut self, at: usize, block: Block) -> Result<()> {
        if at > self.c.blocks.len() {
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
            Block::Image(mut p) => {
                p.normalize();
                Block::Image(p)
            }
            Block::Diagram(mut d) => {
                d.normalize();
                Block::Diagram(d)
            }
            Block::Drawing(mut canvas) => {
                canvas.normalize();
                Block::Drawing(canvas)
            }
            other => other,
        };
        self.edit(EditKind::Other, |c| {
            let mut block = block;
            if let Block::Drawing(canvas) = &mut block {
                for s in &mut canvas.shapes {
                    c.next_shape_id = c.next_shape_id.checked_add(1).ok_or_else(|| OdfError::limit("too many shapes"))?;
                    s.id = c.next_shape_id;
                }
            }
            c.blocks.insert(at, block);
            ensure_final_paragraph(&mut c.blocks);
            Ok(())
        })
    }

    /// Removes a block. The document always keeps a final paragraph.
    pub fn remove_block(&mut self, at: usize) -> Result<()> {
        if at >= self.c.blocks.len() {
            return Err(bad("block index out of range"));
        }
        self.edit(EditKind::Other, |c| {
            c.blocks.remove(at);
            ensure_final_paragraph(&mut c.blocks);
            Ok(())
        })
    }

    /// Resizes a picture, diagram or drawing. Pictures keep their aspect
    /// ratio when `height` is `None`; diagrams always do.
    pub fn set_frame_size(&mut self, at: usize, width: f32, height: Option<f32>) -> Result<()> {
        if !width.is_finite() || height.is_some_and(|h| !h.is_finite()) {
            return Err(bad("size must be a number"));
        }
        self.edit(EditKind::Other, |c| {
            match c.blocks.get_mut(at) {
                Some(Block::Image(p)) => {
                    let h = height.unwrap_or(p.height * width / p.width.max(1.0));
                    p.width = width;
                    p.height = h;
                    p.normalize();
                }
                Some(Block::Diagram(d)) => {
                    d.width = width;
                    d.normalize();
                }
                Some(Block::Drawing(canvas)) => {
                    canvas.width = width;
                    if let Some(h) = height {
                        canvas.height = h;
                    }
                    canvas.normalize();
                }
                _ => return Err(bad("block is not a picture, diagram or drawing")),
            }
            Ok(())
        })
    }

    /// Aligns a picture, diagram or drawing.
    pub fn set_frame_align(&mut self, at: usize, align: Align) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            match c.blocks.get_mut(at) {
                Some(Block::Image(p)) => p.align = align,
                Some(Block::Diagram(d)) => d.align = align,
                Some(Block::Drawing(canvas)) => canvas.align = align,
                _ => return Err(bad("block is not a picture, diagram or drawing")),
            }
            Ok(())
        })
    }

    /// Changes a picture's alternative text.
    pub fn set_picture_description(&mut self, at: usize, description: &str) -> Result<()> {
        self.edit(EditKind::Other, |c| match c.blocks.get_mut(at) {
            Some(Block::Image(p)) => {
                p.description = description.to_owned();
                p.normalize();
                Ok(())
            }
            _ => Err(bad("block is not a picture")),
        })
    }

    /// Replaces a diagram's Mermaid source.
    pub fn set_diagram_source(&mut self, at: usize, source: &str) -> Result<()> {
        if source.len() > office_draw::model::MAX_DIAGRAM_SOURCE {
            return Err(OdfError::limit("diagram is too long"));
        }
        self.edit(EditKind::Other, |c| match c.blocks.get_mut(at) {
            Some(Block::Diagram(d)) => {
                d.source = source.to_owned();
                Ok(())
            }
            _ => Err(bad("block is not a diagram")),
        })
    }

    /// Edits the shapes of a drawing canvas. `f` gets the shapes and the
    /// id counter; with `coalesce`, repeated edits of the same canvas are
    /// one undo step.
    pub fn edit_canvas<T>(&mut self, at: usize, coalesce: bool, f: impl FnOnce(&mut Vec<office_draw::Shape>, &mut u32) -> Result<T>) -> Result<T> {
        let kind = if coalesce { EditKind::Canvas(at) } else { EditKind::Other };
        let mut out = None;
        self.edit(kind, |c| {
            let mut next = c.next_shape_id;
            let Some(Block::Drawing(canvas)) = c.blocks.get_mut(at) else {
                return Err(bad("block is not a drawing"));
            };
            out = Some(f(&mut canvas.shapes, &mut next)?);
            c.next_shape_id = next;
            Ok(())
        })?;
        out.ok_or_else(|| bad("canvas edit failed"))
    }

    /// The picture with the given id, from pictures and drawings.
    pub fn image(&self, id: &str) -> Option<office_draw::Image> {
        self.c.blocks.iter().find_map(|b| match b {
            Block::Image(p) if p.image.id() == id => Some(p.image.clone()),
            Block::Drawing(c) => c.shapes.iter().find_map(|s| match &s.kind {
                office_draw::ShapeKind::Image { image } if image.id() == id => Some(image.clone()),
                _ => None,
            }),
            _ => None,
        })
    }

    /// Inserts or deletes a table row or column. `count` may be negative to
    /// delete. A table that loses its last row or column is removed.
    pub fn resize_table(&mut self, block: usize, rows: bool, at: usize, count: i32) -> Result<()> {
        self.edit(EditKind::Other, |c| {
            let blocks = &mut c.blocks;
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
                if rows {
                    t.insert_rows(at, n);
                } else {
                    t.insert_cols(at, n);
                }
            } else if rows {
                t.delete_rows(at, n);
            } else {
                t.delete_cols(at, n);
            }
            if t.rows.is_empty() || t.cols() == 0 {
                blocks.remove(block);
                ensure_final_paragraph(blocks);
            }
            Ok(())
        })
    }

    // ---- search ---------------------------------------------------------------------

    /// Visits every paragraph: header, body (with table cells), footer.
    fn each_paragraph(&self, f: &mut dyn FnMut(ParaRef, &Paragraph)) {
        for (i, p) in self.c.header.iter().enumerate() {
            f(ParaRef::header(i), p);
        }
        self.each_body_paragraph(f);
        for (i, p) in self.c.footer.iter().enumerate() {
            f(ParaRef::footer(i), p);
        }
    }

    fn each_body_paragraph(&self, f: &mut dyn FnMut(ParaRef, &Paragraph)) {
        for (b, block) in self.c.blocks.iter().enumerate() {
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
                Block::PageBreak | Block::Image(_) | Block::Diagram(_) | Block::Drawing(_) => {}
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
        self.edit(EditKind::Other, |c| {
            // Replace back to front so earlier offsets stay valid.
            for m in matches.iter().rev() {
                let p = Self::paragraph_mut(c, m.para)?;
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
