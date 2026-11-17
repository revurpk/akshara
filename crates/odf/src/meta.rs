//! Document metadata (`meta.xml`).

use crate::ns;
use crate::xml::{Element, XmlWriter};

/// Name written to `meta:generator`.
pub const GENERATOR: &str = concat!("Akshara/", env!("CARGO_PKG_VERSION"));

/// The subset of ODF metadata the suite reads and writes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    /// `dc:title`
    pub title: Option<String>,
    /// `dc:subject`
    pub subject: Option<String>,
    /// `dc:description`
    pub description: Option<String>,
    /// `meta:initial-creator`
    pub initial_creator: Option<String>,
    /// `dc:creator` (last author)
    pub creator: Option<String>,
    /// `meta:creation-date` (ISO 8601, kept verbatim)
    pub creation_date: Option<String>,
    /// `dc:date` (last modification, ISO 8601, kept verbatim)
    pub date: Option<String>,
    /// `meta:keyword` entries
    pub keywords: Vec<String>,
}

impl Metadata {
    /// Extracts metadata from a parsed `meta.xml` (or any document root that
    /// has an `office:meta` child). Unknown elements are ignored.
    pub fn from_xml(root: &Element) -> Metadata {
        let mut meta = Metadata::default();
        let Some(node) = root.child("office:meta") else {
            return meta;
        };
        for e in node.elements() {
            let text = e.text_content();
            let value = (!text.is_empty()).then_some(text);
            match e.name.as_str() {
                "dc:title" => meta.title = value,
                "dc:subject" => meta.subject = value,
                "dc:description" => meta.description = value,
                "meta:initial-creator" => meta.initial_creator = value,
                "dc:creator" => meta.creator = value,
                "meta:creation-date" => meta.creation_date = value,
                "dc:date" => meta.date = value,
                "meta:keyword" => meta.keywords.extend(value),
                _ => {}
            }
        }
        meta
    }

    /// Serialises to a complete `meta.xml` stream.
    pub fn to_xml(&self) -> String {
        let mut w = XmlWriter::new();
        w.start("office:document-meta")
            .namespaces(&["office", "meta", "dc"])
            .attr("office:version", ns::ODF_VERSION);
        w.start("office:meta");
        w.element_text("meta:generator", GENERATOR);
        let fields = [
            ("dc:title", &self.title),
            ("dc:subject", &self.subject),
            ("dc:description", &self.description),
            ("meta:initial-creator", &self.initial_creator),
            ("dc:creator", &self.creator),
            ("meta:creation-date", &self.creation_date),
            ("dc:date", &self.date),
        ];
        for (name, value) in fields {
            if let Some(value) = value {
                w.element_text(name, value);
            }
        }
        for keyword in &self.keywords {
            w.element_text("meta:keyword", keyword);
        }
        w.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{xml, Limits};

    #[test]
    fn round_trips() {
        let meta = Metadata {
            title: Some("Quarterly <report>".into()),
            subject: Some("Numbers & words".into()),
            description: None,
            initial_creator: Some("Ada".into()),
            creator: Some("Grace".into()),
            creation_date: Some("2026-01-02T03:04:05".into()),
            date: Some("2026-02-03T04:05:06".into()),
            keywords: vec!["a".into(), "b".into()],
        };
        let parsed = xml::parse(&meta.to_xml(), &Limits::default()).expect("parse");
        assert_eq!(Metadata::from_xml(&parsed), meta);
    }

    #[test]
    fn missing_meta_is_default() {
        let root = xml::parse("<x/>", &Limits::default()).expect("parse");
        assert_eq!(Metadata::from_xml(&root), Metadata::default());
    }
}
