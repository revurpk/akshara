//! OpenDocument Spreadsheet (`.ods`) import and export.
//!
//! Supported: sheets, cell values of every ODF value type, formulas
//! (OpenFormula), cell styles (bold, italic, underline, colours, alignment,
//! number format class) and column widths. Not preserved: charts, images,
//! comments, merged cells, conditional formatting, named ranges, custom date
//! format patterns and row heights.

use crate::address::{CellAddr, MAX_COLS, MAX_ROWS};
use crate::format::{self, format_value, NumberFormat};
use crate::formula::parse_open_formula;
use crate::value::Value;
use crate::workbook::{CellStyle, Content, HAlign, Workbook};
use office_odf::util::{format_length_in, parse_length_pt};
use office_odf::xml::{Element, Node, XmlWriter};
use office_odf::{ns, Limits, Metadata, OdfError, Package, Result, Rgb};
use std::collections::{BTreeMap, HashMap};

/// Style-only cell runs longer than this (in either direction) are not
/// materialised on import; formatting of large empty areas is dropped rather
/// than turned into millions of cells.
const MAX_STYLE_ONLY_RUN: u32 = 64;

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

struct RawStyle {
    style: CellStyle,
    data_style: Option<String>,
}

/// Data styles whose name starts with this prefix use Indian digit grouping.
/// ODF can only say "grouping on"; the name carries the lakh/crore choice
/// (other applications show ordinary thousands grouping).
const INDIAN_STYLE_PREFIX: &str = "NIN";

fn parse_data_style(e: &Element) -> Option<NumberFormat> {
    let decimals = |child: &Element| -> Option<u8> {
        child.attr("number:decimal-places").and_then(|d| d.parse::<u8>().ok()).map(|d| d.min(format::MAX_DECIMALS))
    };
    Some(match e.name.as_str() {
        "number:number-style" => {
            if let Some(sci) = e.child("number:scientific-number") {
                NumberFormat::Scientific { decimals: decimals(sci).unwrap_or(2) }
            } else {
                let number = e.child("number:number")?;
                let thousands = number.attr("number:grouping") == Some("true");
                if thousands && e.attr("style:name").is_some_and(|n| n.starts_with(INDIAN_STYLE_PREFIX)) {
                    return Some(NumberFormat::IndianNumber { decimals: decimals(number).unwrap_or(0) });
                }
                match decimals(number) {
                    None if !thousands => NumberFormat::General,
                    d => NumberFormat::Number { decimals: d.unwrap_or(0), thousands },
                }
            }
        }
        "number:percentage-style" => {
            NumberFormat::Percent { decimals: e.child("number:number").and_then(decimals).unwrap_or(0) }
        }
        "number:currency-style" => NumberFormat::Currency {
            decimals: e.child("number:number").and_then(decimals).unwrap_or(2),
            symbol: e
                .child("number:currency-symbol")
                .map(Element::text_content)
                .filter(|s| !s.is_empty() && s.chars().count() <= 8)
                .unwrap_or_else(|| "$".into()),
        },
        "number:date-style" => {
            let day_first = e
                .elements()
                .find(|c| matches!(c.name.as_str(), "number:day" | "number:month" | "number:year"))
                .is_some_and(|c| c.name == "number:day");
            if e.child("number:hours").is_some() {
                NumberFormat::DateTime
            } else if day_first {
                NumberFormat::DateDmy
            } else {
                NumberFormat::Date
            }
        }
        "number:time-style" => NumberFormat::Time,
        "number:text-style" => NumberFormat::Text,
        _ => return None,
    })
}

fn parse_cell_style(e: &Element) -> RawStyle {
    let mut style = CellStyle::default();
    if let Some(text) = e.child("style:text-properties") {
        style.bold = text.attr("fo:font-weight") == Some("bold");
        style.italic = text.attr("fo:font-style") == Some("italic");
        style.underline = text.attr("style:text-underline-style").is_some_and(|v| v != "none");
        style.color = text.attr("fo:color").and_then(Rgb::parse);
    }
    if let Some(cell) = e.child("style:table-cell-properties") {
        style.background = cell.attr("fo:background-color").and_then(Rgb::parse);
    }
    if let Some(para) = e.child("style:paragraph-properties") {
        style.align = match para.attr("fo:text-align") {
            Some("start" | "left") => HAlign::Left,
            Some("center") => HAlign::Center,
            Some("end" | "right") => HAlign::Right,
            _ => HAlign::General,
        };
    }
    RawStyle { style, data_style: e.attr("style:data-style-name").map(str::to_owned) }
}

