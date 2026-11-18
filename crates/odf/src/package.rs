//! OpenDocument packages: a ZIP archive with a `mimetype` entry, a manifest and
//! a handful of XML streams.
//!
//! Reading is defensive (see [`Limits`]): sizes are checked against the
//! directory *and* enforced while inflating, suspicious entry names and
//! duplicates are rejected, and encrypted packages are refused rather than
//! half-read. Nothing is ever extracted to disk, so path traversal ("zip
//! slip") cannot occur; names are still validated to keep bad packages out.

use crate::error::{OdfError, Result};
use crate::limits::Limits;
use crate::ns;
use crate::xml::{self, Element, XmlWriter};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Path of the manifest inside a package.
pub const MANIFEST_PATH: &str = "META-INF/manifest.xml";

/// An in-memory OpenDocument package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    mimetype: String,
    entries: BTreeMap<String, Vec<u8>>,
}

fn zip_err(e: zip::result::ZipError) -> OdfError {
    match e {
        zip::result::ZipError::Io(io) => OdfError::Io(io),
        other => OdfError::Zip(other.to_string()),
    }
}

/// Checks that an entry name is a plain relative path.
fn validate_name(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.len() > 1024
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(':')
        || name.split('/').any(|seg| seg == ".." || seg == ".");
    if bad {
        return Err(OdfError::Zip(format!("unsafe entry name {:?}", name.chars().take(64).collect::<String>())));
    }
    Ok(())
}

impl Package {
    /// Creates an empty package of the given MIME type.
    pub fn new(mimetype: impl Into<String>) -> Self {
        Package { mimetype: mimetype.into(), entries: BTreeMap::new() }
    }

    /// Reads every entry of a package.
    pub fn read(bytes: &[u8], limits: &Limits) -> Result<Self> {
        Self::read_filtered(bytes, limits, |_| true)
    }

    /// Reads a package, loading only the entries for which `want` returns true.
    ///
    /// The `mimetype` and manifest entries are always inspected. Skipped
    /// entries are still validated (names, encryption flags, declared sizes)
    /// but are never inflated.
    pub fn read_filtered(bytes: &[u8], limits: &Limits, want: impl Fn(&str) -> bool) -> Result<Self> {
        if bytes.len() as u64 > limits.max_archive_size {
            return Err(OdfError::limit(format!(
                "archive is {} bytes, limit is {}",
                bytes.len(),
                limits.max_archive_size
            )));
        }
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
        if archive.len() > limits.max_entries {
            return Err(OdfError::limit(format!(
                "archive has {} entries, limit is {}",
                archive.len(),
                limits.max_entries
            )));
        }

        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut total: u64 = 0;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(zip_err)?;
            // Some Windows tools write `xl\styles.xml`; read it as a normal
            // path. The checks below still reject `..`, absolute paths etc.
            let name = file.name().map_err(zip_err)?.replace('\\', "/");
            if file.is_dir() {
                continue;
            }
            validate_name(&name)?;
            if !seen.insert(name.clone()) {
                return Err(OdfError::Zip(format!("duplicate entry {name:?}")));
            }
            if file.encrypted() {
                return Err(OdfError::unsupported("password protected (encrypted) packages"));
            }
            let declared = file.size();
            if declared > limits.max_entry_size {
                return Err(OdfError::limit(format!(
                    "entry {name:?} is {declared} bytes, limit is {}",
                    limits.max_entry_size
                )));
            }
            let essential = name == "mimetype" || name == MANIFEST_PATH;
            if !essential && !want(&name) {
                continue;
            }
            let compressed = file.compressed_size().max(1);
            if declared > limits.ratio_floor && declared / compressed > limits.max_compression_ratio {
                return Err(OdfError::limit(format!(
                    "entry {name:?} has a suspicious compression ratio"
                )));
            }
            // Never trust the declared size: cap the actual inflated stream.
            let mut data = Vec::with_capacity(declared.min(1 << 20) as usize);
            let read = (&mut file)
                .take(limits.max_entry_size + 1)
                .read_to_end(&mut data)
                .map_err(|e| OdfError::Zip(format!("cannot read entry {name:?}: {e}")))? as u64;
            if read > limits.max_entry_size {
                return Err(OdfError::limit(format!("entry {name:?} exceeds the size limit")));
            }
            if read > limits.ratio_floor && read / compressed > limits.max_compression_ratio {
                return Err(OdfError::limit(format!(
                    "entry {name:?} has a suspicious compression ratio"
                )));
            }
            total += read;
            if total > limits.max_total_size {
                return Err(OdfError::limit(format!(
                    "package expands to more than {} bytes",
                    limits.max_total_size
                )));
            }
            entries.insert(name, data);
        }

        let manifest = match entries.get(MANIFEST_PATH) {
            Some(data) => Some(xml::parse_bytes(data, limits)?),
            None => None,
        };
        if let Some(manifest) = &manifest {
            let encrypted = manifest
                .children_named("manifest:file-entry")
                .any(|e| e.child("manifest:encryption-data").is_some());
            if encrypted {
                return Err(OdfError::unsupported("password protected (encrypted) documents"));
            }
        }

