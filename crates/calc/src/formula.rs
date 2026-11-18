//! Formula syntax tree, parser and printer.
//!
//! Two concrete syntaxes are supported for the same tree:
//!
//! * the **UI syntax** users type, e.g. `=SUM(A1:B2; Sheet2.C3) * 2`
//!   (`;` or `,` separate arguments, `Sheet.A1` or `Sheet!A1` address other
//!   sheets);
//! * the **OpenFormula** syntax stored in `.ods` files, e.g.
//!   `of:=SUM([.A1:.B2];[$Sheet2.C3])*2`.
//!
//! Parsing is bounded in both input length and nesting depth, and never
//! panics: unparseable input is preserved verbatim in [`Expr::Invalid`] so the
//! user can fix it.

use crate::address::{col_name, parse_col, CellAddr, MAX_COLS, MAX_ROWS};
use crate::value::ErrorKind;

/// Longest accepted formula in bytes.
pub const MAX_FORMULA_LEN: usize = 16 * 1024;
/// Deepest accepted expression nesting.
pub const MAX_FORMULA_DEPTH: usize = 64;
/// Largest accepted number of arguments in one call.
pub const MAX_ARGS: usize = 255;

/// A reference to one cell, possibly on another sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRef {
    /// Sheet name, or `None` for the sheet the formula lives on.
    pub sheet: Option<String>,
    /// Referenced cell.
    pub addr: CellAddr,
    /// `$` on the row: the row does not move when the formula is copied.
    pub abs_row: bool,
    /// `$` on the column.
    pub abs_col: bool,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    /// `-x`
    Neg,
    /// `+x`
    Plus,
    /// `x%`
    Percent,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `^`
    Pow,
    /// `&`
    Concat,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl BinOp {
    fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Concat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
        }
    }

    /// Binding power; higher binds tighter.
    fn precedence(self) -> u8 {
        match self {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 1,
            BinOp::Concat => 2,
            BinOp::Add | BinOp::Sub => 3,
            BinOp::Mul | BinOp::Div => 4,
            BinOp::Pow => 5,
        }
    }
}

/// A formula expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Numeric literal.
    Number(f64),
    /// String literal.
    Text(String),
    /// `TRUE` / `FALSE`.
    Bool(bool),
    /// Error literal such as `#REF!`.
    Error(ErrorKind),
    /// Omitted function argument, as in `IF(A1;;2)`.
    Missing,
    /// Single cell reference.
    Ref(CellRef),
    /// Range reference; the second corner uses the sheet of the first.
    Range(CellRef, CellRef),
    /// Unary operation.
    Unary(UnOp, Box<Expr>),
    /// Binary operation.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Parenthesised expression (kept so formulas print as typed).
    Paren(Box<Expr>),
    /// Function call; the name is stored upper-cased.
    Call(String, Vec<Expr>),
    /// A bare name (named ranges are not supported; evaluates to `#NAME?`).
    Name(String),
    /// Source text that could not be parsed (without the leading `=`).
    Invalid(String),
}

/// Concrete syntax flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// What users type and see.
    Ui,
    /// OpenFormula, as stored in `.ods` files.
    OpenFormula,
    /// Excel, as stored in `.xlsx` files: `Sheet2!A1`, `,` between
    /// arguments, `_xlfn.` before functions newer than Excel 2007. Parsed
    /// like [`Syntax::Ui`].
    Excel,
}

