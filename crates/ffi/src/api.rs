//! The request dispatcher behind the C ABI.
//!
//! Every call is one JSON object with an `"op"` field; every answer is
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": {"kind", "message"}}`.
//! Open documents live in a registry and are addressed by integer handles.
//! See `docs/ffi-protocol.md` for the full list of operations.

use office_calc::{self as calc, Axis, CellAddr, HAlign, NumberFormat, Range, StylePatch, Workbook};
use office_indic::Script;
use office_math::Formula;
use office_odf::{read_file_limited, write_file_atomic, Limits, OdfError, Rgb};
use office_writer::{self as writer, Align, AttrPatch, Block, Document, ParaKind, ParaRef, Table};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// An open document.
pub enum Doc {
    /// Text document.
    Writer(Box<Document>),
    /// Spreadsheet.
    Calc(Box<Workbook>),
    /// Formula.
    Math(Box<Formula>),
}

impl Doc {
    fn kind(&self) -> &'static str {
        match self {
            Doc::Writer(_) => "writer",
            Doc::Calc(_) => "calc",
            Doc::Math(_) => "math",
        }
    }
}

/// Registry of open documents.
#[derive(Default)]
pub struct Registry {
    docs: HashMap<u64, Doc>,
    next: u64,
    limits: Limits,
}

/// Most documents one process may keep open.
pub const MAX_OPEN_DOCS: usize = 256;

type R<T> = Result<T, OdfError>;

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

// ---- argument helpers -------------------------------------------------------

struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn get(&self, key: &str) -> R<&Value> {
        self.0.get(key).ok_or_else(|| bad(format!("missing argument '{key}'")))
    }
    fn str(&self, key: &str) -> R<&str> {
        self.get(key)?.as_str().ok_or_else(|| bad(format!("'{key}' must be a string")))
    }
    fn opt_str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }
    fn u64(&self, key: &str) -> R<u64> {
        self.get(key)?.as_u64().ok_or_else(|| bad(format!("'{key}' must be a non-negative integer")))
    }
    fn usize(&self, key: &str) -> R<usize> {
        usize::try_from(self.u64(key)?).map_err(|_| bad(format!("'{key}' is too large")))
    }
    fn u32(&self, key: &str) -> R<u32> {
        u32::try_from(self.u64(key)?).map_err(|_| bad(format!("'{key}' is too large")))
    }
    fn i32(&self, key: &str) -> R<i32> {
        self.get(key)?
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| bad(format!("'{key}' must be an integer")))
    }
    fn bool(&self, key: &str) -> R<bool> {
        self.get(key)?.as_bool().ok_or_else(|| bad(format!("'{key}' must be a boolean")))
    }
    fn opt_bool(&self, key: &str) -> Option<bool> {
        self.0.get(key).and_then(Value::as_bool)
    }
    fn cell(&self, key: &str) -> R<CellAddr> {
        let text = self.str(key)?;
        CellAddr::parse(text).ok_or_else(|| bad(format!("invalid cell address '{text}'")))
    }
    fn range(&self, key: &str) -> R<Range> {
        let text = self.str(key)?;
        Range::parse(text).filter(Range::is_valid).ok_or_else(|| bad(format!("invalid range '{text}'")))
    }
    fn para(&self, key: &str) -> R<ParaRef> {
        let v = self.get(key)?.as_object().ok_or_else(|| bad(format!("'{key}' must be an object")))?;
        let a = Args(v);
        let block = a.usize("block")?;
        match v.get("cell") {
            None | Some(Value::Null) => Ok(ParaRef::top(block)),
            Some(Value::Array(c)) if c.len() == 3 => {
                let n = |i: usize| -> R<usize> {
                    c[i].as_u64().and_then(|x| usize::try_from(x).ok()).ok_or_else(|| bad("invalid cell index"))
                };
                Ok(ParaRef::in_cell(block, n(0)?, n(1)?, n(2)?))
            }
            _ => Err(bad("'cell' must be [row, column, paragraph]")),
        }
    }
}

