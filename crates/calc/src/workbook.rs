//! The workbook model: sheets, cells, styles, recalculation, undo and
//! structural edits.

use crate::address::{CellAddr, Range, MAX_COLS, MAX_ROWS};
use crate::deps::{precedents, CellKey, DepGraph};
use crate::eval::{compare_values, evaluate, Ctx, Host, VOLATILE_FUNCTIONS};
use crate::format::{self, format_value, parse_input, Input, NumberFormat};
use crate::formula::{self, CellRef, Expr, Syntax};
use crate::value::{ErrorKind, Value};
use office_odf::{Metadata, OdfError, Result, Rgb};
use std::cell::Cell as StdCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Largest number of cells one bulk operation (fill, sort, style, paste) may
/// touch. Keeps a slip of the mouse from allocating gigabytes.
pub const MAX_BULK_CELLS: u64 = 1_000_000;
/// Largest number of sheets in a workbook.
pub const MAX_SHEETS: usize = 1024;
/// Number of undo steps kept.
pub const MAX_UNDO: usize = 100;
/// Longest text stored in one cell, in bytes.
pub const MAX_CELL_TEXT: usize = 1 << 20;

static EMPTY: Value = Value::Empty;

/// What the user put into a cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Nothing (the cell may still carry a style).
    Empty,
    /// A number.
    Number(f64),
    /// Text.
    Text(String),
    /// A logical value.
    Bool(bool),
    /// A formula.
    Formula(Expr),
}

/// Horizontal alignment of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HAlign {
    /// Numbers right, text left.
    #[default]
    General,
    /// Left aligned.
    Left,
    /// Centred.
    Center,
    /// Right aligned.
    Right,
}

/// Visual attributes of a cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CellStyle {
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Horizontal alignment.
    pub align: HAlign,
    /// Text colour.
    pub color: Option<Rgb>,
    /// Background colour.
    pub background: Option<Rgb>,
    /// Number format.
    pub format: NumberFormat,
}

/// A partial style: only the `Some` fields are applied.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StylePatch {
    /// New bold flag.
    pub bold: Option<bool>,
    /// New italic flag.
    pub italic: Option<bool>,
    /// New underline flag.
    pub underline: Option<bool>,
    /// New alignment.
    pub align: Option<HAlign>,
    /// New text colour (`Some(None)` resets to automatic).
    pub color: Option<Option<Rgb>>,
    /// New background (`Some(None)` removes it).
    pub background: Option<Option<Rgb>>,
    /// New number format.
    pub format: Option<NumberFormat>,
}

impl StylePatch {
    fn apply(&self, style: &CellStyle) -> CellStyle {
        CellStyle {
            bold: self.bold.unwrap_or(style.bold),
            italic: self.italic.unwrap_or(style.italic),
            underline: self.underline.unwrap_or(style.underline),
            align: self.align.unwrap_or(style.align),
            color: self.color.unwrap_or(style.color),
            background: self.background.unwrap_or(style.background),
            format: self.format.clone().unwrap_or_else(|| style.format.clone()),
        }
    }
}

/// One cell: its content, cached value and style id.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// What the user entered.
    pub content: Content,
    /// Current value (for formulas: the last calculated result).
    pub value: Value,
    /// Index into the workbook's style table (0 is the default style).
    pub style: u32,
}

type CellData = (Content, u32);

fn value_of(content: &Content) -> Value {
    match content {
        Content::Empty => Value::Empty,
        Content::Number(n) => Value::Number(*n),
        Content::Text(s) => Value::Text(s.clone()),
        Content::Bool(b) => Value::Bool(*b),
        Content::Formula(_) => Value::Empty,
    }
}

/// A sheet: a sparse grid of cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// Sheet name (unique within the workbook, case-insensitively).
    pub name: String,
    cells: BTreeMap<CellAddr, Cell>,
    /// Explicit column widths in points.
    pub col_widths: BTreeMap<u32, f32>,
    max_col: u32,
    print: PrintArea,
}

/// What part of a sheet is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrintArea {
    /// The print range; `None` prints the used part of the sheet.
    pub range: Option<Range>,
    /// Rows (first, last; 0-based) repeated at the top of every page.
    pub repeat_rows: Option<(u32, u32)>,
}

/// Where the span `a..=b` ends up after inserting (`insert`) or deleting
/// `count` rows/columns at `at`; `None` when it is deleted completely.
fn shift_span(a: u32, b: u32, at: u32, count: u32, insert: bool, max: u32) -> Option<(u32, u32)> {
    if insert {
        let move_by = |v: u32| if v >= at { v.saturating_add(count) } else { v };
        let (na, nb) = (move_by(a), move_by(b).min(max - 1));
        return (na < max).then_some((na, nb));
    }
    if b < at {
        Some((a, b))
    } else if a >= at + count {
        Some((a - count, b - count))
    } else if a >= at && b < at + count {
        None
    } else {
        let na = if a < at { a } else { at };
        let nb = if b >= at + count { b - count } else { at - 1 };
        Some((na, nb))
    }
}

impl Sheet {
    fn new(name: String) -> Self {
        Sheet { name, cells: BTreeMap::new(), col_widths: BTreeMap::new(), max_col: 0, print: PrintArea::default() }
    }

    /// What part of the sheet is printed.
    pub fn print_area(&self) -> &PrintArea {
        &self.print
    }

    /// The cell at `addr`, if it has content or a style.
    pub fn cell(&self, addr: CellAddr) -> Option<&Cell> {
        self.cells.get(&addr)
    }

    /// All stored cells in row-major order.
    pub fn cells(&self) -> impl Iterator<Item = (CellAddr, &Cell)> {
        self.cells.iter().map(|(addr, cell)| (*addr, cell))
    }

    /// Stored cells whose row lies in `first_row..=last_row`, row-major.
    pub fn cells_in_rows(&self, first_row: u32, last_row: u32) -> impl Iterator<Item = (CellAddr, &Cell)> {
        self.cells
            .range(CellAddr::new(first_row, 0)..=CellAddr::new(last_row, u32::MAX))
            .map(|(addr, cell)| (*addr, cell))
    }

