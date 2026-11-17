//! CSV / TSV import and export.
//!
//! Export can neutralise "CSV injection": text cells that start with `=`,
//! `+`, `-`, `@`, tab or carriage return are prefixed with an apostrophe so
//! other spreadsheet programs do not execute them as formulas.

use crate::address::{MAX_COLS, MAX_ROWS};
use crate::format::{format_value, NumberFormat};
use crate::value::Value;
use crate::workbook::Workbook;
use office_odf::{Limits, OdfError, Result};

/// Decodes CSV bytes: UTF-8 (with or without BOM), falling back to Latin-1
/// for legacy files that are not valid UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
    }
}

/// Guesses the delimiter from the first lines: the candidate (`,` `;` tab)
/// that occurs most often outside of quotes.
pub fn detect_delimiter(text: &str) -> char {
    let mut counts = [0usize; 3];
    let candidates = [',', ';', '\t'];
    let mut in_quotes = false;
    let mut lines = 0;
    for c in text.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            '\n' if !in_quotes => {
                lines += 1;
                if lines >= 10 {
                    break;
                }
            }
            _ if !in_quotes => {
                if let Some(i) = candidates.iter().position(|d| *d == c) {
                    counts[i] += 1;
                }
            }
            _ => {}
        }
    }
    let best = (0..3).max_by_key(|i| (counts[*i], 2 - *i)).unwrap_or(0);
    candidates[best]
}