fn para_json(r: ParaRef) -> Value {
    match r.cell {
        None => json!({ "block": r.block }),
        Some((row, col, p)) => json!({ "block": r.block, "cell": [row, col, p] }),
    }
}

fn color(v: &Value) -> R<Option<Rgb>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => Rgb::parse(s).map(Some).ok_or_else(|| bad(format!("invalid colour '{s}'"))),
        _ => Err(bad("colours are '#rrggbb' strings or null")),
    }
}

// ---- format and style conversion --------------------------------------------

fn format_to_json(f: &NumberFormat) -> Value {
    match f {
        NumberFormat::General => json!({"kind": "general"}),
        NumberFormat::Number { decimals, thousands } => json!({"kind": "number", "decimals": decimals, "thousands": thousands}),
        NumberFormat::Percent { decimals } => json!({"kind": "percent", "decimals": decimals}),
        NumberFormat::Currency { decimals, symbol } => json!({"kind": "currency", "decimals": decimals, "symbol": symbol}),
        NumberFormat::Scientific { decimals } => json!({"kind": "scientific", "decimals": decimals}),
        NumberFormat::IndianNumber { decimals } => json!({"kind": "indian", "decimals": decimals}),
        NumberFormat::Date => json!({"kind": "date"}),
        NumberFormat::DateDmy => json!({"kind": "date_dmy"}),
        NumberFormat::Time => json!({"kind": "time"}),
        NumberFormat::DateTime => json!({"kind": "datetime"}),
        NumberFormat::Text => json!({"kind": "text"}),
    }
}

fn format_from_json(v: &Value) -> R<NumberFormat> {
    let o = v.as_object().ok_or_else(|| bad("format must be an object"))?;
    let a = Args(o);
    let decimals = || -> u8 { o.get("decimals").and_then(Value::as_u64).unwrap_or(2).min(20) as u8 };
    Ok(match a.str("kind")? {
        "general" => NumberFormat::General,
        "number" => NumberFormat::Number { decimals: decimals(), thousands: a.opt_bool("thousands").unwrap_or(false) },
        "percent" => NumberFormat::Percent { decimals: decimals() },
        "currency" => {
            let symbol = a.opt_str("symbol").unwrap_or("$");
            if symbol.chars().count() > 8 {
                return Err(bad("currency symbol is too long"));
            }
            NumberFormat::Currency { decimals: decimals(), symbol: symbol.to_owned() }
        }
        "scientific" => NumberFormat::Scientific { decimals: decimals() },
        "indian" => NumberFormat::IndianNumber { decimals: decimals() },
        "date" => NumberFormat::Date,
        "date_dmy" => NumberFormat::DateDmy,
        "time" => NumberFormat::Time,
        "datetime" => NumberFormat::DateTime,
        "text" => NumberFormat::Text,
        other => return Err(bad(format!("unknown number format '{other}'"))),
    })
}

fn style_to_json(s: &calc::CellStyle) -> Value {
    json!({
        "bold": s.bold, "italic": s.italic, "underline": s.underline,
        "align": match s.align { HAlign::General => "general", HAlign::Left => "left", HAlign::Center => "center", HAlign::Right => "right" },
        "color": s.color.map(Rgb::to_hex), "background": s.background.map(Rgb::to_hex),
        "format": format_to_json(&s.format),
    })
}

fn style_patch(v: &Value) -> R<StylePatch> {
    let o = v.as_object().ok_or_else(|| bad("style patch must be an object"))?;
    let a = Args(o);
    Ok(StylePatch {
        bold: a.opt_bool("bold"),
        italic: a.opt_bool("italic"),
        underline: a.opt_bool("underline"),
        align: match a.opt_str("align") {
            None => None,
            Some("general") => Some(HAlign::General),
            Some("left") => Some(HAlign::Left),
            Some("center") => Some(HAlign::Center),
            Some("right") => Some(HAlign::Right),
            Some(other) => return Err(bad(format!("unknown alignment '{other}'"))),
        },
        color: o.get("color").map(color).transpose()?,
        background: o.get("background").map(color).transpose()?,
        format: o.get("format").map(format_from_json).transpose()?,
    })
}

