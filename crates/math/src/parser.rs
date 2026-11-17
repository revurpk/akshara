//! Recursive-descent parser for the StarMath command language.
//!
//! The grammar follows the precedence levels of OpenOffice Math:
//!
//! ```text
//! formula    := line ("newline" line)*
//! line       := relation*                      juxtaposition
//! relation   := sum (relop sum)*               =  <  <=  approx  in ...
//! sum        := product (sumop product)*       +  -  +-  or  union ...
//! product    := power (prodop power)*          *  /  cdot  times  over ...
//! power      := term (script term)*            ^  _  sup  sub  csup  lsub ...
//! term       := "{" line "}" | bracketed | number | identifier | "text"
//!             | %symbol | unary power | function | bigop [from..] [to..] power
//!             | attribute power | sqrt power | nroot power power | ...
//! ```
//!
//! The parser is **total**: it never fails and never panics. Missing operands
//! become [`Node::Placeholder`] and every problem is reported in
//! [`Parsed::errors`], so the UI can render a formula while it is being typed.
//! Nesting depth and input size are bounded to keep hostile input from
//! exhausting the stack.

use crate::ast::{Node, OpKind, ParseError, Parsed, Size};
use crate::lexer::{tokenize, Tok, Token};
use crate::symbols::{self, FontAttr, Keyword, Slot};

/// Longest accepted formula source in bytes.
pub const MAX_SOURCE_LEN: usize = 1 << 20;
/// Deepest accepted nesting of terms.
pub const MAX_DEPTH: usize = 96;

/// Parses a formula written in the StarMath command language.
pub fn parse(src: &str) -> Parsed {
    if src.len() > MAX_SOURCE_LEN {
        return Parsed {
            root: Node::Placeholder,
            errors: vec![ParseError { pos: 0, message: "formula is too long".into() }],
        };
    }
    let mut p = Parser { toks: tokenize(src), i: 0, errors: Vec::new(), depth: 0 };
    let root = p.formula();
    Parsed { root, errors: p.errors }
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    errors: Vec<ParseError>,
    depth: usize,
}

/// Token classes that end a juxtaposition sequence.
fn is_stopper(tok: &Tok) -> bool {
    match tok {
        Tok::End => true,
        Tok::Sym(s) => matches!(*s, "}" | ")" | "]" | "#" | "##"),
        Tok::Word(w) => matches!(
            symbols::keyword(w),
            Some(Keyword::Close(_) | Keyword::Right | Keyword::Newline | Keyword::MLine)
        ),
        _ => false,
    }
}

