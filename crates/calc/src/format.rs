//! Number formats, date arithmetic and interpretation of typed input.

use crate::value::Value;

/// How a numeric cell is displayed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum NumberFormat {
    /// Automatic: integers as integers, up to ten decimals and fifteen
    /// significant digits.
    #[default]
    General,
    /// Fixed number of decimals, optional thousands separators.
    Number {
        /// Digits after the decimal point.
        decimals: u8,
        /// Whether to group thousands with `,`.
        thousands: bool,
    },
    /// Value multiplied by 100 with a `%` sign.
    Percent {
        /// Digits after the decimal point.
        decimals: u8,
    },
    /// Currency amount.
    Currency {
        /// Digits after the decimal point.
        decimals: u8,
        /// Currency symbol placed before the amount.
        symbol: String,
    },
    /// Scientific notation.
    Scientific {
        /// Digits after the decimal point of the mantissa.
        decimals: u8,
    },
    /// Fixed decimals with Indian digit grouping (lakh, crore):
    /// `12,34,56,789.00`.
    IndianNumber {
        /// Digits after the decimal point.
        decimals: u8,
    },
    /// ISO date (`2026-03-15`).
    Date,
    /// Day-month-year date (`15-03-2026`), the usual order in India.
    DateDmy,
    /// Time of day (`13:45:00`).
    Time,
    /// ISO date and time.
    DateTime,
    /// Shown as typed.
    Text,
}

/// Largest number of decimals a format may request.
pub const MAX_DECIMALS: u8 = 20;

// ---------------------------------------------------------------------------
// Dates. Serial day 0 is 1899-12-30, like OpenOffice Calc's default null date.
// ---------------------------------------------------------------------------

const EPOCH_OFFSET: i64 = 25_569; // days from 1899-12-30 to 1970-01-01

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in a month (`month` in `1..=12`).
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => 28 + u32::from(is_leap_year(year)),
    }
}

/// Converts a calendar date to a serial day number. Returns `None` for dates
/// that do not exist or lie outside years 1..=9999.
pub fn date_to_serial(year: i64, month: u32, day: u32) -> Option<f64> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some((days_from_civil(year, month, day) + EPOCH_OFFSET) as f64)
}

/// Converts a serial day number to `(year, month, day)`.
pub fn serial_to_date(serial: f64) -> Option<(i64, u32, u32)> {
    if !serial.is_finite() || serial.abs() > 3_000_000.0 {
        return None;
    }
    let (year, month, day) = civil_from_days(serial.floor() as i64 - EPOCH_OFFSET);
    (1..=9999).contains(&year).then_some((year, month, day))
}

/// Splits the fractional part of a serial number into `(hours, minutes,
/// seconds)`, rounded to the nearest second. The boolean is true when rounding
/// carried into the next day.
pub fn serial_to_time(serial: f64) -> (u32, u32, u32, bool) {
    let frac = serial - serial.floor();
    let mut seconds = (frac * 86_400.0).round() as u32;
    let carry = seconds >= 86_400;
    if carry {
        seconds -= 86_400;
    }
    (seconds / 3600, seconds / 60 % 60, seconds % 60, carry)
}

