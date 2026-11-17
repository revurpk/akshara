//! A small, strict XML reader and writer tailored to OpenDocument.
//!
//! # Why not a general purpose XML crate?
//!
//! Documents are attacker controlled input, and the classic XML attacks
//! (entity expansion "billion laughs", external entities / XXE, pathological
//! nesting) all come from features OpenDocument never uses. This parser simply
//! does not implement them:
//!
//! * `<!DOCTYPE ...>` and every other `<!...>` declaration is **rejected**, so
//!   there are no custom entities and no external references at all;
//! * only the five predefined entities and numeric character references are
//!   understood;
//! * parsing is iterative (no recursion), and depth, node count and attribute
//!   count are bounded by [`Limits`];
//! * only UTF-8 input is accepted.
//!
//! # Namespace normalisation
//!
//! Qualified names are resolved against the in-scope `xmlns` declarations and
//! rewritten to the conventional prefix from [`crate::ns`], e.g. `text:p`,
//! regardless of the prefix used in the file. Names in unknown namespaces are
//! rewritten to `{uri}local`. `xmlns` attributes themselves are dropped.

use crate::error::{OdfError, Result};
use crate::limits::Limits;
use crate::ns;
use std::borrow::Cow;

/// A child of an [`Element`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A nested element.
    Element(Element),
    /// Character data (entities already decoded).
    Text(String),
}

/// An XML element with namespace-normalised names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    /// Normalised qualified name, e.g. `text:p`.
    pub name: String,
    /// Attributes in document order, names normalised like element names.
    pub attrs: Vec<(String, String)>,
    /// Child nodes in document order.
    pub children: Vec<Node>,
}

impl Element {
    /// Creates an empty element.
    pub fn new(name: impl Into<String>) -> Self {
        Element { name: name.into(), attrs: Vec::new(), children: Vec::new() }
    }

    /// Builder-style attribute setter.
    pub fn with_attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((name.into(), value.into()));
        self
    }

    /// Builder-style child appender.
    pub fn with_child(mut self, child: Element) -> Self {
        self.children.push(Node::Element(child));
        self
    }

    /// Builder-style text appender.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.children.push(Node::Text(text.into()));
        self
    }

    /// Returns the value of the attribute `name`, if present.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Iterates over the child elements (text nodes are skipped).
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// Returns the first child element called `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// Iterates over the child elements called `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// Follows a path of child element names.
    pub fn descend(&self, path: &[&str]) -> Option<&Element> {
        let mut cur = self;
        for name in path {
            cur = cur.child(name)?;
        }
        Some(cur)
    }

    /// Concatenates all text below this element.
    ///
    /// Recursion depth is bounded by [`Limits::max_xml_depth`] for any element
    /// produced by [`parse`].
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.collect_text(out),
            }
        }
    }

    /// Serialises the element (and its subtree) through `w`.
    pub fn write_to(&self, w: &mut XmlWriter) {
        w.start(&self.name);
        for (k, v) in &self.attrs {
            w.attr(k, v);
        }
        for child in &self.children {
            match child {
                Node::Text(t) => {
                    w.text(t);
                }
                Node::Element(e) => e.write_to(w),
            }
        }
        w.end();
    }

    /// Serialises the element as a standalone fragment without XML declaration.
    pub fn to_xml_fragment(&self) -> String {
        let mut w = XmlWriter::fragment();
        self.write_to(&mut w);
        w.finish()
    }
}

/// Elements whose whitespace-only text children are significant.
///
/// Everywhere else whitespace-only text is formatting noise (pretty printing)
/// and is dropped to keep the tree small.
fn is_mixed_content(name: &str) -> bool {
    matches!(
        name,
        "text:p" | "text:h" | "text:span" | "text:a" | "text:meta" | "text:ruby-base"
    ) || name.starts_with("math:")
        || name.starts_with("dc:")
        || name.starts_with("meta:")
}

/// Parses a UTF-8 byte stream into an element tree.
///
/// A UTF-8 byte order mark is accepted; UTF-16 input is rejected.
pub fn parse_bytes(bytes: &[u8], limits: &Limits) -> Result<Element> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(OdfError::unsupported("UTF-16 encoded XML"));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| OdfError::Xml {
        message: "stream is not valid UTF-8".into(),
        offset: e.valid_up_to(),
    })?;
    parse(text, limits)
}

