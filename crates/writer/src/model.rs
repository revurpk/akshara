//! The text document model.
//!
//! A [`Document`](crate::Document) is a flat list of [`Block`]s: paragraphs, tables and page
//! breaks. A [`Paragraph`] holds a list of [`Run`]s, each a piece of text with
//! uniform character attributes. The model is deliberately flat (lists are
//! paragraphs with a list kind and level) which keeps editing simple and maps
//! directly onto the Flutter editor, one editable line per paragraph.

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

/// A paragraph.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Paragraph {
    /// Paragraph kind.
    #[serde(flatten)]
    pub kind: ParaKind,
    /// Alignment.
    pub align: Align,
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
    /// text).
    pub fn normalize(&mut self) {
        let empty_attrs = self.runs.first().map(|r| r.attrs.clone()).unwrap_or_default();
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in self.runs.drain(..) {
            if run.text.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.attrs == run.attrs => last.text.push_str(&run.text),
                _ => out.push(run),
            }
        }
        if out.is_empty() {
            out.push(Run { text: String::new(), attrs: empty_attrs });
        }
        self.runs = out;
    }
}

/// A table: rows of cells, each cell a list of paragraphs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    /// Rows; every row has the same number of cells.
    pub rows: Vec<Vec<Vec<Paragraph>>>,
}

impl Table {
    /// An empty table with the given size (at least 1×1).
    pub fn new(rows: usize, cols: usize) -> Table {
        let (rows, cols) = (rows.clamp(1, 1000), cols.clamp(1, 64));
        Table { rows: vec![vec![vec![Paragraph::plain("")]; cols]; rows] }
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