    /// Number of stored cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Bounding box of all stored cells.
    pub fn used_range(&self) -> Option<Range> {
        let (first, _) = self.cells.first_key_value()?;
        let (last, _) = self.cells.last_key_value()?;
        let min_col = self.cells.keys().map(|a| a.col).min().unwrap_or(0);
        let max_col = self.cells.keys().map(|a| a.col).max().unwrap_or(0);
        Some(Range::new(CellAddr::new(first.row, min_col), CellAddr::new(last.row, max_col)))
    }

    fn put(&mut self, addr: CellAddr, data: Option<CellData>) {
        match data {
            Some((Content::Empty, 0)) | None => {
                self.cells.remove(&addr);
            }
            Some((content, style)) => {
                self.max_col = self.max_col.max(addr.col);
                let value = value_of(&content);
                self.cells.insert(addr, Cell { content, value, style });
            }
        }
    }

    fn data(&self, addr: CellAddr) -> Option<CellData> {
        self.cells.get(&addr).map(|c| (c.content.clone(), c.style))
    }
}

#[derive(Debug, Clone)]
enum UndoEntry {
    Cells { sheet: u32, changes: Vec<(CellAddr, Option<CellData>, Option<CellData>)> },
    Snapshot { before: Vec<Sheet>, after: Vec<Sheet> },
    Width { sheet: u32, col: u32, before: Option<f32>, after: Option<f32> },
    Page { before: PageSetup, after: PageSetup },
    Print { sheet: u32, before: PrintArea, after: PrintArea },
}

/// How the workbook is printed (the default page style, used by every
/// sheet). Sizes are in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    /// Paper width (wider than tall in landscape).
    pub width: f32,
    /// Paper height.
    pub height: f32,
    /// Margin on all sides.
    pub margin: f32,
    /// Scale in percent (10..=400); ignored with [`fit_width`](Self::fit_width).
    pub scale: u16,
    /// Shrink each sheet so its columns fit the page width.
    pub fit_width: bool,
    /// Print the cell grid.
    pub grid: bool,
    /// Print the header (sheet name) and footer (page number).
    pub header_footer: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        // A4 with 2 cm margins, OpenOffice's default.
        PageSetup {
            width: 595.28,
            height: 841.89,
            margin: 56.69,
            scale: 100,
            fit_width: false,
            grid: true,
            header_footer: true,
        }
    }
}

impl PageSetup {
    /// Whether the values are usable: sides 72..=14400 pt, margins leaving
    /// a third of the shorter side, scale 10..=400 %.
    pub fn is_valid(&self) -> bool {
        let side = 72.0..=14_400.0;
        side.contains(&self.width)
            && side.contains(&self.height)
            && self.margin.is_finite()
            && (0.0..=self.width.min(self.height) / 3.0).contains(&self.margin)
            && (10..=400).contains(&self.scale)
    }
}

/// Row or column axis for structural edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Rows.
    Row,
    /// Columns.
    Col,
}

/// A spreadsheet document.
#[derive(Debug)]
pub struct Workbook {
    sheets: Vec<Sheet>,
    styles: Vec<CellStyle>,
    style_ids: HashMap<CellStyle, u32>,
    deps: DepGraph,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    rng: StdCell<u64>,
    fixed_now: Option<f64>,
    utc_offset_minutes: i32,
    modified: bool,
    page: PageSetup,
    /// Document metadata.
    pub meta: Metadata,
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks a sheet name: 1 to 64 characters, no control characters, none of
/// `' [ ] * ? : / \`, no leading or trailing blanks.
pub fn is_valid_sheet_name(name: &str) -> bool {
    let count = name.chars().count();
    (1..=64).contains(&count)
        && name.trim() == name
        && !name.chars().any(|c| c.is_control() || matches!(c, '\'' | '[' | ']' | '*' | '?' | ':' | '/' | '\\'))
}

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

impl Workbook {
    /// Creates a workbook with one empty sheet called `Sheet1`.
    pub fn new() -> Self {
        let mut wb = Self::empty();
        wb.sheets.push(Sheet::new("Sheet1".into()));
        wb
    }

    pub(crate) fn empty() -> Self {
        Workbook {
            sheets: Vec::new(),
            styles: vec![CellStyle::default()],
            style_ids: HashMap::from([(CellStyle::default(), 0)]),
            deps: DepGraph::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            rng: StdCell::new(0x9E37_79B9_7F4A_7C15),
            fixed_now: None,
            utc_offset_minutes: 0,
            modified: false,
            page: PageSetup::default(),
            meta: Metadata::default(),
        }
    }

    // ---- sheets -------------------------------------------------------------

    /// Number of sheets.
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// The sheet at `index`.
    pub fn sheet(&self, index: usize) -> Result<&Sheet> {
        self.sheets.get(index).ok_or_else(|| bad(format!("no sheet {index}")))
    }

    /// All sheets in order.
    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    fn find_sheet(&self, name: &str) -> Option<usize> {
        self.sheets
            .iter()
            .position(|s| s.name == name)
            .or_else(|| {
                let lower = name.to_lowercase();
                self.sheets.iter().position(|s| s.name.to_lowercase() == lower)
            })
    }

    fn unused_sheet_name(&self) -> String {
        (1..)
            .map(|i| format!("Sheet{i}"))
            .find(|name| self.find_sheet(name).is_none())
            .unwrap_or_else(|| "Sheet".into())
    }

    /// Appends a sheet. With `None` a free `SheetN` name is chosen.
    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<usize> {
        if self.sheets.len() >= MAX_SHEETS {
            return Err(OdfError::limit("too many sheets"));
        }
        let name = match name {
            Some(name) => {
                if !is_valid_sheet_name(name) {
                    return Err(bad("invalid sheet name"));
                }
                if self.find_sheet(name).is_some() {
                    return Err(bad("a sheet with this name already exists"));
                }
                name.to_owned()
            }
            None => self.unused_sheet_name(),
        };
        let before = self.sheets.clone();
        self.sheets.push(Sheet::new(name));
        self.after_structure_change(before);
        Ok(self.sheets.len() - 1)
    }

