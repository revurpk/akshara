//! The formula tree produced by the parser.
//!
//! The tree is *presentation oriented*: it describes how a formula is laid
//! out (fractions, scripts, fences...), mirroring both the StarMath command
//! language and presentation MathML. It serialises to JSON (via `serde`) for
//! the Flutter renderer and to MathML for document exchange.

use serde::Serialize;

/// Spacing class of an operator symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// Binary operator such as `+` (medium space on both sides).
    Binary,
    /// Relation such as `=` (thick space on both sides).
    Relation,
    /// Prefix operator such as unary minus (no space).
    Prefix,
    /// Punctuation and other ordinary symbols.
    Ordinary,
}

/// How a `size` attribute changes the font size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum Size {
    /// Absolute size in points (`size 12`).
    Abs(f64),
    /// Points added to the current size (`size +2`, `size -2`).
    Add(f64),
    /// Factor applied to the current size (`size *1.5`, `size /2`).
    Mul(f64),
}

/// A node of the formula tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Node {
    /// Horizontal sequence.
    Row {
        /// Items laid out left to right.
        items: Vec<Node>,
    },
    /// Several lines separated by `newline`.
    Lines {
        /// One node per line.
        rows: Vec<Node>,
    },
    /// Identifier (variable or symbol).
    Ident {
        /// Text to show.
        text: String,
        /// Whether it is set in italics.
        italic: bool,
    },
    /// Numeric literal.
    Number {
        /// Digits as typed.
        text: String,
    },
    /// Operator symbol.
    Op {
        /// Symbol to show.
        text: String,
        /// Spacing class.
        kind: OpKind,
    },
    /// Quoted text, set upright.
    Text {
        /// The text.
        text: String,
    },
    /// Upright function name such as `sin`.
    Func {
        /// Function name.
        name: String,
    },
    /// Large operator such as a sum or integral sign.
    BigOp {
        /// Symbol to show.
        text: String,
    },
    /// Fraction (`a over b`).
    Frac {
        /// Numerator.
        num: Box<Node>,
        /// Denominator.
        den: Box<Node>,
    },
    /// Sub- and superscripts attached to the corners of a base.
    Script {
        /// The base.
        base: Box<Node>,
        /// Right subscript.
        sub: Option<Box<Node>>,
        /// Right superscript.
        sup: Option<Box<Node>>,
        /// Left subscript.
        lsub: Option<Box<Node>>,
        /// Left superscript.
        lsup: Option<Box<Node>>,
    },
    /// Limits centred below and above a base.
    UnderOver {
        /// The base.
        base: Box<Node>,
        /// Node centred below.
        under: Option<Box<Node>>,
        /// Node centred above.
        over: Option<Box<Node>>,
    },
    /// Square root.
    Sqrt {
        /// Radicand.
        body: Box<Node>,
    },
    /// N-th root.
    Root {
        /// Root index.
        index: Box<Node>,
        /// Radicand.
        body: Box<Node>,
    },
    /// Content between brackets.
    Fenced {
        /// Opening bracket (empty for `none`).
        open: String,
        /// Closing bracket (empty for `none`).
        close: String,
        /// Enclosed content.
        body: Box<Node>,
        /// Whether the brackets grow with the content (`left`/`right`).
        scalable: bool,
    },
    /// Matrix, stack or binomial arrangement.
    Matrix {
        /// Rows of cells.
        rows: Vec<Vec<Node>>,
    },
    /// Accent or line above/below a body.
    Accent {
        /// Decorated node.
        body: Box<Node>,
        /// Accent character.
        accent: String,
        /// Whether the accent stretches to the width of the body.
        stretchy: bool,
        /// Whether the accent is placed below instead of above.
        under: bool,
    },
    /// Font attributes applied to a body.
    Styled {
        /// Affected node.
        body: Box<Node>,
        /// Bold on/off, if changed.
        bold: Option<bool>,
        /// Italic on/off, if changed.
        italic: Option<bool>,
        /// Colour as `#rrggbb`, if changed.
        color: Option<String>,
        /// Size change, if any.
        size: Option<Size>,
        /// Font family class (`serif`, `sans`, `fixed`), if changed.
        font: Option<String>,
    },
    /// Invisible node that still takes up space.
    Phantom {
        /// The hidden node.
        body: Box<Node>,
    },
    /// Horizontal brace above or below a body with a label.
    Brace {
        /// Braced node.
        body: Box<Node>,
        /// Label next to the brace.
        label: Box<Node>,
        /// Brace above (`overbrace`) or below (`underbrace`).
        over: bool,
    },
    /// Explicit horizontal space in em.
    Space {
        /// Width in em.
        width: f64,
    },
    /// Placeholder for a missing operand (`<?>`).
    Placeholder,
}

impl Node {
    /// Wraps a list of nodes in a row unless it has exactly one item.
    pub fn row(mut items: Vec<Node>) -> Node {
        if items.len() == 1 {
            items.remove(0)
        } else {
            Node::Row { items }
        }
    }

    /// An empty row.
    pub fn empty() -> Node {
        Node::Row { items: Vec::new() }
    }
}

/// A problem found while parsing. Parsing never fails outright: the tree
/// contains [`Node::Placeholder`]s where input was missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParseError {
    /// Byte offset into the source.
    pub pos: usize,
    /// Human readable message.
    pub message: String,
}

/// Result of parsing a formula.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parsed {
    /// The formula tree (always present, even for erroneous input).
    pub root: Node,
    /// Problems found, in source order.
    pub errors: Vec<ParseError>,
}