/// Serial number of a Unix timestamp (seconds since 1970, UTC) shifted by a
/// local-time offset in minutes.
pub fn unix_to_serial(unix_seconds: f64, utc_offset_minutes: i32) -> f64 {
    (unix_seconds + f64::from(utc_offset_minutes) * 60.0) / 86_400.0 + EPOCH_OFFSET as f64
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn group_thousands(int_part: &str) -> String {
    let mut out = String::with_capacity(int_part.len() + int_part.len() / 3);
    let len = int_part.len();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Groups the integer digits the Indian way: the last three digits, then
/// pairs (`1234567` -> `12,34,567`).
fn group_indian(int_part: &str) -> String {
    if int_part.len() <= 3 {
        return int_part.to_owned();
    }
    let (head, tail) = int_part.split_at(int_part.len() - 3);
    let mut out = String::with_capacity(int_part.len() + int_part.len() / 2);
    let offset = head.len() % 2;
    for (i, c) in head.chars().enumerate() {
        if i > 0 && (i + 2 - offset) % 2 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push(',');
    out.push_str(tail);
    out
}

/// Digit grouping style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grouping {
    /// No separators.
    None,
    /// Groups of three (`1,234,567`).
    Thousands,
    /// Indian lakh/crore grouping (`12,34,567`).
    Indian,
}

/// Formats with a fixed number of decimals and optional grouping.
pub fn format_fixed(n: f64, decimals: u8, thousands: bool) -> String {
    format_grouped(n, decimals, if thousands { Grouping::Thousands } else { Grouping::None })
}

/// Formats with a fixed number of decimals and the given grouping.
pub fn format_grouped(n: f64, decimals: u8, grouping: Grouping) -> String {
    let decimals = usize::from(decimals.min(MAX_DECIMALS));
    if !n.is_finite() {
        return "#NUM!".into();
    }
    if n.abs() >= 1e21 {
        return format_general(n);
    }
    // Round half away from zero (Rust's formatter rounds half to even), with
    // a few ULPs of slack so that 1.005 shows as 1.01.
    let scale = 10f64.powi(decimals as i32);
    let scaled = n.abs() * scale;
    let rounded = if scaled < 9e15 { (scaled + scaled * 4.0 * f64::EPSILON).round() / scale } else { n.abs() };
    let digits = format!("{:.*}", decimals, rounded);
    let negative = n < 0.0 && digits.bytes().any(|b| b.is_ascii_digit() && b != b'0');
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (digits.as_str(), None),
    };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    match grouping {
        Grouping::Thousands => out.push_str(&group_thousands(int_part)),
        Grouping::Indian => out.push_str(&group_indian(int_part)),
        Grouping::None => out.push_str(int_part),
    }
    if let Some(frac) = frac_part {
        out.push('.');
        out.push_str(frac);
    }
    out
}

fn format_scientific(n: f64, decimals: usize) -> String {
    let raw = format!("{:.*e}", decimals, n);
    match raw.split_once('e') {
        Some((mantissa, exp)) => {
            let (sign, digits) = match exp.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', exp),
            };
            format!("{mantissa}E{sign}{digits:0>2}")
        }
        None => raw,
    }
}

/// Formats a number the way the "General" format does.
pub fn format_general(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let abs = n.abs();
    if abs >= 1e15 || abs < 1e-9 {
        let s = format_scientific(n, 5);
        // Trim trailing zeros of the mantissa: 1.50000E+20 -> 1.5E+20.
        return match s.split_once('E') {
            Some((mantissa, exp)) if mantissa.contains('.') => {
                format!("{}E{exp}", mantissa.trim_end_matches('0').trim_end_matches('.'))
            }
            _ => s,
        };
    }
    let int_digits = if abs >= 1.0 { abs.log10().floor() as i32 + 1 } else { 0 };
    // At most ten decimals and fifteen significant digits.
    let decimals = (15 - int_digits).clamp(0, 10) as usize;
    let s = format!("{:.*}", decimals, n);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".into() } else { s.to_owned() }
}

fn format_date(serial: f64) -> Option<String> {
    serial_to_date(serial).map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}"))
}

/// Formats a number according to `format`.
pub fn format_number(n: f64, format: &NumberFormat) -> String {
    match format {
        NumberFormat::General | NumberFormat::Text => format_general(n),
        NumberFormat::Number { decimals, thousands } => format_fixed(n, *decimals, *thousands),
        NumberFormat::Percent { decimals } => format!("{}%", format_fixed(n * 100.0, *decimals, false)),
        NumberFormat::IndianNumber { decimals } => format_grouped(n, *decimals, Grouping::Indian),
        NumberFormat::Currency { decimals, symbol } => {
            // Rupee amounts use lakh/crore grouping.
            let grouping = if symbol == "₹" { Grouping::Indian } else { Grouping::Thousands };
            let body = format_grouped(n.abs(), *decimals, grouping);
            let negative = n < 0.0 && body.bytes().any(|b| b.is_ascii_digit() && b != b'0');
            format!("{}{symbol}{body}", if negative { "-" } else { "" })
        }
        NumberFormat::Scientific { decimals } => format_scientific(n, usize::from((*decimals).min(MAX_DECIMALS))),
        NumberFormat::Date => format_date(n).unwrap_or_else(|| format_general(n)),
        NumberFormat::DateDmy => serial_to_date(n)
            .map(|(y, m, d)| format!("{d:02}-{m:02}-{y:04}"))
            .unwrap_or_else(|| format_general(n)),
        NumberFormat::Time => {
            let (h, m, s, _) = serial_to_time(n);
            format!("{h:02}:{m:02}:{s:02}")
        }
        NumberFormat::DateTime => {
            let (h, m, s, carry) = serial_to_time(n);
            match format_date(n.floor() + f64::from(u8::from(carry))) {
                Some(date) => format!("{date} {h:02}:{m:02}:{s:02}"),
                None => format_general(n),
            }
        }
    }
}

