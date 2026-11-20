//! The text document model.
//!
//! A [`Document`](crate::Document) is a flat list of [`Block`]s: paragraphs, tables and page
//! breaks. A [`Paragraph`] holds a list of [`Run`]s, each a piece of text with
//! uniform character attributes. The model is deliberately flat (lists are
//! paragraphs with a list kind and level) which keeps editing simple and maps
//! directly onto the Flutter editor, one editable line per paragraph.

use office_draw::{Image, Shape};
use office_odf::Rgb;
use serde::{Serialize, Serializer};

fn ser_rgb<S: Serializer>(value: &Option<Rgb>, s: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(rgb) => s.serialize_some(&rgb.to_hex()),
        None => s.serialize_none(),
    }
}

/// Character attributes of a run.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct TextAttrs {
    /// Bold.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Italic.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Underlined.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Struck through.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    /// Monospaced ("code").
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub code: bool,
    /// Raised (superscript).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub superscript: bool,
    /// Lowered (subscript).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub subscript: bool,
    /// Text colour.
    #[serde(serialize_with = "ser_rgb", skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
    /// Highlight (background) colour.
    #[serde(serialize_with = "ser_rgb", skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Rgb>,
    /// Font size in points (`None` = paragraph default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Hyperlink target. Only `http`, `https` and `mailto` links are kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The run is a field whose text is computed when the page is laid out
    /// (the stored text is the last known value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<Field>,
}

/// A computed text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// Number of the current page.
    PageNumber,
    /// Total number of pages.
    PageCount,
}

/// A partial set of attributes: only the `Some` fields are changed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttrPatch {
    /// New bold flag.
    pub bold: Option<bool>,
    /// New italic flag.
    pub italic: Option<bool>,
    /// New underline flag.
    pub underline: Option<bool>,
    /// New strike-through flag.
    pub strike: Option<bool>,
    /// New code flag.
    pub code: Option<bool>,
    /// New superscript flag (clears subscript when set).
    pub superscript: Option<bool>,
    /// New subscript flag (clears superscript when set).
    pub subscript: Option<bool>,
    /// New colour (`Some(None)` resets).
    pub color: Option<Option<Rgb>>,
    /// New highlight (`Some(None)` removes).
    pub highlight: Option<Option<Rgb>>,
    /// New size (`Some(None)` resets).
    pub size: Option<Option<f32>>,
    /// New link (`Some(None)` removes).
    pub link: Option<Option<String>>,
}

/// Returns the link if it uses an allowed scheme.
///
/// Documents may contain `javascript:`, `file:` or custom-scheme links that
/// would do something nasty when clicked; those are dropped on import and
/// refused on edit.
pub fn sanitize_link(link: &str) -> Option<String> {
    let link = link.trim();
    let lower = link.to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"].iter().any(|p| lower.starts_with(p));
    (allowed && link.len() <= 2048 && !link.chars().any(char::is_control)).then(|| link.to_owned())
}

impl AttrPatch {
    /// Applies the patch.
    pub fn apply(&self, a: &TextAttrs) -> TextAttrs {
        let mut out = a.clone();
        if let Some(v) = self.bold {
            out.bold = v;
        }
        if let Some(v) = self.italic {
            out.italic = v;
        }
        if let Some(v) = self.underline {
            out.underline = v;
        }
        if let Some(v) = self.strike {
            out.strike = v;
        }
        if let Some(v) = self.code {
            out.code = v;
        }
        if let Some(v) = self.superscript {
            out.superscript = v;
            if v {
                out.subscript = false;
            }
        }
        if let Some(v) = self.subscript {
            out.subscript = v;
            if v {
                out.superscript = false;
            }
        }
        if let Some(v) = self.color {
            out.color = v;
        }
        if let Some(v) = self.highlight {
            out.highlight = v;
        }
        if let Some(v) = self.size {
            out.size = v.filter(|s| s.is_finite()).map(|s| s.clamp(4.0, 400.0));
        }
        if let Some(v) = &self.link {
            out.link = v.as_deref().and_then(sanitize_link);
        }
        out
    }
}

