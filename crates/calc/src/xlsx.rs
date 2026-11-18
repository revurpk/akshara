//! Microsoft Excel (`.xlsx`) import.
//!
//! Read: sheets (names, order), shared and inline strings, numbers,
//! booleans, errors, ISO dates, formulas (including shared formulas; a
//! formula Akshara cannot parse keeps its last calculated value), cell
//! styles (bold, italic, underline, text and fill colour, horizontal
//! alignment, number formats), column widths, the 1904 date system, print
//! ranges and print titles (rows), the page setup of the first sheet and
//! the title.
//!
//! Charts, images, comments, merged cells, conditional formats and
//! validation are not modelled. Workbooks are only read: saving goes to
//! `.ods`. Macros (`.xlsm`) are never run, only skipped.

use crate::address::{CellAddr, Range, MAX_COLS, MAX_ROWS};
use crate::format::{date_to_serial, NumberFormat};
use crate::formula::{parse, Expr, Syntax};
use crate::value::{ErrorKind, Value};
use crate::workbook::{CellStyle, Content, HAlign, PageSetup, PrintArea, Workbook};
use office_odf::ooxml;
use office_odf::xml::Element;
use office_odf::{Limits, Metadata, OdfError, Package, Result, Rgb};
use std::collections::HashMap;

/// `FFRRGGBB` (or `RRGGBB`) colours; theme and indexed colours are ignored.
fn argb(e: Option<&Element>) -> Option<Rgb> {
    let v = e?.attr("rgb")?;
    let hex = if v.len() == 8 { &v[2..] } else { v };
    Rgb::parse(&format!("#{hex}"))
}

fn count_decimals(code: &str) -> u8 {
    code.split_once('.')
        .map(|(_, after)| after.chars().take_while(|c| *c == '0' || *c == '#').count().min(10) as u8)
        .unwrap_or(0)
}