/// Splits CSV text into rows of fields (RFC 4180 quoting; `\n`, `\r\n` and
/// `\r` line ends).
pub fn parse_csv(text: &str, delimiter: char, limits: &Limits) -> Result<Vec<Vec<String>>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false;
    let mut cells = 0usize;
    let mut chars = text.chars().peekable();

    let mut end_field = |row: &mut Vec<String>, field: &mut String| -> Result<()> {
        cells += 1;
        if cells > limits.max_cells {
            return Err(OdfError::limit(format!("CSV has more than {} cells", limits.max_cells)));
        }
        if row.len() >= MAX_COLS as usize {
            return Err(OdfError::limit(format!("CSV has more than {MAX_COLS} columns")));
        }
        row.push(std::mem::take(field));
        Ok(())
    };

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() && !field_started {
            in_quotes = true;
            field_started = true;
        } else if c == delimiter {
            end_field(&mut row, &mut field)?;
            field_started = false;
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            end_field(&mut row, &mut field)?;
            field_started = false;
            if rows.len() >= MAX_ROWS as usize {
                return Err(OdfError::limit(format!("CSV has more than {MAX_ROWS} rows")));
            }
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(c);
            field_started = true;
        }
    }
    if field_started || !field.is_empty() || !row.is_empty() {
        end_field(&mut row, &mut field)?;
        if rows.len() >= MAX_ROWS as usize {
            return Err(OdfError::limit(format!("CSV has more than {MAX_ROWS} rows")));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Loads CSV text into a new single-sheet workbook. Fields are interpreted
/// like typed input (numbers, dates, booleans, formulas, text).
///
/// Formulas in CSV files are evaluated; this is safe because the formula
/// engine has no functions with side effects or external access.
pub fn load_csv(bytes: &[u8], delimiter: Option<char>, limits: &Limits) -> Result<Workbook> {
    if bytes.len() as u64 > limits.max_entry_size {
        return Err(OdfError::limit("CSV file is too large"));
    }
    let text = decode(bytes);
    let delimiter = delimiter.unwrap_or_else(|| detect_delimiter(&text));
    let rows = parse_csv(&text, delimiter, limits)?;
    let mut wb = Workbook::new();
    wb.set_inputs_unchecked(0, &rows)?;
    wb.finish_loading();
    Ok(wb)
}

fn needs_neutralising(text: &str) -> bool {
    matches!(text.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r'))
}

/// Serialises one sheet as CSV.
///
/// Numbers in the General format are written with full precision; formatted
/// numbers are written as displayed. With `sanitize`, text that another
/// program could mistake for a formula is prefixed with `'`.
pub fn sheet_to_csv(wb: &Workbook, sheet: usize, delimiter: char, sanitize: bool) -> Result<String> {
    let sh = wb.sheet(sheet)?;
    let mut out = String::new();
    let last_content_row = sh
        .cells()
        .filter(|(_, cell)| cell.value != Value::Empty)
        .map(|(addr, _)| addr.row)
        .max();
    let Some(last_row) = last_content_row else {
        return Ok(out);
    };
    for row in 0..=last_row {
        let mut fields: Vec<String> = Vec::new();
        for (addr, cell) in sh.cells_in_rows(row, row) {
            if cell.value == Value::Empty {
                continue;
            }
            let col = addr.col as usize;
            if fields.len() <= col {
                fields.resize(col + 1, String::new());
            }
            let format = &wb.style_by_id(cell.style).format;
            let mut text = match (&cell.value, format) {
                (Value::Number(n), NumberFormat::General) => n.to_string(),
                (value, format) => format_value(value, format),
            };
            if sanitize && matches!(cell.value, Value::Text(_)) && needs_neutralising(&text) {
                text.insert(0, '\'');
            }
            let quote = text.contains(delimiter)
                || text.contains(['"', '\n', '\r'])
                || text.starts_with(' ')
                || text.ends_with(' ');
            fields[col] = if quote { format!("\"{}\"", text.replace('"', "\"\"")) } else { text };
        }
        out.push_str(&fields.join(&delimiter.to_string()));
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Vec<String>> {
        parse_csv(text, ',', &Limits::default()).expect("parse")
    }

    #[test]
    fn parses_quotes_and_line_endings() {
        assert_eq!(parse("a,b\n1,2\n"), vec![vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(parse("a,b\r\n1,2"), vec![vec!["a", "b"], vec!["1", "2"]]);
        assert_eq!(parse("\"x,y\",\"he said \"\"hi\"\"\"\n"), vec![vec!["x,y", "he said \"hi\""]]);
        assert_eq!(parse("\"multi\nline\",z"), vec![vec!["multi\nline", "z"]]);
        assert_eq!(parse("a,,c\n,\n"), vec![vec!["a", "", "c"], vec!["", ""]]);
        assert_eq!(parse(""), Vec::<Vec<String>>::new());
        assert_eq!(parse("\"\"\n"), vec![vec![""]]);
        assert_eq!(parse("a\rb"), vec![vec!["a"], vec!["b"]]);
        assert_eq!(parse("ab\"c\n"), vec![vec!["ab\"c"]], "quote inside an unquoted field is literal");
    }

    #[test]
    fn detects_delimiters() {
        assert_eq!(detect_delimiter("a,b,c\n1,2,3"), ',');
        assert_eq!(detect_delimiter("a;b;c\n1,5;2;3"), ';');
        assert_eq!(detect_delimiter("a\tb\tc"), '\t');
        assert_eq!(detect_delimiter("\"a;b;c;d\",x"), ',');
        assert_eq!(detect_delimiter("plain"), ',');
    }

    #[test]
    fn decodes_bom_and_latin1() {
        assert_eq!(decode(b"\xEF\xBB\xBFa"), "a");
        assert_eq!(decode(b"caf\xe9"), "café");
        assert_eq!(decode("naïve".as_bytes()), "naïve");
    }

    #[test]
    fn enforces_limits() {
        let limits = Limits { max_cells: 3, ..Limits::default() };
        assert!(parse_csv("1,2,3", ',', &limits).is_ok());
        assert!(matches!(parse_csv("1,2,3,4", ',', &limits), Err(OdfError::Limit(_))));
        let wide = vec!["x"; MAX_COLS as usize + 1].join(",");
        assert!(matches!(parse_csv(&wide, ',', &Limits::default()), Err(OdfError::Limit(_))));
    }
}
