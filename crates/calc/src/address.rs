//! Cell addresses and rectangular ranges.

use std::fmt;

/// Number of rows in a sheet (same as OpenOffice Calc).
pub const MAX_ROWS: u32 = 1_048_576;
/// Number of columns in a sheet (`A` to `AMJ`, same as OpenOffice Calc).
pub const MAX_COLS: u32 = 1024;

/// A cell position, zero based. Ordered row-major (row first, then column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellAddr {
    /// Zero-based row.
    pub row: u32,
    /// Zero-based column.
    pub col: u32,
}

impl CellAddr {
    /// Creates an address. The caller is responsible for bounds.
    pub const fn new(row: u32, col: u32) -> Self {
        CellAddr { row, col }
    }

    /// Whether the address lies inside the sheet.
    pub fn is_valid(self) -> bool {
        self.row < MAX_ROWS && self.col < MAX_COLS
    }

    /// Parses `A1` notation (case insensitive, `$` markers ignored).
    pub fn parse(s: &str) -> Option<CellAddr> {
        let s = s.trim();
        let bytes = s.as_bytes();
        let mut i = 0;
        if bytes.get(i) == Some(&b'$') {
            i += 1;
        }
        let col_start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let col = parse_col(&s[col_start..i])?;
        if bytes.get(i) == Some(&b'$') {
            i += 1;
        }
        let digits = &s[i..];
        if digits.is_empty() || digits.len() > 7 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let row: u32 = digits.parse().ok()?;
        if row == 0 || row > MAX_ROWS {
            return None;
        }
        Some(CellAddr { row: row - 1, col })
    }

    /// Formats in `A1` notation.
    pub fn to_a1(self) -> String {
        format!("{}{}", col_name(self.col), self.row + 1)
    }
}

impl fmt::Display for CellAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_a1())
    }
}

/// Column letters for a zero-based column index (`0 -> A`, `26 -> AA`).
pub fn col_name(col: u32) -> String {
    let mut n = col + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(b'A' + rem);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Parses column letters (`A`, `aa`, `AMJ`) into a zero-based index.
pub fn parse_col(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 3 || !s.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut n: u32 = 0;
    for b in s.bytes() {
        n = n * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1;
    }
    (n <= MAX_COLS).then(|| n - 1)
}

/// An inclusive rectangular range with `start <= end` on both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    /// Top-left corner.
    pub start: CellAddr,
    /// Bottom-right corner (inclusive).
    pub end: CellAddr,
}

impl Range {
    /// Creates a normalised range from two arbitrary corners.
    pub fn new(a: CellAddr, b: CellAddr) -> Self {
        Range {
            start: CellAddr::new(a.row.min(b.row), a.col.min(b.col)),
            end: CellAddr::new(a.row.max(b.row), a.col.max(b.col)),
        }
    }

    /// A single-cell range.
    pub fn single(addr: CellAddr) -> Self {
        Range { start: addr, end: addr }
    }

    /// Parses `A1:B2` or `A1`.
    pub fn parse(s: &str) -> Option<Range> {
        match s.split_once(':') {
            Some((a, b)) => Some(Range::new(CellAddr::parse(a)?, CellAddr::parse(b)?)),
            None => CellAddr::parse(s).map(Range::single),
        }
    }

    /// Whether `addr` lies inside the range.
    pub fn contains(&self, addr: CellAddr) -> bool {
        addr.row >= self.start.row
            && addr.row <= self.end.row
            && addr.col >= self.start.col
            && addr.col <= self.end.col
    }

    /// Whether the two ranges share at least one cell.
    pub fn intersects(&self, other: &Range) -> bool {
        self.start.row <= other.end.row
            && other.start.row <= self.end.row
            && self.start.col <= other.end.col
            && other.start.col <= self.end.col
    }

    /// Number of rows.
    pub fn rows(&self) -> u32 {
        self.end.row - self.start.row + 1
    }

    /// Number of columns.
    pub fn cols(&self) -> u32 {
        self.end.col - self.start.col + 1
    }

    /// Number of cells.
    pub fn cell_count(&self) -> u64 {
        u64::from(self.rows()) * u64::from(self.cols())
    }

    /// Whether both corners lie inside the sheet.
    pub fn is_valid(&self) -> bool {
        self.start.is_valid() && self.end.is_valid()
    }

    /// Formats as `A1:B2` (or `A1` for a single cell).
    pub fn to_a1(&self) -> String {
        if self.start == self.end {
            self.start.to_a1()
        } else {
            format!("{}:{}", self.start.to_a1(), self.end.to_a1())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_round_trip() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        assert_eq!(col_name(1023), "AMJ");
        for col in 0..MAX_COLS {
            assert_eq!(parse_col(&col_name(col)), Some(col));
        }
        assert_eq!(parse_col("amj"), Some(1023));
        assert_eq!(parse_col("AMK"), None);
        assert_eq!(parse_col(""), None);
        assert_eq!(parse_col("A1"), None);
        assert_eq!(parse_col("ABCD"), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(CellAddr::parse("A1"), Some(CellAddr::new(0, 0)));
        assert_eq!(CellAddr::parse("$b$10"), Some(CellAddr::new(9, 1)));
        assert_eq!(CellAddr::parse("AMJ1048576"), Some(CellAddr::new(MAX_ROWS - 1, MAX_COLS - 1)));
        for bad in ["", "A", "1", "A0", "A1048577", "A-1", "A1B", "A 1", "AMK1", "A99999999999"] {
            assert_eq!(CellAddr::parse(bad), None, "{bad}");
        }
        assert_eq!(CellAddr::new(9, 27).to_a1(), "AB10");
    }

    #[test]
    fn ranges() {
        let r = Range::parse("C3:A1").expect("range");
        assert_eq!(r, Range::new(CellAddr::new(0, 0), CellAddr::new(2, 2)));
        assert_eq!(r.to_a1(), "A1:C3");
        assert_eq!((r.rows(), r.cols(), r.cell_count()), (3, 3, 9));
        assert!(r.contains(CellAddr::new(1, 1)));
        assert!(!r.contains(CellAddr::new(3, 1)));
        assert_eq!(Range::parse("B2").map(|r| r.to_a1()).as_deref(), Some("B2"));
        assert!(r.intersects(&Range::parse("C3:D4").expect("range")));
        assert!(!r.intersects(&Range::parse("D4:E5").expect("range")));
        assert_eq!(Range::parse("A1:"), None);
    }
}