/// Maps an Excel number format (built-in id or format code) to the closest
/// Akshara format.
fn number_format(id: u32, code: Option<&str>) -> NumberFormat {
    match id {
        0 => return NumberFormat::General,
        1 => return NumberFormat::Number { decimals: 0, thousands: false },
        2 => return NumberFormat::Number { decimals: 2, thousands: false },
        3 => return NumberFormat::Number { decimals: 0, thousands: true },
        4 => return NumberFormat::Number { decimals: 2, thousands: true },
        9 => return NumberFormat::Percent { decimals: 0 },
        10 => return NumberFormat::Percent { decimals: 2 },
        11 | 48 => return NumberFormat::Scientific { decimals: 2 },
        14..=17 => return NumberFormat::Date,
        18..=21 | 45..=47 => return NumberFormat::Time,
        22 => return NumberFormat::DateTime,
        49 => return NumberFormat::Text,
        _ => {}
    }
    let Some(code) = code else { return NumberFormat::General };
    // Only the positive section matters; drop quoted text and escapes.
    let section = code.split(';').next().unwrap_or("");
    let mut plain = String::new();
    let mut in_quote = false;
    let mut chars = section.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '\\' => {
                chars.next();
            }
            _ if in_quote => {}
            c => plain.push(c),
        }
    }
    let lower = plain.to_lowercase();
    // Locale / currency tags: [$₹-4009], [$$-409].
    let symbol = lower.split("[$").nth(1).and_then(|t| t.split(['-', ']']).next()).filter(|s| !s.is_empty());
    let without_tags: String = {
        let mut out = String::new();
        let mut depth = 0;
        for c in lower.chars() {
            match c {
                '[' => depth += 1,
                ']' if depth > 0 => depth -= 1,
                c if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out
    };
    if lower.trim() == "@" {
        return NumberFormat::Text;
    }
    let has_date = without_tags.contains('y') || without_tags.contains('d');
    let has_time = without_tags.contains('h') || without_tags.contains('s');
    if has_date && has_time {
        return NumberFormat::DateTime;
    }
    if has_date {
        return if without_tags.find('d') < without_tags.find('m') { NumberFormat::DateDmy } else { NumberFormat::Date };
    }
    if has_time {
        return NumberFormat::Time;
    }
    let decimals = count_decimals(&without_tags);
    if without_tags.contains('%') {
        return NumberFormat::Percent { decimals };
    }
    if without_tags.contains("e+") || without_tags.contains("e-") {
        return NumberFormat::Scientific { decimals };
    }
    let currency = symbol
        .map(str::to_owned)
        .or_else(|| ['₹', '$', '€', '£', '¥'].iter().find(|c| without_tags.contains(**c)).map(|c| c.to_string()));
    if let Some(symbol) = currency {
        return NumberFormat::Currency { decimals, symbol };
    }
    if without_tags.contains("#,##,##0") {
        return NumberFormat::IndianNumber { decimals };
    }
    if without_tags.contains('0') || without_tags.contains('#') {
        return NumberFormat::Number { decimals, thousands: without_tags.contains(',') };
    }
    NumberFormat::General
}

/// Cell style ids (Akshara's) for each Excel `cellXfs` entry.
fn read_styles(xml: &Element, wb: &mut Workbook) -> Vec<u32> {
    let codes: HashMap<u32, String> = xml
        .child("x:numFmts")
        .map(|n| {
            n.children_named("x:numFmt")
                .filter_map(|f| Some((f.attr("numFmtId")?.parse().ok()?, f.attr("formatCode")?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let fonts: Vec<&Element> = xml.child("x:fonts").map(|f| f.children_named("x:font").collect()).unwrap_or_default();
    let fills: Vec<&Element> = xml.child("x:fills").map(|f| f.children_named("x:fill").collect()).unwrap_or_default();
    let on = |e: Option<&Element>| e.is_some_and(|e| !matches!(e.attr("val"), Some("0" | "false" | "none")));
    let mut out = Vec::new();
    for xf in xml.child("x:cellXfs").into_iter().flat_map(|x| x.children_named("x:xf")) {
        let num = |a: &str| xf.attr(a).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let mut style = CellStyle::default();
        let fmt_id = num("numFmtId");
        style.format = number_format(fmt_id, codes.get(&fmt_id).map(String::as_str));
        if let Some(font) = fonts.get(num("fontId") as usize) {
            style.bold = on(font.child("x:b"));
            style.italic = on(font.child("x:i"));
            style.underline = on(font.child("x:u"));
            style.color = argb(font.child("x:color")).filter(|c| *c != Rgb(0, 0, 0));
        }
        if let Some(pattern) = fills.get(num("fillId") as usize).and_then(|f| f.child("x:patternFill")) {
            if pattern.attr("patternType") == Some("solid") {
                style.background = argb(pattern.child("x:fgColor"));
            }
        }
        style.align = match xf.child("x:alignment").and_then(|a| a.attr("horizontal")) {
            Some("left") => HAlign::Left,
            Some("center" | "centerContinuous") => HAlign::Center,
            Some("right") => HAlign::Right,
            _ => HAlign::General,
        };
        out.push(wb.intern_style(style));
    }
    out
}

fn shared_strings(xml: &Element, limits: &Limits) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for si in xml.children_named("x:si") {
        if out.len() >= limits.max_cells {
            return Err(OdfError::limit("too many shared strings"));
        }
        out.push(string_item(si));
    }
    Ok(out)
}

/// The text of a string item: `<t>` or rich-text runs `<r><t>`, without
/// phonetic hints.
fn string_item(e: &Element) -> String {
    let mut s = String::new();
    for child in e.elements() {
        match child.name.as_str() {
            "x:t" => s.push_str(&child.text_content()),
            "x:r" => {
                if let Some(t) = child.child("x:t") {
                    s.push_str(&t.text_content());
                }
            }
            _ => {}
        }
    }
    s
}

/// An ISO 8601 date (`2026-08-15` or `2026-08-15T10:30:00`) as a serial.
fn iso_date(text: &str) -> Option<f64> {
    let (date, time) = text.split_once('T').unwrap_or((text, ""));
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let mut serial = date_to_serial(y, m, day)?;
    let mut t = time.trim_end_matches('Z').split(':');
    if let (Some(h), Some(min)) = (t.next(), t.next()) {
        let s: f64 = t.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        serial += (h.parse::<f64>().ok()? * 3600.0 + min.parse::<f64>().ok()? * 60.0 + s) / 86_400.0;
    }
    Some(serial)
}

/// Excel formula text to an expression, or `None` when Akshara cannot
/// parse it (unknown syntax: whole-column refs, tables, other workbooks).
fn formula(text: &str) -> Option<Expr> {
    let body = text.trim().trim_start_matches('=');
    let body = body.replace("_xlfn._xlws.", "").replace("_xlfn.", "").replace("_xlws.", "");
    match parse(&body, Syntax::Ui) {
        Expr::Invalid(_) => None,
        e => Some(e),
    }
}

/// A sheet reference in a defined name: the part after the last `!`
/// outside quotes, `$` removed.
fn after_sheet(text: &str) -> &str {
    let mut in_quote = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '\'' => in_quote = !in_quote,
            '!' if !in_quote => start = i + 1,
            _ => {}
        }
    }
    &text[start..]
}

fn parse_area(text: &str) -> Option<Range> {
    let first = text.split(',').next()?;
    let r = after_sheet(first).replace('$', "");
    let (a, b) = r.split_once(':').unwrap_or((&r, &r));
    let range = Range::new(CellAddr::parse(a)?, CellAddr::parse(b)?);
    range.is_valid().then_some(range)
}

/// Print titles such as `Sheet1!$1:$2` or `Sheet1!$A:$A,Sheet1!$1:$1`
/// (only rows are used).
fn parse_title_rows(text: &str) -> Option<(u32, u32)> {
    text.split(',').find_map(|part| {
        let r = after_sheet(part).replace('$', "");
        let (a, b) = r.split_once(':')?;
        let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
        (a >= 1 && b >= a).then(|| (a - 1, b - 1))
    })
}

struct SheetReader<'a> {
    wb: &'a mut Workbook,
    limits: &'a Limits,
    shared: &'a [String],
    styles: &'a [u32],
    date1904: bool,
    cells: usize,
}

impl SheetReader<'_> {
    fn read(&mut self, sheet: usize, xml: &Element, first: bool) -> Result<()> {
        // Column widths: Excel measures in characters of the default font
        // (about 7 px each, plus 5 px padding, at 96 dpi).
        for col in xml.child("x:cols").into_iter().flat_map(|c| c.children_named("x:col")) {
            if !matches!(col.attr("customWidth"), Some("1" | "true")) {
                continue;
            }
            let num = |a: &str| col.attr(a).and_then(|v| v.parse::<u32>().ok());
            let (Some(min), Some(max)) = (num("min"), num("max")) else { continue };
            let Some(width) = col.attr("width").and_then(|w| w.parse::<f32>().ok()).filter(|w| w.is_finite()) else { continue };
            if max >= min && max - min < 256 {
                let pt = (width * 7.0 + 5.0) * 0.75;
                for c in min.saturating_sub(1)..max.min(MAX_COLS) {
                    self.wb.load_col_width(sheet, c, pt);
                }
            }
        }

        let mut shared_formulas: HashMap<&str, (CellAddr, Expr)> = HashMap::new();
        let mut next_row = 0u32;
        for row in xml.child("x:sheetData").into_iter().flat_map(|d| d.children_named("x:row")) {
            let row_index = row.attr("r").and_then(|r| r.parse::<u32>().ok()).map(|r| r.saturating_sub(1)).unwrap_or(next_row);
            next_row = row_index + 1;
            let mut next_col = 0u32;
            for c in row.children_named("x:c") {
                let addr = c.attr("r").and_then(CellAddr::parse).unwrap_or(CellAddr::new(row_index, next_col));
                next_col = addr.col + 1;
                if addr.row >= MAX_ROWS || addr.col >= MAX_COLS {
                    continue;
                }
                self.cells += 1;
                if self.cells > self.limits.max_cells {
                    return Err(OdfError::limit(format!("workbook has more than {} cells", self.limits.max_cells)));
                }
                let style_index = c.attr("s").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
                let style = self.styles.get(style_index).copied().unwrap_or(0);
                let raw = c.child("x:v").map(Element::text_content);
                let value = match c.attr("t").unwrap_or("n") {
                    "s" => raw
                        .and_then(|i| i.trim().parse::<usize>().ok())
                        .and_then(|i| self.shared.get(i))
                        .map(|s| Value::Text(s.clone()))
                        .unwrap_or_default(),
                    "str" => raw.map(Value::Text).unwrap_or_default(),
                    "inlineStr" => c.child("x:is").map(|is| Value::Text(string_item(is))).unwrap_or_default(),
                    "b" => raw.map(|v| Value::Bool(v.trim() == "1" || v.trim() == "true")).unwrap_or_default(),
                    "e" => raw.map(|v| Value::Error(ErrorKind::parse(v.trim()).unwrap_or(ErrorKind::Value))).unwrap_or_default(),
                    "d" => raw.as_deref().and_then(iso_date).map(Value::Number).unwrap_or_default(),
                    _ => raw.and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()).map(Value::Number).unwrap_or_default(),
                };
                // Dates in the 1904 system are 1462 days later in ours.
                let value = match value {
                    Value::Number(n) if self.date1904 && is_date(&self.wb.style_by_id(style).format) => Value::Number(n + 1462.0),
                    v => v,
                };
                let expr = c.child("x:f").and_then(|f| {
                    let text = f.text_content();
                    if f.attr("t") == Some("shared") {
                        let si = f.attr("si")?;
                        if !text.trim().is_empty() {
                            let e = formula(&text)?;
                            shared_formulas.insert(si, (addr, e.clone()));
                            return Some(e);
                        }
                        let (origin, e) = shared_formulas.get(si)?;
                        return Some(e.shifted(i64::from(addr.row) - i64::from(origin.row), i64::from(addr.col) - i64::from(origin.col)));
                    }
                    if text.trim().is_empty() { None } else { formula(&text) }
                });
                let content = match (expr, &value) {
                    (Some(e), _) => Content::Formula(e),
                    (None, Value::Number(n)) => Content::Number(*n),
                    (None, Value::Text(t)) => Content::Text(t.clone()),
                    (None, Value::Bool(b)) => Content::Bool(*b),
                    (None, Value::Error(e)) => Content::Text(e.to_string()),
                    (None, Value::Empty) => Content::Empty,
                };
                self.wb.load_cell(sheet, addr, content, value, style);
            }
        }

        if first {
            self.page_setup(xml);
        }
        Ok(())
    }

    /// The first sheet's page setup becomes the workbook's.
    fn page_setup(&mut self, xml: &Element) {
        let mut page = PageSetup { grid: false, header_footer: false, ..PageSetup::default() };
        let setup = xml.child("x:pageSetup");
        let (w, h) = match setup.and_then(|s| s.attr("paperSize")).unwrap_or("9") {
            "1" => (612.0, 792.0),
            "5" => (612.0, 1008.0),
            "8" => (841.89, 1190.55),
            "11" => (419.53, 595.28),
            _ => (595.28, 841.89),
        };
        let landscape = setup.and_then(|s| s.attr("orientation")) == Some("landscape");
        (page.width, page.height) = if landscape { (h, w) } else { (w, h) };
        if let Some(m) = xml.child("x:pageMargins").and_then(|m| m.attr("left")).and_then(|v| v.parse::<f32>().ok()) {
            page.margin = (m * 72.0).clamp(0.0, page.width.min(page.height) / 3.0);
        }
        if let Some(scale) = setup.and_then(|s| s.attr("scale")).and_then(|v| v.parse::<u16>().ok()) {
            page.scale = scale.clamp(10, 400);
        }
        let fit = xml
            .child("x:sheetPr")
            .and_then(|p| p.child("x:pageSetUpPr"))
            .is_some_and(|p| matches!(p.attr("fitToPage"), Some("1" | "true")));
        page.fit_width = fit && setup.and_then(|s| s.attr("fitToWidth")).unwrap_or("1") == "1";
        page.grid = xml.child("x:printOptions").is_some_and(|p| matches!(p.attr("gridLines"), Some("1" | "true")));
        page.header_footer = xml.child("x:headerFooter").is_some_and(|hf| {
            ["x:oddHeader", "x:oddFooter"].iter().any(|n| hf.child(n).is_some_and(|e| !e.text_content().trim().is_empty()))
        });
        self.wb.set_loaded_page(page);
    }
}

