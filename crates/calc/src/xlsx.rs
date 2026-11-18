//! Microsoft Excel (`.xlsx`) import and export.
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
//! validation are not modelled. Macros (`.xlsm`) are never run, only
//! skipped.
//!
//! [`save_xlsx`] writes everything the model holds: sheets, values,
//! formulas (in Excel syntax, with their last value), styles and number
//! formats, column widths, print ranges, title rows and the page setup.

use crate::address::{CellAddr, Range, MAX_COLS, MAX_ROWS};
use crate::format::{date_to_serial, NumberFormat};
use crate::formula::{parse, Expr, Syntax};
use crate::value::{ErrorKind, Value};
use crate::workbook::{CellStyle, Content, HAlign, PageSetup, PrintArea, Workbook};
use office_odf::ooxml;
use office_odf::xml::{Element, XmlWriter};
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

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const DOC_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Excel format code (and built-in id when there is one) for a format.
fn format_code(f: &NumberFormat) -> (Option<u32>, String) {
    let decimals = |d: u8| if d == 0 { String::new() } else { format!(".{}", "0".repeat(usize::from(d))) };
    match f {
        NumberFormat::General => (Some(0), "General".into()),
        NumberFormat::Number { decimals: 0, thousands: false } => (Some(1), "0".into()),
        NumberFormat::Number { decimals: 2, thousands: false } => (Some(2), "0.00".into()),
        NumberFormat::Number { decimals: 0, thousands: true } => (Some(3), "#,##0".into()),
        NumberFormat::Number { decimals: 2, thousands: true } => (Some(4), "#,##0.00".into()),
        NumberFormat::Number { decimals: d, thousands } => {
            (None, format!("{}{}", if *thousands { "#,##0" } else { "0" }, decimals(*d)))
        }
        NumberFormat::Percent { decimals: 0 } => (Some(9), "0%".into()),
        NumberFormat::Percent { decimals: 2 } => (Some(10), "0.00%".into()),
        NumberFormat::Percent { decimals: d } => (None, format!("0{}%", decimals(*d))),
        NumberFormat::Currency { decimals: d, symbol } => (None, format!("[${symbol}]#,##0{}", decimals(*d))),
        NumberFormat::Scientific { decimals: d } => (None, format!("0{}E+00", decimals(*d))),
        NumberFormat::IndianNumber { decimals: d } => (None, format!("#,##,##0{}", decimals(*d))),
        NumberFormat::Date => (None, "yyyy-mm-dd".into()),
        NumberFormat::DateDmy => (None, "dd-mm-yyyy".into()),
        NumberFormat::Time => (None, "hh:mm:ss".into()),
        NumberFormat::DateTime => (None, "yyyy-mm-dd hh:mm:ss".into()),
        NumberFormat::Text => (Some(49), "@".into()),
    }
}

fn hex_argb(c: Rgb) -> String {
    format!("FF{}", c.to_hex().trim_start_matches('#').to_uppercase())
}