fn op(text: &str, kind: OpKind) -> Node {
    Node::Op { text: text.to_owned(), kind }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].tok
    }

    fn pos(&self) -> usize {
        self.toks[self.i.min(self.toks.len() - 1)].pos
    }

    fn bump(&mut self) -> Tok {
        let tok = self.peek().clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        tok
    }

    fn error(&mut self, message: impl Into<String>) {
        // Cap the list so pathological input cannot build a huge error vector.
        if self.errors.len() < 100 {
            let pos = self.pos();
            self.errors.push(ParseError { pos, message: message.into() });
        }
    }

    fn peek_keyword(&self) -> Option<Keyword> {
        match self.peek() {
            Tok::Word(w) => symbols::keyword(w),
            _ => None,
        }
    }

    fn eat_sym(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(s) if *s == sym) {
            self.bump();
            true
        } else {
            false
        }
    }

    // formula := line ("newline" line)*
    fn formula(&mut self) -> Node {
        let mut rows = vec![self.line()];
        loop {
            if self.peek_keyword() == Some(Keyword::Newline) {
                self.bump();
                rows.push(self.line());
            } else if matches!(self.peek(), Tok::End) {
                break;
            } else {
                // A stray closer at top level: report it, skip it, carry on.
                self.error("unexpected closing token");
                self.bump();
                let more = self.line();
                if let Some(last) = rows.last_mut() {
                    let prev = std::mem::replace(last, Node::Placeholder);
                    *last = Node::row(vec![prev, more]);
                }
            }
        }
        if rows.len() == 1 {
            rows.remove(0)
        } else {
            Node::Lines { rows }
        }
    }

    // line := relation*
    fn line(&mut self) -> Node {
        let mut items = Vec::new();
        while !is_stopper(self.peek()) {
            let before = self.i;
            if self.peek_keyword() == Some(Keyword::Align) {
                self.bump();
                continue;
            }
            items.push(self.relation());
            if self.i == before {
                // Defensive: guarantee progress whatever the token is.
                self.error("unexpected token");
                self.bump();
            }
        }
        Node::row(items)
    }

    fn relation_op(&self) -> Option<&'static str> {
        match self.peek() {
            Tok::Sym(s) => Some(match *s {
                "=" => "=",
                "<" => "<",
                ">" => ">",
                "<=" => "≤",
                ">=" => "≥",
                "<>" => "≠",
                "<<" => "≪",
                ">>" => "≫",
                "->" => "→",
                _ => return None,
            }),
            Tok::Word(w) => match symbols::keyword(w) {
                Some(Keyword::Rel(s)) => Some(s),
                _ => None,
            },
            _ => None,
        }
    }

    // relation := sum (relop sum)*
    fn relation(&mut self) -> Node {
        let mut items = vec![self.sum()];
        while let Some(sym) = self.relation_op() {
            self.bump();
            items.push(op(sym, OpKind::Relation));
            items.push(self.sum());
        }
        Node::row(items)
    }

    fn sum_op(&self) -> Option<&'static str> {
        match self.peek() {
            Tok::Sym(s) => Some(match *s {
                "+" => "+",
                "-" => "−",
                "+-" => "±",
                "-+" => "∓",
                "|" => "∨",
                _ => return None,
            }),
            Tok::Word(w) => match symbols::keyword(w) {
                Some(Keyword::Sum(s)) => Some(s),
                _ => None,
            },
            _ => None,
        }
    }

    // sum := product (sumop product)*
    fn sum(&mut self) -> Node {
        let mut items = vec![self.product()];
        while let Some(sym) = self.sum_op() {
            self.bump();
            items.push(op(sym, OpKind::Binary));
            items.push(self.product());
        }
        Node::row(items)
    }

    // product := power (prodop power)*
    fn product(&mut self) -> Node {
        let mut items = vec![self.power()];
        loop {
            let sym: &'static str = match self.peek() {
                Tok::Sym("*") => "∗",
                Tok::Sym("/") => "/",
                Tok::Sym("&") => "∧",
                Tok::Word(w) => match symbols::keyword(w) {
                    Some(Keyword::Prod(s)) => s,
                    Some(Keyword::Over) => {
                        self.bump();
                        let den = self.power();
                        let num = items.pop().unwrap_or(Node::Placeholder);
                        items.push(Node::Frac { num: Box::new(num), den: Box::new(den) });
                        continue;
                    }
                    Some(Keyword::Brace(over)) => {
                        self.bump();
                        let label = self.power();
                        let body = items.pop().unwrap_or(Node::Placeholder);
                        items.push(Node::Brace { body: Box::new(body), label: Box::new(label), over });
                        continue;
                    }
                    Some(Keyword::BOper) => {
                        self.bump();
                        let symbol = self.operator_symbol();
                        items.push(symbol);
                        items.push(self.power());
                        continue;
                    }
                    _ => break,
                },
                _ => break,
            };
            self.bump();
            items.push(op(sym, OpKind::Binary));
            items.push(self.power());
        }
        Node::row(items)
    }

    /// Reads the user-defined operator symbol after `boper` / `uoper` / `oper`.
    fn operator_symbol(&mut self) -> Node {
        match self.bump() {
            Tok::Special(name) => match symbols::special(&name) {
                Some((text, _)) => op(text, OpKind::Binary),
                None => op(&format!("%{name}"), OpKind::Binary),
            },
            Tok::Word(w) => match symbols::keyword(&w) {
                Some(Keyword::Symbol(s, _) | Keyword::Rel(s) | Keyword::Sum(s) | Keyword::Prod(s)) => {
                    op(s, OpKind::Binary)
                }
                _ => op(&w, OpKind::Binary),
            },
            Tok::Sym(s) => op(s, OpKind::Binary),
            Tok::Char(c) | Tok::Escaped(c) => op(&c.to_string(), OpKind::Binary),
            Tok::Number(n) => op(&n, OpKind::Binary),
            Tok::Text(t) => op(&t, OpKind::Binary),
            Tok::End => {
                self.error("operator symbol expected");
                Node::Placeholder
            }
        }
    }

    fn script_slot(&self) -> Option<Slot> {
        match self.peek() {
            Tok::Sym("^") => Some(Slot::Sup),
            Tok::Sym("_") => Some(Slot::Sub),
            Tok::Word(w) => match symbols::keyword(w) {
                Some(Keyword::Script(slot)) => Some(slot),
                _ => None,
            },
            _ => None,
        }
    }

    // power := term (script term)*
    fn power(&mut self) -> Node {
        let base = self.term();
        self.scripts(base)
    }

    /// Attaches any following sub/superscripts to `base`.
    fn scripts(&mut self, base: Node) -> Node {
        let mut node = base;
        let mut slots: [Option<Box<Node>>; 6] = Default::default();
        let mut any = false;
        while let Some(slot) = self.script_slot() {
            self.bump();
            let arg = Box::new(self.term());
            let idx = slot as usize;
            if slots[idx].is_some() {
                // Same slot twice: nest, like OpenOffice does.
                node = Self::build_scripts(node, &mut slots);
            }
            slots[idx] = Some(arg);
            any = true;
        }
        if any {
            node = Self::build_scripts(node, &mut slots);
        }
        node
    }

    fn build_scripts(base: Node, slots: &mut [Option<Box<Node>>; 6]) -> Node {
        let mut node = base;
        let sub = slots[Slot::Sub as usize].take();
        let sup = slots[Slot::Sup as usize].take();
        let lsub = slots[Slot::LSub as usize].take();
        let lsup = slots[Slot::LSup as usize].take();
        let under = slots[Slot::Under as usize].take();
        let over = slots[Slot::Over as usize].take();
        if sub.is_some() || sup.is_some() || lsub.is_some() || lsup.is_some() {
            node = Node::Script { base: Box::new(node), sub, sup, lsub, lsup };
        }
        if under.is_some() || over.is_some() {
            node = Node::UnderOver { base: Box::new(node), under, over };
        }
        node
    }

    fn missing_operand(&mut self) -> Node {
        self.error("operand expected");
        Node::Placeholder
    }

    fn term(&mut self) -> Node {
        if self.depth >= MAX_DEPTH {
            self.error("formula is nested too deeply");
            if !matches!(self.peek(), Tok::End) {
                self.bump();
            }
            return Node::Placeholder;
        }
        self.depth += 1;
        let node = self.term_inner();
        self.depth -= 1;
        node
    }

    fn term_inner(&mut self) -> Node {
        match self.peek().clone() {
            Tok::End => self.missing_operand(),
            Tok::Number(text) => {
                self.bump();
                Node::Number { text }
            }
            Tok::Text(text) => {
                self.bump();
                Node::Text { text }
            }
            Tok::Special(name) => {
                self.bump();
                match symbols::special(&name) {
                    Some((text, italic)) => Node::Ident { text: text.to_owned(), italic },
                    None => {
                        self.error(format!("unknown symbol %{name}"));
                        Node::Text { text: format!("%{name}") }
                    }
                }
            }
            Tok::Escaped(c) => {
                self.bump();
                op(&c.to_string(), OpKind::Ordinary)
            }
            Tok::Char(c) => {
                self.bump();
                Node::Ident { text: c.to_string(), italic: c.is_alphabetic() }
            }
            Tok::Sym(sym) => self.term_sym(sym),
            Tok::Word(word) => match symbols::keyword(&word) {
                None => {
                    self.bump();
                    Node::Ident { text: word, italic: true }
                }
                Some(kw) => self.term_keyword(kw, &word),
            },
        }
    }

    fn term_sym(&mut self, sym: &'static str) -> Node {
        match sym {
            "{" => {
                self.bump();
                let body = self.line();
                if !self.eat_sym("}") {
                    self.error("'}' expected");
                }
                // Keep groups as rows so `{}` can serve as an empty operand.
                match body {
                    row @ Node::Row { .. } => row,
                    other => other,
                }
            }
            "(" => self.bracketed("(", ")"),
            "[" => self.bracketed("[", "]"),
            "+" | "-" | "+-" | "-+" => {
                self.bump();
                let text = match sym {
                    "+" => "+",
                    "-" => "−",
                    "+-" => "±",
                    _ => "∓",
                };
                let operand = self.power();
                Node::Row { items: vec![op(text, OpKind::Prefix), operand] }
            }
            "<?>" => {
                self.bump();
                Node::Placeholder
            }
            "~" => {
                self.bump();
                Node::Space { width: 0.5 }
            }
            "`" => {
                self.bump();
                Node::Space { width: 0.17 }
            }
            "," | ";" | ":" | "." | "!" | "'" | "?" | "@" | "$" => {
                self.bump();
                op(sym, OpKind::Ordinary)
            }
            // Binary operators and closers in operand position: do not consume,
            // the enclosing loop will.
            _ => self.missing_operand(),
        }
    }

    /// `( ... )` and friends with fixed-size brackets.
    fn bracketed(&mut self, open: &str, close: &'static str) -> Node {
        self.bump();
        let body = self.bracket_body();
        let closed = match self.peek() {
            Tok::Sym(s) if *s == close => true,
            Tok::Word(w) => matches!(symbols::keyword(w), Some(Keyword::Close(c)) if c == close),
            _ => false,
        };
        if closed {
            self.bump();
        } else {
            self.error(format!("'{close}' expected"));
        }
        Node::Fenced { open: open.to_owned(), close: close.to_owned(), body: Box::new(body), scalable: false }
    }

    /// Content of a bracket pair; `mline` separators become vertical bars.
    fn bracket_body(&mut self) -> Node {
        let mut items = vec![self.line()];
        while self.peek_keyword() == Some(Keyword::MLine) {
            self.bump();
            items.push(op("|", OpKind::Relation));
            items.push(self.line());
        }
        Node::row(items)
    }

    /// Reads the bracket symbol after `left` / `right`.
    fn any_bracket(&mut self) -> String {
        let text = match self.peek() {
            Tok::Sym(s) if matches!(*s, "(" | ")" | "[" | "]" | "|") => Some((*s).to_owned()),
            Tok::Escaped(c) => Some(c.to_string()),
            Tok::Word(w) => match symbols::keyword(w) {
                Some(Keyword::Open(s) | Keyword::Close(s)) => Some(s.to_owned()),
                Some(Keyword::NoBracket) => Some(String::new()),
                _ => None,
            },
            _ => None,
        };
        match text {
            Some(text) => {
                self.bump();
                text
            }
            None => {
                self.error("bracket expected");
                String::new()
            }
        }
    }

    fn term_keyword(&mut self, kw: Keyword, word: &str) -> Node {
        match kw {
            Keyword::Open(open) => {
                let close = match open {
                    "{" => "}",
                    "⟨" => "⟩",
                    "⌈" => "⌉",
                    "⌊" => "⌋",
                    "|" => "|",
                    "‖" => "‖",
                    _ => "⟧",
                };
                self.bracketed(open, close)
            }
            Keyword::Left => {
                self.bump();
                let open = self.any_bracket();
                let body = self.bracket_body();
                let close = if self.peek_keyword() == Some(Keyword::Right) {
                    self.bump();
                    self.any_bracket()
                } else {
                    self.error("'right' expected");
                    String::new()
                };
                Node::Fenced { open, close, body: Box::new(body), scalable: true }
            }
            Keyword::Func(name) => {
                self.bump();
                Node::Func { name: name.to_owned() }
            }
            Keyword::FuncKw => {
                self.bump();
                match self.bump() {
                    Tok::Word(name) => Node::Func { name },
                    _ => {
                        self.error("function name expected");
                        Node::Placeholder
                    }
                }
            }
            Keyword::BigOp(symbol, limits) => {
                self.bump();
                let base = if symbol.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') {
                    Node::Func { name: symbol.to_owned() }
                } else {
                    Node::BigOp { text: symbol.to_owned() }
                };
                self.big_operator(base, limits)
            }
            Keyword::Oper => {
                self.bump();
                let symbol = match self.operator_symbol() {
                    Node::Op { text, .. } => Node::BigOp { text },
                    other => other,
                };
                self.big_operator(symbol, true)
            }
            Keyword::UOper => {
                self.bump();
                let symbol = match self.operator_symbol() {
                    Node::Op { text, .. } => op(&text, OpKind::Prefix),
                    other => other,
                };
                let operand = self.power();
                Node::Row { items: vec![symbol, operand] }
            }
            Keyword::Neg => {
                self.bump();
                let operand = self.power();
                Node::Row { items: vec![op("¬", OpKind::Prefix), operand] }
            }
            Keyword::Sqrt => {
                self.bump();
                Node::Sqrt { body: Box::new(self.power()) }
            }
            Keyword::NRoot => {
                self.bump();
                let index = Box::new(self.power());
                let body = Box::new(self.power());
                Node::Root { index, body }
            }
            Keyword::Abs => {
                self.bump();
                let body = Box::new(self.power());
                Node::Fenced { open: "|".into(), close: "|".into(), body, scalable: true }
            }
            Keyword::Fact => {
                self.bump();
                let operand = self.power();
                Node::Row { items: vec![operand, op("!", OpKind::Ordinary)] }
            }
            Keyword::Binom => {
                self.bump();
                let top = self.sum();
                let bottom = self.sum();
                Node::Matrix { rows: vec![vec![top], vec![bottom]] }
            }
            Keyword::Stack | Keyword::Matrix => {
                self.bump();
                self.matrix(kw == Keyword::Matrix)
            }
            Keyword::Accent(accent, stretchy, under) => {
                self.bump();
                let body = Box::new(self.power());
                Node::Accent { body, accent: accent.to_owned(), stretchy, under }
            }
            Keyword::Overstrike => {
                self.bump();
                let body = Box::new(self.power());
                Node::Accent { body, accent: "—".to_owned(), stretchy: true, under: false }
            }
            Keyword::Font(attr) => {
                self.bump();
                self.font_attribute(attr)
            }
            Keyword::Symbol(text, italic) => {
                self.bump();
                Node::Ident { text: text.to_owned(), italic }
            }
            Keyword::Align => {
                self.bump();
                self.term()
            }
            Keyword::NoBracket | Keyword::From | Keyword::To => {
                // Out of context: show the word itself rather than losing it.
                self.error(format!("unexpected '{word}'"));
                self.bump();
                Node::Text { text: word.to_owned() }
            }
            // Operators in operand position: leave them for the enclosing loop.
            Keyword::Rel(_)
            | Keyword::Sum(_)
            | Keyword::Prod(_)
            | Keyword::Over
            | Keyword::Brace(_)
            | Keyword::BOper
            | Keyword::Script(_)
            | Keyword::Close(_)
            | Keyword::Right
            | Keyword::MLine
            | Keyword::Newline => self.missing_operand(),
        }
    }

    /// Limits and operand of a large operator.
    fn big_operator(&mut self, symbol: Node, limits_below: bool) -> Node {
        let mut under = None;
        let mut over = None;
        let mut sub = None;
        let mut sup = None;
        loop {
            match self.peek_keyword() {
                Some(Keyword::From) => {
                    self.bump();
                    under = Some(Box::new(self.term()));
                    continue;
                }
                Some(Keyword::To) => {
                    self.bump();
                    over = Some(Box::new(self.term()));
                    continue;
                }
                _ => {}
            }
            match self.script_slot() {
                Some(Slot::Sub) => {
                    self.bump();
                    let arg = Some(Box::new(self.term()));
                    if limits_below { under = arg } else { sub = arg }
                }
                Some(Slot::Sup) => {
                    self.bump();
                    let arg = Some(Box::new(self.term()));
                    if limits_below { over = arg } else { sup = arg }
                }
                Some(Slot::Under) => {
                    self.bump();
                    under = Some(Box::new(self.term()));
                }
                Some(Slot::Over) => {
                    self.bump();
                    over = Some(Box::new(self.term()));
                }
                _ => break,
            }
        }
        let mut node = symbol;
        if sub.is_some() || sup.is_some() {
            node = Node::Script { base: Box::new(node), sub, sup, lsub: None, lsup: None };
        }
        if under.is_some() || over.is_some() {
            node = Node::UnderOver { base: Box::new(node), under, over };
        }
        if is_stopper(self.peek()) {
            return node;
        }
        let operand = self.power();
        Node::Row { items: vec![node, operand] }
    }

    /// `stack{a # b}` and `matrix{a # b ## c # d}`.
    fn matrix(&mut self, two_dimensional: bool) -> Node {
        if !self.eat_sym("{") {
            self.error("'{' expected");
            return Node::Placeholder;
        }
        let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
        loop {
            let cell = self.line();
            if two_dimensional {
                if let Some(row) = rows.last_mut() {
                    row.push(cell);
                }
                if self.eat_sym("##") {
                    rows.push(Vec::new());
                } else if !self.eat_sym("#") {
                    break;
                }
            } else {
                if let Some(row) = rows.last_mut() {
                    row.push(cell);
                }
                if self.eat_sym("#") {
                    rows.push(Vec::new());
                } else {
                    break;
                }
            }
        }
        if !self.eat_sym("}") {
            self.error("'}' expected");
        }
        // Rectangularise so renderers can rely on equal row lengths.
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        for row in &mut rows {
            while row.len() < width {
                row.push(Node::empty());
            }
        }
        Node::Matrix { rows }
    }

    fn font_attribute(&mut self, attr: FontAttr) -> Node {
        let mut bold = None;
        let mut italic = None;
        let mut color = None;
        let mut size = None;
        let mut font = None;
        match attr {
            FontAttr::Bold => bold = Some(true),
            FontAttr::NoBold => bold = Some(false),
            FontAttr::Italic => italic = Some(true),
            FontAttr::NoItalic => italic = Some(false),
            FontAttr::Phantom => {
                return Node::Phantom { body: Box::new(self.power()) };
            }
            FontAttr::Color => match self.peek().clone() {
                Tok::Word(name) => {
                    self.bump();
                    match symbols::color(&name) {
                        Some(hex) => color = Some(hex.to_owned()),
                        None => self.error(format!("unknown colour '{name}'")),
                    }
                }
                _ => self.error("colour name expected"),
            },
            FontAttr::Font => match self.peek().clone() {
                Tok::Word(name) if matches!(name.as_str(), "serif" | "sans" | "fixed") => {
                    self.bump();
                    font = Some(name);
                }
                _ => self.error("font name expected (serif, sans or fixed)"),
            },
            FontAttr::Size => {
                let mode = match self.peek() {
                    Tok::Sym(s @ ("+" | "-" | "*" | "/")) => Some(*s),
                    _ => None,
                };
                if mode.is_some() {
                    self.bump();
                }
                match self.peek().clone() {
                    Tok::Number(text) => {
                        self.bump();
                        let value: f64 = text.parse().unwrap_or(0.0);
                        size = Some(match mode {
                            None => Size::Abs(value.clamp(1.0, 500.0)),
                            Some("+") => Size::Add(value.min(500.0)),
                            Some("-") => Size::Add(-value.min(500.0)),
                            Some("*") => Size::Mul(value.clamp(0.05, 50.0)),
                            _ => Size::Mul(if value > 0.0 { (1.0 / value).clamp(0.05, 50.0) } else { 1.0 }),
                        });
                    }
                    _ => self.error("size expected"),
                }
            }
        }
        let body = Box::new(self.power());
        Node::Styled { body, bold, italic, color, size, font }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Node {
        let parsed = parse(src);
        assert!(parsed.errors.is_empty(), "{src:?} gave errors {:?}", parsed.errors);
        parsed.root
    }

    fn ident(s: &str) -> Node {
        Node::Ident { text: s.into(), italic: true }
    }

    fn num(s: &str) -> Node {
        Node::Number { text: s.into() }
    }

    #[test]
    fn precedence_of_sum_and_product() {
        assert_eq!(
            ok("a + b * c"),
            Node::Row {
                items: vec![
                    ident("a"),
                    op("+", OpKind::Binary),
                    Node::Row { items: vec![ident("b"), op("∗", OpKind::Binary), ident("c")] },
                ]
            }
        );
    }

    #[test]
    fn over_builds_a_fraction_of_adjacent_powers() {
        assert_eq!(
            ok("a + b over c"),
            Node::Row {
                items: vec![
                    ident("a"),
                    op("+", OpKind::Binary),
                    Node::Frac { num: Box::new(ident("b")), den: Box::new(ident("c")) },
                ]
            }
        );
        assert_eq!(
            ok("{a + b} over c"),
            Node::Frac {
                num: Box::new(Node::Row { items: vec![ident("a"), op("+", OpKind::Binary), ident("b")] }),
                den: Box::new(ident("c")),
            }
        );
    }

    #[test]
    fn scripts() {
        assert_eq!(
            ok("x_1^2"),
            Node::Script {
                base: Box::new(ident("x")),
                sub: Some(Box::new(num("1"))),
                sup: Some(Box::new(num("2"))),
                lsub: None,
                lsup: None
            }
        );
        // A repeated slot nests.
        match ok("x^2^3") {
            Node::Script { base, sup, .. } => {
                assert!(matches!(*base, Node::Script { .. }));
                assert_eq!(sup, Some(Box::new(num("3"))));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(ok("x csub a csup b"), Node::UnderOver { under: Some(_), over: Some(_), .. }));
        assert!(matches!(ok("x lsub a lsup b"), Node::Script { lsub: Some(_), lsup: Some(_), .. }));
    }

    #[test]
    fn big_operators_take_limits_and_operand() {
        match ok("sum from {i=1} to n i^2") {
            Node::Row { items } => {
                assert!(matches!(&items[0], Node::UnderOver { under: Some(_), over: Some(_), .. }));
                assert!(matches!(&items[1], Node::Script { .. }));
            }
            other => panic!("unexpected {other:?}"),
        }
        // Integrals put ^/_ at the side, sums below/above.
        match ok("int_0^1 x") {
            Node::Row { items } => assert!(matches!(&items[0], Node::Script { .. })),
            other => panic!("unexpected {other:?}"),
        }
        match ok("sum_0^1 x") {
            Node::Row { items } => assert!(matches!(&items[0], Node::UnderOver { .. })),
            other => panic!("unexpected {other:?}"),
        }
        match ok("lim from {x -> 0} x") {
            Node::Row { items } => match &items[0] {
                Node::UnderOver { base, .. } => assert_eq!(**base, Node::Func { name: "lim".into() }),
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn brackets() {
        assert_eq!(
            ok("(a)"),
            Node::Fenced { open: "(".into(), close: ")".into(), body: Box::new(ident("a")), scalable: false }
        );
        assert!(matches!(
            ok("left lbrace a right none"),
            Node::Fenced { ref open, ref close, scalable: true, .. } if open == "{" && close.is_empty()
        ));
        assert!(matches!(ok("lline a rline"), Node::Fenced { .. }));
        assert!(matches!(ok("abs{a}"), Node::Fenced { scalable: true, .. }));
        assert!(matches!(ok("langle a mline b rangle"), Node::Fenced { .. }));
    }

    #[test]
    fn roots_and_matrices() {
        assert_eq!(ok("sqrt x"), Node::Sqrt { body: Box::new(ident("x")) });
        assert_eq!(ok("nroot 3 x"), Node::Root { index: Box::new(num("3")), body: Box::new(ident("x")) });
        match ok("matrix{a # b ## c # d}") {
            Node::Matrix { rows } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec![ident("a"), ident("b")]);
                assert_eq!(rows[1], vec![ident("c"), ident("d")]);
            }
            other => panic!("unexpected {other:?}"),
        }
        match ok("matrix{a # b ## c}") {
            Node::Matrix { rows } => assert_eq!(rows[1].len(), 2, "ragged rows are padded"),
            other => panic!("unexpected {other:?}"),
        }
        match ok("stack{a # b # c}") {
            Node::Matrix { rows } => assert_eq!(rows.len(), 3),
            other => panic!("unexpected {other:?}"),
        }
        match ok("binom n k") {
            Node::Matrix { rows } => assert_eq!(rows, vec![vec![ident("n")], vec![ident("k")]]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn attributes() {
        assert!(matches!(ok("bold x"), Node::Styled { bold: Some(true), .. }));
        assert!(matches!(ok("color red x"), Node::Styled { color: Some(ref c), .. } if c == "#ff0000"));
        assert!(matches!(ok("size 20 x"), Node::Styled { size: Some(Size::Abs(v)), .. } if v == 20.0));
        assert!(matches!(ok("size *2 x"), Node::Styled { size: Some(Size::Mul(v)), .. } if v == 2.0));
        assert!(matches!(ok("size /2 x"), Node::Styled { size: Some(Size::Mul(v)), .. } if v == 0.5));
        assert!(matches!(ok("size -2 x"), Node::Styled { size: Some(Size::Add(v)), .. } if v == -2.0));
        assert!(matches!(ok("font sans x"), Node::Styled { font: Some(ref f), .. } if f == "sans"));
        assert!(matches!(ok("phantom x"), Node::Phantom { .. }));
        assert!(matches!(ok("vec a"), Node::Accent { stretchy: false, under: false, .. }));
        assert!(matches!(ok("underline a"), Node::Accent { stretchy: true, under: true, .. }));
    }

    #[test]
    fn symbols_text_and_lines() {
        assert_eq!(ok("%pi"), Node::Ident { text: "π".into(), italic: false });
        assert_eq!(ok("%ipi"), Node::Ident { text: "π".into(), italic: true });
        assert_eq!(ok("infinity"), Node::Ident { text: "∞".into(), italic: false });
        assert_eq!(ok("\"hello world\""), Node::Text { text: "hello world".into() });
        assert!(matches!(ok("a newline b"), Node::Lines { ref rows } if rows.len() == 2));
        assert_eq!(ok(""), Node::Row { items: vec![] });
        assert_eq!(ok("{}"), Node::Row { items: vec![] });
        assert_eq!(ok("<?>"), Node::Placeholder);
    }

    #[test]
    fn unary_operators() {
        assert_eq!(ok("-a"), Node::Row { items: vec![op("−", OpKind::Prefix), ident("a")] });
        assert!(matches!(ok("neg a"), Node::Row { .. }));
        assert!(matches!(ok("a = -b"), Node::Row { .. }));
    }

    #[test]
    fn errors_are_reported_but_parsing_continues() {
        for src in [
            "a +", "* b", "a over", "{a", "a}", "(a", "left( a", "sqrt", "x^", "matrix a", "color x",
            "size x", "%nosuch", "a right)", "from", "= =", "a # b", "func 1", ")", "}}}", "left",
        ] {
            let parsed = parse(src);
            assert!(!parsed.errors.is_empty(), "{src:?} should report an error");
        }
        // Missing operands become placeholders.
        match parse("a +").root {
            Node::Row { items } => assert_eq!(items[2], Node::Placeholder),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hostile_input_is_bounded() {
        let deep = "{".repeat(100_000);
        let parsed = parse(&deep);
        assert!(!parsed.errors.is_empty());
        assert!(parsed.errors.len() <= 100);
        let deep = "sqrt ".repeat(50_000);
        assert!(!parse(&deep).errors.is_empty());
        let deep = "-".repeat(50_000);
        assert!(!parse(&deep).errors.is_empty());
        let big = "x".repeat(MAX_SOURCE_LEN + 1);
        assert_eq!(parse(&big).root, Node::Placeholder);
    }
}