/// Functions that Excel files must name with the `_xlfn.` prefix.
const XLFN: &[&str] = &["CONCAT", "TEXTJOIN", "IFNA", "DAYS", "XOR", "IFS", "SWITCH", "MAXIFS", "MINIFS"];

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Number(f64),
    Text(String),
    Ref(CellRef),
    Ident(String),
    Error(ErrorKind),
    Op(&'static str),
    LParen,
    RParen,
    Sep,
    Colon,
    End,
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    syntax: Syntax,
    /// Second corner of an OpenFormula `[a:b]` range, handed to the parser
    /// right after the first corner.
    pending_range: Option<CellRef>,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// Parses `$A$1`-style coordinates at the start of `s`. Returns the reference
/// (without sheet) and the number of bytes consumed.
fn parse_coords(s: &str) -> Option<(CellRef, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let abs_col = bytes.first() == Some(&b'$');
    if abs_col {
        i += 1;
    }
    let col_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() && i - col_start < 4 {
        i += 1;
    }
    let col = parse_col(&s[col_start..i])?;
    let abs_row = bytes.get(i) == Some(&b'$');
    if abs_row {
        i += 1;
    }
    let row_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() && i - row_start < 8 {
        i += 1;
    }
    let row: u32 = s[row_start..i].parse().ok()?;
    if row == 0 || row > MAX_ROWS {
        return None;
    }
    // Must not run into more identifier characters (e.g. `A1B`, `LOG10(`).
    if bytes.get(i).is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'(') {
        return None;
    }
    Some((CellRef { sheet: None, addr: CellAddr::new(row - 1, col), abs_row, abs_col }, i))
}

/// Parses an optional sheet prefix (`Sheet1.`, `$Sheet1.`, `'My Sheet'.`,
/// `Sheet1!`). Returns the sheet name and bytes consumed (including the
/// separator).
fn parse_sheet_prefix(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'$') {
        i += 1;
    }
    let name = if bytes.get(i) == Some(&b'\'') {
        i += 1;
        let mut out = String::new();
        loop {
            let rest = s.get(i..)?;
            let c = rest.chars().next()?;
            if c == '\'' {
                if rest[1..].starts_with('\'') {
                    out.push('\'');
                    i += 2;
                } else {
                    i += 1;
                    break;
                }
            } else {
                out.push(c);
                i += c.len_utf8();
            }
        }
        out
    } else {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
            i += 1;
        }
        if i == start {
            return None;
        }
        s.get(start..i)?.to_owned()
    };
    if name.is_empty() || name.len() > 255 {
        return None;
    }
    match bytes.get(i) {
        Some(b'.' | b'!') => Some((name, i + 1)),
        _ => None,
    }
}