/// Parses an XML document into an element tree. See the module docs for the
/// accepted subset.
pub fn parse(src: &str, limits: &Limits) -> Result<Element> {
    Parser { src, bytes: src.as_bytes(), pos: 0, limits, nodes: 0, bindings: Vec::new() }.run()
}

struct Frame<'a> {
    elem: Element,
    raw_name: &'a str,
    ns_mark: usize,
}

struct Parser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    limits: &'a Limits,
    nodes: usize,
    /// In-scope namespace bindings, innermost last: `(prefix, uri)`.
    bindings: Vec<(&'a str, String)>,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

impl<'a> Parser<'a> {
    fn err<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(OdfError::Xml { message: message.into(), offset: self.pos })
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.bytes[self.pos..].starts_with(pat.as_bytes())
    }

    /// Finds `pat` at or after `self.pos + skip`, returning its absolute offset.
    fn find(&self, pat: &str, skip: usize) -> Option<usize> {
        let from = self.pos + skip;
        self.src.get(from..)?.find(pat).map(|i| i + from)
    }

    fn count_node(&mut self) -> Result<()> {
        self.nodes += 1;
        if self.nodes > self.limits.max_xml_nodes {
            return Err(OdfError::limit(format!(
                "XML stream has more than {} nodes",
                self.limits.max_xml_nodes
            )));
        }
        Ok(())
    }

    fn run(mut self) -> Result<Element> {
        self.check_declaration()?;
        let mut stack: Vec<Frame<'a>> = Vec::new();
        let mut root: Option<Element> = None;

        while self.pos < self.bytes.len() {
            if self.bytes[self.pos] != b'<' {
                let end = self.find("<", 0).unwrap_or(self.bytes.len());
                let raw = &self.src[self.pos..end];
                match stack.last_mut() {
                    None => {
                        if !raw.bytes().all(is_ws) {
                            return self.err("text outside of the root element");
                        }
                    }
                    Some(frame) => {
                        let text = decode_entities(raw, self.pos, false)?;
                        Self::append_text(&mut frame.elem, &text, &mut self.nodes, self.limits)?;
                    }
                }
                self.pos = end;
            } else if self.starts_with("<!--") {
                match self.find("-->", 4) {
                    Some(end) => self.pos = end + 3,
                    None => return self.err("unterminated comment"),
                }
            } else if self.starts_with("<![CDATA[") {
                let Some(end) = self.find("]]>", 9) else {
                    return self.err("unterminated CDATA section");
                };
                let raw = &self.src[self.pos + 9..end];
                let Some(frame) = stack.last_mut() else {
                    return self.err("CDATA outside of the root element");
                };
                if !raw.chars().all(is_xml_char) {
                    return self.err("invalid character in CDATA section");
                }
                Self::append_text(&mut frame.elem, raw, &mut self.nodes, self.limits)?;
                self.pos = end + 3;
            } else if self.starts_with("<!") {
                return self.err("DTDs and markup declarations are not allowed");
            } else if self.starts_with("<?") {
                match self.find("?>", 2) {
                    Some(end) => self.pos = end + 2,
                    None => return self.err("unterminated processing instruction"),
                }
            } else if self.starts_with("</") {
                let Some(end) = self.find(">", 2) else {
                    return self.err("unterminated end tag");
                };
                let raw_name = self.src[self.pos + 2..end].trim_end_matches([' ', '\t', '\n', '\r']);
                let Some(frame) = stack.pop() else {
                    return self.err("unexpected end tag");
                };
                if frame.raw_name != raw_name {
                    return self.err(format!(
                        "end tag </{raw_name}> does not match <{}>",
                        frame.raw_name
                    ));
                }
                self.bindings.truncate(frame.ns_mark);
                self.pos = end + 1;
                match stack.last_mut() {
                    Some(parent) => parent.elem.children.push(Node::Element(frame.elem)),
                    None => root = Some(frame.elem),
                }
            } else {
                if root.is_some() {
                    return self.err("more than one root element");
                }
                if stack.len() >= self.limits.max_xml_depth {
                    return Err(OdfError::limit(format!(
                        "XML nesting deeper than {}",
                        self.limits.max_xml_depth
                    )));
                }
                self.count_node()?;
                let ns_mark = self.bindings.len();
                let (raw_name, elem, self_closing) = self.parse_start_tag()?;
                if self_closing {
                    self.bindings.truncate(ns_mark);
                    match stack.last_mut() {
                        Some(parent) => parent.elem.children.push(Node::Element(elem)),
                        None => root = Some(elem),
                    }
                } else {
                    stack.push(Frame { elem, raw_name, ns_mark });
                }
            }
        }

        if let Some(frame) = stack.last() {
            return self.err(format!("unexpected end of input inside <{}>", frame.raw_name));
        }
        match root {
            Some(root) => Ok(root),
            None => self.err("no root element"),
        }
    }

    fn append_text(elem: &mut Element, text: &str, nodes: &mut usize, limits: &Limits) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if let Some(Node::Text(last)) = elem.children.last_mut() {
            last.push_str(text);
            return Ok(());
        }
        if text.bytes().all(is_ws) && !is_mixed_content(&elem.name) {
            return Ok(());
        }
        *nodes += 1;
        if *nodes > limits.max_xml_nodes {
            return Err(OdfError::limit(format!(
                "XML stream has more than {} nodes",
                limits.max_xml_nodes
            )));
        }
        elem.children.push(Node::Text(text.to_owned()));
        Ok(())
    }

    /// Validates the optional `<?xml ...?>` declaration (encoding must be UTF-8).
    fn check_declaration(&mut self) -> Result<()> {
        if !self.starts_with("<?xml") || !self.bytes.get(5).copied().is_some_and(is_ws) {
            return Ok(());
        }
        let Some(end) = self.find("?>", 0) else {
            return self.err("unterminated XML declaration");
        };
        let decl = &self.src[..end];
        if let Some(idx) = decl.find("encoding") {
            let rest = decl[idx + 8..].trim_start().trim_start_matches('=').trim_start();
            let quote = rest.chars().next().unwrap_or(' ');
            let value = rest
                .get(1..)
                .and_then(|r| r.split(quote).next())
                .unwrap_or("")
                .to_ascii_lowercase();
            if (quote != '"' && quote != '\'') || !matches!(value.as_str(), "utf-8" | "utf8" | "us-ascii") {
                return Err(OdfError::unsupported(format!("XML encoding {value:?}")));
            }
        }
        self.pos = end + 2;
        Ok(())
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && is_ws(self.bytes[self.pos]) {
            self.pos += 1;
        }
    }

    /// Reads a name token ending at whitespace, `/`, `>` or `=`.
    fn read_name(&mut self) -> Result<&'a str> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if is_ws(b) || matches!(b, b'/' | b'>' | b'=') {
                break;
            }
            if matches!(b, b'<' | b'&' | b'"' | b'\'') || b < 0x20 {
                return self.err("invalid character in name");
            }
            self.pos += 1;
        }
        if self.pos == start {
            return self.err("expected a name");
        }
        Ok(&self.src[start..self.pos])
    }

    /// Parses `<name attr="v" ...>` or `<name ... />`; `self.pos` is at `<`.
    fn parse_start_tag(&mut self) -> Result<(&'a str, Element, bool)> {
        self.pos += 1;
        let raw_name = self.read_name()?;
        let mut raw_attrs: Vec<(&'a str, String)> = Vec::new();
        let self_closing;
        loop {
            self.skip_ws();
            match self.bytes.get(self.pos) {
                None => return self.err("unterminated start tag"),
                Some(b'>') => {
                    self.pos += 1;
                    self_closing = false;
                    break;
                }
                Some(b'/') => {
                    if self.bytes.get(self.pos + 1) != Some(&b'>') {
                        return self.err("expected '>' after '/'");
                    }
                    self.pos += 2;
                    self_closing = true;
                    break;
                }
                Some(_) => {}
            }
            let key = self.read_name()?;
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b'=') {
                return self.err(format!("attribute {key:?} has no value"));
            }
            self.pos += 1;
            self.skip_ws();
            let quote = match self.bytes.get(self.pos) {
                Some(&q @ (b'"' | b'\'')) => q,
                _ => return self.err("attribute value must be quoted"),
            };
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos] != quote {
                if self.bytes[self.pos] == b'<' {
                    return self.err("'<' in attribute value");
                }
                self.pos += 1;
            }
            if self.pos >= self.bytes.len() {
                return self.err("unterminated attribute value");
            }
            let value = decode_entities(&self.src[start..self.pos], start, true)?.into_owned();
            self.pos += 1;
            if raw_attrs.iter().any(|(k, _)| *k == key) {
                return self.err(format!("duplicate attribute {key:?}"));
            }
            if raw_attrs.len() >= self.limits.max_xml_attributes {
                return Err(OdfError::limit(format!(
                    "element has more than {} attributes",
                    self.limits.max_xml_attributes
                )));
            }
            raw_attrs.push((key, value));
        }

        // Namespace declarations first: they apply to the element itself too.
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        let mut plain: Vec<(&'a str, String)> = Vec::with_capacity(raw_attrs.len());
        for (key, value) in raw_attrs {
            if key == "xmlns" {
                self.bindings.push(("", value));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                if prefix.is_empty() || value.is_empty() {
                    return self.err("invalid namespace declaration");
                }
                self.bindings.push((prefix, value));
            } else {
                plain.push((key, value));
            }
        }
        let name = self.resolve(raw_name, false)?;
        for (key, value) in plain {
            attrs.push((self.resolve(key, true)?, value));
        }
        Ok((raw_name, Element { name, attrs, children: Vec::new() }, self_closing))
    }

    fn lookup(&self, prefix: &str) -> Option<&str> {
        self.bindings.iter().rev().find(|(p, _)| *p == prefix).map(|(_, u)| u.as_str())
    }

    /// Rewrites a raw qualified name to its normalised form.
    fn resolve(&self, raw: &str, is_attr: bool) -> Result<String> {
        let (prefix, local) = match raw.split_once(':') {
            Some((p, l)) => (p, l),
            None => ("", raw),
        };
        if local.is_empty() || local.contains(':') {
            return self.err(format!("invalid qualified name {raw:?}"));
        }
        if prefix == "xml" {
            return Ok(raw.to_owned());
        }
        let uri = if prefix.is_empty() {
            if is_attr {
                return Ok(raw.to_owned());
            }
            match self.lookup("") {
                Some(uri) if !uri.is_empty() => uri,
                _ => return Ok(raw.to_owned()),
            }
        } else {
            match self.lookup(prefix) {
                Some(uri) => uri,
                None => return self.err(format!("undeclared namespace prefix {prefix:?}")),
            }
        };
        Ok(match ns::canonical_prefix(uri) {
            Some(canonical) => format!("{canonical}:{local}"),
            None => format!("{{{uri}}}{local}"),
        })
    }
}