    /// Appends a sheet while loading, repairing the name if necessary.
    pub(crate) fn push_loaded_sheet(&mut self, name: &str) -> Result<usize> {
        if self.sheets.len() >= MAX_SHEETS {
            return Err(OdfError::limit("too many sheets"));
        }
        let mut clean: String = name
            .trim()
            .chars()
            .map(|c| if c.is_control() || matches!(c, '\'' | '[' | ']' | '*' | '?' | ':' | '/' | '\\') { '_' } else { c })
            .take(64)
            .collect();
        clean = clean.trim().to_owned();
        if clean.is_empty() || self.find_sheet(&clean).is_some() {
            clean = self.unused_sheet_name();
        }
        self.sheets.push(Sheet::new(clean));
        Ok(self.sheets.len() - 1)
    }

    /// Removes a sheet. The last sheet cannot be removed.
    pub fn remove_sheet(&mut self, index: usize) -> Result<()> {
        self.sheet(index)?;
        if self.sheets.len() == 1 {
            return Err(bad("a workbook needs at least one sheet"));
        }
        let before = self.sheets.clone();
        self.sheets.remove(index);
        self.after_structure_change(before);
        Ok(())
    }

    /// Renames a sheet and updates the formulas that mention it.
    pub fn rename_sheet(&mut self, index: usize, name: &str) -> Result<()> {
        let old = self.sheet(index)?.name.clone();
        if !is_valid_sheet_name(name) {
            return Err(bad("invalid sheet name"));
        }
        if self.find_sheet(name).is_some_and(|i| i != index) {
            return Err(bad("a sheet with this name already exists"));
        }
        let before = self.sheets.clone();
        let old_lower = old.to_lowercase();
        for sheet in &mut self.sheets {
            for cell in sheet.cells.values_mut() {
                if let Content::Formula(expr) = &mut cell.content {
                    expr.map_refs(&mut |a, _| {
                        if a.sheet.as_ref().is_some_and(|s| s.to_lowercase() == old_lower) {
                            a.sheet = Some(name.to_owned());
                        }
                        true
                    });
                }
            }
        }
        self.sheets[index].name = name.to_owned();
        self.after_structure_change(before);
        Ok(())
    }

    // ---- styles -------------------------------------------------------------

    /// Returns the id of `style`, adding it to the style table if needed.
    pub fn intern_style(&mut self, style: CellStyle) -> u32 {
        if let Some(id) = self.style_ids.get(&style) {
            return *id;
        }
        let id = self.styles.len() as u32;
        self.styles.push(style.clone());
        self.style_ids.insert(style, id);
        id
    }

    /// The style with the given id (the default style for unknown ids).
    pub fn style_by_id(&self, id: u32) -> &CellStyle {
        self.styles.get(id as usize).unwrap_or(&self.styles[0])
    }

    /// The style of a cell.
    pub fn style(&self, sheet: usize, addr: CellAddr) -> &CellStyle {
        let id = self.sheets.get(sheet).and_then(|s| s.cells.get(&addr)).map(|c| c.style).unwrap_or(0);
        self.style_by_id(id)
    }

    // ---- reading ------------------------------------------------------------

    /// The current value of a cell.
    pub fn value(&self, sheet: usize, addr: CellAddr) -> &Value {
        self.sheets.get(sheet).and_then(|s| s.cells.get(&addr)).map(|c| &c.value).unwrap_or(&EMPTY)
    }

    /// The text displayed in a cell (value formatted with the cell's format).
    pub fn display(&self, sheet: usize, addr: CellAddr) -> String {
        format_value(self.value(sheet, addr), &self.style(sheet, addr).format)
    }

    /// The text to edit for a cell: what, typed back in, reproduces it.
    pub fn input_text(&self, sheet: usize, addr: CellAddr) -> String {
        let Some(cell) = self.sheets.get(sheet).and_then(|s| s.cells.get(&addr)) else {
            return String::new();
        };
        match &cell.content {
            Content::Empty => String::new(),
            Content::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Content::Formula(expr) => format!("={}", expr.to_source(Syntax::Ui)),
            Content::Text(s) => match parse_input(s) {
                Input::Text(same) if same == *s => s.clone(),
                _ => format!("'{s}"),
            },
            Content::Number(n) => match &self.style_by_id(cell.style).format {
                fmt @ (NumberFormat::Date | NumberFormat::DateDmy | NumberFormat::DateTime) if format::serial_to_date(*n).is_some() => {
                    format::format_number(*n, fmt)
                }
                NumberFormat::Time if (0.0..1.0).contains(n) => format::format_number(*n, &NumberFormat::Time),
                NumberFormat::Percent { .. } => format!("{}%", n * 100.0),
                _ => format!("{n}"),
            },
        }
    }

    /// Whether the workbook changed since it was created, loaded or saved.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Marks the current state as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    // ---- environment ----------------------------------------------------------

    /// Pins `NOW()`/`TODAY()` to a fixed serial number (for reproducible tests).
    pub fn set_fixed_now(&mut self, serial: Option<f64>) {
        self.fixed_now = serial;
    }

    /// Sets the local-time offset from UTC used by `NOW()` and `TODAY()`.
    pub fn set_utc_offset_minutes(&mut self, minutes: i32) {
        self.utc_offset_minutes = minutes.clamp(-24 * 60, 24 * 60);
    }

    /// Seeds the pseudo-random generator behind `RAND()`.
    pub fn seed_random(&mut self, seed: u64) {
        self.rng.set(seed | 1);
    }

    // ---- editing --------------------------------------------------------------

    fn check(&self, sheet: usize, range: Range) -> Result<()> {
        self.sheet(sheet)?;
        if !range.is_valid() {
            return Err(bad("cell address outside of the sheet"));
        }
        Ok(())
    }

