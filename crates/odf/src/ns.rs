//! XML namespaces used by OpenDocument, with their conventional prefixes.
//!
//! The parser in [`crate::xml`] rewrites every qualified name to the
//! *conventional* prefix listed here, whatever prefix the file itself used, so
//! the rest of the suite can match on plain strings such as `"text:p"`.

/// `(conventional prefix, namespace URI)` for every namespace we understand.
pub const NAMESPACES: &[(&str, &str)] = &[
    ("office", "urn:oasis:names:tc:opendocument:xmlns:office:1.0"),
    ("style", "urn:oasis:names:tc:opendocument:xmlns:style:1.0"),
    ("text", "urn:oasis:names:tc:opendocument:xmlns:text:1.0"),
    ("table", "urn:oasis:names:tc:opendocument:xmlns:table:1.0"),
    ("draw", "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"),
    ("fo", "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"),
    ("xlink", "http://www.w3.org/1999/xlink"),
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("meta", "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"),
    ("number", "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0"),
    ("svg", "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"),
    ("chart", "urn:oasis:names:tc:opendocument:xmlns:chart:1.0"),
    ("dr3d", "urn:oasis:names:tc:opendocument:xmlns:dr3d:1.0"),
    ("math", "http://www.w3.org/1998/Math/MathML"),
    ("form", "urn:oasis:names:tc:opendocument:xmlns:form:1.0"),
    ("script", "urn:oasis:names:tc:opendocument:xmlns:script:1.0"),
    ("config", "urn:oasis:names:tc:opendocument:xmlns:config:1.0"),
    ("manifest", "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"),
    ("of", "urn:oasis:names:tc:opendocument:xmlns:of:1.2"),
    ("presentation", "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"),
    ("ooo", "http://openoffice.org/2004/office"),
    ("ooow", "http://openoffice.org/2004/writer"),
    ("oooc", "http://openoffice.org/2004/calc"),
    ("xhtml", "http://www.w3.org/1999/xhtml"),
    ("grddl", "http://www.w3.org/2003/g/data-view#"),
    ("loext", "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0"),
    ("calcext", "urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0"),
    // Office Open XML (Word and Excel files, read-only), transitional and
    // strict variants.
    ("w", "http://schemas.openxmlformats.org/wordprocessingml/2006/main"),
    ("w", "http://purl.oclc.org/ooxml/wordprocessingml/main"),
    ("x", "http://schemas.openxmlformats.org/spreadsheetml/2006/main"),
    ("x", "http://purl.oclc.org/ooxml/spreadsheetml/main"),
    ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
    ("r", "http://purl.oclc.org/ooxml/officeDocument/relationships"),
    ("pr", "http://schemas.openxmlformats.org/package/2006/relationships"),
    ("cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties"),
    // DrawingML (pictures in Word files, PowerPoint slides).
    ("a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
    ("a", "http://purl.oclc.org/ooxml/drawingml/main"),
    ("wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
    ("wp", "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing"),
    ("pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ("pic", "http://purl.oclc.org/ooxml/drawingml/picture"),
    ("p", "http://schemas.openxmlformats.org/presentationml/2006/main"),
    ("p", "http://purl.oclc.org/ooxml/presentationml/main"),
    ("asvg", "http://schemas.microsoft.com/office/drawing/2016/SVG/main"),
];

/// Returns the conventional prefix for a namespace URI, if it is known.
pub fn canonical_prefix(uri: &str) -> Option<&'static str> {
    NAMESPACES.iter().find(|(_, u)| *u == uri).map(|(p, _)| *p)
}

/// Returns the namespace URI for a conventional prefix, if it is known.
pub fn uri_for_prefix(prefix: &str) -> Option<&'static str> {
    NAMESPACES.iter().find(|(p, _)| *p == prefix).map(|(_, u)| *u)
}

/// MIME type of text documents (`.odt`).
pub const MIME_ODT: &str = "application/vnd.oasis.opendocument.text";
/// MIME type of spreadsheet documents (`.ods`).
pub const MIME_ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";
/// MIME type of formula documents (`.odf`).
pub const MIME_ODF: &str = "application/vnd.oasis.opendocument.formula";

/// MIME type of presentations (`.odp`).
pub const MIME_ODP: &str = "application/vnd.oasis.opendocument.presentation";
/// MIME type of drawings (`.odg`).
pub const MIME_ODG: &str = "application/vnd.oasis.opendocument.graphics";

/// Value written to `office:version` and the manifest.
pub const ODF_VERSION: &str = "1.2";