/// A piece of text with uniform attributes.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Run {
    /// The text. `'\n'` is a line break inside the paragraph.
    pub text: String,
    /// Attributes.
    #[serde(flatten)]
    pub attrs: TextAttrs,
}

/// What kind of paragraph this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParaKind {
    /// Body text.
    #[default]
    Normal,
    /// Heading with outline level 1..=6.
    Heading {
        /// Outline level.
        level: u8,
    },
    /// Item of a bulleted list.
    Bullet {
        /// Nesting level 0..=9.
        level: u8,
    },
    /// Item of a numbered list.
    Numbered {
        /// Nesting level 0..=9.
        level: u8,
    },
    /// Block quotation.
    Quote,
    /// Preformatted (code) block.
    Code,
    /// Document title.
    Title,
}

impl ParaKind {
    /// Clamps levels into their valid ranges.
    pub fn normalized(self) -> ParaKind {
        match self {
            ParaKind::Heading { level } => ParaKind::Heading { level: level.clamp(1, 6) },
            ParaKind::Bullet { level } => ParaKind::Bullet { level: level.min(9) },
            ParaKind::Numbered { level } => ParaKind::Numbered { level: level.min(9) },
            other => other,
        }
    }
}

/// Horizontal paragraph alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// Start (left in left-to-right text).
    #[default]
    Left,
    /// Centred.
    Center,
    /// End.
    Right,
    /// Justified.
    Justify,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Largest paragraph indent or spacing in points (10 inches).
pub const MAX_PARA_SPACING: f32 = 720.0;

/// A paragraph.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Paragraph {
    /// Paragraph kind.
    #[serde(flatten)]
    pub kind: ParaKind,
    /// Alignment.
    pub align: Align,
    /// Left indent in points (not used by list items, whose indent comes
    /// from their level).
    #[serde(skip_serializing_if = "is_zero")]
    pub indent: f32,
    /// Extra space above the paragraph in points.
    #[serde(skip_serializing_if = "is_zero")]
    pub space_before: f32,
    /// Text runs. Never contains empty runs except as the only run.
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// A plain paragraph with the given text.
    pub fn plain(text: impl Into<String>) -> Paragraph {
        Paragraph { runs: vec![Run { text: text.into(), attrs: TextAttrs::default() }], ..Default::default() }
    }

    /// A paragraph of the given kind with plain text.
    pub fn with_kind(kind: ParaKind, text: impl Into<String>) -> Paragraph {
        Paragraph { kind: kind.normalized(), ..Paragraph::plain(text) }
    }

    /// The paragraph's text without attributes.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Merges adjacent runs with equal attributes and drops empty runs
    /// (keeping one empty run so the paragraph still has attributes for new
    /// text). Fields are never merged: each is its own run. Indent and
    /// spacing are clamped to their valid range.
    pub fn normalize(&mut self) {
        let mut empty_attrs = self.runs.first().map(|r| r.attrs.clone()).unwrap_or_default();
        empty_attrs.field = None;
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            if run.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.attrs == run.attrs && run.attrs.field.is_none() => last.text.push_str(&run.text),
                _ => out.push(run),
            }
        }
        if out.is_empty() {
            out.push(Run { text: String::new(), attrs: empty_attrs });
        }
        self.runs = out;
        let clamp = |v: f32| if v.is_finite() { v.clamp(0.0, MAX_PARA_SPACING) } else { 0.0 };
        self.indent = clamp(self.indent);
        self.space_before = clamp(self.space_before);
    }
}

fn is_true(v: &bool) -> bool {
    *v
}

/// A table: rows of cells, each cell a list of paragraphs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    /// Rows; every row has the same number of cells.
    pub rows: Vec<Vec<Vec<Paragraph>>>,
    /// Fixed column widths in points; empty = columns share the text width.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub col_widths: Vec<f32>,
    /// Exact row heights in points (content that does not fit is cut off);
    /// empty = rows grow with their content.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub row_heights: Vec<f32>,
    /// Whether cells have borders (label sheets have none).
    #[serde(skip_serializing_if = "is_true")]
    pub borders: bool,
}