/// Text of a paragraph, expanding ODF whitespace elements.
fn paragraph_text(e: &Element, out: &mut String) {
    for child in &e.children {
        match child {
            Node::Text(t) => out.push_str(t),
            Node::Element(el) => match el.name.as_str() {
                "text:s" => {
                    let count = el.attr("text:c").and_then(|c| c.parse::<usize>().ok()).unwrap_or(1).min(1024);
                    out.extend(std::iter::repeat_n(' ', count));
                }
                "text:tab" => out.push('\t'),
                "text:line-break" => out.push('\n'),
                "office:annotation" | "text:note" => {}
                _ => paragraph_text(el, out),
            },
        }
    }
}

fn cell_text(cell: &Element) -> String {
    let mut out = String::new();
    for (i, p) in cell.children_named("text:p").enumerate() {
        if i > 0 {
            out.push('\n');
        }
        paragraph_text(p, &mut out);
    }
    out
}

/// Parses an ISO 8601 duration such as `PT13H45M30S` into a day fraction.
fn parse_duration(s: &str) -> Option<f64> {
    let rest = s.strip_prefix("PT")?;
    let mut seconds = 0.0;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' | 'M' | 'S' => {
                let value: f64 = number.parse().ok()?;
                seconds += value * match c {
                    'H' => 3600.0,
                    'M' => 60.0,
                    _ => 1.0,
                };
                number.clear();
            }
            _ => return None,
        }
    }
    (number.is_empty() && seconds.is_finite()).then_some(seconds / 86_400.0)
}

struct Loader<'a> {
    wb: Workbook,
    limits: &'a Limits,
    cell_styles: HashMap<String, u32>,
    column_widths: HashMap<String, f32>,
    cells_loaded: usize,
    /// Cache of "style id with an implied format applied".
    implied: HashMap<(u32, NumberFormat), u32>,
}

struct ProtoCell {
    content: Content,
    value: Value,
    style: u32,
}

