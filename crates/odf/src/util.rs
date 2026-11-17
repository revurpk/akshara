//! Small value types shared by the applications: colours, lengths, and
//! UTF-16 offset conversion.

use crate::error::{OdfError, Result};

/// An opaque sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parses `#rrggbb` (the only colour syntax ODF allows).
    pub fn parse(s: &str) -> Option<Rgb> {
        let hex = s.trim().strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    /// Formats as `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Parses an ODF length (`2.54cm`, `1in`, `12pt`, `10mm`, `1pc`, `16px`) into
/// typographic points. Returns `None` for malformed or non-finite values.
pub fn parse_length_pt(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split);
    let value: f64 = num.trim().parse().ok()?;
    let factor = match unit {
        "pt" => 1.0,
        "in" | "inch" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "pc" => 12.0,
        "px" => 0.75,
        _ => return None,
    };
    let pt = value * factor;
    pt.is_finite().then_some(pt)
}

/// Formats a length in points as an ODF length in inches (four decimals).
pub fn format_length_in(pt: f64) -> String {
    format!("{:.4}in", pt / 72.0)
}

/// Converts a UTF-16 code unit offset into a byte offset into `s`.
///
/// The UI toolkit (and ODF itself) addresses text in UTF-16 code units while
/// Rust strings are UTF-8. Offsets past the end or in the middle of a
/// surrogate pair are rejected instead of silently corrupting text.
pub fn utf16_to_byte(s: &str, utf16_offset: usize) -> Result<usize> {
    let mut units = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        if units == utf16_offset {
            return Ok(byte_idx);
        }
        units += ch.len_utf16();
        if units > utf16_offset {
            return Err(OdfError::invalid("text offset splits a surrogate pair"));
        }
    }
    if units == utf16_offset {
        Ok(s.len())
    } else {
        Err(OdfError::invalid(format!(
            "text offset {utf16_offset} is past the end ({units})"
        )))
    }
}

/// Length of `s` in UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Converts a byte offset (on a char boundary) to a UTF-16 offset.
pub fn byte_to_utf16(s: &str, byte_offset: usize) -> usize {
    utf16_len(&s[..byte_offset.min(s.len())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Rgb::parse("#ff8000"), Some(Rgb(255, 128, 0)));
        assert_eq!(Rgb::parse(" #FFFFFF "), Some(Rgb(255, 255, 255)));
        assert_eq!(Rgb::parse("ff8000"), None);
        assert_eq!(Rgb::parse("#ff80"), None);
        assert_eq!(Rgb::parse("#+f8000"), None);
        assert_eq!(Rgb(1, 2, 255).to_hex(), "#0102ff");
    }

    #[test]
    fn lengths() {
        assert_eq!(parse_length_pt("1in"), Some(72.0));
        assert_eq!(parse_length_pt("12pt"), Some(12.0));
        assert!((parse_length_pt("2.54cm").expect("cm") - 72.0).abs() < 1e-9);
        assert!((parse_length_pt("25.4mm").expect("mm") - 72.0).abs() < 1e-9);
        assert_eq!(parse_length_pt("12"), None);
        assert_eq!(parse_length_pt("abc"), None);
        assert_eq!(parse_length_pt("1e999in"), None);
        assert_eq!(format_length_in(72.0), "1.0000in");
    }

    #[test]
    fn utf16_offsets() {
        let s = "a\u{1F600}b"; // emoji is 2 UTF-16 units, 4 bytes
        assert_eq!(utf16_len(s), 4);
        assert_eq!(utf16_to_byte(s, 0).expect("0"), 0);
        assert_eq!(utf16_to_byte(s, 1).expect("1"), 1);
        assert!(utf16_to_byte(s, 2).is_err());
        assert_eq!(utf16_to_byte(s, 3).expect("3"), 5);
        assert_eq!(utf16_to_byte(s, 4).expect("4"), 6);
        assert!(utf16_to_byte(s, 5).is_err());
        assert_eq!(byte_to_utf16(s, 5), 3);
    }
}