impl Default for Table {
    fn default() -> Self {
        Table { rows: Vec::new(), col_widths: Vec::new(), row_heights: Vec::new(), borders: true }
    }
}

impl Table {
    /// An empty table with the given size (at least 1×1).
    pub fn new(rows: usize, cols: usize) -> Table {
        let (rows, cols) = (rows.clamp(1, 1000), cols.clamp(1, 64));
        Table { rows: vec![vec![vec![Paragraph::plain("")]; cols]; rows], ..Default::default() }
    }

    /// A table from rows of cells (normalised).
    pub fn from_rows(rows: Vec<Vec<Vec<Paragraph>>>) -> Table {
        let mut t = Table { rows, ..Default::default() };
        t.normalize();
        t
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.rows.first().map(Vec::len).unwrap_or(0)
    }

    /// Pads rows to equal length and makes every cell hold a paragraph.
    pub fn normalize(&mut self) {
        let cols = self.rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        for row in &mut self.rows {
            row.resize_with(cols, || vec![Paragraph::plain("")]);
            for cell in row.iter_mut() {
                if cell.is_empty() {
                    cell.push(Paragraph::plain(""));
                }
                for p in cell.iter_mut() {
                    p.normalize();
                }
            }
        }
        // Sizes are all-or-nothing: one per column / row, each sane.
        let sane = |v: &f32| v.is_finite() && (0.0..=MAX_PAGE_SIDE).contains(v);
        if self.col_widths.len() != cols || !self.col_widths.iter().all(sane) {
            self.col_widths.clear();
        }
        if self.row_heights.len() != self.rows.len() || !self.row_heights.iter().all(sane) {
            self.row_heights.clear();
        }
    }

    /// Inserts `n` rows at `at`, keeping fixed row heights consistent.
    pub(crate) fn insert_rows(&mut self, at: usize, n: usize) {
        let cols = self.cols();
        for _ in 0..n {
            self.rows.insert(at, vec![vec![Paragraph::plain("")]; cols]);
            if !self.row_heights.is_empty() {
                let h = self.row_heights.get(at).or(self.row_heights.last()).copied().unwrap_or(20.0);
                self.row_heights.insert(at, h);
            }
        }
    }

    /// Inserts `n` columns at `at`, keeping fixed widths consistent.
    pub(crate) fn insert_cols(&mut self, at: usize, n: usize) {
        for _ in 0..n {
            for row in &mut self.rows {
                row.insert(at, vec![Paragraph::plain("")]);
            }
            if !self.col_widths.is_empty() {
                let w = self.col_widths.get(at).or(self.col_widths.last()).copied().unwrap_or(72.0);
                self.col_widths.insert(at, w);
            }
        }
    }

    /// Deletes rows `at..at + n`.
    pub(crate) fn delete_rows(&mut self, at: usize, n: usize) {
        self.rows.drain(at..at + n);
        if !self.row_heights.is_empty() {
            self.row_heights.drain(at..at + n);
        }
    }

    /// Deletes columns `at..at + n`.
    pub(crate) fn delete_cols(&mut self, at: usize, n: usize) {
        for row in &mut self.rows {
            row.drain(at..at + n);
        }
        if !self.col_widths.is_empty() {
            self.col_widths.drain(at..at + n);
        }
    }
}

/// Smallest picture, diagram or drawing side in points.
pub const MIN_FRAME_SIDE: f32 = 4.0;

fn frame_side(v: f32, default: f32) -> f32 {
    if v.is_finite() { v.clamp(MIN_FRAME_SIDE, MAX_PAGE_SIDE) } else { default }
}

/// A picture on its own line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picture {
    /// The picture (serialised as its id; fetch the bytes with
    /// `image.data`).
    pub image: Image,
    /// Displayed width in points.
    pub width: f32,
    /// Displayed height in points.
    pub height: f32,
    /// Horizontal position.
    pub align: Align,
    /// Alternative text.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Picture {
    /// A picture at its natural size, at most `max_width` wide.
    pub fn new(image: Image, max_width: f32) -> Picture {
        let (width, height) = image.fit_size(max_width, MAX_PAGE_SIDE);
        Picture { image, width, height, align: Align::Center, description: String::new() }
    }

    /// Clamps the size into range.
    pub fn normalize(&mut self) {
        self.width = frame_side(self.width, 144.0);
        self.height = frame_side(self.height, 144.0);
        self.description.truncate(self.description.char_indices().nth(2000).map_or(self.description.len(), |(i, _)| i));
    }
}