    fn content_from_input(&mut self, sheet: usize, addr: CellAddr, text: &str) -> Result<CellData> {
        if text.len() > MAX_CELL_TEXT {
            return Err(OdfError::limit("cell text is too long"));
        }
        let style_id = self.sheets[sheet].cells.get(&addr).map(|c| c.style).unwrap_or(0);
        let style = self.style_by_id(style_id).clone();
        if style.format == NumberFormat::Text && !text.is_empty() {
            return Ok((Content::Text(text.to_owned()), style_id));
        }
        Ok(match parse_input(text) {
            Input::Empty => (Content::Empty, style_id),
            Input::Text(s) => (Content::Text(s), style_id),
            Input::Bool(b) => (Content::Bool(b), style_id),
            Input::Formula(body) => (Content::Formula(formula::parse(&body, Syntax::Ui)), style_id),
            Input::Number(n, implied) => {
                let style_id = match implied {
                    Some(format) if style.format == NumberFormat::General => {
                        self.intern_style(CellStyle { format, ..style })
                    }
                    _ => style_id,
                };
                (Content::Number(n), style_id)
            }
        })
    }

    /// Sets a cell from typed text (see [`parse_input`]).
    pub fn set_input(&mut self, sheet: usize, addr: CellAddr, text: &str) -> Result<()> {
        self.check(sheet, Range::single(addr))?;
        let data = self.content_from_input(sheet, addr, text)?;
        self.commit_cells(sheet, vec![(addr, Some(data))]);
        Ok(())
    }

    /// Sets a rectangular block from text rows (paste), starting at `origin`.
    /// Cells that would fall off the sheet are ignored.
    pub fn set_inputs(&mut self, sheet: usize, origin: CellAddr, rows: &[Vec<String>]) -> Result<()> {
        self.check(sheet, Range::single(origin))?;
        let total: u64 = rows.iter().map(|r| r.len() as u64).sum();
        if total > MAX_BULK_CELLS {
            return Err(OdfError::limit("too many cells in one paste"));
        }
        let mut new = Vec::with_capacity(total as usize);
        for (dr, row) in rows.iter().enumerate() {
            for (dc, text) in row.iter().enumerate() {
                let addr = CellAddr::new(origin.row.saturating_add(dr as u32), origin.col.saturating_add(dc as u32));
                if addr.is_valid() {
                    let data = self.content_from_input(sheet, addr, text)?;
                    new.push((addr, Some(data)));
                }
            }
        }
        self.commit_cells(sheet, new);
        Ok(())
    }

    /// Sets cell content directly, keeping the cell's style.
    pub fn set_content(&mut self, sheet: usize, addr: CellAddr, content: Content) -> Result<()> {
        self.check(sheet, Range::single(addr))?;
        if matches!(&content, Content::Text(s) if s.len() > MAX_CELL_TEXT) {
            return Err(OdfError::limit("cell text is too long"));
        }
        let style = self.sheets[sheet].cells.get(&addr).map(|c| c.style).unwrap_or(0);
        self.commit_cells(sheet, vec![(addr, Some((content, style)))]);
        Ok(())
    }

    /// Clears the contents (not the styles) of a range.
    pub fn clear_range(&mut self, sheet: usize, range: Range) -> Result<()> {
        self.check(sheet, range)?;
        let new: Vec<_> = self.sheets[sheet]
            .cells_in_rows(range.start.row, range.end.row)
            .filter(|(addr, cell)| range.contains(*addr) && cell.content != Content::Empty)
            .map(|(addr, cell)| (addr, Some((Content::Empty, cell.style))))
            .collect();
        self.commit_cells(sheet, new);
        Ok(())
    }

    /// Replaces the text of every *text* cell in `range` with `f(text)`
    /// (numbers, formulas and empty cells are left alone), as one undo step.
    /// Returns the number of cells changed. Used for transliteration.
    pub fn map_text(&mut self, sheet: usize, range: Range, f: &dyn Fn(&str) -> String) -> Result<usize> {
        self.check(sheet, range)?;
        let mut new = Vec::new();
        for (addr, cell) in self.sheets[sheet].cells_in_rows(range.start.row, range.end.row) {
            if !range.contains(addr) {
                continue;
            }
            if let Content::Text(text) = &cell.content {
                let mapped = f(text);
                if mapped != *text {
                    if mapped.len() > MAX_CELL_TEXT {
                        return Err(OdfError::limit("cell text is too long"));
                    }
                    new.push((addr, Some((Content::Text(mapped), cell.style))));
                }
            }
        }
        let count = new.len();
        self.commit_cells(sheet, new);
        Ok(count)
    }

    /// Applies a style patch to every cell of a range.
    pub fn set_style(&mut self, sheet: usize, range: Range, patch: &StylePatch) -> Result<()> {
        self.check(sheet, range)?;
        if range.cell_count() > MAX_BULK_CELLS {
            return Err(OdfError::limit("range is too large to format"));
        }
        let mut new = Vec::with_capacity(range.cell_count() as usize);
        let mut cache: HashMap<u32, u32> = HashMap::new();
        for row in range.start.row..=range.end.row {
            for col in range.start.col..=range.end.col {
                let addr = CellAddr::new(row, col);
                let (content, old_id) = self.sheets[sheet].data(addr).unwrap_or((Content::Empty, 0));
                let new_id = match cache.get(&old_id) {
                    Some(id) => *id,
                    None => {
                        let style = patch.apply(self.style_by_id(old_id));
                        let id = self.intern_style(style);
                        cache.insert(old_id, id);
                        id
                    }
                };
                if new_id != old_id {
                    new.push((addr, Some((content, new_id))));
                }
            }
        }
        self.commit_cells(sheet, new);
        Ok(())
    }

    /// Sets (or with `None` resets) the width of a column in points.
    pub fn set_col_width(&mut self, sheet: usize, col: u32, width: Option<f32>) -> Result<()> {
        self.check(sheet, Range::single(CellAddr::new(0, col)))?;
        let width = width.filter(|w| w.is_finite()).map(|w| w.clamp(4.0, 2000.0));
        let before = match width {
            Some(w) => self.sheets[sheet].col_widths.insert(col, w),
            None => self.sheets[sheet].col_widths.remove(&col),
        };
        self.push_undo(UndoEntry::Width { sheet: sheet as u32, col, before, after: width });
        Ok(())
    }

    /// How the workbook is printed.
    pub fn page(&self) -> &PageSetup {
        &self.page
    }