fn attr_patch(v: &Value) -> R<AttrPatch> {
    let o = v.as_object().ok_or_else(|| bad("attribute patch must be an object"))?;
    let a = Args(o);
    Ok(AttrPatch {
        bold: a.opt_bool("bold"),
        italic: a.opt_bool("italic"),
        underline: a.opt_bool("underline"),
        strike: a.opt_bool("strike"),
        code: a.opt_bool("code"),
        superscript: a.opt_bool("superscript"),
        subscript: a.opt_bool("subscript"),
        color: o.get("color").map(color).transpose()?,
        highlight: o.get("highlight").map(color).transpose()?,
        size: match o.get("size") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => Some(Some(v.as_f64().ok_or_else(|| bad("size must be a number"))? as f32)),
        },
        link: match o.get("link") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(s)) => Some(Some(s.clone())),
            Some(_) => return Err(bad("link must be a string or null")),
        },
    })
}

fn para_kind(v: &Value) -> R<ParaKind> {
    let o = v.as_object().ok_or_else(|| bad("kind must be an object"))?;
    let a = Args(o);
    let level = || -> u8 { o.get("level").and_then(Value::as_u64).unwrap_or(0).min(9) as u8 };
    Ok(match a.str("kind")? {
        "normal" => ParaKind::Normal,
        "heading" => ParaKind::Heading { level: level().max(1) },
        "bullet" => ParaKind::Bullet { level: level() },
        "numbered" => ParaKind::Numbered { level: level() },
        "quote" => ParaKind::Quote,
        "code" => ParaKind::Code,
        "title" => ParaKind::Title,
        other => return Err(bad(format!("unknown paragraph kind '{other}'"))),
    })
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    // `serde_json` cannot fail on our model types (string keys, finite data);
    // fall back to null rather than panic just in case.
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn extension(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

// ---- transliteration ------------------------------------------------------

/// Applies one of the transliteration modes to `text`:
/// `roman` (teltools RTS/ITRANS typing -> script), `script` (any Indic
/// script -> script), `to_iast`, `from_iast`.
fn indic_transform(a: &Args, text: &str) -> R<String> {
    let script = || -> R<Script> {
        let code = a.str("script")?;
        Script::from_code(code).ok_or_else(|| bad(format!("unknown script '{code}'")))
    };
    let options = office_indic::Options {
        native_digits: a.opt_bool("native_digits").unwrap_or(false),
        ..office_indic::Options::default()
    };
    Ok(match a.opt_str("mode").unwrap_or("roman") {
        "roman" => office_indic::transliterate(text, script()?, &options),
        "script" => office_indic::convert_script(text, script()?),
        "to_iast" => office_indic::to_iast(text),
        "from_iast" => office_indic::from_iast(text, script()?),
        other => return Err(bad(format!("unknown transliteration mode '{other}'"))),
    })
}

/// Longest text accepted by the transliteration operations.
const MAX_TRANSLIT_LEN: usize = 4 << 20;

// ---- registry ---------------------------------------------------------------

impl Registry {
    fn insert(&mut self, doc: Doc) -> R<Value> {
        if self.docs.len() >= MAX_OPEN_DOCS {
            return Err(OdfError::limit("too many open documents"));
        }
        self.next += 1;
        let kind = doc.kind();
        self.docs.insert(self.next, doc);
        Ok(json!({ "doc": self.next, "kind": kind }))
    }

    fn doc(&mut self, a: &Args) -> R<&mut Doc> {
        let id = a.u64("doc")?;
        self.docs.get_mut(&id).ok_or_else(|| bad(format!("no open document {id}")))
    }

    fn writer(&mut self, a: &Args) -> R<&mut Document> {
        match self.doc(a)? {
            Doc::Writer(d) => Ok(d),
            _ => Err(bad("not a text document")),
        }
    }

    fn calc(&mut self, a: &Args) -> R<&mut Workbook> {
        match self.doc(a)? {
            Doc::Calc(d) => Ok(d),
            _ => Err(bad("not a spreadsheet")),
        }
    }

    fn math(&mut self, a: &Args) -> R<&mut Formula> {
        match self.doc(a)? {
            Doc::Math(d) => Ok(d),
            _ => Err(bad("not a formula")),
        }
    }

    /// Loads a document from bytes; `ext` selects the application.
    fn load_bytes(&mut self, bytes: &[u8], ext: &str) -> R<Value> {
        let limits = self.limits.clone();
        let doc = match ext {
            "odt" | "ott" | "txt" | "text" => Doc::Writer(Box::new(writer::load(bytes, &limits)?)),
            "ods" | "ots" => Doc::Calc(Box::new(calc::load_ods(bytes, &limits)?)),
            "csv" => Doc::Calc(Box::new(calc::load_csv(bytes, None, &limits)?)),
            "tsv" | "tab" => Doc::Calc(Box::new(calc::load_csv(bytes, Some('\t'), &limits)?)),
            "odf" | "otf" | "mml" | "mathml" => Doc::Math(Box::new(Formula::from_bytes(bytes, &limits)?)),
            other => return Err(OdfError::unsupported(format!("file type '.{other}'"))),
        };
        self.insert(doc)
    }

    fn info(doc: &Doc) -> Value {
        let (modified, undo, redo, title) = match doc {
            Doc::Writer(d) => (d.is_modified(), d.can_undo(), d.can_redo(), d.meta.title.clone()),
            Doc::Calc(d) => (d.is_modified(), d.can_undo(), d.can_redo(), d.meta.title.clone()),
            Doc::Math(d) => (false, false, false, d.meta.title.clone()),
        };
        json!({ "kind": doc.kind(), "modified": modified, "can_undo": undo, "can_redo": redo, "title": title })
    }

    /// Serialises a document into the format implied by `ext`.
    fn save_bytes(doc: &mut Doc, ext: &str, a: &Args) -> R<Vec<u8>> {
        let bytes = match (doc, ext) {
            (Doc::Writer(d), "odt") => writer::save_odt(d)?,
            (Doc::Writer(d), "txt") => d.plain_text().into_bytes(),
            (Doc::Writer(d), "html" | "htm") => writer::export::to_html(d).into_bytes(),
            (Doc::Writer(d), "md") => writer::export::to_markdown(d).into_bytes(),
            (Doc::Calc(d), "ods") => calc::save_ods(d)?,
            (Doc::Calc(d), "csv" | "tsv") => {
                let sheet = a.0.get("sheet").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delimiter = if ext == "tsv" { '\t' } else { ',' };
                calc::sheet_to_csv(d, sheet, delimiter, a.opt_bool("sanitize").unwrap_or(true))?.into_bytes()
            }
            (Doc::Math(d), "odf") => d.to_odf_bytes()?,
            (Doc::Math(d), "mml") => d.to_mathml().into_bytes(),
            (doc, other) => {
                return Err(OdfError::unsupported(format!("saving a {} document as '.{other}'", doc.kind())))
            }
        };
        Ok(bytes)
    }

    fn mark_saved(doc: &mut Doc, ext: &str) {
        // Only the native formats are lossless enough to clear "modified".
        match (doc, ext) {
            (Doc::Writer(d), "odt") => d.mark_saved(),
            (Doc::Calc(d), "ods") => d.mark_saved(),
            _ => {}
        }
    }

    /// Executes one request.
    pub fn dispatch(&mut self, req: &Value) -> R<Value> {
        let obj = req.as_object().ok_or_else(|| bad("request must be a JSON object"))?;
        let a = Args(obj);
        let op = a.str("op")?;
        match op {
            // ---- general ----
            "version" => Ok(json!({
                "name": "Akshara", "version": env!("CARGO_PKG_VERSION"), "odf": office_odf::ns::ODF_VERSION,
            })),
            "new" => {
                let doc = match a.str("kind")? {
                    "writer" => Doc::Writer(Box::new(Document::new())),
                    "calc" => Doc::Calc(Box::new(Workbook::new())),
                    "math" => Doc::Math(Box::new(Formula::new(a.opt_str("source").unwrap_or("")))),
                    other => return Err(bad(format!("unknown document kind '{other}'"))),
                };
                self.insert(doc)
            }
            "open" => {
                let path = a.str("path")?;
                let bytes = read_file_limited(Path::new(path), &self.limits)?;
                self.load_bytes(&bytes, &extension(path))
            }
            "save" => {
                let path = a.str("path")?.to_owned();
                let ext = extension(&path);
                let doc = self.doc(&a)?;
                let bytes = Self::save_bytes(doc, &ext, &a)?;
                write_file_atomic(Path::new(&path), &bytes)?;
                Self::mark_saved(doc, &ext);
                Ok(Self::info(doc))
            }
            "close" => {
                let id = a.u64("doc")?;
                Ok(json!(self.docs.remove(&id).is_some()))
            }
            "info" => Ok(Self::info(self.doc(&a)?)),
            "undo" | "redo" => {
                let doc = self.doc(&a)?;
                let done = match (doc, op) {
                    (Doc::Writer(d), "undo") => d.undo(),
                    (Doc::Writer(d), _) => d.redo(),
                    (Doc::Calc(d), "undo") => d.undo(),
                    (Doc::Calc(d), _) => d.redo(),
                    (Doc::Math(_), _) => false,
                };
                Ok(json!(done))
            }
            "set_title" => {
                let title = a.opt_str("title").map(str::to_owned).filter(|t| !t.is_empty());
                match self.doc(&a)? {
                    Doc::Writer(d) => d.meta.title = title,
                    Doc::Calc(d) => d.meta.title = title,
                    Doc::Math(d) => d.meta.title = title,
                }
                Ok(Value::Null)
            }

            // ---- transliteration (teltools) ----
            "indic.scripts" => Ok(json!(Script::ALL
                .iter()
                .map(|s| json!({ "code": s.code(), "name": s.name(), "native": s.native_name() }))
                .collect::<Vec<_>>())),
            "indic.translit" => {
                let text = a.str("text")?;
                if text.len() > MAX_TRANSLIT_LEN {
                    return Err(OdfError::limit("text is too long"));
                }
                Ok(json!(indic_transform(&a, text)?))
            }
            "indic.detect" => Ok(json!(office_indic::detect_script(a.str("text")?).map(Script::code))),
            "calc.transliterate" => {
                let (sheet, range) = (a.usize("sheet")?, a.range("range")?);
                // Validate the mode and script before touching the workbook.
                indic_transform(&a, "")?;
                let wb = self.calc(&a)?;
                let count = wb.map_text(sheet, range, &|text| indic_transform(&a, text).unwrap_or_else(|_| text.to_owned()))?;
                Ok(json!(count))
            }

            // ---- calc ----
            "calc.sheets" => {
                let wb = self.calc(&a)?;
                let sheets: Vec<Value> = wb
                    .sheets()
                    .iter()
                    .map(|s| json!({ "name": s.name, "used": s.used_range().map(|r| r.to_a1()) }))
                    .collect();
                Ok(json!(sheets))
            }
            "calc.view" => {
                let (sheet, row, col) = (a.usize("sheet")?, a.u32("row")?, a.u32("col")?);
                let rows = a.u32("rows")?.min(500);
                let cols = a.u32("cols")?.min(200);
                let wb = self.calc(&a)?;
                let sh = wb.sheet(sheet)?;
                let last_row = row.saturating_add(rows.saturating_sub(1));
                let last_col = col.saturating_add(cols.saturating_sub(1));
                let mut cells = Vec::new();
                let mut styles: Map<String, Value> = Map::new();
                for (addr, cell) in sh.cells_in_rows(row, last_row) {
                    if addr.col < col || addr.col > last_col {
                        continue;
                    }
                    if cell.style != 0 && !styles.contains_key(&cell.style.to_string()) {
                        styles.insert(cell.style.to_string(), style_to_json(wb.style_by_id(cell.style)));
                    }
                    cells.push(json!({
                        "r": addr.row, "c": addr.col, "t": wb.display(sheet, addr),
                        "v": cell.value.type_tag(), "s": cell.style,
                    }));
                }
                let widths: Map<String, Value> = sh.col_widths.iter().map(|(c, w)| (c.to_string(), json!(w))).collect();
                Ok(json!({
                    "cells": cells, "styles": styles, "widths": widths,
                    "used": sh.used_range().map(|r| json!({"rows": r.end.row + 1, "cols": r.end.col + 1})),
                }))
            }
            "calc.cell" => {
                let (sheet, cell) = (a.usize("sheet")?, a.cell("cell")?);
                let wb = self.calc(&a)?;
                wb.sheet(sheet)?;
                Ok(json!({
                    "input": wb.input_text(sheet, cell), "display": wb.display(sheet, cell),
                    "type": wb.value(sheet, cell).type_tag(), "style": style_to_json(wb.style(sheet, cell)),
                }))
            }
            "calc.set_input" => {
                let (sheet, cell) = (a.usize("sheet")?, a.cell("cell")?);
                let text = a.str("text")?.to_owned();
                self.calc(&a)?.set_input(sheet, cell, &text)?;
                Ok(Value::Null)
            }
            "calc.paste" => {
                let (sheet, cell) = (a.usize("sheet")?, a.cell("cell")?);
                let rows: Vec<Vec<String>> = match a.get("rows")? {
                    Value::Array(rows) => rows
                        .iter()
                        .map(|r| {
                            r.as_array()
                                .map(|cells| cells.iter().map(|c| c.as_str().unwrap_or("").to_owned()).collect())
                                .ok_or_else(|| bad("rows must be arrays of strings"))
                        })
                        .collect::<R<_>>()?,
                    Value::String(text) => {
                        office_calc::csv::parse_csv(text, '\t', &self.limits)?
                    }
                    _ => return Err(bad("rows must be an array or tab-separated text")),
                };
                self.calc(&a)?.set_inputs(sheet, cell, &rows)?;
                Ok(Value::Null)
            }
            "calc.copy_text" => {
                let (sheet, range) = (a.usize("sheet")?, a.range("range")?);
                if range.cell_count() > calc::MAX_BULK_CELLS {
                    return Err(OdfError::limit("range is too large to copy"));
                }
                let wb = self.calc(&a)?;
                wb.sheet(sheet)?;
                let mut out = String::new();
                for row in range.start.row..=range.end.row {
                    let line: Vec<String> = (range.start.col..=range.end.col)
                        .map(|c| wb.display(sheet, CellAddr::new(row, c)).replace(['\t', '\n'], " "))
                        .collect();
                    out.push_str(&line.join("\t"));
                    out.push('\n');
                }
                Ok(json!(out))
            }
            "calc.clear" => {
                let (sheet, range) = (a.usize("sheet")?, a.range("range")?);
                self.calc(&a)?.clear_range(sheet, range)?;
                Ok(Value::Null)
            }
            "calc.style" => {
                let (sheet, range) = (a.usize("sheet")?, a.range("range")?);
                let patch = style_patch(a.get("patch")?)?;
                self.calc(&a)?.set_style(sheet, range, &patch)?;
                Ok(Value::Null)
            }
            "calc.insert" | "calc.delete" => {
                let sheet = a.usize("sheet")?;
                let axis = match a.str("axis")? {
                    "row" => Axis::Row,
                    "col" => Axis::Col,
                    other => return Err(bad(format!("unknown axis '{other}'"))),
                };
                let (at, count) = (a.u32("at")?, a.u32("count")?);
                let wb = self.calc(&a)?;
                if op == "calc.insert" {
                    wb.insert(sheet, axis, at, count)?;
                } else {
                    wb.delete(sheet, axis, at, count)?;
                }
                Ok(Value::Null)
            }
            "calc.sort" => {
                let (sheet, range, col) = (a.usize("sheet")?, a.range("range")?, a.u32("col")?);
                let ascending = a.opt_bool("ascending").unwrap_or(true);
                self.calc(&a)?.sort_range(sheet, range, col, ascending)?;
                Ok(Value::Null)
            }
            "calc.fill" => {
                let (sheet, src, dst) = (a.usize("sheet")?, a.range("src")?, a.range("dst")?);
                self.calc(&a)?.fill(sheet, src, dst)?;
                Ok(Value::Null)
            }
            "calc.copy" => {
                let (sheet, src) = (a.usize("sheet")?, a.range("src")?);
                let (dst_sheet, dst) = (a.usize("dst_sheet")?, a.cell("dst")?);
                self.calc(&a)?.copy_range(sheet, src, dst_sheet, dst)?;
                Ok(Value::Null)
            }
            "calc.add_sheet" => {
                let name = a.opt_str("name").map(str::to_owned);
                Ok(json!(self.calc(&a)?.add_sheet(name.as_deref())?))
            }
            "calc.rename_sheet" => {
                let (sheet, name) = (a.usize("sheet")?, a.str("name")?.to_owned());
                self.calc(&a)?.rename_sheet(sheet, &name)?;
                Ok(Value::Null)
            }
            "calc.remove_sheet" => {
                let sheet = a.usize("sheet")?;
                self.calc(&a)?.remove_sheet(sheet)?;
                Ok(Value::Null)
            }
            "calc.col_width" => {
                let (sheet, col) = (a.usize("sheet")?, a.u32("col")?);
                let width = a.0.get("width").and_then(Value::as_f64).map(|w| w as f32);
                self.calc(&a)?.set_col_width(sheet, col, width)?;
                Ok(Value::Null)
            }
            "calc.functions" => Ok(json!(calc::eval::FUNCTION_NAMES)),

            // ---- writer ----
            "writer.get" => {
                let d = self.writer(&a)?;
                Ok(json!({ "blocks": to_value(&d.blocks()), "page": to_value(&d.page), "stats": to_value(&d.stats()) }))
            }
            "writer.stats" => Ok(to_value(&self.writer(&a)?.stats())),
            "writer.para" => {
                let r = a.para("para")?;
                Ok(to_value(self.writer(&a)?.paragraph(r)?))
            }
            "writer.set_text" => {
                let (r, text) = (a.para("para")?, a.str("text")?.to_owned());
                self.writer(&a)?.set_text(r, &text)?;
                Ok(Value::Null)
            }
            "writer.replace" => {
                let (r, start, end) = (a.para("para")?, a.usize("start")?, a.usize("end")?);
                let text = a.str("text")?.to_owned();
                self.writer(&a)?.replace_text(r, start, end, &text, None)?;
                Ok(Value::Null)
            }
            "writer.attrs" => {
                let (r, start, end) = (a.para("para")?, a.usize("start")?, a.usize("end")?);
                let patch = attr_patch(a.get("patch")?)?;
                self.writer(&a)?.apply_attrs(r, start, end, &patch)?;
                Ok(Value::Null)
            }
            "writer.split" => {
                let (r, at) = (a.para("para")?, a.usize("at")?);
                Ok(para_json(self.writer(&a)?.split_paragraph(r, at)?))
            }
            "writer.merge" => {
                let r = a.para("para")?;
                Ok(match self.writer(&a)?.merge_with_previous(r)? {
                    Some((p, at)) => json!({ "para": para_json(p), "at": at }),
                    None => Value::Null,
                })
            }
            "writer.kind" => {
                let (r, kind) = (a.para("para")?, para_kind(a.get("kind")?)?);
                self.writer(&a)?.set_kind(r, kind)?;
                Ok(Value::Null)
            }
            "writer.align" => {
                let r = a.para("para")?;
                let align = match a.str("align")? {
                    "left" => Align::Left,
                    "center" => Align::Center,
                    "right" => Align::Right,
                    "justify" => Align::Justify,
                    other => return Err(bad(format!("unknown alignment '{other}'"))),
                };
                self.writer(&a)?.set_align(r, align)?;
                Ok(Value::Null)
            }
            "writer.indent" => {
                let (r, delta) = (a.para("para")?, a.i32("delta")?);
                self.writer(&a)?.indent(r, delta)?;
                Ok(Value::Null)
            }
            "writer.insert_table" => {
                let (at, rows, cols) = (a.usize("at")?, a.usize("rows")?, a.usize("cols")?);
                self.writer(&a)?.insert_block(at, Block::Table(Table::new(rows, cols)))?;
                Ok(Value::Null)
            }
            "writer.insert_page_break" => {
                let at = a.usize("at")?;
                self.writer(&a)?.insert_block(at, Block::PageBreak)?;
                Ok(Value::Null)
            }
            "writer.remove_block" => {
                let at = a.usize("at")?;
                self.writer(&a)?.remove_block(at)?;
                Ok(Value::Null)
            }
            "writer.table" => {
                let (block, rows, at, count) = (a.usize("block")?, a.bool("rows")?, a.usize("at")?, a.i32("count")?);
                self.writer(&a)?.resize_table(block, rows, at, count)?;
                Ok(Value::Null)
            }
            "writer.find" => {
                let needle = a.str("needle")?.to_owned();
                let case = a.opt_bool("match_case").unwrap_or(false);
                let hits = self.writer(&a)?.find(&needle, case, 10_000);
                Ok(json!(hits
                    .iter()
                    .map(|m| json!({ "para": para_json(m.para), "start": m.start, "end": m.end }))
                    .collect::<Vec<_>>()))
            }
            "writer.replace_all" => {
                let (needle, replacement) = (a.str("needle")?.to_owned(), a.str("replacement")?.to_owned());
                let case = a.opt_bool("match_case").unwrap_or(false);
                Ok(json!(self.writer(&a)?.replace_all(&needle, &replacement, case)?))
            }
            "writer.break_undo" => {
                self.writer(&a)?.break_undo_group();
                Ok(Value::Null)
            }
            "writer.export_html" => Ok(json!(writer::export::to_html(self.writer(&a)?))),

            // ---- math ----
            "math.get" => {
                let f = self.math(&a)?;
                let parsed = f.parse();
                Ok(json!({ "source": f.source, "tree": to_value(&parsed.root), "errors": to_value(&parsed.errors) }))
            }
            "math.set" => {
                let source = a.str("source")?.to_owned();
                if source.len() > office_math::MAX_SOURCE_LEN {
                    return Err(OdfError::limit("formula is too long"));
                }
                let f = self.math(&a)?;
                f.source = source;
                let parsed = f.parse();
                Ok(json!({ "tree": to_value(&parsed.root), "errors": to_value(&parsed.errors) }))
            }
            "math.parse" => {
                let parsed = office_math::parse(a.str("source")?);
                Ok(json!({ "tree": to_value(&parsed.root), "errors": to_value(&parsed.errors) }))
            }
            "math.mathml" => Ok(json!(self.math(&a)?.to_mathml())),
            "math.symbols" => Ok(json!(office_math::symbols::greek_catalog()
                .map(|(name, ch)| json!({ "name": name, "char": ch }))
                .collect::<Vec<_>>())),

            other => Err(bad(format!("unknown operation '{other}'"))),
        }
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

/// Largest accepted request in bytes (pasted text, long formulas...).
pub const MAX_REQUEST_LEN: usize = 64 << 20;

/// Handles one raw request and always returns a JSON response. Panics inside
/// the cores are caught and reported as errors so they can never unwind into
/// Dart; a poisoned registry lock is recovered.
pub fn handle(request: &[u8]) -> Vec<u8> {
    let response = std::panic::catch_unwind(|| -> R<Value> {
        if request.len() > MAX_REQUEST_LEN {
            return Err(OdfError::limit("request is too large"));
        }
        let req: Value =
            serde_json::from_slice(request).map_err(|e| bad(format!("request is not valid JSON: {e}")))?;
        let mut reg = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        reg.dispatch(&req)
    });
    let value = match response {
        Ok(Ok(result)) => json!({ "ok": true, "result": result }),
        Ok(Err(e)) => json!({ "ok": false, "error": { "kind": e.kind(), "message": e.to_string() } }),
        Err(_) => json!({ "ok": false, "error": { "kind": "internal", "message": "internal error" } }),
    };
    value.to_string().into_bytes()
}