/// A Mermaid diagram on its own line; its height follows from the drawing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagram {
    /// Mermaid source.
    pub source: String,
    /// Displayed width in points.
    pub width: f32,
    /// Horizontal position.
    pub align: Align,
}

impl Diagram {
    /// The rendered figure.
    pub fn figure(&self) -> office_draw::Figure {
        office_draw::mermaid::render(&self.source).figure
    }

    /// Displayed height for the current width.
    pub fn height(&self) -> f32 {
        let f = self.figure();
        self.width * f.height / f.width.max(1.0)
    }

    /// Clamps values into range.
    pub fn normalize(&mut self) {
        self.width = frame_side(self.width, 300.0);
        if self.source.len() > office_draw::model::MAX_DIAGRAM_SOURCE {
            let mut cut = office_draw::model::MAX_DIAGRAM_SOURCE;
            while !self.source.is_char_boundary(cut) {
                cut -= 1;
            }
            self.source.truncate(cut);
        }
    }
}

/// A drawing canvas: shapes in a box of their own.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Canvas {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
    /// Horizontal position.
    pub align: Align,
    /// Shapes, bottom to top, positioned relative to the canvas.
    pub shapes: Vec<Shape>,
}

impl Canvas {
    /// An empty canvas.
    pub fn new(width: f32, height: f32) -> Canvas {
        Canvas { width, height, align: Align::Center, shapes: Vec::new() }
    }

    /// Clamps values into range.
    pub fn normalize(&mut self) {
        self.width = frame_side(self.width, 400.0);
        self.height = frame_side(self.height, 200.0);
        self.shapes.truncate(office_draw::model::MAX_SHAPES);
        for s in &mut self.shapes {
            s.normalize();
        }
    }
}

/// A top-level block.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// A paragraph.
    Paragraph(Paragraph),
    /// A table.
    Table(Table),
    /// A forced page break.
    PageBreak,
    /// A picture.
    Image(Picture),
    /// A Mermaid diagram.
    Diagram(Diagram),
    /// A drawing canvas.
    Drawing(Canvas),
}

/// Page setup.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageSetup {
    /// Page width in points.
    pub width: f32,
    /// Page height in points.
    pub height: f32,
    /// Margin on all sides in points.
    pub margin: f32,
}

impl Default for PageSetup {
    fn default() -> Self {
        // A4 with 2 cm margins, OpenOffice's default.
        PageSetup { width: 595.28, height: 841.89, margin: 56.69 }
    }
}

/// Smallest page side in points (1 inch).
pub const MIN_PAGE_SIDE: f32 = 72.0;
/// Largest page side in points (200 inches).
pub const MAX_PAGE_SIDE: f32 = 14_400.0;

impl PageSetup {
    /// Whether the size and margin are usable: sides within
    /// [`MIN_PAGE_SIDE`]..=[`MAX_PAGE_SIDE`] and margins leaving at least a
    /// third of the shorter side for text.
    pub fn is_valid(&self) -> bool {
        let side = MIN_PAGE_SIDE..=MAX_PAGE_SIDE;
        side.contains(&self.width)
            && side.contains(&self.height)
            && self.margin.is_finite()
            && (0.0..=self.width.min(self.height) / 3.0).contains(&self.margin)
    }

    /// Whether the page is wider than it is tall.
    pub fn is_landscape(&self) -> bool {
        self.width > self.height
    }

    /// The paper size this page matches (in either orientation), if any.
    pub fn paper(&self) -> Option<&'static PaperSize> {
        PAPER_SIZES.iter().find(|p| {
            let close = |a: f32, b: f32| (a - b).abs() < 1.5;
            (close(p.width, self.width) && close(p.height, self.height))
                || (close(p.width, self.height) && close(p.height, self.width))
        })
    }
}

