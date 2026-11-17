//! # office-writer
//!
//! The core of Akshara Writer, a word processor compatible with OpenOffice
//! Writer.
//!
//! * [`model`]: paragraphs of attributed runs, lists, tables, page breaks;
//! * [`Document`]: validated editing operations addressed in UTF-16 offsets,
//!   with coalescing undo/redo, find and replace;
//! * [`load_odt`] / [`save_odt`]: OpenDocument Text import and export;
//! * [`export`]: plain text import, and text, HTML and Markdown export.
//!
//! The crate contains no `unsafe` code.
//!
//! ```
//! use office_writer::{AttrPatch, Document, ParaKind, ParaRef};
//!
//! let mut doc = Document::new();
//! let first = ParaRef::top(0);
//! doc.set_text(first, "Hello world").unwrap();
//! doc.apply_attrs(first, 6, 11, &AttrPatch { bold: Some(true), ..Default::default() }).unwrap();
//! doc.set_kind(first, ParaKind::Heading { level: 1 }).unwrap();
//!
//! let bytes = office_writer::save_odt(&doc).unwrap();
//! let back = office_writer::load_odt(&bytes, &Default::default()).unwrap();
//! assert_eq!(back.blocks(), doc.blocks());
//! ```

mod document;
pub mod export;
pub mod model;
mod odt;

pub use document::{Document, Match, ParaRef, MAX_BLOCKS, MAX_PARAGRAPH_LEN, MAX_UNDO};
pub use model::{
    sanitize_link, Align, AttrPatch, Block, PageSetup, ParaKind, Paragraph, Run, Stats, Table, TextAttrs,
};
pub use odt::{load_odt, save_odt};

use office_odf::{Limits, Result};

/// Loads a document from file bytes, detecting the format: ZIP packages are
/// read as `.odt`, everything else as plain text.
pub fn load(bytes: &[u8], limits: &Limits) -> Result<Document> {
    if bytes.starts_with(b"PK") {
        load_odt(bytes, limits)
    } else {
        export::from_plain_text(bytes, limits)
    }
}