/// The text shown in a cell for `value` under `format`.
pub fn format_value(value: &Value, format: &NumberFormat) -> String {
    match value {
        Value::Empty => String::new(),
        Value::Number(n) => format_number(*n, format),
        Value::Text(s) => s.clone(),
        Value::Bool(true) => "TRUE".into(),
        Value::Bool(false) => "FALSE".into(),
        Value::Error(e) => e.as_str().into(),
    }
}

// ---------------------------------------------------------------------------
// Input interpretation
// ---------------------------------------------------------------------------

/// What a piece of typed (or imported) text means.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// Nothing.
    Empty,
    /// A number, with the format implied by how it was typed.
    Number(f64, Option<NumberFormat>),
    /// Plain text.
    Text(String),
    /// `TRUE` / `FALSE`.
    Bool(bool),
    /// A formula; the string is the body after the `=`.
    Formula(String),
}

/// Strictly parses a decimal number: optional sign, digits, optional
/// fraction, optional exponent, optional `1,234,567` grouping. Rejects
/// everything else Rust's float parser would accept (`inf`, `nan`, ...).
pub fn parse_number(text: &str) -> Option<f64> {
    let s = text.trim();
    let unsigned = s.strip_prefix(['-', '+']).unwrap_or(s);
    let b = unsigned.as_bytes();
    if b.is_empty() || unsigned.len() > 64 {
        return None;
    }
    let mut i = 0;
    let mut int_digits = 0;
    let mut groups_ok = true;
    let mut saw_comma = false;
    let mut since_comma = 0;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b',') {
        if b[i] == b',' {
            if (saw_comma && since_comma != 3) || (!saw_comma && !(1..=3).contains(&int_digits)) {
                groups_ok = false;
            }
            saw_comma = true;
            since_comma = 0;
        } else {
            int_digits += 1;
            since_comma += 1;
        }
        i += 1;
    }
    if saw_comma && (since_comma != 3 || !groups_ok) && !is_indian_grouped(&unsigned[..i]) {
        return None;
    }
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            frac_digits += 1;
            i += 1;
        }
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let cleaned: String = s.chars().filter(|c| *c != ',').collect();
    cleaned.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Whether `s` (digits and commas only) uses Indian grouping: an optional
