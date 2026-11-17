//! Resource limits applied while reading untrusted documents.

/// Hard limits enforced on every document that is opened.
///
/// Office documents are routinely received from untrusted sources, and both
/// layers of the format (ZIP and XML) allow tiny files to expand into huge
/// in-memory structures. Every reader in the suite takes a `Limits` value and
/// fails with [`crate::OdfError::Limit`] instead of exhausting memory or stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted archive (compressed, on disk) in bytes.
    pub max_archive_size: u64,
    /// Largest accepted number of entries in the ZIP directory.
    pub max_entries: usize,
    /// Largest accepted uncompressed size of one entry in bytes.
    pub max_entry_size: u64,
    /// Largest accepted sum of uncompressed sizes of all loaded entries.
    pub max_total_size: u64,
    /// Largest accepted compression ratio for entries above
    /// [`Limits::ratio_floor`] bytes.
    pub max_compression_ratio: u64,
    /// Entries smaller than this (uncompressed) are exempt from the ratio check.
    pub ratio_floor: u64,
    /// Deepest accepted XML element nesting.
    pub max_xml_depth: usize,
    /// Largest accepted number of XML nodes (elements + text nodes) per stream.
    pub max_xml_nodes: usize,
    /// Largest accepted number of attributes on one element.
    pub max_xml_attributes: usize,
    /// Largest number of spreadsheet cells materialised from one document.
    pub max_cells: usize,
    /// Largest number of text blocks (paragraphs, table cells) per document.
    pub max_blocks: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_archive_size: 256 << 20,
            max_entries: 20_000,
            max_entry_size: 128 << 20,
            max_total_size: 512 << 20,
            max_compression_ratio: 500,
            ratio_floor: 8 << 20,
            max_xml_depth: 256,
            max_xml_nodes: 4_000_000,
            max_xml_attributes: 1024,
            max_cells: 4_000_000,
            max_blocks: 2_000_000,
        }
    }
}

impl Limits {
    /// Very small limits, handy for tests that exercise the limit paths.
    pub fn tiny() -> Self {
        Limits {
            max_archive_size: 64 << 10,
            max_entries: 16,
            max_entry_size: 64 << 10,
            max_total_size: 128 << 10,
            max_compression_ratio: 50,
            ratio_floor: 4 << 10,
            max_xml_depth: 16,
            max_xml_nodes: 512,
            max_xml_attributes: 16,
            max_cells: 256,
            max_blocks: 64,
        }
    }
}