fn is_date(f: &NumberFormat) -> bool {
    matches!(f, NumberFormat::Date | NumberFormat::DateDmy | NumberFormat::DateTime)
}

/// Loads a workbook from the bytes of an `.xlsx` (or `.xlsm`, `.xltx`) file.
pub fn load_xlsx(bytes: &[u8], limits: &Limits) -> Result<Workbook> {
    ooxml::reject_binary_office(bytes)?;
    let package = Package::read_filtered(bytes, limits, |n| {
        (n.starts_with("xl/") && (n.ends_with(".xml") || n.ends_with(".rels"))
            && !n.starts_with("xl/media/")
            && !n.starts_with("xl/drawings/")
            && !n.starts_with("xl/charts/"))
            || n == "_rels/.rels"
            || n == "docProps/core.xml"
    })?;
    let main = ooxml::main_part(&package, "xl/workbook.xml", limits)?;
    let book = package
        .get_xml(&main, limits)?
        .ok_or_else(|| OdfError::invalid("not an Excel workbook (no xl/workbook.xml)"))?;
    let rels = ooxml::relationships(&package, &main, limits)?;
    let part = |kind: &str| rels.values().find(|r| r.kind.ends_with(kind) && !r.external).map(|r| r.target.clone());

    let mut wb = Workbook::empty();
    let shared = match part("/sharedStrings") {
        Some(p) => match package.get_xml(&p, limits)? {
            Some(x) => shared_strings(&x, limits)?,
            None => Vec::new(),
        },
        None => Vec::new(),
    };
    let styles = match part("/styles") {
        Some(p) => package.get_xml(&p, limits)?.map(|x| read_styles(&x, &mut wb)).unwrap_or_default(),
        None => Vec::new(),
    };
    let date1904 = book.child("x:workbookPr").is_some_and(|p| matches!(p.attr("date1904"), Some("1" | "true")));

    // Print ranges and titles are defined names local to a sheet.
    let mut areas: HashMap<usize, PrintArea> = HashMap::new();
    for name in book.child("x:definedNames").into_iter().flat_map(|d| d.children_named("x:definedName")) {
        let Some(sheet) = name.attr("localSheetId").and_then(|v| v.parse::<usize>().ok()) else { continue };
        let text = name.text_content();
        match name.attr("name") {
            Some("_xlnm.Print_Area") => areas.entry(sheet).or_default().range = parse_area(&text),
            Some("_xlnm.Print_Titles") => areas.entry(sheet).or_default().repeat_rows = parse_title_rows(&text),
            _ => {}
        }
    }

    let mut reader = SheetReader { wb: &mut wb, limits, shared: &shared, styles: &styles, date1904, cells: 0 };
    let sheets: Vec<&Element> = book.child("x:sheets").map(|s| s.children_named("x:sheet").collect()).unwrap_or_default();
    for (i, s) in sheets.iter().enumerate() {
        let index = reader.wb.push_loaded_sheet(s.attr("name").unwrap_or(""))?;
        let target = s.attr("r:id").and_then(|id| rels.get(id)).filter(|r| !r.external).map(|r| r.target.clone());
        // Chart sheets and missing parts give an empty sheet.
        if let Some(xml) = match target {
            Some(t) => package.get_xml(&t, limits)?,
            None => None,
        } {
            if xml.name == "x:worksheet" {
                reader.read(index, &xml, i == 0)?;
            }
        }
        if let Some(area) = areas.get(&i) {
            reader.wb.load_print_area(index, *area);
        }
    }

    if let Some(core) = package.get_xml("docProps/core.xml", limits)? {
        let text = |name: &str| core.child(name).map(Element::text_content).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
        wb.meta = Metadata { title: text("dc:title"), subject: text("dc:subject"), creator: text("dc:creator"), ..Default::default() };
    }
    wb.finish_loading();
    Ok(wb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats_map_to_the_closest_kind() {
        assert_eq!(number_format(0, None), NumberFormat::General);
        assert_eq!(number_format(4, None), NumberFormat::Number { decimals: 2, thousands: true });
        assert_eq!(number_format(14, None), NumberFormat::Date);
        let custom = |code: &str| number_format(164, Some(code));
        assert_eq!(custom("dd/mm/yyyy"), NumberFormat::DateDmy);
        assert_eq!(custom("yyyy-mm-dd hh:mm"), NumberFormat::DateTime);
        assert_eq!(custom("hh:mm:ss"), NumberFormat::Time);
        assert_eq!(custom("0.0%"), NumberFormat::Percent { decimals: 1 });
        assert_eq!(custom("[$₹-4009]\\ #,##,##0.00"), NumberFormat::Currency { decimals: 2, symbol: "₹".into() });
        assert_eq!(custom("\"$\"#,##0.00;[Red]\\-\"$\"#,##0.00"), NumberFormat::Number { decimals: 2, thousands: true });
        assert_eq!(custom("$#,##0"), NumberFormat::Currency { decimals: 0, symbol: "$".into() });
        assert_eq!(custom("#,##,##0"), NumberFormat::IndianNumber { decimals: 0 });
        assert_eq!(custom("0.000E+00"), NumberFormat::Scientific { decimals: 3 });
        assert_eq!(custom("@"), NumberFormat::Text);
        assert_eq!(custom("General"), NumberFormat::General);
    }

    #[test]
    fn defined_names_and_dates_parse() {
        assert_eq!(parse_area("'My Sheet'!$A$1:$D$20").map(|r| r.to_a1()), Some("A1:D20".into()));
        assert_eq!(parse_title_rows("Sheet1!$A:$A,Sheet1!$1:$2"), Some((0, 1)));
        assert_eq!(parse_title_rows("Sheet1!$A:$B"), None);
        assert_eq!(iso_date("1900-01-01"), date_to_serial(1900, 1, 1));
        assert!(iso_date("2026-08-15T12:00:00").is_some_and(|s| (s.fract() - 0.5).abs() < 1e-9));
        assert!(formula("_xlfn.CONCAT(A1,B1)").is_some());
    }
}