/// A sheet name quoted for a reference (`'My sheet'`).
fn quoted(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

fn excel_ref(a: CellAddr) -> String {
    format!("${}${}", crate::address::col_name(a.col), a.row + 1)
}

/// Shared strings, in order of first use.
#[derive(Default)]
struct Strings {
    list: Vec<String>,
    index: HashMap<String, usize>,
}

impl Strings {
    fn id(&mut self, s: &str) -> usize {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        self.list.push(s.to_owned());
        self.index.insert(s.to_owned(), self.list.len() - 1);
        self.list.len() - 1
    }
}

fn styles_xml(wb: &Workbook, used: &[u32]) -> (String, HashMap<u32, usize>) {
    // Distinct fonts, fills and formats of the used styles.
    let mut fonts: Vec<(bool, bool, bool, Option<Rgb>)> = vec![(false, false, false, None)];
    let mut fills: Vec<Option<Rgb>> = vec![None, None]; // Excel reserves 0 (none) and 1 (gray125)
    let mut formats: Vec<String> = Vec::new();
    let mut xfs: Vec<(u32, usize, usize, HAlign)> = vec![(0, 0, 0, HAlign::General)];
    let mut xf_of = HashMap::from([(0u32, 0usize)]);
    for id in used {
        if *id == 0 {
            continue;
        }
        let st = wb.style_by_id(*id);
        let font = (st.bold, st.italic, st.underline, st.color);
        let font_id = fonts.iter().position(|f| *f == font).unwrap_or_else(|| {
            fonts.push(font);
            fonts.len() - 1
        });
        let fill_id = match st.background {
            None => 0,
            Some(c) => fills.iter().skip(2).position(|f| *f == Some(c)).map(|i| i + 2).unwrap_or_else(|| {
                fills.push(Some(c));
                fills.len() - 1
            }),
        };
        let (builtin, code) = format_code(&st.format);
        let fmt_id = builtin.unwrap_or_else(|| {
            let i = formats.iter().position(|f| *f == code).unwrap_or_else(|| {
                formats.push(code.clone());
                formats.len() - 1
            });
            164 + i as u32
        });
        let xf = (fmt_id, font_id, fill_id, st.align);
        let index = xfs.iter().position(|x| *x == xf).unwrap_or_else(|| {
            xfs.push(xf);
            xfs.len() - 1
        });
        xf_of.insert(*id, index);
    }
    let mut w = XmlWriter::new();
    w.start("styleSheet").attr("xmlns", MAIN_NS);
    if !formats.is_empty() {
        w.start("numFmts").attr("count", &formats.len().to_string());
        for (i, code) in formats.iter().enumerate() {
            w.start("numFmt").attr("numFmtId", &(164 + i).to_string()).attr("formatCode", code).end();
        }
        w.end();
    }
    w.start("fonts").attr("count", &fonts.len().to_string());
    for (bold, italic, underline, color) in &fonts {
        w.start("font");
        if *bold {
            w.start("b").end();
        }
        if *italic {
            w.start("i").end();
        }
        if *underline {
            w.start("u").end();
        }
        w.start("sz").attr("val", "10").end();
        if let Some(c) = color {
            w.start("color").attr("rgb", &hex_argb(*c)).end();
        }
        w.start("name").attr("val", "Arial").end();
        w.end();
    }
    w.end();
    w.start("fills").attr("count", &fills.len().to_string());
    for (i, fill) in fills.iter().enumerate() {
        w.start("fill").start("patternFill");
        match (i, fill) {
            (0, _) => {
                w.attr("patternType", "none");
            }
            (1, _) => {
                w.attr("patternType", "gray125");
            }
            (_, Some(c)) => {
                w.attr("patternType", "solid");
                w.start("fgColor").attr("rgb", &hex_argb(*c)).end();
                w.start("bgColor").attr("indexed", "64").end();
            }
            _ => {}
        }
        w.end().end();
    }
    w.end();
    w.start("borders").attr("count", "1").start("border");
    for side in ["left", "right", "top", "bottom", "diagonal"] {
        w.start(side).end();
    }
    w.end().end();
    w.start("cellStyleXfs").attr("count", "1").start("xf").attr("numFmtId", "0").attr("fontId", "0").attr("fillId", "0").attr("borderId", "0").end().end();
    w.start("cellXfs").attr("count", &xfs.len().to_string());
    for (fmt, font, fill, align) in &xfs {
        w.start("xf")
            .attr("numFmtId", &fmt.to_string())
            .attr("fontId", &font.to_string())
            .attr("fillId", &fill.to_string())
            .attr("borderId", "0")
            .attr("xfId", "0");
        if *fmt != 0 {
            w.attr("applyNumberFormat", "1");
        }
        if *font != 0 {
            w.attr("applyFont", "1");
        }
        if *fill != 0 {
            w.attr("applyFill", "1");
        }
        let horizontal = match align {
            HAlign::General => None,
            HAlign::Left => Some("left"),
            HAlign::Center => Some("center"),
            HAlign::Right => Some("right"),
        };
        if let Some(h) = horizontal {
            w.attr("applyAlignment", "1");
            w.start("alignment").attr("horizontal", h).end();
        }
        w.end();
    }
    w.end();
    w.start("cellStyles").attr("count", "1").start("cellStyle").attr("name", "Normal").attr("xfId", "0").attr("builtinId", "0").end().end();
    (w.finish(), xf_of)
}

fn number_text(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 { format!("{n:.0}") } else { format!("{n}") }
}

fn sheet_xml(wb: &Workbook, index: usize, strings: &mut Strings, xf_of: &HashMap<u32, usize>) -> Result<String> {
    let sheet = wb.sheet(index)?;
    let page = wb.page();
    let mut w = XmlWriter::new();
    w.start("worksheet").attr("xmlns", MAIN_NS).attr("xmlns:r", REL_NS);
    if page.fit_width {
        w.start("sheetPr").start("pageSetUpPr").attr("fitToPage", "1").end().end();
    }
    if let Some(r) = sheet.used_range() {
        w.start("dimension").attr("ref", &r.to_a1()).end();
    }
    if !sheet.col_widths.is_empty() {
        w.start("cols");
        for (col, pt) in &sheet.col_widths {
            let chars = ((pt / 0.75 - 5.0) / 7.0).max(0.5);
            w.start("col")
                .attr("min", &(col + 1).to_string())
                .attr("max", &(col + 1).to_string())
                .attr("width", &format!("{chars:.4}"))
                .attr("customWidth", "1")
                .end();
        }
        w.end();
    }
    w.start("sheetData");
    let mut current: Option<u32> = None;
    for (addr, cell) in sheet.cells() {
        if current != Some(addr.row) {
            if current.is_some() {
                w.end();
            }
            w.start("row").attr("r", &(addr.row + 1).to_string());
            current = Some(addr.row);
        }
        w.start("c").attr("r", &addr.to_a1());
        if let Some(xf) = xf_of.get(&cell.style).filter(|x| **x != 0) {
            w.attr("s", &xf.to_string());
        }
        match &cell.content {
            Content::Empty => {}
            Content::Number(n) => {
                w.start("v").text(&number_text(*n)).end();
            }
            Content::Text(t) => {
                w.attr("t", "s");
                w.start("v").text(&strings.id(t).to_string()).end();
            }
            Content::Bool(b) => {
                w.attr("t", "b");
                w.start("v").text(if *b { "1" } else { "0" }).end();
            }
            Content::Formula(expr) => {
                match &cell.value {
                    Value::Text(_) => {
                        w.attr("t", "str");
                    }
                    Value::Bool(_) => {
                        w.attr("t", "b");
                    }
                    Value::Error(_) => {
                        w.attr("t", "e");
                    }
                    _ => {}
                }
                w.start("f").text(&expr.to_source(Syntax::Excel)).end();
                match &cell.value {
                    Value::Number(n) => {
                        w.start("v").text(&number_text(*n)).end();
                    }
                    Value::Text(t) => {
                        w.start("v").text(t).end();
                    }
                    Value::Bool(b) => {
                        w.start("v").text(if *b { "1" } else { "0" }).end();
                    }
                    Value::Error(e) => {
                        w.start("v").text(&e.to_string()).end();
                    }
                    Value::Empty => {}
                }
            }
        }
        w.end();
    }
    if current.is_some() {
        w.end();
    }
    w.end(); // sheetData
    if page.grid {
        w.start("printOptions").attr("gridLines", "1").end();
    }
    let inches = |pt: f32| format!("{:.4}", pt / 72.0);
    w.start("pageMargins")
        .attr("left", &inches(page.margin))
        .attr("right", &inches(page.margin))
        .attr("top", &inches(page.margin))
        .attr("bottom", &inches(page.margin))
        .attr("header", "0.3")
        .attr("footer", "0.3")
        .end();
    let (short, long) = (page.width.min(page.height), page.width.max(page.height));
    let close = |a: f32, b: f32| (a - b).abs() < 2.0;
    let paper = if close(short, 612.0) && close(long, 792.0) {
        "1"
    } else if close(short, 612.0) && close(long, 1008.0) {
        "5"
    } else if close(short, 841.89) && close(long, 1190.55) {
        "8"
    } else if close(short, 419.53) && close(long, 595.28) {
        "11"
    } else {
        "9"
    };
    w.start("pageSetup")
        .attr("paperSize", paper)
        .attr("orientation", if page.width > page.height { "landscape" } else { "portrait" });
    if page.fit_width {
        w.attr("fitToWidth", "1").attr("fitToHeight", "0");
    } else {
        w.attr("scale", &page.scale.to_string());
    }
    w.end();
    if page.header_footer {
        // Sheet name at the top, page number at the bottom, centred.
        w.start("headerFooter");
        w.start("oddHeader").text("&C&A").end();
        w.start("oddFooter").text("&CPage &P").end();
        w.end();
    }
    Ok(w.finish())
}

/// Serialises a workbook to the bytes of an `.xlsx` file.
pub fn save_xlsx(wb: &Workbook) -> Result<Vec<u8>> {
    let mut package = Package::new("");
    let count = wb.sheet_count();
    let mut used: Vec<u32> = wb.sheets().iter().flat_map(|s| s.cells().map(|(_, c)| c.style)).collect();
    used.sort_unstable();
    used.dedup();
    let (styles, xf_of) = styles_xml(wb, &used);
    let mut strings = Strings::default();
    for i in 0..count {
        package.insert(format!("xl/worksheets/sheet{}.xml", i + 1), sheet_xml(wb, i, &mut strings, &xf_of)?.into_bytes())?;
    }

    let mut types = XmlWriter::new();
    types.start("Types").attr("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types");
    types.start("Default").attr("Extension", "rels").attr("ContentType", "application/vnd.openxmlformats-package.relationships+xml").end();
    types.start("Default").attr("Extension", "xml").attr("ContentType", "application/xml").end();
    let over = |w: &mut XmlWriter, part: &str, kind: &str| {
        w.start("Override").attr("PartName", part).attr("ContentType", kind).end();
    };
    over(&mut types, "/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml");
    for i in 0..count {
        over(&mut types, &format!("/xl/worksheets/sheet{}.xml", i + 1), "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml");
    }
    over(&mut types, "/xl/styles.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml");
    over(&mut types, "/xl/sharedStrings.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml");
    over(&mut types, "/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml");
    package.insert("[Content_Types].xml", types.finish().into_bytes())?;

    let mut rels = XmlWriter::new();
    rels.start("Relationships").attr("xmlns", PKG_REL_NS);
    rels.start("Relationship").attr("Id", "rId1").attr("Type", &format!("{DOC_REL}/officeDocument")).attr("Target", "xl/workbook.xml").end();
    rels.start("Relationship")
        .attr("Id", "rId2")
        .attr("Type", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties")
        .attr("Target", "docProps/core.xml")
        .end();
    package.insert("_rels/.rels", rels.finish().into_bytes())?;

    let mut book_rels = XmlWriter::new();
    book_rels.start("Relationships").attr("xmlns", PKG_REL_NS);
    for i in 0..count {
        book_rels
            .start("Relationship")
            .attr("Id", &format!("rId{}", i + 1))
            .attr("Type", &format!("{DOC_REL}/worksheet"))
            .attr("Target", &format!("worksheets/sheet{}.xml", i + 1))
            .end();
    }
    book_rels.start("Relationship").attr("Id", &format!("rId{}", count + 1)).attr("Type", &format!("{DOC_REL}/styles")).attr("Target", "styles.xml").end();
    book_rels
        .start("Relationship")
        .attr("Id", &format!("rId{}", count + 2))
        .attr("Type", &format!("{DOC_REL}/sharedStrings"))
        .attr("Target", "sharedStrings.xml")
        .end();
    package.insert("xl/_rels/workbook.xml.rels", book_rels.finish().into_bytes())?;

    let mut book = XmlWriter::new();
    book.start("workbook").attr("xmlns", MAIN_NS).attr("xmlns:r", REL_NS);
    book.start("workbookPr").end();
    book.start("bookViews").start("workbookView").end().end();
    book.start("sheets");
    for (i, sheet) in wb.sheets().iter().enumerate() {
        book.start("sheet")
            .attr("name", &sheet.name)
            .attr("sheetId", &(i + 1).to_string())
            .attr("r:id", &format!("rId{}", i + 1))
            .end();
    }
    book.end();
    let names: Vec<(usize, &str, String)> = wb
        .sheets()
        .iter()
        .enumerate()
        .flat_map(|(i, sheet)| {
            let area = sheet.print_area();
            let q = quoted(&sheet.name);
            let range = area.range.map(|r| (i, "_xlnm.Print_Area", format!("{q}!{}:{}", excel_ref(r.start), excel_ref(r.end))));
            let titles = area.repeat_rows.map(|(a, b)| (i, "_xlnm.Print_Titles", format!("{q}!${}:${}", a + 1, b + 1)));
            range.into_iter().chain(titles)
        })
        .collect();
    if !names.is_empty() {
        book.start("definedNames");
        for (i, name, value) in &names {
            book.start("definedName").attr("name", name).attr("localSheetId", &i.to_string()).text(value).end();
        }
        book.end();
    }
    package.insert("xl/workbook.xml", book.finish().into_bytes())?;

    package.insert("xl/styles.xml", styles.into_bytes())?;
    let mut sst = XmlWriter::new();
    sst.start("sst").attr("xmlns", MAIN_NS).attr("count", &strings.list.len().to_string()).attr("uniqueCount", &strings.list.len().to_string());
    for s in &strings.list {
        sst.start("si").start("t");
        if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) || s.contains('\n') {
            sst.attr("xml:space", "preserve");
        }
        sst.text(s).end().end();
    }
    package.insert("xl/sharedStrings.xml", sst.finish().into_bytes())?;
    package.insert("docProps/core.xml", core_xml(&wb.meta).into_bytes())?;
    package.write_plain()
}

/// `docProps/core.xml` with the title, subject and author.
pub(crate) fn core_xml(meta: &Metadata) -> String {
    let mut w = XmlWriter::new();
    w.start("cp:coreProperties")
        .attr("xmlns:cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties")
        .attr("xmlns:dc", "http://purl.org/dc/elements/1.1/");
    for (name, value) in [("dc:title", &meta.title), ("dc:subject", &meta.subject), ("dc:creator", &meta.creator)] {
        if let Some(v) = value {
            w.start(name).text(v).end();
        }
    }
    w.finish()
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
