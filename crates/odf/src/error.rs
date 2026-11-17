//! Error type shared by every crate in the suite.

use std::fmt;

/// Convenient result alias used throughout the suite.
pub type Result<T> = std::result::Result<T, OdfError>;

/// Everything that can go wrong while reading or writing a document.
///
/// The variants are deliberately coarse: callers (ultimately the UI) only need
/// to distinguish "your file is broken", "your file is too big / hostile",
/// "we do not support that feature" and plain I/O failures.
#[derive(Debug)]
pub enum OdfError {
    /// Underlying operating-system I/O failure.
    Io(std::io::Error),
    /// The ZIP container is damaged or uses an unsupported feature.
    Zip(String),
    /// The XML is not well formed. `offset` is a byte offset into the stream.
    Xml {
        /// Human readable description.
        message: String,
        /// Byte offset at which the problem was detected.
        offset: usize,
    },
    /// A configured resource limit (see [`crate::Limits`]) was exceeded.
    Limit(String),
    /// The document is structurally invalid (missing parts, bad values...).
    Invalid(String),
    /// The document uses a feature that is recognised but not supported.
    Unsupported(String),
}

impl OdfError {
    /// Shorthand for an [`OdfError::Invalid`] error.
    pub fn invalid(msg: impl Into<String>) -> Self {
        OdfError::Invalid(msg.into())
    }

    /// Shorthand for an [`OdfError::Limit`] error.
    pub fn limit(msg: impl Into<String>) -> Self {
        OdfError::Limit(msg.into())
    }

    /// Shorthand for an [`OdfError::Unsupported`] error.
    pub fn unsupported(msg: impl Into<String>) -> Self {
        OdfError::Unsupported(msg.into())
    }

    /// Stable machine readable category, used by the FFI layer.
    pub fn kind(&self) -> &'static str {
        match self {
            OdfError::Io(_) => "io",
            OdfError::Zip(_) => "zip",
            OdfError::Xml { .. } => "xml",
            OdfError::Limit(_) => "limit",
            OdfError::Invalid(_) => "invalid",
            OdfError::Unsupported(_) => "unsupported",
        }
    }
}

impl fmt::Display for OdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OdfError::Io(e) => write!(f, "I/O error: {e}"),
            OdfError::Zip(m) => write!(f, "invalid package: {m}"),
            OdfError::Xml { message, offset } => {
                write!(f, "malformed XML at byte {offset}: {message}")
            }
            OdfError::Limit(m) => write!(f, "resource limit exceeded: {m}"),
            OdfError::Invalid(m) => write!(f, "invalid document: {m}"),
            OdfError::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl std::error::Error for OdfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OdfError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for OdfError {
    fn from(e: std::io::Error) -> Self {
        OdfError::Io(e)
    }
}