    /// Changes the page setup (one undo step).
    pub fn set_page(&mut self, page: PageSetup) -> Result<()> {
        if !page.is_valid() {
            return Err(bad("page size, margins or scale are out of range"));
        }
        if page != self.page {
            let before = std::mem::replace(&mut self.page, page);
            self.push_undo(UndoEntry::Page { before, after: page });
        }
        Ok(())
    }

    /// Sets the print range and the repeated rows of a sheet (one undo
    /// step). The range must be valid and the rows in order.
    pub fn set_print_area(&mut self, sheet: usize, area: PrintArea) -> Result<()> {
        self.sheet(sheet)?;
        if let Some(r) = area.range {
            self.check(sheet, r)?;
        }
        if let Some((first, last)) = area.repeat_rows {
            if first > last || last >= MAX_ROWS || last - first >= 100 {
                return Err(bad("rows to repeat must be at most 100 rows in order"));
            }
        }
        let before = self.sheets[sheet].print;
        if before != area {
            self.sheets[sheet].print = area;
            self.push_undo(UndoEntry::Print { sheet: sheet as u32, before, after: area });
        }
        Ok(())
    }

    /// Print settings of a freshly loaded sheet (no undo step; invalid
    /// values are dropped).
    pub(crate) fn load_print_area(&mut self, sheet: usize, area: PrintArea) {
        if let Some(sh) = self.sheets.get_mut(sheet) {
            sh.print.range = area.range.filter(|r| r.is_valid() && r.end.row < MAX_ROWS && r.end.col < MAX_COLS);
            sh.print.repeat_rows = area.repeat_rows.filter(|(a, b)| a <= b && *b < MAX_ROWS && b - a < 100);
        }
    }

    /// Sets the page setup of a freshly loaded workbook (no undo step).
    pub(crate) fn set_loaded_page(&mut self, page: PageSetup) {
        if page.is_valid() {
            self.page = page;
        }
    }

    /// Copies `src` so that its top-left corner lands on `dst`. Relative
    /// references in formulas are adjusted; the destination is overwritten.
    pub fn copy_range(&mut self, src_sheet: usize, src: Range, dst_sheet: usize, dst: CellAddr) -> Result<()> {
        self.check(src_sheet, src)?;
        self.check(dst_sheet, Range::single(dst))?;
        if src.cell_count() > MAX_BULK_CELLS {
            return Err(OdfError::limit("range is too large to copy"));
        }
        let d_row = i64::from(dst.row) - i64::from(src.start.row);
        let d_col = i64::from(dst.col) - i64::from(src.start.col);
        let mut new = Vec::new();
        for row in src.start.row..=src.end.row {
            for col in src.start.col..=src.end.col {
                let target = CellAddr::new(
                    (i64::from(row) + d_row).clamp(0, i64::from(u32::MAX)) as u32,
                    (i64::from(col) + d_col).clamp(0, i64::from(u32::MAX)) as u32,
                );
                if !target.is_valid() {
                    continue;
                }
                let data = self.sheets[src_sheet].data(CellAddr::new(row, col)).map(|(content, style)| {
                    let content = match content {
                        Content::Formula(expr) => Content::Formula(expr.shifted(d_row, d_col)),
                        other => other,
                    };
                    (content, style)
                });
                new.push((target, data));
            }
        }
        self.commit_cells(dst_sheet, new);
        Ok(())
    }

    /// Fills `dst` by repeating `src` (which must share its top-left corner
    /// with `dst`), adjusting relative references: "fill down" / "fill right".
    pub fn fill(&mut self, sheet: usize, src: Range, dst: Range) -> Result<()> {
        self.check(sheet, src)?;
        self.check(sheet, dst)?;
        if dst.cell_count() > MAX_BULK_CELLS {
            return Err(OdfError::limit("range is too large to fill"));
        }
        if src.start != dst.start || !dst.contains(src.end) {
            return Err(bad("the fill source must be the top-left part of the target"));
        }
        let mut new = Vec::new();
        for row in dst.start.row..=dst.end.row {
            for col in dst.start.col..=dst.end.col {
                let target = CellAddr::new(row, col);
                if src.contains(target) {
                    continue;
                }
                let from = CellAddr::new(
                    src.start.row + (row - dst.start.row) % src.rows(),
                    src.start.col + (col - dst.start.col) % src.cols(),
                );
                let d_row = i64::from(row) - i64::from(from.row);
                let d_col = i64::from(col) - i64::from(from.col);
                let data = self.sheets[sheet].data(from).map(|(content, style)| {
                    let content = match content {
                        Content::Formula(expr) => Content::Formula(expr.shifted(d_row, d_col)),
                        other => other,
                    };
                    (content, style)
                });
                new.push((target, data));
            }
        }
        self.commit_cells(sheet, new);
        Ok(())
    }

    /// Sorts the rows of `range` by the values in column `key_col`. Empty
    /// cells sort last in both directions; the sort is stable.
    pub fn sort_range(&mut self, sheet: usize, range: Range, key_col: u32, ascending: bool) -> Result<()> {
        self.check(sheet, range)?;
        if range.cell_count() > MAX_BULK_CELLS {
            return Err(OdfError::limit("range is too large to sort"));
        }
        if key_col < range.start.col || key_col > range.end.col {
            return Err(bad("the sort column lies outside of the range"));
        }
        let mut rows: Vec<(u32, Value)> = (range.start.row..=range.end.row)
            .map(|row| (row, self.value(sheet, CellAddr::new(row, key_col)).clone()))
            .collect();
        rows.sort_by(|(_, a), (_, b)| match (a, b) {
            (Value::Empty, Value::Empty) => std::cmp::Ordering::Equal,
            (Value::Empty, _) => std::cmp::Ordering::Greater,
            (_, Value::Empty) => std::cmp::Ordering::Less,
            _ => {
                let ord = compare_values(a, b);
                if ascending { ord } else { ord.reverse() }
            }
        });
        let mut new = Vec::new();
        for (offset, (source_row, _)) in rows.iter().enumerate() {
            let target_row = range.start.row + offset as u32;
            if target_row == *source_row {
                continue;
            }
            let d_row = i64::from(target_row) - i64::from(*source_row);
            for col in range.start.col..=range.end.col {
                let data = self.sheets[sheet].data(CellAddr::new(*source_row, col)).map(|(content, style)| {
                    let content = match content {
                        Content::Formula(expr) => Content::Formula(expr.shifted(d_row, 0)),
                        other => other,
                    };
                    (content, style)
                });
                new.push((CellAddr::new(target_row, col), data));
            }
        }
        self.commit_cells(sheet, new);
        Ok(())
    }