/// Parses a full reference (optional sheet prefix plus coordinates).
fn parse_reference(s: &str) -> Option<(CellRef, usize)> {
    if let Some((cell, len)) = parse_coords(s) {
        return Some((cell, len));
    }
    let (sheet, prefix_len) = parse_sheet_prefix(s)?;
    let (mut cell, len) = parse_coords(&s[prefix_len..])?;
    cell.sheet = Some(sheet);
    Some((cell, prefix_len + len))
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Tok, ()> {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
        let Some(&b) = self.bytes.get(self.pos) else {
            return Ok(Tok::End);
        };
        let rest = &self.src[self.pos..];
        match b {
            b'0'..=b'9' | b'.' => {
                let len = number_len(rest).ok_or(())?;
                let value: f64 = rest[..len].parse().map_err(|_| ())?;
                if !value.is_finite() {
                    return Err(());
                }
                self.pos += len;
                Ok(Tok::Number(value))
            }
            b'"' => {
                let mut out = String::new();
                let mut i = 1;
                loop {
                    let c = rest[i..].chars().next().ok_or(())?;
                    if c == '"' {
                        if rest[i + 1..].starts_with('"') {
                            out.push('"');
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        out.push(c);
                        i += c.len_utf8();
                    }
                }
                self.pos += i;
                Ok(Tok::Text(out))
            }
            b'[' if self.syntax == Syntax::OpenFormula => {
                let end = rest.find(']').ok_or(())?;
                let inner = &rest[1..end];
                self.pos += end + 1;
                self.bracket_reference(inner)
            }
            b'#' => {
                for lit in ["#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#N/A", "#NUM!"] {
                    if rest.starts_with(lit) {
                        self.pos += lit.len();
                        return ErrorKind::parse(lit).map(Tok::Error).ok_or(());
                    }
                }
                Err(())
            }
            b'(' => self.single(Tok::LParen),
            b')' => self.single(Tok::RParen),
            b';' | b',' => self.single(Tok::Sep),
            b':' => self.single(Tok::Colon),
            b'+' => self.single(Tok::Op("+")),
            b'-' => self.single(Tok::Op("-")),
            b'*' => self.single(Tok::Op("*")),
            b'/' => self.single(Tok::Op("/")),
            b'^' => self.single(Tok::Op("^")),
            b'&' => self.single(Tok::Op("&")),
            b'%' => self.single(Tok::Op("%")),
            b'=' => self.single(Tok::Op("=")),
            b'<' => {
                if rest.starts_with("<=") {
                    self.pos += 2;
                    Ok(Tok::Op("<="))
                } else if rest.starts_with("<>") {
                    self.pos += 2;
                    Ok(Tok::Op("<>"))
                } else {
                    self.single(Tok::Op("<"))
                }
            }
            b'>' => {
                if rest.starts_with(">=") {
                    self.pos += 2;
                    Ok(Tok::Op(">="))
                } else {
                    self.single(Tok::Op(">"))
                }
            }
            _ => {
                if matches!(b, b'$' | b'\'') || is_ident_start(b) || b >= 0x80 {
                    if let Some((cell, len)) = parse_reference(rest) {
                        self.pos += len;
                        return Ok(Tok::Ref(cell));
                    }
                }
                if is_ident_start(b) {
                    let mut i = 0;
                    while i < rest.len() && is_ident_char(rest.as_bytes()[i]) {
                        i += 1;
                    }
                    self.pos += i;
                    return Ok(Tok::Ident(rest[..i].to_ascii_uppercase()));
                }
                Err(())
            }
        }
    }

    fn single(&mut self, tok: Tok) -> Result<Tok, ()> {
        self.pos += 1;
        Ok(tok)
    }

    /// Parses the inside of an OpenFormula `[...]` reference. Ranges are
    /// returned as the first corner followed by a pushed-back `:corner`.
    fn bracket_reference(&mut self, inner: &str) -> Result<Tok, ()> {
        let (first, second) = match split_range(inner) {
            Some((a, b)) => (a, Some(b)),
            None => (inner, None),
        };
        let a = parse_of_ref(first).ok_or(())?;
        match second {
            None => Ok(Tok::Ref(a)),
            Some(second) => {
                let b = parse_of_ref(second).ok_or(())?;
                self.pending_range = Some(b);
                Ok(Tok::Ref(a))
            }
        }
    }
}

/// Splits `a:b` at the colon that is not inside a quoted sheet name.
fn split_range(s: &str) -> Option<(&str, &str)> {
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' => in_quote = !in_quote,
            ':' if !in_quote => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

/// Parses one corner of an OpenFormula reference: `.A1`, `$Sheet.A1`,
/// `$'My Sheet'.$A$1`. `#REF!` components make the whole reference invalid.
fn parse_of_ref(s: &str) -> Option<CellRef> {
    let s = s.trim();
    if let Some(local) = s.strip_prefix('.') {
        let (cell, len) = parse_coords(local)?;
        return (len == local.len()).then_some(cell);
    }
    let (cell, len) = parse_reference(s)?;
    (len == s.len()).then_some(cell)
}

/// Length of the numeric literal at the start of `s`.
fn number_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - start;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > start {
            i = j;
        }
    }
    Some(i)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    lexer: Lexer<'a>,
    tok: Tok,
    depth: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, syntax: Syntax) -> Self {
        Lexer { src, bytes: src.as_bytes(), pos: 0, syntax, pending_range: None }
    }
}

impl<'a> Parser<'a> {
    fn advance(&mut self) -> Result<(), ()> {
        self.tok = self.lexer.next()?;
        Ok(())
    }

    fn binary_op(&self) -> Option<BinOp> {
        match &self.tok {
            Tok::Op(s) => Some(match *s {
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "^" => BinOp::Pow,
                "&" => BinOp::Concat,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                "<=" => BinOp::Le,
                ">" => BinOp::Gt,
                ">=" => BinOp::Ge,
                _ => return None,
            }),
            _ => None,
        }
    }