/// What a paper size is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperKind {
    /// A sheet of paper.
    Sheet,
    /// An envelope.
    Envelope,
    /// An index card.
    Card,
}

/// A standard paper size. Sheets are portrait; envelopes and index cards
/// are landscape, the way they are written on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PaperSize {
    /// Stable identifier (`a4`, `env_dl`, `card_3x5`...).
    pub id: &'static str,
    /// Short name: the paper name, envelope size code or card size (the UI
    /// adds "envelope" / "index card").
    pub name: &'static str,
    /// What it is.
    pub kind: PaperKind,
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

const MM: f32 = 72.0 / 25.4;
const IN: f32 = 72.0;

/// The paper sizes offered by page setup.
pub const PAPER_SIZES: &[PaperSize] = &[
    PaperSize { id: "a4", name: "A4", kind: PaperKind::Sheet, width: 210.0 * MM, height: 297.0 * MM },
    PaperSize { id: "a5", name: "A5", kind: PaperKind::Sheet, width: 148.0 * MM, height: 210.0 * MM },
    PaperSize { id: "a3", name: "A3", kind: PaperKind::Sheet, width: 297.0 * MM, height: 420.0 * MM },
    PaperSize { id: "letter", name: "Letter", kind: PaperKind::Sheet, width: 8.5 * IN, height: 11.0 * IN },
    PaperSize { id: "legal", name: "Legal", kind: PaperKind::Sheet, width: 8.5 * IN, height: 14.0 * IN },
    PaperSize { id: "env_dl", name: "DL", kind: PaperKind::Envelope, width: 220.0 * MM, height: 110.0 * MM },
    PaperSize { id: "env_c6", name: "C6", kind: PaperKind::Envelope, width: 162.0 * MM, height: 114.0 * MM },
    PaperSize { id: "env_c5", name: "C5", kind: PaperKind::Envelope, width: 229.0 * MM, height: 162.0 * MM },
    PaperSize { id: "env_c4", name: "C4", kind: PaperKind::Envelope, width: 324.0 * MM, height: 229.0 * MM },
    PaperSize { id: "env_10", name: "#10", kind: PaperKind::Envelope, width: 9.5 * IN, height: 4.125 * IN },
    PaperSize { id: "card_3x5", name: "3 × 5 in", kind: PaperKind::Card, width: 5.0 * IN, height: 3.0 * IN },
    PaperSize { id: "card_4x6", name: "4 × 6 in", kind: PaperKind::Card, width: 6.0 * IN, height: 4.0 * IN },
    PaperSize { id: "card_5x8", name: "5 × 8 in", kind: PaperKind::Card, width: 8.0 * IN, height: 5.0 * IN },
    PaperSize { id: "card_a7", name: "A7", kind: PaperKind::Card, width: 105.0 * MM, height: 74.0 * MM },
];

/// Looks up a paper size by its identifier.
pub fn paper_size(id: &str) -> Option<&'static PaperSize> {
    PAPER_SIZES.iter().find(|p| p.id == id)
}

/// Word, character and paragraph counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Stats {
    /// Words (runs of non-whitespace).
    pub words: usize,
    /// Characters including spaces.
    pub chars: usize,
    /// Characters excluding whitespace.
    pub chars_no_spaces: usize,
    /// Non-empty paragraphs (table cells included).
    pub paragraphs: usize,
    /// Tables.
    pub tables: usize,
}

/// A sheet of self-adhesive labels: where the labels sit on the paper.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LabelSheet {
    /// Stable identifier.
    pub id: &'static str,
    /// Product name.
    pub name: &'static str,
    /// Paper size identifier (see [`PAPER_SIZES`]).
    pub paper: &'static str,
    /// Labels across.
    pub cols: usize,
    /// Labels down.
    pub rows: usize,
    /// Label width in points.
    pub width: f32,
    /// Label height in points.
    pub height: f32,
    /// Distance from the top of the paper to the first label, in points.
    pub top: f32,
    /// Distance from the left of the paper to the first label, in points.
    pub left: f32,
    /// Horizontal distance between the left edges of neighbouring labels.
    pub h_pitch: f32,
    /// Vertical distance between the top edges of neighbouring labels.
    pub v_pitch: f32,
}