/// leading group of 1-2 digits, pairs, and a final group of three.
fn is_indian_grouped(s: &str) -> bool {
    let groups: Vec<&str> = s.split(',').collect();
    if groups.len() < 2 || groups.iter().any(|g| g.is_empty() || !g.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let last = groups.len() - 1;
    groups[last].len() == 3
        && groups[1..last].iter().all(|g| g.len() == 2)
        && (1..=2).contains(&groups[0].len())
}

fn fraction_digits(text: &str) -> u8 {
    text.split_once('.')
        .map(|(_, frac)| frac.bytes().take_while(u8::is_ascii_digit).count().min(usize::from(MAX_DECIMALS)) as u8)
        .unwrap_or(0)
}

/// Parses `HH:MM` or `HH:MM:SS` into a fraction of a day.
pub fn parse_time(text: &str) -> Option<f64> {
    let mut parts = text.trim().split(':');
    let h = parts.next()?;
    let m = parts.next()?;
    let s = parts.next();
    if parts.next().is_some() {
        return None;
    }
    let digits = |p: &str, max_len: usize| -> Option<u32> {
        (!p.is_empty() && p.len() <= max_len && p.bytes().all(|b| b.is_ascii_digit())).then(|| p.parse().ok())?
    };
    let h = digits(h, 4)?;
    let m = digits(m, 2)?;
    let s = match s {
        Some(s) => digits(s, 2)?,
        None => 0,
    };
    if m > 59 || s > 59 {
        return None;
    }
    Some((f64::from(h) * 3600.0 + f64::from(m) * 60.0 + f64::from(s)) / 86_400.0)
}

/// Parses an ISO date `YYYY-MM-DD`, optionally followed by a time.
pub fn parse_iso_date(text: &str) -> Option<(f64, bool)> {
    let s = text.trim();
    let (date, time) = match s.find(['T', ' ']) {
        Some(idx) => (&s[..idx], Some(s[idx + 1..].trim())),
        None => (s, None),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    if ![y, m, d].iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let serial = date_to_serial(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)?;
    match time {
        None => Some((serial, false)),
        Some(time) => {
            // Tolerate fractional seconds and a trailing Z as written by ODF.
            let time = time.trim_end_matches('Z');
            let time = time.split_once('.').map(|(t, _)| t).unwrap_or(time);
            Some((serial + parse_time(time)?, true))
        }
    }
}

fn is_thousands_grouped(s: &str) -> bool {
    let groups: Vec<&str> = s.split(',').collect();
    groups.len() >= 2 && (1..=3).contains(&groups[0].len()) && groups[1..].iter().all(|g| g.len() == 3)
}

/// Parses a day-month-year date: `31-12-2026`, `31/12/2026` or
/// `31.12.2026` (one or two digit day and month, four digit year).
pub fn parse_dmy_date(text: &str) -> Option<f64> {
    let sep = text.chars().find(|c| matches!(c, '-' | '/' | '.'))?;
    let parts: Vec<&str> = text.split(sep).collect();
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    if parts[0].len() > 2 || parts[1].len() > 2 || parts[2].len() != 4 {
        return None;
    }
    date_to_serial(parts[2].parse().ok()?, parts[1].parse().ok()?, parts[0].parse().ok()?)
}

/// Interprets text typed into a cell.
pub fn parse_input(text: &str) -> Input {
    if text.is_empty() {
        return Input::Empty;
    }
    if let Some(body) = text.strip_prefix('=') {
        if !body.is_empty() {
            return Input::Formula(body.to_owned());
        }
        return Input::Text(text.to_owned());
    }
    if let Some(literal) = text.strip_prefix('\'') {
        return Input::Text(literal.to_owned());
    }
    let trimmed = text.trim();
    if let Some(n) = parse_number(trimmed) {
        // Typing "1,00,000" asks for Indian grouping.
        let unsigned = trimmed.trim_start_matches(['-', '+']);
        let int_part = unsigned.split(['.', 'e', 'E']).next().unwrap_or("");
        if int_part.contains(',') && is_indian_grouped(int_part) && !is_thousands_grouped(int_part) {
            return Input::Number(n, Some(NumberFormat::IndianNumber { decimals: fraction_digits(unsigned) }));
        }
        return Input::Number(n, None);
    }
    if let Some(body) = trimmed.strip_suffix('%') {
        if let Some(n) = parse_number(body) {
            return Input::Number(n / 100.0, Some(NumberFormat::Percent { decimals: fraction_digits(body) }));
        }
    }
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    for symbol in ["₹", "$", "€", "£", "¥"] {
        if let Some(body) = unsigned.strip_prefix(symbol) {
            if let Some(n) = parse_number(body).filter(|_| !body.starts_with(['-', '+'])) {
                let value = if negative { -n } else { n };
                return Input::Number(value, Some(NumberFormat::Currency { decimals: 2, symbol: symbol.into() }));
            }
        }
    }
    if let Some((serial, has_time)) = parse_iso_date(trimmed) {
        let format = if has_time { NumberFormat::DateTime } else { NumberFormat::Date };
        return Input::Number(serial, Some(format));
    }
    if let Some(serial) = parse_dmy_date(trimmed) {
        return Input::Number(serial, Some(NumberFormat::DateDmy));
    }
    if let Some(fraction) = parse_time(trimmed) {
        return Input::Number(fraction, Some(NumberFormat::Time));
    }
    match trimmed.to_ascii_uppercase().as_str() {
        "TRUE" => Input::Bool(true),
        "FALSE" => Input::Bool(false),
        _ => Input::Text(text.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ErrorKind;

    #[test]
    fn date_conversions() {
        assert_eq!(date_to_serial(1899, 12, 30), Some(0.0));
        assert_eq!(date_to_serial(1900, 1, 1), Some(2.0));
        assert_eq!(date_to_serial(1970, 1, 1), Some(25_569.0));
        assert_eq!(date_to_serial(2024, 2, 29), Some(45_351.0));
        assert_eq!(date_to_serial(2023, 2, 29), None);
        assert_eq!(date_to_serial(2024, 13, 1), None);
        assert_eq!(date_to_serial(0, 1, 1), None);
        assert_eq!(serial_to_date(45_351.0), Some((2024, 2, 29)));
        assert_eq!(serial_to_date(45_351.99), Some((2024, 2, 29)));
        assert_eq!(serial_to_date(-1.0), Some((1899, 12, 29)));
        assert_eq!(serial_to_date(f64::NAN), None);
        assert_eq!(serial_to_date(1e12), None);
        // Every day of several years round-trips.
        for serial in (30_000..50_000).step_by(7) {
            let (y, m, d) = serial_to_date(f64::from(serial)).expect("date");
            assert_eq!(date_to_serial(y, m, d), Some(f64::from(serial)));
        }
        assert_eq!(unix_to_serial(0.0, 0), 25_569.0);
        assert_eq!(unix_to_serial(0.0, 60), 25_569.0 + 1.0 / 24.0);
    }

    #[test]
    fn general_format() {
        let cases = [
            (0.0, "0"), (1.0, "1"), (-1.0, "-1"), (1.5, "1.5"), (0.1 + 0.2, "0.3"), (1.0 / 3.0, "0.3333333333"),
            (1234567.891234, "1234567.891234"), (1e15, "1E+15"), (1.5e20, "1.5E+20"), (1e-10, "1E-10"),
            (123456789012.0, "123456789012"), (-0.00000000001, "-1E-11"), (0.000001, "0.000001"),
            (2.5e-10, "2.5E-10"), (99999999999.5, "99999999999.5"),
        ];
        for (n, expected) in cases {
            assert_eq!(format_general(n), expected, "{n}");
        }
        assert_eq!(format_general(f64::NAN), "#NUM!");
        assert_eq!(format_general(-1e-300), "-1E-300");
    }

    #[test]
    fn fixed_formats() {
        assert_eq!(format_fixed(1234567.891, 2, true), "1,234,567.89");
        assert_eq!(format_fixed(-1234.5, 0, true), "-1,235");
        assert_eq!(format_fixed(-0.001, 2, false), "0.00");
        assert_eq!(format_fixed(999.999, 2, true), "1,000.00");
        assert_eq!(format_fixed(12.0, 0, false), "12");
        assert_eq!(format_number(0.256, &NumberFormat::Percent { decimals: 1 }), "25.6%");
        assert_eq!(
            format_number(-1234.5, &NumberFormat::Currency { decimals: 2, symbol: "$".into() }),
            "-$1,234.50"
        );
        assert_eq!(format_number(12345.678, &NumberFormat::Scientific { decimals: 2 }), "1.23E+04");
        assert_eq!(format_number(0.00012, &NumberFormat::Scientific { decimals: 1 }), "1.2E-04");
        assert_eq!(format_number(45_351.0, &NumberFormat::Date), "2024-02-29");
        assert_eq!(format_number(0.75, &NumberFormat::Time), "18:00:00");
        assert_eq!(format_number(45_351.5, &NumberFormat::DateTime), "2024-02-29 12:00:00");
        assert_eq!(format_number(45_351.999_999_9, &NumberFormat::DateTime), "2024-03-01 00:00:00");
        assert_eq!(format_number(1e300, &NumberFormat::Date), "1E+300");
        assert_eq!(format_number(1e30, &NumberFormat::Number { decimals: 2, thousands: true }), "1E+30");
    }

    #[test]
    fn indian_formats() {
        assert_eq!(group_indian("1"), "1");
        assert_eq!(group_indian("123"), "123");
        assert_eq!(group_indian("1234"), "1,234");
        assert_eq!(group_indian("12345"), "12,345");
        assert_eq!(group_indian("123456"), "1,23,456");
        assert_eq!(group_indian("1234567"), "12,34,567");
        assert_eq!(group_indian("123456789"), "12,34,56,789");
        assert_eq!(format_number(-123456789.5, &NumberFormat::IndianNumber { decimals: 2 }), "-12,34,56,789.50");
        assert_eq!(
            format_number(1500000.0, &NumberFormat::Currency { decimals: 2, symbol: "₹".into() }),
            "₹15,00,000.00"
        );
        assert_eq!(format_number(45_351.0, &NumberFormat::DateDmy), "29-02-2024");
        assert_eq!(parse_number("1,00,000"), Some(100_000.0));
        assert_eq!(parse_number("12,34,567.5"), Some(1_234_567.5));
        assert_eq!(parse_number("1,0,000"), None);
        assert_eq!(parse_input("1,00,000"), Input::Number(100_000.0, Some(NumberFormat::IndianNumber { decimals: 0 })));
        assert_eq!(parse_input("100,000"), Input::Number(100_000.0, None), "thousands grouping stays general");
        assert_eq!(
            parse_input("₹2,50,000"),
            Input::Number(250_000.0, Some(NumberFormat::Currency { decimals: 2, symbol: "₹".into() }))
        );
        assert_eq!(parse_input("29-02-2024"), Input::Number(45_351.0, Some(NumberFormat::DateDmy)));
        assert_eq!(parse_input("1/3/2024"), Input::Number(45_352.0, Some(NumberFormat::DateDmy)));
        assert_eq!(parse_input("31-02-2024"), Input::Text("31-02-2024".into()));
    }

    #[test]
    fn value_formatting() {
        assert_eq!(format_value(&Value::Empty, &NumberFormat::General), "");
        assert_eq!(format_value(&Value::Bool(true), &NumberFormat::General), "TRUE");
        assert_eq!(format_value(&Value::Text("x".into()), &NumberFormat::Date), "x");
        assert_eq!(format_value(&Value::Error(ErrorKind::DivZero), &NumberFormat::General), "#DIV/0!");
    }

    #[test]
    fn strict_number_parsing() {
        assert_eq!(parse_number("42"), Some(42.0));
        assert_eq!(parse_number(" -1.5e3 "), Some(-1500.0));
        assert_eq!(parse_number("+.5"), Some(0.5));
        assert_eq!(parse_number("5."), Some(5.0));
        assert_eq!(parse_number("1,234,567.5"), Some(1_234_567.5));
        for bad in ["", "-", ".", "inf", "NaN", "0x10", "1e", "1,23", "1,2,34,567", ",123", "1,2345", "1 2", "1e999", "--1", "1,234,"] {
            assert_eq!(parse_number(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn input_interpretation() {
        assert_eq!(parse_input(""), Input::Empty);
        assert_eq!(parse_input("12.5"), Input::Number(12.5, None));
        assert_eq!(parse_input("=A1+1"), Input::Formula("A1+1".into()));
        assert_eq!(parse_input("="), Input::Text("=".into()));
        assert_eq!(parse_input("'=A1"), Input::Text("=A1".into()));
        assert_eq!(parse_input("'007"), Input::Text("007".into()));
        assert_eq!(parse_input("hello"), Input::Text("hello".into()));
        assert_eq!(parse_input("true"), Input::Bool(true));
        assert_eq!(parse_input("12.5%"), Input::Number(0.125, Some(NumberFormat::Percent { decimals: 1 })));
        assert_eq!(
            parse_input("-$1,200.50"),
            Input::Number(-1200.5, Some(NumberFormat::Currency { decimals: 2, symbol: "$".into() }))
        );
        assert_eq!(parse_input("2024-02-29"), Input::Number(45_351.0, Some(NumberFormat::Date)));
        assert_eq!(parse_input("2024-02-29 12:00"), Input::Number(45_351.5, Some(NumberFormat::DateTime)));
        assert_eq!(parse_input("2024-02-29T06:00:00"), Input::Number(45_351.25, Some(NumberFormat::DateTime)));
        assert_eq!(parse_input("18:00"), Input::Number(0.75, Some(NumberFormat::Time)));
        assert_eq!(parse_input("2023-02-29"), Input::Text("2023-02-29".into()));
        assert_eq!(parse_input("12:60"), Input::Text("12:60".into()));
        assert_eq!(parse_input("$-5"), Input::Text("$-5".into()));
        assert_eq!(parse_input(" padded "), Input::Text(" padded ".into()));
    }
}