/// Decodes the predefined entities and numeric character references.
///
/// `attr` enables attribute-value normalisation (literal tabs and newlines
/// become spaces).
fn decode_entities(raw: &str, base: usize, attr: bool) -> Result<Cow<'_, str>> {
    let needs_work = raw.bytes().any(|b| b == b'&' || b < 0x20 || b == 0xEF);
    if !needs_work {
        return Ok(Cow::Borrowed(raw));
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    let mut offset = base;
    while let Some(c) = rest.chars().next() {
        if c == '&' {
            let Some(end) = rest.find(';') else {
                return Err(OdfError::Xml { message: "unterminated entity reference".into(), offset });
            };
            let name = &rest[1..end];
            let decoded = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let code = if let Some(hex) = name.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16).ok()
                    } else if let Some(dec) = name.strip_prefix('#') {
                        dec.parse::<u32>().ok()
                    } else {
                        return Err(OdfError::Xml {
                            message: format!("unknown entity &{};", truncate(name, 32)),
                            offset,
                        });
                    };
                    match code.and_then(char::from_u32).filter(|c| is_xml_char(*c)) {
                        Some(c) => c,
                        None => {
                            return Err(OdfError::Xml {
                                message: "invalid character reference".into(),
                                offset,
                            })
                        }
                    }
                }
            };
            out.push(decoded);
            rest = &rest[end + 1..];
            offset += end + 1;
        } else {
            if !is_xml_char(c) {
                return Err(OdfError::Xml { message: "invalid character".into(), offset });
            }
            if attr && matches!(c, '\t' | '\n' | '\r') {
                out.push(' ');
            } else if c == '\r' {
                // XML line-end normalisation: CRLF and lone CR become LF.
                out.push('\n');
                if rest[1..].starts_with('\n') {
                    rest = &rest[1..];
                    offset += 1;
                }
            } else {
                out.push(c);
            }
            rest = &rest[c.len_utf8()..];
            offset += c.len_utf8();
        }
    }
    Ok(Cow::Owned(out))
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Streaming XML writer that always produces well-formed output.
///
/// Text and attribute values are escaped, and characters that are not legal in
/// XML 1.0 (most C0 control characters) are silently dropped, so arbitrary
/// user text can never corrupt a saved document.
#[derive(Debug)]
pub struct XmlWriter {
    out: String,
    stack: Vec<String>,
    tag_open: bool,
}