    /// Precedence climbing; all binary operators are left associative.
    fn expr(&mut self, min_prec: u8) -> Result<Expr, ()> {
        if self.depth >= MAX_FORMULA_DEPTH {
            return Err(());
        }
        self.depth += 1;
        let mut left = self.unary()?;
        while let Some(op) = self.binary_op() {
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            self.advance()?;
            let right = self.expr(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ()> {
        // Prefix signs bind tighter than every binary operator (-2^2 = 4).
        let mut signs = Vec::new();
        loop {
            match &self.tok {
                Tok::Op("-") => signs.push(UnOp::Neg),
                Tok::Op("+") => signs.push(UnOp::Plus),
                _ => break,
            }
            if signs.len() > MAX_FORMULA_DEPTH {
                return Err(());
            }
            self.advance()?;
        }
        let mut node = self.postfix()?;
        for op in signs.into_iter().rev() {
            node = Expr::Unary(op, Box::new(node));
        }
        Ok(node)
    }

    fn postfix(&mut self) -> Result<Expr, ()> {
        let mut node = self.primary()?;
        while self.tok == Tok::Op("%") {
            self.advance()?;
            node = Expr::Unary(UnOp::Percent, Box::new(node));
        }
        Ok(node)
    }

    fn primary(&mut self) -> Result<Expr, ()> {
        match std::mem::replace(&mut self.tok, Tok::End) {
            Tok::Number(n) => {
                self.advance()?;
                Ok(Expr::Number(n))
            }
            Tok::Text(s) => {
                self.advance()?;
                Ok(Expr::Text(s))
            }
            Tok::Error(e) => {
                self.advance()?;
                Ok(Expr::Error(e))
            }
            Tok::Ref(first) => {
                if let Some(mut second) = self.lexer.pending_range.take() {
                    self.advance()?;
                    second.sheet = None;
                    return Ok(Expr::Range(first, second));
                }
                self.advance()?;
                if self.tok == Tok::Colon {
                    self.advance()?;
                    match std::mem::replace(&mut self.tok, Tok::End) {
                        Tok::Ref(mut second) => {
                            if second.sheet.is_some() && second.sheet != first.sheet {
                                return Err(());
                            }
                            second.sheet = None;
                            self.advance()?;
                            Ok(Expr::Range(first, second))
                        }
                        _ => Err(()),
                    }
                } else {
                    Ok(Expr::Ref(first))
                }
            }
            Tok::Ident(name) => {
                self.advance()?;
                if self.tok == Tok::LParen {
                    self.advance()?;
                    let args = self.arguments()?;
                    return Ok(Expr::Call(name, args));
                }
                match name.as_str() {
                    "TRUE" => Ok(Expr::Bool(true)),
                    "FALSE" => Ok(Expr::Bool(false)),
                    // Named ranges are not supported: evaluates to #NAME?.
                    _ => Ok(Expr::Name(name)),
                }
            }
            Tok::LParen => {
                self.advance()?;
                let inner = self.expr(0)?;
                if self.tok != Tok::RParen {
                    return Err(());
                }
                self.advance()?;
                Ok(Expr::Paren(Box::new(inner)))
            }
            _ => Err(()),
        }
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, ()> {
        let mut args = Vec::new();
        if self.tok == Tok::RParen {
            self.advance()?;
            return Ok(args);
        }
        loop {
            if args.len() >= MAX_ARGS {
                return Err(());
            }
            if self.tok == Tok::Sep || self.tok == Tok::RParen {
                args.push(Expr::Missing);
            } else {
                args.push(self.expr(0)?);
            }
            match self.tok {
                Tok::Sep => self.advance()?,
                Tok::RParen => {
                    self.advance()?;
                    return Ok(args);
                }
                _ => return Err(()),
            }
        }
    }
}

/// Parses a formula body (the text after the leading `=`).
///
/// Never fails: input that cannot be parsed becomes [`Expr::Invalid`].
pub fn parse(body: &str, syntax: Syntax) -> Expr {
    try_parse(body, syntax).unwrap_or_else(|| Expr::Invalid(body.chars().take(MAX_FORMULA_LEN).collect()))
}

fn try_parse(body: &str, syntax: Syntax) -> Option<Expr> {
    if body.len() > MAX_FORMULA_LEN {
        return None;
    }
    let mut parser = Parser { lexer: Lexer::new(body, syntax), tok: Tok::End, depth: 0 };
    parser.advance().ok()?;
    let expr = parser.expr(0).ok()?;
    (parser.tok == Tok::End).then_some(expr)
}

/// Parses the value of a `table:formula` attribute (`of:=...`, `oooc:=...` or
/// a bare `=...`).
pub fn parse_open_formula(attr: &str) -> Expr {
    let body = match attr.find('=') {
        Some(idx) if attr[..idx].chars().all(|c| c.is_ascii_alphanumeric() || c == ':') => &attr[idx + 1..],
        _ => attr,
    };
    parse(body, Syntax::OpenFormula)
}

// ---------------------------------------------------------------------------
// Printer
// ---------------------------------------------------------------------------

fn needs_quotes(sheet: &str) -> bool {
    sheet.is_empty()
        || sheet.bytes().next().is_some_and(|b| b.is_ascii_digit())
        || !sheet.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn push_sheet(out: &mut String, sheet: &str) {
    if needs_quotes(sheet) {
        out.push('\'');
        out.push_str(&sheet.replace('\'', "''"));
        out.push('\'');
    } else {
        out.push_str(sheet);
    }
}

fn push_coords(out: &mut String, cell: &CellRef) {
    if cell.abs_col {
        out.push('$');
    }
    out.push_str(&col_name(cell.addr.col));
    if cell.abs_row {
        out.push('$');
    }
    out.push_str(&(cell.addr.row + 1).to_string());
}

fn push_ref(out: &mut String, cell: &CellRef, syntax: Syntax) {
    match (&cell.sheet, syntax) {
        (Some(sheet), Syntax::Ui) => {
            push_sheet(out, sheet);
            out.push('.');
        }
        (Some(sheet), Syntax::OpenFormula) => {
            out.push('$');
            push_sheet(out, sheet);
            out.push('.');
        }
        (Some(sheet), Syntax::Excel) => {
            push_sheet(out, sheet);
            out.push('!');
        }
        (None, Syntax::Ui | Syntax::Excel) => {}
        (None, Syntax::OpenFormula) => out.push('.'),
    }
    push_coords(out, cell);
}

fn push_number(out: &mut String, n: f64) {
    if n == n.trunc() && n.abs() < 1e15 {
        out.push_str(&format!("{n:.0}"));
    } else {
        out.push_str(&format!("{n}"));
    }
}

fn print(expr: &Expr, syntax: Syntax, out: &mut String) {
    match expr {
        Expr::Number(n) => push_number(out, *n),
        Expr::Text(s) => {
            out.push('"');
            out.push_str(&s.replace('"', "\"\""));
            out.push('"');
        }
        Expr::Bool(true) => out.push_str("TRUE"),
        Expr::Bool(false) => out.push_str("FALSE"),
        Expr::Error(e) => out.push_str(e.as_str()),
        Expr::Missing => {}
        Expr::Ref(cell) => {
            if syntax == Syntax::OpenFormula {
                out.push('[');
                push_ref(out, cell, syntax);
                out.push(']');
            } else {
                push_ref(out, cell, syntax);
            }
        }
        Expr::Range(a, b) => {
            if syntax == Syntax::OpenFormula {
                out.push('[');
                push_ref(out, a, syntax);
                out.push_str(":.");
                push_coords(out, b);
                out.push(']');
            } else {
                push_ref(out, a, syntax);
                out.push(':');
                push_coords(out, b);
            }
        }
        Expr::Unary(UnOp::Percent, inner) => {
            print(inner, syntax, out);
            out.push('%');
        }
        Expr::Unary(op, inner) => {
            out.push(if *op == UnOp::Neg { '-' } else { '+' });
            print(inner, syntax, out);
        }
        Expr::Binary(op, left, right) => {
            print(left, syntax, out);
            out.push_str(op.symbol());
            print(right, syntax, out);
        }
        Expr::Paren(inner) => {
            out.push('(');
            print(inner, syntax, out);
            out.push(')');
        }
        Expr::Call(name, args) => {
            if syntax == Syntax::Excel && XLFN.contains(&name.as_str()) {
                out.push_str("_xlfn.");
            }
            out.push_str(name);
            out.push('(');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push(if syntax == Syntax::Excel { ',' } else { ';' });
                }
                print(arg, syntax, out);
            }
            out.push(')');
        }
        Expr::Name(name) => out.push_str(name),
        Expr::Invalid(src) => out.push_str(src),
    }
}

impl Expr {
    /// Prints the expression in the given syntax, without the leading `=`.
    ///
    /// Recursion is bounded because parsed trees are at most
    /// [`MAX_FORMULA_DEPTH`] operators deep.
    pub fn to_source(&self, syntax: Syntax) -> String {
        let mut out = String::new();
        print(self, syntax, &mut out);
        out
    }

    /// The formula as stored in a `table:formula` attribute.
    pub fn to_open_formula(&self) -> String {
        format!("of:={}", self.to_source(Syntax::OpenFormula))
    }

    /// Calls `f` for every reference in the tree.
    pub fn visit_refs<'a>(&'a self, f: &mut dyn FnMut(&'a CellRef, Option<&'a CellRef>)) {
        match self {
            Expr::Ref(cell) => f(cell, None),
            Expr::Range(a, b) => f(a, Some(b)),
            Expr::Unary(_, inner) | Expr::Paren(inner) => inner.visit_refs(f),
            Expr::Binary(_, left, right) => {
                left.visit_refs(f);
                right.visit_refs(f);
            }
            Expr::Call(_, args) => {
                for arg in args {
                    arg.visit_refs(f);
                }
            }
            _ => {}
        }
    }

    /// Rewrites every reference in place. `f` returns `false` to turn the
    /// reference into `#REF!`.
    pub fn map_refs(&mut self, f: &mut dyn FnMut(&mut CellRef, Option<&mut CellRef>) -> bool) {
        let keep = match self {
            Expr::Ref(cell) => f(cell, None),
            Expr::Range(a, b) => f(a, Some(b)),
            Expr::Unary(_, inner) | Expr::Paren(inner) => {
                inner.map_refs(f);
                true
            }
            Expr::Binary(_, left, right) => {
                left.map_refs(f);
                right.map_refs(f);
                true
            }
            Expr::Call(_, args) => {
                for arg in args {
                    arg.map_refs(f);
                }
                true
            }
            _ => true,
        };
        if !keep {
            *self = Expr::Error(ErrorKind::Ref);
        }
    }

    /// Whether the tree calls any of the given (upper-case) functions.
    pub fn calls_any(&self, names: &[&str]) -> bool {
        match self {
            Expr::Call(name, args) => {
                names.contains(&name.as_str()) || args.iter().any(|a| a.calls_any(names))
            }
            Expr::Unary(_, inner) | Expr::Paren(inner) => inner.calls_any(names),
            Expr::Binary(_, left, right) => left.calls_any(names) || right.calls_any(names),
            _ => false,
        }
    }

    /// Returns a copy shifted by `(d_row, d_col)`: relative parts of
    /// references move, absolute (`$`) parts stay. References pushed off the
    /// sheet become `#REF!`.
    pub fn shifted(&self, d_row: i64, d_col: i64) -> Expr {
        let mut copy = self.clone();
        let shift = |cell: &mut CellRef| -> bool {
            let row = if cell.abs_row { i64::from(cell.addr.row) } else { i64::from(cell.addr.row) + d_row };
            let col = if cell.abs_col { i64::from(cell.addr.col) } else { i64::from(cell.addr.col) + d_col };
            if row < 0 || col < 0 || row >= i64::from(MAX_ROWS) || col >= i64::from(MAX_COLS) {
                return false;
            }
            cell.addr = CellAddr::new(row as u32, col as u32);
            true
        };
        copy.map_refs(&mut |a, b| {
            let ok_a = shift(a);
            let ok_b = b.map(shift).unwrap_or(true);
            ok_a && ok_b
        });
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui(src: &str) -> Expr {
        parse(src, Syntax::Ui)
    }

    fn round_trip(src: &str) -> String {
        ui(src).to_source(Syntax::Ui)
    }

    #[test]
    fn literals_and_operators() {
        assert_eq!(ui("1+2"), Expr::Binary(BinOp::Add, Box::new(Expr::Number(1.0)), Box::new(Expr::Number(2.0))));
        assert_eq!(ui("\"a\"\"b\""), Expr::Text("a\"b".into()));
        assert_eq!(ui("true"), Expr::Bool(true));
        assert_eq!(ui("#DIV/0!"), Expr::Error(ErrorKind::DivZero));
        assert_eq!(ui("1.5e3"), Expr::Number(1500.0));
        assert_eq!(ui(".5"), Expr::Number(0.5));
        assert_eq!(ui("50%"), Expr::Unary(UnOp::Percent, Box::new(Expr::Number(50.0))));
    }

    #[test]
    fn precedence_and_associativity() {
        // 1+2*3 => 1+(2*3)
        match ui("1+2*3") {
            Expr::Binary(BinOp::Add, _, right) => assert!(matches!(*right, Expr::Binary(BinOp::Mul, ..))),
            other => panic!("{other:?}"),
        }
        // 2^3^2 => (2^3)^2, left associative like Calc
        match ui("2^3^2") {
            Expr::Binary(BinOp::Pow, left, _) => assert!(matches!(*left, Expr::Binary(BinOp::Pow, ..))),
            other => panic!("{other:?}"),
        }
        // -2^2 => (-2)^2
        match ui("-2^2") {
            Expr::Binary(BinOp::Pow, left, _) => assert!(matches!(*left, Expr::Unary(UnOp::Neg, _))),
            other => panic!("{other:?}"),
        }
        // comparison binds loosest, & below +
        match ui("1+2&3=4") {
            Expr::Binary(BinOp::Eq, left, _) => assert!(matches!(*left, Expr::Binary(BinOp::Concat, ..))),
            other => panic!("{other:?}"),
        }
        // 10-2-3 => (10-2)-3
        match ui("10-2-3") {
            Expr::Binary(BinOp::Sub, left, _) => assert!(matches!(*left, Expr::Binary(BinOp::Sub, ..))),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn references() {
        assert_eq!(
            ui("$B$2"),
            Expr::Ref(CellRef { sheet: None, addr: CellAddr::new(1, 1), abs_row: true, abs_col: true })
        );
        match ui("Sheet2.A1:B3") {
            Expr::Range(a, b) => {
                assert_eq!(a.sheet.as_deref(), Some("Sheet2"));
                assert_eq!(b.addr, CellAddr::new(2, 1));
                assert_eq!(b.sheet, None);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(ui("'My Sheet'.A1"), Expr::Ref(CellRef { sheet: Some(ref s), .. }) if s == "My Sheet"));
        assert!(matches!(ui("'It''s'!A1"), Expr::Ref(CellRef { sheet: Some(ref s), .. }) if s == "It's"));
        assert!(matches!(ui("Sheet1!A1"), Expr::Ref(_)));
        assert!(matches!(ui("LOG10(5)"), Expr::Call(ref n, _) if n == "LOG10"));
        assert!(matches!(ui("A1048577"), Expr::Name(_)), "out of range row is a name, not a ref");
        assert!(matches!(ui("Sheet1.A1:Sheet2.B2"), Expr::Invalid(_)));
    }

    #[test]
    fn calls_and_arguments() {
        match ui("sum(A1:A3; 5, B2)") {
            Expr::Call(name, args) => {
                assert_eq!(name, "SUM");
                assert_eq!(args.len(), 3);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ui("PI()"), Expr::Call("PI".into(), vec![]));
        match ui("IF(A1;;2)") {
            Expr::Call(_, args) => assert_eq!(args[1], Expr::Missing),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn printing_round_trips() {
        for src in [
            "1+2*3", "(1+2)*3", "SUM(A1:B2;Sheet2.C3)", "-A1%", "\"x\"\"y\"&B2", "IF(A1>=3;\"y\";\"n\")",
            "'My Sheet'.$A$1+1", "2^-1", "IF(A1;;2)", "PI()", "1.5", "A1<>B1", "FOO(1)", "#REF!+1", "MYNAME*2",
        ] {
            assert_eq!(round_trip(src), src);
            // Printing then parsing again yields the same tree.
            assert_eq!(ui(&round_trip(src)), ui(src));
        }
    }

    #[test]
    fn open_formula_syntax() {
        let expr = parse_open_formula("of:=SUM([.A1:.B2];[$Sheet2.C3])*[.$D$4]");
        assert_eq!(expr.to_source(Syntax::Ui), "SUM(A1:B2;Sheet2.C3)*$D$4");
        assert_eq!(expr.to_source(Syntax::Excel), "SUM(A1:B2,Sheet2!C3)*$D$4");
        assert_eq!(ui("CONCAT('My Sheet'.A1;\"x\")").to_source(Syntax::Excel), "_xlfn.CONCAT('My Sheet'!A1,\"x\")");
        assert_eq!(expr.to_open_formula(), "of:=SUM([.A1:.B2];[$Sheet2.C3])*[.$D$4]");
        let expr = parse_open_formula("oooc:=[$'My Sheet'.A1:.B2]");
        assert_eq!(expr.to_source(Syntax::Ui), "'My Sheet'.A1:B2");
        let expr = parse_open_formula("=1+1");
        assert_eq!(expr.to_source(Syntax::Ui), "1+1");
        assert!(matches!(parse_open_formula("of:=[.#REF!]"), Expr::Invalid(_)));
        // UI formulas convert to OpenFormula and back.
        let expr = ui("VLOOKUP(A1;'Data 1'.A1:C9;2;0)");
        assert_eq!(parse_open_formula(&expr.to_open_formula()), expr);
    }

    #[test]
    fn invalid_input_is_preserved() {
        for src in ["1+", "(1", "SUM(1", "1 2", "\"open", "@", "1..2", "A1:", "#BAD", ")", "", "[.A1]"] {
            assert_eq!(ui(src), Expr::Invalid(src.into()), "{src}");
            assert_eq!(round_trip(src), src);
        }
    }

    #[test]
    fn nesting_and_size_are_bounded() {
        let deep = "(".repeat(1000) + "1" + &")".repeat(1000);
        assert!(matches!(ui(&deep), Expr::Invalid(_)));
        let signs = "-".repeat(1000) + "1";
        assert!(matches!(ui(&signs), Expr::Invalid(_)));
        let long = "1+".repeat(MAX_FORMULA_LEN) + "1";
        assert!(matches!(ui(&long), Expr::Invalid(_)));
        let ok = "(".repeat(20) + "1" + &")".repeat(20);
        assert!(!matches!(ui(&ok), Expr::Invalid(_)));
        let many_args = format!("SUM({})", vec!["1"; 300].join(";"));
        assert!(matches!(ui(&many_args), Expr::Invalid(_)));
    }

    #[test]
    fn shifting_references() {
        let expr = ui("A1+$B$2+C$3+$D4");
        assert_eq!(expr.shifted(1, 1).to_source(Syntax::Ui), "B2+$B$2+D$3+$D5");
        assert_eq!(ui("A1").shifted(-1, 0), Expr::Error(ErrorKind::Ref));
        assert_eq!(ui("SUM(A1:B2)").shifted(0, -1).to_source(Syntax::Ui), "SUM(#REF!)");
        assert_eq!(ui("A1:B2").shifted(2, 0).to_source(Syntax::Ui), "A3:B4");
    }

    #[test]
    fn helpers() {
        let expr = ui("IF(RAND()>0.5;A1;SUM(B1:B3))");
        assert!(expr.calls_any(&["RAND"]));
        assert!(!expr.calls_any(&["NOW"]));
        let mut count = 0;
        expr.visit_refs(&mut |_, _| count += 1);
        assert_eq!(count, 2);
    }
}
