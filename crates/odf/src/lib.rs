//! # office-odf
//!
//! The shared foundation of the Akshara office suite: hardened reading and
//! writing of OpenDocument packages.
//!
//! An OpenDocument file (`.odt`, `.ods`, `.odf`, ...) is a ZIP archive that
//! holds a few XML streams. This crate provides:
//!
//! * [`Package`]: the ZIP container, with defensive limits on reading and
//!   deterministic output on writing;
//! * [`xml`]: a strict, non-recursive XML parser (no DTDs, no entity
//!   expansion) producing a namespace-normalised tree, and an escaping writer;
//! * [`Limits`]: the resource limits applied to untrusted input;
//! * [`Metadata`]: `meta.xml` support;
//! * [`util`]: colours, lengths and UTF-16 offset helpers.
//!
//! The crate contains no `unsafe` code.
//!
//! ```
//! use office_odf::{ns, Limits, Package};
//!
//! let mut package = Package::new(ns::MIME_ODT);
//! package.insert("content.xml", b"<doc/>".to_vec()).unwrap();
//! let bytes = package.write().unwrap();
//!
//! let reread = Package::read(&bytes, &Limits::default()).unwrap();
//! assert_eq!(reread.mimetype(), ns::MIME_ODT);
//! let root = reread.require_xml("content.xml", &Limits::default()).unwrap();
//! assert_eq!(root.name, "doc");
//! ```

mod error;
mod limits;
pub mod meta;
pub mod ns;
pub mod ooxml;
mod package;
pub mod util;
pub mod xml;

pub use error::{OdfError, Result};
pub use limits::Limits;
pub use meta::Metadata;
pub use package::{Package, MANIFEST_PATH};
pub use util::Rgb;
pub use xml::{Element, Node, XmlWriter};

/// Reads a whole file, refusing files larger than [`Limits::max_archive_size`]
/// before allocating memory for them.
pub fn read_file_limited(path: &std::path::Path, limits: &Limits) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path)?.len();
    if len > limits.max_archive_size {
        return Err(OdfError::limit(format!(
            "file is {len} bytes, limit is {}",
            limits.max_archive_size
        )));
    }
    Ok(std::fs::read(path)?)
}

/// Writes a file atomically: the data goes to a temporary sibling first and is
/// renamed over the target, so a crash or full disk never leaves a truncated
/// document behind.
pub fn write_file_atomic(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| OdfError::invalid("target path has no file name"))?
        .to_string_lossy();
    let tmp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_and_limited_read() {
        let dir = std::env::temp_dir().join(format!("office-odf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("doc.bin");
        write_file_atomic(&path, b"hello").expect("write");
        write_file_atomic(&path, b"hello again").expect("overwrite");
        assert_eq!(read_file_limited(&path, &Limits::default()).expect("read"), b"hello again");
        let limits = Limits { max_archive_size: 3, ..Limits::default() };
        assert!(matches!(read_file_limited(&path, &limits), Err(OdfError::Limit(_))));
        assert_eq!(std::fs::read_dir(&dir).expect("ls").count(), 1, "no temp files left behind");
        std::fs::remove_dir_all(&dir).expect("cleanup");
        assert!(read_file_limited(&path, &Limits::default()).is_err());
    }
}