impl Default for XmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlWriter {
    /// Creates a writer that starts with the XML declaration.
    pub fn new() -> Self {
        XmlWriter {
            out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
            stack: Vec::new(),
            tag_open: false,
        }
    }

    /// Creates a writer without an XML declaration.
    pub fn fragment() -> Self {
        XmlWriter { out: String::new(), stack: Vec::new(), tag_open: false }
    }

    fn close_pending(&mut self) {
        if self.tag_open {
            self.out.push('>');
            self.tag_open = false;
        }
    }

    /// Opens an element. Attributes may be added until the next call that
    /// produces content.
    pub fn start(&mut self, name: &str) -> &mut Self {
        self.close_pending();
        self.out.push('<');
        self.out.push_str(name);
        self.stack.push(name.to_owned());
        self.tag_open = true;
        self
    }

    /// Adds an attribute to the element opened by the last [`XmlWriter::start`].
    ///
    /// Calling this after content has been written is a programming error; the
    /// attribute is ignored in release builds.
    pub fn attr(&mut self, name: &str, value: &str) -> &mut Self {
        debug_assert!(self.tag_open, "attr() called outside of a start tag");
        if self.tag_open {
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value, true);
            self.out.push('"');
        }
        self
    }

    /// Writes the standard namespace declarations for the given prefixes.
    pub fn namespaces(&mut self, prefixes: &[&str]) -> &mut Self {
        for prefix in prefixes {
            if let Some(uri) = ns::uri_for_prefix(prefix) {
                self.attr(&format!("xmlns:{prefix}"), uri);
            }
        }
        self
    }

    /// Writes escaped character data.
    pub fn text(&mut self, text: &str) -> &mut Self {
        if !text.is_empty() {
            self.close_pending();
            escape_into(&mut self.out, text, false);
        }
        self
    }

    /// Closes the innermost open element.
    pub fn end(&mut self) -> &mut Self {
        if let Some(name) = self.stack.pop() {
            if self.tag_open {
                self.out.push_str("/>");
                self.tag_open = false;
            } else {
                self.out.push_str("</");
                self.out.push_str(&name);
                self.out.push('>');
            }
        }
        self
    }

    /// Convenience: `<name>text</name>`.
    pub fn element_text(&mut self, name: &str, text: &str) -> &mut Self {
        self.start(name).text(text).end()
    }

    /// Closes any elements still open and returns the document.
    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.end();
        }
        self.out
    }
}

