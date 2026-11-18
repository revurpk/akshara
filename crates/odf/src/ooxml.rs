//! Helpers for reading Office Open XML packages (`.docx`, `.xlsx`).
//!
//! An OOXML file is a ZIP package of XML parts that point at each other
//! through relationship parts (`_rels/*.rels`). These helpers resolve those
//! links and recognise the files that are not OOXML at all: old binary
//! Office documents and encrypted (password protected) ones, which are OLE
//! compound files.

use crate::{Limits, OdfError, Package, Result};
use std::collections::HashMap;

/// Signature of OLE compound files (`.doc`, `.xls`, encrypted OOXML).
const OLE_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Fails with a clear message for OLE compound files.
pub fn reject_binary_office(bytes: &[u8]) -> Result<()> {
    if bytes.starts_with(OLE_MAGIC) {
        return Err(OdfError::unsupported(
            "old binary Microsoft Office files (.doc, .xls) and password protected Office files; \
             save the file as .docx or .xlsx without a password in Office first",
        ));
    }
    Ok(())
}

/// Joins a relationship target to the folder of the part that refers to it
/// (`word/document.xml` + `media/a.png` = `word/media/a.png`). Absolute
/// targets start at the package root; `..` is resolved and cannot escape.
pub fn resolve_target(part: &str, target: &str) -> String {
    let mut path: Vec<&str> = if let Some(abs) = target.strip_prefix('/') {
        return normalize(abs.split('/'));
    } else {
        part.split('/').collect()
    };
    path.pop(); // the part's own file name
    path.extend(target.split('/'));
    normalize(path.into_iter())
}

fn normalize<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for p in parts {
        match p {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// One relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship type URI (only the last path segment matters in
    /// practice, e.g. `.../header`).
    pub kind: String,
    /// Resolved package path, or the external URI as written.
    pub target: String,
    /// The target is outside the package (a hyperlink).
    pub external: bool,
}

/// The relationships of `part` (read from `<folder>/_rels/<name>.rels`),
/// by id. Missing relationship parts give an empty map.
pub fn relationships(package: &Package, part: &str, limits: &Limits) -> Result<HashMap<String, Relationship>> {
    let (folder, name) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_path = if folder.is_empty() { format!("_rels/{name}.rels") } else { format!("{folder}/_rels/{name}.rels") };
    let mut out = HashMap::new();
    let Some(xml) = package.get_xml(&rels_path, limits)? else { return Ok(out) };
    for rel in xml.children_named("pr:Relationship") {
        let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else { continue };
        let external = rel.attr("TargetMode") == Some("External");
        let target = if external { target.to_owned() } else { resolve_target(part, target) };
        out.insert(id.to_owned(), Relationship { kind: rel.attr("Type").unwrap_or("").to_owned(), target, external });
    }
    Ok(out)
}

/// The main part of the package (the target of the officeDocument
/// relationship in `_rels/.rels`), falling back to `default`.
pub fn main_part(package: &Package, default: &str, limits: &Limits) -> Result<String> {
    let rels = relationships(package, "", limits)?;
    Ok(rels
        .values()
        .find(|r| r.kind.ends_with("/officeDocument") && !r.external)
        .map(|r| r.target.clone())
        .unwrap_or_else(|| default.to_owned()))
}

/// Twentieths of a point (twips) to points.
pub fn twips(value: Option<&str>) -> Option<f32> {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map(|v| v / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_relative_to_the_part() {
        assert_eq!(resolve_target("word/document.xml", "header1.xml"), "word/header1.xml");
        assert_eq!(resolve_target("xl/workbook.xml", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve_target("xl/workbook.xml", "/xl/styles.xml"), "xl/styles.xml");
        assert_eq!(resolve_target("word/document.xml", "../../../etc/passwd"), "etc/passwd");
        assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
    }

    #[test]
    fn binary_office_files_are_named() {
        let mut ole = OLE_MAGIC.to_vec();
        ole.extend_from_slice(&[0; 32]);
        let err = reject_binary_office(&ole).expect_err("rejected");
        assert!(err.to_string().contains(".doc"));
        assert!(reject_binary_office(b"PK\x03\x04").is_ok());
    }
}
