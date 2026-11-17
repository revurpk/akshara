//! # office-math
//!
//! The core of Akshara Math, a formula editor compatible with OpenOffice
//! Math.
//!
//! Formulas are written in the *StarMath* command language (`a over b`,
//! `sum from {i=1} to n i^2`, `%alpha`...). This crate provides:
//!
//! * [`parse`]: a total, panic-free parser producing a presentation tree
//!   ([`Node`]) plus a list of problems, suitable for live preview;
//! * [`to_mathml`] / [`mathml_to_starmath`]: MathML export and import;
//! * [`Formula`]: reading and writing OpenDocument formula files (`.odf`) and
//!   standalone MathML (`.mml`).
//!
//! The crate contains no `unsafe` code and bounds input size and nesting.
//!
//! ```
//! use office_math::{parse, Formula, Node};
//!
//! let parsed = parse("x = {-b +- sqrt{b^2 - 4 a c}} over {2 a}");
//! assert!(parsed.errors.is_empty());
//!
//! let formula = Formula::new("E = m c^2");
//! let bytes = formula.to_odf_bytes().unwrap();
//! let reread = Formula::from_bytes(&bytes, &Default::default()).unwrap();
//! assert_eq!(reread.source, "E = m c^2");
//! # let _ : Node = parsed.root;
//! ```

pub mod ast;
pub mod lexer;
pub mod mathml;
pub mod parser;
pub mod symbols;

pub use ast::{Node, OpKind, ParseError, Parsed, Size};
pub use mathml::{mathml_to_starmath, to_mathml};
pub use parser::{parse, MAX_DEPTH, MAX_SOURCE_LEN};

use office_odf::{ns, xml, Limits, Metadata, OdfError, Package, Result};

/// A formula document: StarMath source plus document metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Formula {
    /// The formula in the StarMath command language.
    pub source: String,
    /// Document metadata.
    pub meta: Metadata,
}

impl Formula {
    /// Creates a formula from StarMath source.
    pub fn new(source: impl Into<String>) -> Self {
        Formula { source: source.into(), meta: Metadata::default() }
    }

    /// Parses the source into a presentation tree.
    pub fn parse(&self) -> Parsed {
        parse(&self.source)
    }

    /// The formula as a standalone MathML document (with StarMath annotation).
    pub fn to_mathml(&self) -> String {
        to_mathml(&self.parse().root, Some(&self.source))
    }

    /// Loads a formula from the bytes of an `.odf` package or a MathML file;
    /// the format is detected from the content.
    pub fn from_bytes(bytes: &[u8], limits: &Limits) -> Result<Formula> {
        if bytes.starts_with(b"PK") {
            Self::from_odf_bytes(bytes, limits)
        } else {
            Self::from_mathml_bytes(bytes, limits)
        }
    }

    /// Loads an OpenDocument formula package.
    pub fn from_odf_bytes(bytes: &[u8], limits: &Limits) -> Result<Formula> {
        let package =
            Package::read_filtered(bytes, limits, |name| matches!(name, "content.xml" | "meta.xml"))?;
        package.expect_mimetype(&[ns::MIME_ODF])?;
        let content = package.require_xml("content.xml", limits)?;
        let mut formula = Self::from_mathml_root(&content)?;
        if let Some(meta) = package.get_xml("meta.xml", limits)? {
            formula.meta = Metadata::from_xml(&meta);
        }
        Ok(formula)
    }

    /// Loads a standalone MathML document.
    pub fn from_mathml_bytes(bytes: &[u8], limits: &Limits) -> Result<Formula> {
        if bytes.len() as u64 > limits.max_entry_size {
            return Err(OdfError::limit("MathML document is too large"));
        }
        Self::from_mathml_root(&xml::parse_bytes(bytes, limits)?)
    }

    fn from_mathml_root(root: &xml::Element) -> Result<Formula> {
        if root.name != "math:math" {
            return Err(OdfError::invalid(format!("expected a MathML <math> element, found <{}>", root.name)));
        }
        let source = mathml_to_starmath(root);
        if source.len() > MAX_SOURCE_LEN {
            return Err(OdfError::limit("formula is too long"));
        }
        Ok(Formula { source, meta: Metadata::default() })
    }

    /// Serialises to an OpenDocument formula package (`.odf`).
    pub fn to_odf_bytes(&self) -> Result<Vec<u8>> {
        let mut package = Package::new(ns::MIME_ODF);
        package.insert("content.xml", self.to_mathml().into_bytes())?;
        package.insert("meta.xml", self.meta.to_xml().into_bytes())?;
        package.write()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odf_round_trip_preserves_source_and_metadata() {
        let mut formula = Formula::new("f(x) = sum from {n=0} to infinity {a_n x^n} newline \"<&>\"");
        formula.meta.title = Some("Series".into());
        let bytes = formula.to_odf_bytes().expect("save");
        let back = Formula::from_bytes(&bytes, &Limits::default()).expect("load");
        assert_eq!(back, formula);
    }

    #[test]
    fn loads_plain_mathml() {
        let mml = br#"<?xml version="1.0"?><math xmlns="http://www.w3.org/1998/Math/MathML"><mfrac><mn>1</mn><mn>2</mn></mfrac></math>"#;
        let formula = Formula::from_bytes(mml, &Limits::default()).expect("load");
        assert_eq!(formula.source, "{1} over {2}");
    }

    #[test]
    fn rejects_wrong_content() {
        assert!(Formula::from_bytes(b"<html/>", &Limits::default()).is_err());
        assert!(Formula::from_bytes(b"PK\x03\x04garbage", &Limits::default()).is_err());
        assert!(Formula::from_bytes(b"", &Limits::default()).is_err());
        // A text document is not a formula.
        let mut package = Package::new(ns::MIME_ODT);
        package.insert("content.xml", b"<a/>".to_vec()).expect("insert");
        let bytes = package.write().expect("write");
        assert!(matches!(Formula::from_bytes(&bytes, &Limits::default()), Err(OdfError::Invalid(_))));
    }
}