impl LabelSheet {
    /// Labels per sheet.
    pub fn per_sheet(&self) -> usize {
        self.cols * self.rows
    }

    /// Whether the labels fit on the paper without overlapping.
    pub fn is_valid(&self) -> bool {
        let Some(paper) = paper_size(self.paper) else { return false };
        let finite = [self.width, self.height, self.top, self.left, self.h_pitch, self.v_pitch]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0);
        finite
            && (1..=20).contains(&self.cols)
            && (1..=40).contains(&self.rows)
            && self.width >= 18.0
            && self.height >= 9.0
            && self.h_pitch >= self.width
            && self.v_pitch >= self.height
            && self.left + self.h_pitch * (self.cols - 1) as f32 + self.width <= paper.width + 0.5
            && self.top + self.v_pitch * (self.rows - 1) as f32 + self.height <= paper.height + 0.5
    }
}

/// Common label sheets (Avery codes; compatible sheets from other makers
/// use the same codes).
pub const LABEL_SHEETS: &[LabelSheet] = &[
    LabelSheet { id: "l7160", name: "Avery L7160", paper: "a4", cols: 3, rows: 7, width: 63.5 * MM, height: 38.1 * MM, top: 15.15 * MM, left: 7.25 * MM, h_pitch: 66.04 * MM, v_pitch: 38.1 * MM },
    LabelSheet { id: "l7163", name: "Avery L7163", paper: "a4", cols: 2, rows: 7, width: 99.1 * MM, height: 38.1 * MM, top: 15.15 * MM, left: 4.65 * MM, h_pitch: 101.6 * MM, v_pitch: 38.1 * MM },
    LabelSheet { id: "l7173", name: "Avery L7173", paper: "a4", cols: 2, rows: 5, width: 99.1 * MM, height: 57.0 * MM, top: 6.0 * MM, left: 4.65 * MM, h_pitch: 101.6 * MM, v_pitch: 57.0 * MM },
    LabelSheet { id: "l7651", name: "Avery L7651", paper: "a4", cols: 5, rows: 13, width: 38.1 * MM, height: 21.2 * MM, top: 10.7 * MM, left: 4.75 * MM, h_pitch: 40.6 * MM, v_pitch: 21.2 * MM },
    LabelSheet { id: "5160", name: "Avery 5160", paper: "letter", cols: 3, rows: 10, width: 2.625 * IN, height: 1.0 * IN, top: 0.5 * IN, left: 0.1875 * IN, h_pitch: 2.75 * IN, v_pitch: 1.0 * IN },
    LabelSheet { id: "5163", name: "Avery 5163", paper: "letter", cols: 2, rows: 5, width: 4.0 * IN, height: 2.0 * IN, top: 0.5 * IN, left: 0.15625 * IN, h_pitch: 4.1875 * IN, v_pitch: 2.0 * IN },
    LabelSheet { id: "5167", name: "Avery 5167", paper: "letter", cols: 4, rows: 20, width: 1.75 * IN, height: 0.5 * IN, top: 0.5 * IN, left: 0.28125 * IN, h_pitch: 2.0625 * IN, v_pitch: 0.5 * IN },
];

/// Looks up a label sheet by its identifier.
pub fn label_sheet(id: &str) -> Option<&'static LabelSheet> {
    LABEL_SHEETS.iter().find(|l| l.id == id)
}

/// The parts of a letterhead: the sender's name at the top of every page,
/// address lines below it, and contact details in the footer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Letterhead {
    /// Organisation or person name.
    pub name: String,
    /// Address or tagline, one line per `\n`.
    pub details: String,
    /// Contact details for the footer (phone, e-mail, web), one line per `\n`.
    pub contact: String,
    /// Alignment of the header.
    pub align: Align,
    /// Colour of the name.
    pub color: Option<Rgb>,
}