    /// Inserts `count` empty rows or columns before index `at`.
    pub fn insert(&mut self, sheet: usize, axis: Axis, at: u32, count: u32) -> Result<()> {
        self.restructure(sheet, axis, at, count, true)
    }

    /// Deletes `count` rows or columns starting at index `at`.
    pub fn delete(&mut self, sheet: usize, axis: Axis, at: u32, count: u32) -> Result<()> {
        self.restructure(sheet, axis, at, count, false)
    }

    fn restructure(&mut self, sheet: usize, axis: Axis, at: u32, count: u32, insert: bool) -> Result<()> {
        self.sheet(sheet)?;
        let max = if axis == Axis::Row { MAX_ROWS } else { MAX_COLS };
        if count == 0 || at >= max || count > max - at {
            return Err(bad("invalid row or column span"));
        }
        // Where does index `v` end up? None = deleted or pushed off the sheet.
        let shift = move |v: u32| -> Option<u32> {
            if insert {
                if v < at { Some(v) } else { v.checked_add(count).filter(|n| *n < max) }
            } else if v < at {
                Some(v)
            } else if v >= at + count {
                Some(v - count)
            } else {
                None
            }
        };
        if insert {
            let would_lose = self.sheets[sheet].cells.iter().any(|(addr, cell)| {
                let v = if axis == Axis::Row { addr.row } else { addr.col };
                cell.content != Content::Empty && shift(v).is_none()
            });
            if would_lose {
                return Err(bad("cannot insert: cells at the end of the sheet would be lost"));
            }
        }
        let before = self.sheets.clone();

        // 1. Move the cells of the edited sheet.
        let old_cells = std::mem::take(&mut self.sheets[sheet].cells);
        let mut max_col = 0;
        for (addr, cell) in old_cells {
            let moved = match axis {
                Axis::Row => shift(addr.row).map(|row| CellAddr::new(row, addr.col)),
                Axis::Col => shift(addr.col).map(|col| CellAddr::new(addr.row, col)),
            };
            if let Some(moved) = moved {
                max_col = max_col.max(moved.col);
                self.sheets[sheet].cells.insert(moved, cell);
            }
        }
        self.sheets[sheet].max_col = max_col;
        if axis == Axis::Col {
            let widths = std::mem::take(&mut self.sheets[sheet].col_widths);
            self.sheets[sheet].col_widths =
                widths.into_iter().filter_map(|(col, w)| shift(col).map(|c| (c, w))).collect();
        }
        // The print range and repeated rows follow the rows and columns.
        let print = &mut self.sheets[sheet].print;
        if let Some(r) = print.range {
            print.range = match axis {
                Axis::Row => shift_span(r.start.row, r.end.row, at, count, insert, max)
                    .map(|(a, b)| Range::new(CellAddr::new(a, r.start.col), CellAddr::new(b, r.end.col))),
                Axis::Col => shift_span(r.start.col, r.end.col, at, count, insert, max)
                    .map(|(a, b)| Range::new(CellAddr::new(r.start.row, a), CellAddr::new(r.end.row, b))),
            };
        }
        if axis == Axis::Row {
            if let Some((a, b)) = print.repeat_rows {
                print.repeat_rows = shift_span(a, b, at, count, insert, max);
            }
        }

        // 2. Adjust references to the edited sheet in every formula.
        let target_name = self.sheets[sheet].name.to_lowercase();
        let coord = move |cell: &CellRef| if axis == Axis::Row { cell.addr.row } else { cell.addr.col };
        let set = move |cell: &mut CellRef, v: u32| {
            if axis == Axis::Row {
                cell.addr.row = v;
            } else {
                cell.addr.col = v;
            }
        };
        for (index, sh) in self.sheets.iter_mut().enumerate() {
            for cell in sh.cells.values_mut() {
                let Content::Formula(expr) = &mut cell.content else { continue };
                expr.map_refs(&mut |a, b| {
                    let targets = match &a.sheet {
                        None => index == sheet,
                        Some(name) => name.to_lowercase() == target_name,
                    };
                    if !targets {
                        return true;
                    }
                    match b {
                        None => match shift(coord(a)) {
                            Some(v) => {
                                set(a, v);
                                true
                            }
                            None => false,
                        },
                        Some(b) => {
                            let (lo, hi) = (coord(a).min(coord(b)), coord(a).max(coord(b)));
                            let (new_lo, new_hi) = if insert {
                                (shift(lo), shift(hi))
                            } else {
                                // A deleted end snaps to the nearest surviving cell.
                                let new_lo = shift(lo).or(Some(at));
                                let new_hi = shift(hi).or_else(|| at.checked_sub(1));
                                let fully_deleted = lo >= at && hi < at + count;
                                if fully_deleted { (None, None) } else { (new_lo, new_hi) }
                            };
                            match (new_lo, new_hi) {
                                (Some(new_lo), Some(new_hi)) if new_lo <= new_hi => {
                                    let a_is_lo = coord(a) <= coord(b);
                                    set(a, if a_is_lo { new_lo } else { new_hi });
                                    set(b, if a_is_lo { new_hi } else { new_lo });
                                    true
                                }
                                _ => false,
                            }
                        }
                    }
                });
            }
        }
        self.after_structure_change(before);
        Ok(())
    }

    // ---- undo / redo ------------------------------------------------------------

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.modified = true;
    }