        let mimetype = match entries.remove("mimetype") {
            Some(data) => String::from_utf8(data)
                .map_err(|_| OdfError::invalid("mimetype entry is not UTF-8"))?
                .trim()
                .to_owned(),
            None => manifest
                .as_ref()
                .and_then(|m| {
                    m.children_named("manifest:file-entry")
                        .find(|e| e.attr("manifest:full-path") == Some("/"))
                        .and_then(|e| e.attr("manifest:media-type"))
                        .map(str::to_owned)
                })
                .unwrap_or_default(),
        };
        if mimetype.len() > 256 || mimetype.chars().any(char::is_control) {
            return Err(OdfError::invalid("implausible mimetype"));
        }
        entries.remove(MANIFEST_PATH);
        Ok(Package { mimetype, entries })
    }

    /// The package MIME type (may be empty for damaged packages).
    pub fn mimetype(&self) -> &str {
        &self.mimetype
    }

    /// Fails unless the package has one of the `accepted` MIME types.
    ///
    /// Template variants (`...-template`) of an accepted type are accepted too.
    pub fn expect_mimetype(&self, accepted: &[&str]) -> Result<()> {
        let ok = accepted.iter().any(|a| {
            self.mimetype == *a || self.mimetype.strip_suffix("-template") == Some(*a)
        });
        if ok {
            Ok(())
        } else {
            Err(OdfError::invalid(format!(
                "unexpected document type {:?} (expected {})",
                self.mimetype,
                accepted.join(" or ")
            )))
        }
    }

    /// Returns the bytes of an entry.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.get(name).map(Vec::as_slice)
    }

    /// Parses an entry as XML. Returns `Ok(None)` when the entry is absent.
    pub fn get_xml(&self, name: &str, limits: &Limits) -> Result<Option<Element>> {
        match self.entries.get(name) {
            Some(data) => xml::parse_bytes(data, limits).map(Some),
            None => Ok(None),
        }
    }

    /// Parses a mandatory XML entry.
    pub fn require_xml(&self, name: &str, limits: &Limits) -> Result<Element> {
        self.get_xml(name, limits)?
            .ok_or_else(|| OdfError::invalid(format!("package has no {name}")))
    }

    /// Adds or replaces an entry.
    pub fn insert(&mut self, name: impl Into<String>, data: impl Into<Vec<u8>>) -> Result<()> {
        let name = name.into();
        validate_name(&name)?;
        if name == "mimetype" || name == MANIFEST_PATH {
            return Err(OdfError::invalid(format!("{name} is generated automatically")));
        }
        self.entries.insert(name, data.into());
        Ok(())
    }

    /// Names of the entries, sorted.
    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    fn manifest_xml(&self) -> String {
        let mut w = XmlWriter::new();
        w.start("manifest:manifest")
            .namespaces(&["manifest"])
            .attr("manifest:version", ns::ODF_VERSION);
        w.start("manifest:file-entry")
            .attr("manifest:full-path", "/")
            .attr("manifest:version", ns::ODF_VERSION)
            .attr("manifest:media-type", &self.mimetype)
            .end();
        for name in self.entries.keys() {
            let media_type = if name.ends_with(".xml") {
                "text/xml"
            } else if name.ends_with(".png") {
                "image/png"
            } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
                "image/jpeg"
            } else {
                ""
            };
            w.start("manifest:file-entry")
                .attr("manifest:full-path", name)
                .attr("manifest:media-type", media_type)
                .end();
        }
        w.finish()
    }

    /// Serialises the package.
    ///
    /// The output is deterministic (fixed timestamps, sorted entries) and
    /// follows the ODF packaging rules: `mimetype` is the first entry and is
    /// stored uncompressed.
    pub fn write(&self) -> Result<Vec<u8>> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(DateTime::default());
        let deflated = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default());

        zip.start_file("mimetype", stored).map_err(zip_err)?;
        zip.write_all(self.mimetype.as_bytes())?;
        for (name, data) in &self.entries {
            zip.start_file(name.as_str(), deflated).map_err(zip_err)?;
            zip.write_all(data)?;
        }
        zip.start_file(MANIFEST_PATH, deflated).map_err(zip_err)?;
        zip.write_all(self.manifest_xml().as_bytes())?;
        Ok(zip.finish().map_err(zip_err)?.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Package {
        let mut p = Package::new(ns::MIME_ODT);
        p.insert("content.xml", b"<a/>".to_vec()).expect("insert");
        p.insert("Pictures/x.png", vec![1, 2, 3]).expect("insert");
        p
    }

    #[test]
    fn round_trips() {
        let bytes = sample().write().expect("write");
        let back = Package::read(&bytes, &Limits::default()).expect("read");
        assert_eq!(back, sample());
        assert_eq!(back.mimetype(), ns::MIME_ODT);
        assert_eq!(back.get("content.xml"), Some(&b"<a/>"[..]));
        assert!(back.require_xml("content.xml", &Limits::default()).is_ok());
        assert!(back.require_xml("styles.xml", &Limits::default()).is_err());
    }

    #[test]
    fn output_is_deterministic_and_mimetype_is_first_and_stored() {
        let a = sample().write().expect("write");
        let b = sample().write().expect("write");
        assert_eq!(a, b);
        // Local file header: signature(4) ... name at offset 30; stored data follows.
        assert_eq!(&a[0..4], b"PK\x03\x04");
        assert_eq!(&a[30..38], b"mimetype");
        assert_eq!(&a[38..38 + ns::MIME_ODT.len()], ns::MIME_ODT.as_bytes());
    }

    #[test]
    fn filter_skips_entries() {
        let bytes = sample().write().expect("write");
        let back = Package::read_filtered(&bytes, &Limits::default(), |n| n == "content.xml").expect("read");
        assert!(back.get("content.xml").is_some());
        assert!(back.get("Pictures/x.png").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Package::read(b"not a zip", &Limits::default()).is_err());
        assert!(Package::read(&[], &Limits::default()).is_err());
    }

    #[test]
    fn rejects_unsafe_names() {
        for name in ["../evil", "/abs", "a/../b", "a\\b", "c:evil", "", "./x"] {
            assert!(validate_name(name).is_err(), "{name:?}");
            assert!(Package::new("x").insert(name, vec![]).is_err(), "{name:?}");
        }
        assert!(validate_name("Pictures/a b.png").is_ok());
    }

    #[test]
    fn enforces_size_limits() {
        let mut p = Package::new(ns::MIME_ODT);
        p.insert("content.xml", vec![b' '; 100_000]).expect("insert");
        let bytes = p.write().expect("write");
        let err = Package::read(&bytes, &Limits::tiny()).expect_err("must fail");
        assert!(matches!(err, OdfError::Limit(_)), "{err}");

        let limits = Limits { max_archive_size: 10, ..Limits::default() };
        assert!(matches!(Package::read(&bytes, &limits), Err(OdfError::Limit(_))));

        let limits = Limits { max_total_size: 150_000, max_entry_size: 100_000, ..Limits::default() };
        let mut p = Package::new(ns::MIME_ODT);
        p.insert("a.xml", vec![b' '; 100_000]).expect("insert");
        p.insert("b.xml", vec![b' '; 100_000]).expect("insert");
        assert!(matches!(Package::read(&p.write().expect("write"), &limits), Err(OdfError::Limit(_))));
    }

    #[test]
    fn enforces_compression_ratio() {
        let mut p = Package::new(ns::MIME_ODT);
        p.insert("content.xml", vec![b'a'; 20 << 20]).expect("insert");
        let bytes = p.write().expect("write");
        assert!(matches!(Package::read(&bytes, &Limits::default()), Err(OdfError::Limit(_))));
    }

    #[test]
    fn enforces_entry_count() {
        let mut p = Package::new(ns::MIME_ODT);
        for i in 0..20 {
            p.insert(format!("f{i}.xml"), b"<a/>".to_vec()).expect("insert");
        }
        let bytes = p.write().expect("write");
        assert!(matches!(Package::read(&bytes, &Limits::tiny()), Err(OdfError::Limit(_))));
    }

    #[test]
    fn detects_manifest_encryption() {
        // Build an archive by hand with an encryption-data manifest entry.
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", opts).expect("start");
        zip.write_all(ns::MIME_ODT.as_bytes()).expect("write");
        zip.start_file(MANIFEST_PATH, opts).expect("start");
        let manifest = format!(
            "<manifest:manifest xmlns:manifest=\"{}\"><manifest:file-entry manifest:full-path=\"content.xml\">\
             <manifest:encryption-data/></manifest:file-entry></manifest:manifest>",
            ns::uri_for_prefix("manifest").expect("ns")
        );
        zip.write_all(manifest.as_bytes()).expect("write");
        let bytes = zip.finish().expect("finish").into_inner();
        assert!(matches!(Package::read(&bytes, &Limits::default()), Err(OdfError::Unsupported(_))));
    }

    #[test]
    fn mimetype_falls_back_to_manifest() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default();
        zip.start_file(MANIFEST_PATH, opts).expect("start");
        let manifest = format!(
            "<manifest:manifest xmlns:manifest=\"{}\"><manifest:file-entry manifest:full-path=\"/\" \
             manifest:media-type=\"{}\"/></manifest:manifest>",
            ns::uri_for_prefix("manifest").expect("ns"),
            ns::MIME_ODS
        );
        zip.write_all(manifest.as_bytes()).expect("write");
        let bytes = zip.finish().expect("finish").into_inner();
        let p = Package::read(&bytes, &Limits::default()).expect("read");
        assert_eq!(p.mimetype(), ns::MIME_ODS);
        assert!(p.expect_mimetype(&[ns::MIME_ODS]).is_ok());
        assert!(p.expect_mimetype(&[ns::MIME_ODT]).is_err());
    }

    #[test]
    fn accepts_template_mimetypes() {
        let p = Package::new(format!("{}-template", ns::MIME_ODT));
        assert!(p.expect_mimetype(&[ns::MIME_ODT]).is_ok());
    }
}