fn escape_into(out: &mut String, text: &str, attr: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\t' if attr => out.push_str("&#9;"),
            '\n' if attr => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c if is_xml_char(c) => out.push(c),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Element> {
        parse(s, &Limits::default())
    }

    const TEXT_NS: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";

    #[test]
    fn parses_simple_document() {
        let e = p(r#"<?xml version="1.0" encoding="UTF-8"?><a x="1" y='two'><b/>hi<c>there</c></a>"#)
            .expect("parse");
        assert_eq!(e.name, "a");
        assert_eq!(e.attr("x"), Some("1"));
        assert_eq!(e.attr("y"), Some("two"));
        assert_eq!(e.elements().count(), 2);
        assert_eq!(e.text_content(), "hithere");
        assert_eq!(e.child("c").map(Element::text_content).as_deref(), Some("there"));
    }

    #[test]
    fn normalises_namespace_prefixes() {
        let src = format!(r#"<t:p xmlns:t="{TEXT_NS}" t:style-name="P1"><t:span>x</t:span></t:p>"#);
        let e = p(&src).expect("parse");
        assert_eq!(e.name, "text:p");
        assert_eq!(e.attr("text:style-name"), Some("P1"));
        assert!(e.child("text:span").is_some());
        assert!(e.attrs.iter().all(|(k, _)| !k.starts_with("xmlns")));
    }

    #[test]
    fn default_namespace_applies_to_elements_only() {
        let e = p(r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><mi>x</mi></math>"#)
            .expect("parse");
        assert_eq!(e.name, "math:math");
        assert_eq!(e.attr("display"), Some("block"));
        assert!(e.child("math:mi").is_some());
    }

    #[test]
    fn unknown_namespace_uses_clark_notation() {
        let e = p(r#"<text:p xmlns:text="urn:evil"/>"#).expect("parse");
        assert_eq!(e.name, "{urn:evil}p");
    }

    #[test]
    fn namespace_scopes_end_with_their_element() {
        let src = format!(r#"<r><a xmlns:t="{TEXT_NS}"><t:p/></a><t:p/></r>"#);
        assert!(p(&src).is_err());
    }

    #[test]
    fn decodes_entities_and_cdata() {
        let e = p("<a t=\"&lt;&amp;&#65;&#x42;\">&lt;x&gt; &amp; &quot;&apos;<![CDATA[<raw&>]]></a>").expect("parse");
        assert_eq!(e.attr("t"), Some("<&AB"));
        assert_eq!(e.text_content(), "<x> & \"'<raw&>");
    }

    #[test]
    fn rejects_doctype_and_entities() {
        let bomb = r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY lol "lol">]><lolz>&lol;</lolz>"#;
        assert!(matches!(p(bomb), Err(OdfError::Xml { .. })));
        assert!(p("<a>&unknown;</a>").is_err());
        assert!(p("<a>&#0;</a>").is_err());
        assert!(p("<a>&#xD800;</a>").is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "", "   ", "<a>", "<a></b>", "</a>", "<a/><b/>", "text<a/>", "<a x=1/>", "<a x='1' x='2'/>",
            "<a x='<'/>", "<a><!-- oops</a>", "<a b/>", "<a:b/>", "<a><![CDATA[x</a>", "<a/>trailing",
            "<a", "<a x='1", "<>", "< a/>",
        ] {
            assert!(p(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_non_utf8_encodings() {
        assert!(matches!(
            p(r#"<?xml version="1.0" encoding="ISO-8859-1"?><a/>"#),
            Err(OdfError::Unsupported(_))
        ));
        assert!(matches!(parse_bytes(&[0xFF, 0xFE, b'<', 0], &Limits::default()), Err(OdfError::Unsupported(_))));
        assert!(parse_bytes(b"<a>\xff</a>", &Limits::default()).is_err());
        assert!(parse_bytes(b"\xEF\xBB\xBF<a/>", &Limits::default()).is_ok());
    }

    #[test]
    fn enforces_depth_limit_without_overflowing_the_stack() {
        let depth = 200_000;
        let src = "<a>".repeat(depth) + &"</a>".repeat(depth);
        assert!(matches!(p(&src), Err(OdfError::Limit(_))));
        let limits = Limits { max_xml_depth: 8, ..Limits::default() };
        assert!(parse(&("<a>".repeat(8) + &"</a>".repeat(8)), &limits).is_ok());
        assert!(parse(&("<a>".repeat(9) + &"</a>".repeat(9)), &limits).is_err());
    }

    #[test]
    fn enforces_node_and_attribute_limits() {
        let limits = Limits { max_xml_nodes: 10, max_xml_attributes: 2, ..Limits::default() };
        assert!(parse(&format!("<r>{}</r>", "<a/>".repeat(9)), &limits).is_ok());
        assert!(matches!(parse(&format!("<r>{}</r>", "<a/>".repeat(10)), &limits), Err(OdfError::Limit(_))));
        assert!(parse("<a x='1' y='2'/>", &limits).is_ok());
        assert!(matches!(parse("<a x='1' y='2' z='3'/>", &limits), Err(OdfError::Limit(_))));
    }

    #[test]
    fn drops_insignificant_whitespace_only() {
        let src = format!(
            "<r xmlns:text=\"{TEXT_NS}\">\n  <text:p> <text:span>a</text:span> <text:span>b</text:span></text:p>\n</r>"
        );
        let e = p(&src).expect("parse");
        assert_eq!(e.children.len(), 1);
        assert_eq!(e.text_content(), " a b");
    }

    #[test]
    fn writer_escapes_and_round_trips() {
        let mut w = XmlWriter::new();
        w.start("root").attr("a", "x<y>&\"z\"\n").text("1 < 2 & 3 > 2\u{1}").start("empty").end();
        let xml = w.finish();
        assert!(xml.contains("<empty/>"));
        assert!(!xml.contains('\u{1}'));
        let e = p(&xml).expect("round trip");
        assert_eq!(e.attr("a"), Some("x<y>&\"z\"\n"));
        assert_eq!(e.text_content(), "1 < 2 & 3 > 2");
    }

    #[test]
    fn element_serialisation_round_trips() {
        let e = Element::new("a")
            .with_attr("k", "v&")
            .with_child(Element::new("b").with_text("t<"))
            .with_text("tail");
        assert_eq!(p(&e.to_xml_fragment()).expect("parse"), e);
    }

    #[test]
    fn normalises_line_endings() {
        let e = p("<a>l1\r\nl2\rl3</a>").expect("parse");
        assert_eq!(e.text_content(), "l1\nl2\nl3");
    }
}