    /// Whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts the most recent change. Returns `false` if there was none.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else { return false };
        self.apply_entry(&entry, true);
        self.redo.push(entry);
        self.modified = true;
        true
    }

    /// Re-applies the most recently undone change.
    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else { return false };
        self.apply_entry(&entry, false);
        self.undo.push(entry);
        self.modified = true;
        true
    }

    fn apply_entry(&mut self, entry: &UndoEntry, backwards: bool) {
        match entry {
            UndoEntry::Cells { sheet, changes } => {
                let new: Vec<_> = changes
                    .iter()
                    .map(|(addr, before, after)| (*addr, if backwards { before.clone() } else { after.clone() }))
                    .collect();
                self.apply_cells(*sheet as usize, new);
            }
            UndoEntry::Snapshot { before, after } => {
                self.sheets = if backwards { before.clone() } else { after.clone() };
                self.rebuild_dependencies();
                self.recalculate_all();
            }
            UndoEntry::Width { sheet, col, before, after } => {
                if let Some(sh) = self.sheets.get_mut(*sheet as usize) {
                    match if backwards { before } else { after } {
                        Some(w) => sh.col_widths.insert(*col, *w),
                        None => sh.col_widths.remove(col),
                    };
                }
            }
            UndoEntry::Page { before, after } => self.page = if backwards { *before } else { *after },
            UndoEntry::Print { sheet, before, after } => {
                if let Some(sh) = self.sheets.get_mut(*sheet as usize) {
                    sh.print = if backwards { *before } else { *after };
                }
            }
        }
    }

    // ---- internals: cell updates ---------------------------------------------

    fn commit_cells(&mut self, sheet: usize, new: Vec<(CellAddr, Option<CellData>)>) {
        if new.is_empty() {
            return;
        }
        let changes = self.apply_cells(sheet, new);
        if !changes.is_empty() {
            self.push_undo(UndoEntry::Cells { sheet: sheet as u32, changes });
        }
    }

    /// Replaces cells, maintains the dependency graph and recalculates.
    /// Returns `(address, before, after)` for every cell that changed.
    fn apply_cells(
        &mut self,
        sheet: usize,
        new: Vec<(CellAddr, Option<CellData>)>,
    ) -> Vec<(CellAddr, Option<CellData>, Option<CellData>)> {
        let mut changes = Vec::with_capacity(new.len());
        let mut seeds = Vec::with_capacity(new.len());
        for (addr, after) in new {
            let after = after.filter(|data| *data != (Content::Empty, 0));
            let before = self.sheets[sheet].data(addr);
            if before == after {
                continue;
            }
            let key = CellKey::new(sheet as u32, addr);
            if let Some((Content::Formula(expr), _)) = &before {
                let p = precedents(expr, key.sheet, &|name| self.find_sheet(name));
                self.deps.unregister(key, &p);
            }
            if let Some((Content::Formula(expr), _)) = &after {
                let p = precedents(expr, key.sheet, &|name| self.find_sheet(name));
                self.deps.register(key, &p, expr.calls_any(VOLATILE_FUNCTIONS));
            }
            self.sheets[sheet].put(addr, after.clone());
            seeds.push(key);
            changes.push((addr, before, after));
        }
        self.recalculate(&seeds);
        changes
    }

    fn after_structure_change(&mut self, before: Vec<Sheet>) {
        self.rebuild_dependencies();
        self.recalculate_all();
        let after = self.sheets.clone();
        self.push_undo(UndoEntry::Snapshot { before, after });
    }

    /// Rebuilds the dependency graph from scratch.
    pub(crate) fn rebuild_dependencies(&mut self) {
        let mut deps = DepGraph::default();
        for (index, sheet) in self.sheets.iter().enumerate() {
            for (addr, cell) in &sheet.cells {
                if let Content::Formula(expr) = &cell.content {
                    let p = precedents(expr, index as u32, &|name| self.find_sheet(name));
                    deps.register(CellKey::new(index as u32, *addr), &p, expr.calls_any(VOLATILE_FUNCTIONS));
                }
            }
        }
        self.deps = deps;
    }

    /// Raw insertion used by the file loaders (no undo, no recalculation).
    pub(crate) fn load_cell(&mut self, sheet: usize, addr: CellAddr, content: Content, value: Value, style: u32) {
        if let Some(sh) = self.sheets.get_mut(sheet) {
            if addr.is_valid() && (content != Content::Empty || style != 0) {
                sh.max_col = sh.max_col.max(addr.col);
                sh.cells.insert(addr, Cell { content, value, style });
            }
        }
    }

    /// Bulk load of typed text into a fresh sheet (CSV import): no undo
    /// entry and no per-cell recalculation.
    pub(crate) fn set_inputs_unchecked(&mut self, sheet: usize, rows: &[Vec<String>]) -> Result<()> {
        self.sheet(sheet)?;
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                let addr = CellAddr::new(r as u32, c as u32);
                if !addr.is_valid() || text.is_empty() {
                    continue;
                }
                let (content, style) = self.content_from_input(sheet, addr, text)?;
                let value = value_of(&content);
                self.load_cell(sheet, addr, content, value, style);
            }
        }
        Ok(())
    }

    /// Raw column width used by the file loaders.
    pub(crate) fn load_col_width(&mut self, sheet: usize, col: u32, width: f32) {
        if let Some(sh) = self.sheets.get_mut(sheet) {
            if col < MAX_COLS && width.is_finite() {
                sh.col_widths.insert(col, width.clamp(4.0, 2000.0));
            }
        }
    }

    /// Finishes loading: builds dependencies, calculates, clears history.
    pub(crate) fn finish_loading(&mut self) {
        if self.sheets.is_empty() {
            self.sheets.push(Sheet::new("Sheet1".into()));
        }
        self.rebuild_dependencies();
        self.recalculate_all();
        self.undo.clear();
        self.redo.clear();
        self.modified = false;
    }

    // ---- internals: recalculation ---------------------------------------------

    fn is_formula(&self, key: CellKey) -> bool {
        self.sheets
            .get(key.sheet as usize)
            .and_then(|s| s.cells.get(&key.addr))
            .is_some_and(|c| matches!(c.content, Content::Formula(_)))
    }

    /// Recalculates every formula in the workbook.
    pub fn recalculate_all(&mut self) {
        let mut dirty = BTreeSet::new();
        for (index, sheet) in self.sheets.iter().enumerate() {
            for (addr, cell) in &sheet.cells {
                if matches!(cell.content, Content::Formula(_)) {
                    dirty.insert((index as u32, addr.col, addr.row));
                }
            }
        }
        self.evaluate_dirty(dirty);
    }

    /// Recalculates the formulas affected by a change to `seeds`, plus all
    /// volatile formulas and their dependents.
    fn recalculate(&mut self, seeds: &[CellKey]) {
        let mut dirty: BTreeSet<(u32, u32, u32)> = BTreeSet::new();
        let mut visited: HashSet<CellKey> = HashSet::new();
        let mut queue: Vec<CellKey> = seeds.to_vec();
        queue.extend(self.deps.volatile.iter().copied());
        while let Some(key) = queue.pop() {
            if !visited.insert(key) {
                continue;
            }
            if self.is_formula(key) {
                dirty.insert((key.sheet, key.addr.col, key.addr.row));
            }
            self.deps.for_each_dependent(key, &mut |dependent| {
                if !visited.contains(&dependent) {
                    queue.push(dependent);
                }
            });
        }
        self.evaluate_dirty(dirty);
    }

    /// Evaluates the dirty formulas in dependency order.
    ///
    /// This is an iterative depth-first traversal (no recursion, so a chain of
    /// a million dependent cells cannot overflow the stack). `dirty` is keyed
    /// `(sheet, column, row)` so the dirty precedents inside a range can be
    /// found with one ordered lookup per column.
    fn evaluate_dirty(&mut self, dirty: BTreeSet<(u32, u32, u32)>) {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            InProgress,
            Done,
        }
        enum Frame {
            Enter(CellKey),
            Exit(CellKey),
        }
        let mut marks: HashMap<CellKey, Mark> = HashMap::with_capacity(dirty.len());
        let mut cyclic: HashSet<CellKey> = HashSet::new();
        let mut stack: Vec<Frame> = Vec::new();

        for &(sheet, col, row) in &dirty {
            let root = CellKey::new(sheet, CellAddr::new(row, col));
            if marks.contains_key(&root) {
                continue;
            }
            stack.push(Frame::Enter(root));
            while let Some(frame) = stack.pop() {
                match frame {
                    Frame::Enter(key) => match marks.get(&key) {
                        Some(Mark::Done) => {}
                        Some(Mark::InProgress) => {
                            // `key` is an ancestor on the current path: every
                            // cell from it to the top of the path is circular.
                            for f in stack.iter().rev() {
                                if let Frame::Exit(on_path) = f {
                                    cyclic.insert(*on_path);
                                    if *on_path == key {
                                        break;
                                    }
                                }
                            }
                        }
                        None => {
                            marks.insert(key, Mark::InProgress);
                            stack.push(Frame::Exit(key));
                            let Some(Content::Formula(expr)) = self
                                .sheets
                                .get(key.sheet as usize)
                                .and_then(|s| s.cells.get(&key.addr))
                                .map(|c| &c.content)
                            else {
                                continue;
                            };
                            let p = precedents(expr, key.sheet, &|name| self.find_sheet(name));
                            for cell in p.cells {
                                if dirty.contains(&(cell.sheet, cell.addr.col, cell.addr.row))
                                    && marks.get(&cell) != Some(&Mark::Done)
                                {
                                    stack.push(Frame::Enter(cell));
                                }
                            }
                            for (sheet, range) in p.ranges {
                                for col in range.start.col..=range.end.col {
                                    for &(_, _, row) in
                                        dirty.range((sheet, col, range.start.row)..=(sheet, col, range.end.row))
                                    {
                                        let cell = CellKey::new(sheet, CellAddr::new(row, col));
                                        if marks.get(&cell) != Some(&Mark::Done) {
                                            stack.push(Frame::Enter(cell));
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Frame::Exit(key) => {
                        let value = if cyclic.contains(&key) {
                            Value::Error(ErrorKind::Circular)
                        } else {
                            self.evaluate_cell(key)
                        };
                        if let Some(cell) =
                            self.sheets.get_mut(key.sheet as usize).and_then(|s| s.cells.get_mut(&key.addr))
                        {
                            cell.value = value;
                        }
                        marks.insert(key, Mark::Done);
                    }
                }
            }
        }
    }

    fn evaluate_cell(&self, key: CellKey) -> Value {
        match self.sheets.get(key.sheet as usize).and_then(|s| s.cells.get(&key.addr)).map(|c| &c.content) {
            Some(Content::Formula(expr)) => {
                evaluate(expr, &Ctx { host: self, sheet: key.sheet as usize, cell: key.addr })
            }
            Some(other) => value_of(other),
            None => Value::Empty,
        }
    }
}

impl Host for Workbook {
    fn sheet_index(&self, name: &str) -> Option<usize> {
        self.find_sheet(name)
    }

    fn value(&self, sheet: usize, addr: CellAddr) -> &Value {
        Workbook::value(self, sheet, addr)
    }

    fn for_each_in_range(&self, sheet: usize, range: Range, f: &mut dyn FnMut(CellAddr, &Value)) {
        let Some(sh) = self.sheets.get(sheet) else { return };
        for (addr, cell) in sh.cells_in_rows(range.start.row, range.end.row) {
            if addr.col >= range.start.col && addr.col <= range.end.col && cell.value != Value::Empty {
                f(addr, &cell.value);
            }
        }
    }

    fn extent(&self, sheet: usize) -> Option<CellAddr> {
        let sh = self.sheets.get(sheet)?;
        let (last, _) = sh.cells.last_key_value()?;
        Some(CellAddr::new(last.row, sh.max_col))
    }

    fn now(&self) -> f64 {
        if let Some(fixed) = self.fixed_now {
            return fixed;
        }
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        format::unix_to_serial(unix, self.utc_offset_minutes)
    }

    fn random(&self) -> f64 {
        // xorshift64*: fast and good enough for spreadsheet randomness.
        // Not suitable for (and not used for) anything security related.
        let mut x = self.rng.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng.set(x);
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}