impl Loader<'_> {
    fn with_format(&mut self, style: u32, format: NumberFormat) -> u32 {
        if self.wb.style_by_id(style).format != NumberFormat::General {
            return style;
        }
        if let Some(id) = self.implied.get(&(style, format.clone())) {
            return *id;
        }
        let new_style = CellStyle { format: format.clone(), ..self.wb.style_by_id(style).clone() };
        let id = self.wb.intern_style(new_style);
        self.implied.insert((style, format), id);
        id
    }

    fn parse_cell(&mut self, cell: &Element) -> Option<ProtoCell> {
        if cell.name != "table:table-cell" {
            return None;
        }
        let mut style =
            cell.attr("table:style-name").and_then(|name| self.cell_styles.get(name)).copied().unwrap_or(0);
        let number = |attr: &str| cell.attr(attr).and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| n.is_finite());
        let value = match cell.attr("office:value-type") {
            Some("float") => number("office:value").map(Value::Number),
            Some("percentage") => {
                style = self.with_format(style, NumberFormat::Percent { decimals: 0 });
                number("office:value").map(Value::Number)
            }
            Some("currency") => {
                style = self.with_format(style, NumberFormat::Currency { decimals: 2, symbol: "$".into() });
                number("office:value").map(Value::Number)
            }
            Some("date") => cell.attr("office:date-value").and_then(format::parse_iso_date).map(|(serial, time)| {
                style = self.with_format(style, if time { NumberFormat::DateTime } else { NumberFormat::Date });
                Value::Number(serial)
            }),
            Some("time") => cell.attr("office:time-value").and_then(parse_duration).map(|fraction| {
                style = self.with_format(style, NumberFormat::Time);
                Value::Number(fraction)
            }),
            Some("boolean") => Some(Value::Bool(cell.attr("office:boolean-value") == Some("true"))),
            Some("string") => Some(Value::Text(
                cell.attr("office:string-value").map(str::to_owned).unwrap_or_else(|| cell_text(cell)),
            )),
            _ => None,
        }
        .unwrap_or(Value::Empty);

        let content = match cell.attr("table:formula") {
            Some(formula) => Content::Formula(parse_open_formula(formula)),
            None => match &value {
                Value::Empty | Value::Error(_) => Content::Empty,
                Value::Number(n) => Content::Number(*n),
                Value::Text(s) => Content::Text(s.clone()),
                Value::Bool(b) => Content::Bool(*b),
            },
        };
        (content != Content::Empty || style != 0).then_some(ProtoCell { content, value, style })
    }

    fn count_cells(&mut self, n: usize) -> Result<()> {
        self.cells_loaded = self.cells_loaded.saturating_add(n);
        if self.cells_loaded > self.limits.max_cells {
            return Err(OdfError::limit(format!("spreadsheet has more than {} cells", self.limits.max_cells)));
        }
        Ok(())
    }

    fn load_row(&mut self, sheet: usize, row_el: &Element, row: &mut u32) -> Result<()> {
        let row_repeat =
            row_el.attr("table:number-rows-repeated").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).max(1);
        // Parse the row once into (first column, repeat count, cell).
        let mut col: u32 = 0;
        let mut runs: Vec<(u32, u32, ProtoCell)> = Vec::new();
        for cell_el in row_el.elements() {
            if cell_el.name != "table:table-cell" && cell_el.name != "table:covered-table-cell" {
                continue;
            }
            let repeat =
                cell_el.attr("table:number-columns-repeated").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).max(1);
            if col < MAX_COLS {
                if let Some(proto) = self.parse_cell(cell_el) {
                    let style_only = proto.content == Content::Empty;
                    if !style_only || (repeat <= MAX_STYLE_ONLY_RUN && row_repeat <= MAX_STYLE_ONLY_RUN) {
                        runs.push((col, repeat.min(MAX_COLS - col), proto));
                    }
                }
            }
            col = col.saturating_add(repeat);
        }
        if !runs.is_empty() {
            let rows_here = row_repeat.min(MAX_ROWS.saturating_sub(*row));
            let per_row: u64 = runs.iter().map(|(_, repeat, _)| u64::from(*repeat)).sum();
            let total = per_row.saturating_mul(u64::from(rows_here));
            self.count_cells(usize::try_from(total).unwrap_or(usize::MAX))?;
            for r in 0..rows_here {
                for (first_col, repeat, proto) in &runs {
                    for c in 0..*repeat {
                        self.wb.load_cell(
                            sheet,
                            CellAddr::new(*row + r, first_col + c),
                            proto.content.clone(),
                            proto.value.clone(),
                            proto.style,
                        );
                    }
                }
            }
        }
        *row = row.saturating_add(row_repeat);
        Ok(())
    }

    /// Walks the children of a table (or of one of its grouping elements).
    fn load_table_children(&mut self, sheet: usize, parent: &Element, row: &mut u32, col: &mut u32) -> Result<()> {
        for child in parent.elements() {
            match child.name.as_str() {
                "table:table-column" => {
                    let repeat = child
                        .attr("table:number-columns-repeated")
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(1)
                        .max(1);
                    let width = child.attr("table:style-name").and_then(|name| self.column_widths.get(name)).copied();
                    if let Some(width) = width {
                        // Only explicit, reasonably short runs keep their width.
                        if repeat <= 256 {
                            for c in *col..col.saturating_add(repeat).min(MAX_COLS) {
                                self.wb.load_col_width(sheet, c, width);
                            }
                        }
                    }
                    *col = col.saturating_add(repeat);
                }
                "table:table-row" => self.load_row(sheet, child, row)?,
                "table:table-columns"
                | "table:table-header-columns"
                | "table:table-column-group"
                | "table:table-rows"
                | "table:table-header-rows"
                | "table:table-row-group" => self.load_table_children(sheet, child, row, col)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Loads a workbook from the bytes of an `.ods` file.
pub fn load_ods(bytes: &[u8], limits: &Limits) -> Result<Workbook> {
    let package = Package::read_filtered(bytes, limits, |name| matches!(name, "content.xml" | "meta.xml"))?;
    package.expect_mimetype(&[ns::MIME_ODS])?;
    let content = package.require_xml("content.xml", limits)?;

    let mut loader = Loader {
        wb: Workbook::empty(),
        limits,
        cell_styles: HashMap::new(),
        column_widths: HashMap::new(),
        cells_loaded: 0,
        implied: HashMap::new(),
    };

    if let Some(styles) = content.child("office:automatic-styles") {
        let mut data_styles: HashMap<&str, NumberFormat> = HashMap::new();
        for e in styles.elements() {
            if let (Some(name), Some(format)) = (e.attr("style:name"), parse_data_style(e)) {
                data_styles.insert(name, format);
            }
        }
        for e in styles.children_named("style:style") {
            let Some(name) = e.attr("style:name") else { continue };
            match e.attr("style:family") {
                Some("table-cell") => {
                    let raw = parse_cell_style(e);
                    let mut style = raw.style;
                    if let Some(format) = raw.data_style.as_deref().and_then(|n| data_styles.get(n)) {
                        style.format = format.clone();
                    }
                    let id = loader.wb.intern_style(style);
                    loader.cell_styles.insert(name.to_owned(), id);
                }
                Some("table-column") => {
                    let width = e
                        .child("style:table-column-properties")
                        .and_then(|p| p.attr("style:column-width"))
                        .and_then(parse_length_pt)
                        .filter(|w| (1.0..=2000.0).contains(w));
                    if let Some(width) = width {
                        loader.column_widths.insert(name.to_owned(), width as f32);
                    }
                }
                _ => {}
            }
        }
    }

    let spreadsheet = content
        .descend(&["office:body", "office:spreadsheet"])
        .ok_or_else(|| OdfError::invalid("document has no spreadsheet body"))?;
    for table in spreadsheet.children_named("table:table") {
        let sheet = loader.wb.push_loaded_sheet(table.attr("table:name").unwrap_or(""))?;
        let (mut row, mut col) = (0u32, 0u32);
        loader.load_table_children(sheet, table, &mut row, &mut col)?;
    }

    let mut wb = loader.wb;
    if let Some(meta) = package.get_xml("meta.xml", limits)? {
        wb.meta = Metadata::from_xml(&meta);
    }
    wb.finish_loading();
    Ok(wb)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn write_data_style(w: &mut XmlWriter, name: &str, format: &NumberFormat) {
    let number = |w: &mut XmlWriter, decimals: u8, grouping: bool| {
        w.start("number:number")
            .attr("number:decimal-places", &decimals.to_string())
            .attr("number:min-integer-digits", "1");
        if grouping {
            w.attr("number:grouping", "true");
        }
        w.end();
    };
    let date_parts = |w: &mut XmlWriter| {
        w.start("number:year").attr("number:style", "long").end();
        w.element_text("number:text", "-");
        w.start("number:month").attr("number:style", "long").end();
        w.element_text("number:text", "-");
        w.start("number:day").attr("number:style", "long").end();
    };
    let time_parts = |w: &mut XmlWriter| {
        w.start("number:hours").attr("number:style", "long").end();
        w.element_text("number:text", ":");
        w.start("number:minutes").attr("number:style", "long").end();
        w.element_text("number:text", ":");
        w.start("number:seconds").attr("number:style", "long").end();
    };
    match format {
        NumberFormat::General => {}
        NumberFormat::Number { decimals, thousands } => {
            w.start("number:number-style").attr("style:name", name);
            number(w, *decimals, *thousands);
            w.end();
        }
        NumberFormat::Percent { decimals } => {
            w.start("number:percentage-style").attr("style:name", name);
            number(w, *decimals, false);
            w.element_text("number:text", "%");
            w.end();
        }
        NumberFormat::Currency { decimals, symbol } => {
            w.start("number:currency-style").attr("style:name", name);
            w.element_text("number:currency-symbol", symbol);
            number(w, *decimals, true);
            w.end();
        }
        NumberFormat::Scientific { decimals } => {
            w.start("number:number-style").attr("style:name", name);
            w.start("number:scientific-number")
                .attr("number:decimal-places", &decimals.to_string())
                .attr("number:min-integer-digits", "1")
                .attr("number:min-exponent-digits", "2")
                .end();
            w.end();
        }
        NumberFormat::IndianNumber { decimals } => {
            w.start("number:number-style").attr("style:name", name);
            number(w, *decimals, true);
            w.end();
        }
        NumberFormat::Date => {
            w.start("number:date-style").attr("style:name", name);
            date_parts(w);
            w.end();
        }
        NumberFormat::DateDmy => {
            w.start("number:date-style").attr("style:name", name);
            w.start("number:day").attr("number:style", "long").end();
            w.element_text("number:text", "-");
            w.start("number:month").attr("number:style", "long").end();
            w.element_text("number:text", "-");
            w.start("number:year").attr("number:style", "long").end();
            w.end();
        }
        NumberFormat::Time => {
            w.start("number:time-style").attr("style:name", name);
            time_parts(w);
            w.end();
        }
        NumberFormat::DateTime => {
            w.start("number:date-style").attr("style:name", name);
            date_parts(w);
            w.element_text("number:text", " ");
            time_parts(w);
            w.end();
        }
        NumberFormat::Text => {
            w.start("number:text-style").attr("style:name", name);
            w.start("number:text-content").end();
            w.end();
        }
    }
}

fn write_cell_style(w: &mut XmlWriter, name: &str, style: &CellStyle, data_style: Option<&str>) {
    w.start("style:style")
        .attr("style:name", name)
        .attr("style:family", "table-cell")
        .attr("style:parent-style-name", "Default");
    if let Some(data_style) = data_style {
        w.attr("style:data-style-name", data_style);
    }
    if let Some(background) = style.background {
        w.start("style:table-cell-properties").attr("fo:background-color", &background.to_hex()).end();
    }
    if style.align != HAlign::General {
        let align = match style.align {
            HAlign::Left => "start",
            HAlign::Center => "center",
            _ => "end",
        };
        w.start("style:paragraph-properties").attr("fo:text-align", align).end();
    }
    if style.bold || style.italic || style.underline || style.color.is_some() {
        w.start("style:text-properties");
        if style.bold {
            w.attr("fo:font-weight", "bold");
        }
        if style.italic {
            w.attr("fo:font-style", "italic");
        }
        if style.underline {
            w.attr("style:text-underline-style", "solid")
                .attr("style:text-underline-width", "auto")
                .attr("style:text-underline-color", "font-color");
        }
        if let Some(color) = style.color {
            w.attr("fo:color", &color.to_hex());
        }
        w.end();
    }
    w.end();
}

fn duration(fraction: f64) -> String {
    let total = (fraction * 86_400.0).round().max(0.0) as u64;
    format!("PT{:02}H{:02}M{:02}S", total / 3600, total / 60 % 60, total % 60)
}

fn write_cell(w: &mut XmlWriter, wb: &Workbook, cell: &crate::workbook::Cell) {
    w.start("table:table-cell");
    if cell.style != 0 {
        w.attr("table:style-name", &format!("ce{}", cell.style));
    }
    if let Content::Formula(expr) = &cell.content {
        w.attr("table:formula", &expr.to_open_formula());
    }
    let format = &wb.style_by_id(cell.style).format;
    let display = format_value(&cell.value, format);
    match &cell.value {
        Value::Empty => {}
        Value::Number(n) => {
            let date = format::serial_to_date(*n);
            match (format, date) {
                (NumberFormat::Date | NumberFormat::DateDmy, Some((y, m, d))) => {
                    w.attr("office:value-type", "date").attr("office:date-value", &format!("{y:04}-{m:02}-{d:02}"));
                }
                (NumberFormat::DateTime, Some(_)) => {
                    w.attr("office:value-type", "date").attr("office:date-value", &display.replace(' ', "T"));
                }
                (NumberFormat::Time, _) if *n >= 0.0 => {
                    w.attr("office:value-type", "time").attr("office:time-value", &duration(*n));
                }
                (NumberFormat::Percent { .. }, _) => {
                    w.attr("office:value-type", "percentage").attr("office:value", &n.to_string());
                }
                (NumberFormat::Currency { .. }, _) => {
                    w.attr("office:value-type", "currency").attr("office:value", &n.to_string());
                }
                _ => {
                    w.attr("office:value-type", "float").attr("office:value", &n.to_string());
                }
            }
        }
        Value::Bool(b) => {
            w.attr("office:value-type", "boolean").attr("office:boolean-value", if *b { "true" } else { "false" });
        }
        Value::Text(_) | Value::Error(_) => {
            w.attr("office:value-type", "string");
        }
    }
    if !display.is_empty() {
        for line in display.split('\n') {
            w.element_text("text:p", line);
        }
    }
    w.end();
}

fn content_xml(wb: &Workbook) -> String {
    let mut w = XmlWriter::new();
    w.start("office:document-content")
        .namespaces(&["office", "style", "text", "table", "fo", "number", "of", "svg"])
        .attr("office:version", ns::ODF_VERSION);

    // Collect the styles and column widths actually in use.
    let mut used_styles: BTreeMap<u32, ()> = BTreeMap::new();
    let mut widths: Vec<f32> = Vec::new();
    for sheet in wb.sheets() {
        for (_, cell) in sheet.cells() {
            if cell.style != 0 {
                used_styles.insert(cell.style, ());
            }
        }
        for width in sheet.col_widths.values() {
            if !widths.contains(width) {
                widths.push(*width);
            }
        }
    }
    let mut data_style_names: HashMap<NumberFormat, String> = HashMap::new();

    w.start("office:automatic-styles");
    for (i, width) in widths.iter().enumerate() {
        w.start("style:style").attr("style:name", &format!("co{}", i + 1)).attr("style:family", "table-column");
        w.start("style:table-column-properties")
            .attr("style:column-width", &format_length_in(f64::from(*width)))
            .end();
        w.end();
    }
    for id in used_styles.keys() {
        let format = &wb.style_by_id(*id).format;
        if *format != NumberFormat::General && !data_style_names.contains_key(format) {
            let prefix = if matches!(format, NumberFormat::IndianNumber { .. }) { INDIAN_STYLE_PREFIX } else { "N" };
            let name = format!("{prefix}{}", data_style_names.len() + 1);
            write_data_style(&mut w, &name, format);
            data_style_names.insert(format.clone(), name);
        }
    }
    for id in used_styles.keys() {
        let style = wb.style_by_id(*id);
        write_cell_style(&mut w, &format!("ce{id}"), style, data_style_names.get(&style.format).map(String::as_str));
    }
    w.end();

    w.start("office:body").start("office:spreadsheet");
    for sheet in wb.sheets() {
        w.start("table:table").attr("table:name", &sheet.name);

        // Columns: runs of default-width columns between explicit widths.
        let last_col = sheet.used_range().map(|r| r.end.col).unwrap_or(0).max(
            sheet.col_widths.keys().next_back().copied().unwrap_or(0),
        );
        let mut next = 0u32;
        let plain = |w: &mut XmlWriter, count: u32| {
            if count > 0 {
                w.start("table:table-column");
                if count > 1 {
                    w.attr("table:number-columns-repeated", &count.to_string());
                }
                w.end();
            }
        };
        for (col, width) in &sheet.col_widths {
            plain(&mut w, col - next);
            let index = widths.iter().position(|x| x == width).unwrap_or(0);
            w.start("table:table-column").attr("table:style-name", &format!("co{}", index + 1)).end();
            next = col + 1;
        }
        plain(&mut w, (last_col + 1).saturating_sub(next).max(u32::from(next == 0)));

        // Rows.
        let mut current_row: Option<u32> = None;
        let mut next_row = 0u32;
        let mut next_col = 0u32;
        for (addr, cell) in sheet.cells() {
            if current_row != Some(addr.row) {
                if current_row.is_some() {
                    w.end();
                }
                let gap = addr.row - next_row;
                if gap > 0 {
                    w.start("table:table-row");
                    if gap > 1 {
                        w.attr("table:number-rows-repeated", &gap.to_string());
                    }
                    w.start("table:table-cell").end();
                    w.end();
                }
                w.start("table:table-row");
                current_row = Some(addr.row);
                next_row = addr.row + 1;
                next_col = 0;
            }
            let gap = addr.col - next_col;
            if gap > 0 {
                w.start("table:table-cell");
                if gap > 1 {
                    w.attr("table:number-columns-repeated", &gap.to_string());
                }
                w.end();
            }
            write_cell(&mut w, wb, cell);
            next_col = addr.col + 1;
        }
        if current_row.is_some() {
            w.end();
        } else {
            w.start("table:table-row").start("table:table-cell").end().end();
        }
        w.end();
    }
    w.finish()
}

fn styles_xml() -> String {
    let mut w = XmlWriter::new();
    w.start("office:document-styles")
        .namespaces(&["office", "style", "text", "table", "fo", "number", "svg"])
        .attr("office:version", ns::ODF_VERSION);
    w.start("office:styles");
    w.start("style:style").attr("style:name", "Default").attr("style:family", "table-cell").end();
    w.finish()
}

/// Serialises a workbook to the bytes of an `.ods` file.
pub fn save_ods(wb: &Workbook) -> Result<Vec<u8>> {
    let mut package = Package::new(ns::MIME_ODS);
    package.insert("content.xml", content_xml(wb).into_bytes())?;
    package.insert("styles.xml", styles_xml().into_bytes())?;
    package.insert("meta.xml", wb.meta.to_xml().into_bytes())?;
    package.write()
}
