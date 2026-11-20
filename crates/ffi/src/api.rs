//! The request dispatcher behind the C ABI.
//!
//! Every call is one JSON object with an `"op"` field; every answer is
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": {"kind", "message"}}`.
//! Open documents live in a registry and are addressed by integer handles.
//! See `docs/ffi-protocol.md` for the full list of operations.

use office_calc::{self as calc, Axis, CellAddr, HAlign, NumberFormat, Range, StylePatch, Workbook};
use office_calc::{PageSetup as CalcPage, PrintArea};
use office_draw::model::ops as shape_ops;
use office_draw::{Drawing, DrawingKind, HAlign as ShapeAlign, Image, Layout, Shape, ShapeKind, ShapePatch, VAlign, ZOrder};
use office_indic::Script;
use office_math::Formula;
use office_odf::{read_file_limited, write_file_atomic, Limits, OdfError, Rgb};
use office_writer::{
    self as writer, Align, Area, AttrPatch, Block, Document, Field, Letterhead, PageSetup, ParaKind, ParaRef, Table,
};
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
    /// Presentation or drawing.
    Draw(Box<Drawing>),
}

impl Doc {
    fn kind(&self) -> &'static str {
        match self {
            Doc::Writer(_) => "writer",
            Doc::Calc(_) => "calc",
            Doc::Math(_) => "math",
            Doc::Draw(d) => match d.kind() {
                DrawingKind::Presentation => "impress",
                DrawingKind::Drawing => "draw",
            },
        }
    }
}

/// Registry of open documents.
#[derive(Default)]
pub struct Registry {
    docs: HashMap<u64, Doc>,
    next: u64,
    limits: Limits,
    /// Shapes copied with `shapes.copy`, pasted with `shapes.paste` into
    /// any drawing, slide or Writer canvas.
    clipboard: Vec<Shape>,
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
        match a.opt_str("area") {
            None | Some("body") => {}
            Some("header") => return Ok(ParaRef::header(block)),
            Some("footer") => return Ok(ParaRef::footer(block)),
            Some(other) => return Err(bad(format!("unknown area '{other}'"))),
        }
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
    match (r.area, r.cell) {
        (Area::Header, _) => json!({ "area": "header", "block": r.block }),
        (Area::Footer, _) => json!({ "area": "footer", "block": r.block }),
        (Area::Body, None) => json!({ "block": r.block }),
        (Area::Body, Some((row, col, p))) => json!({ "block": r.block, "cell": [row, col, p] }),
    }
}

fn f32_arg(a: &Args, key: &str) -> R<f32> {
    a.get(key)?
        .as_f64()
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .ok_or_else(|| bad(format!("'{key}' must be a number")))
}

fn page_arg(a: &Args) -> R<PageSetup> {
    let page = PageSetup { width: f32_arg(a, "width")?, height: f32_arg(a, "height")?, margin: f32_arg(a, "margin")? };
    if page.is_valid() {
        Ok(page)
    } else {
        Err(bad("page size or margins are out of range"))
    }
}

fn align_arg(s: &str) -> R<Align> {
    match s {
        "left" => Ok(Align::Left),
        "center" => Ok(Align::Center),
        "right" => Ok(Align::Right),
        "justify" => Ok(Align::Justify),
        other => Err(bad(format!("unknown alignment '{other}'"))),
    }
}

/// Parses rows to repeat: `1`, `1:2` or `$1:$2` (1-based) to 0-based rows.
fn parse_rows(text: &str) -> R<(u32, u32)> {
    let row = |p: &str| -> R<u32> {
        p.trim()
            .trim_start_matches('$')
            .parse::<u32>()
            .ok()
            .filter(|n| *n >= 1)
            .map(|n| n - 1)
            .ok_or_else(|| bad(format!("invalid rows '{text}'")))
    };
    match text.split_once(':') {
        Some((a, b)) => Ok((row(a)?, row(b)?)),
        None => {
            let r = row(text)?;
            Ok((r, r))
        }
    }
}

/// Most page images one PDF request may name.
const MAX_PDF_PAGES: usize = writer::pdf::MAX_PAGES;

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

// ---- shapes ----------------------------------------------------------------

fn opt_f32(o: &Map<String, Value>, key: &str) -> R<Option<f32>> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().map(|v| Some(v as f32)).filter(|v| v.is_some_and(f32::is_finite)).ok_or_else(|| bad(format!("'{key}' must be a number"))),
    }
}

fn shape_patch(v: &Value) -> R<ShapePatch> {
    let o = v.as_object().ok_or_else(|| bad("shape patch must be an object"))?;
    let a = Args(o);
    let text = |key: &str| -> R<Option<String>> {
        match o.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(bad(format!("'{key}' must be a string"))),
        }
    };
    Ok(ShapePatch {
        x: opt_f32(o, "x")?,
        y: opt_f32(o, "y")?,
        w: opt_f32(o, "w")?,
        h: opt_f32(o, "h")?,
        rotation: opt_f32(o, "rotation")?,
        fill: o.get("fill").map(color).transpose()?,
        stroke: o.get("stroke").map(color).transpose()?,
        stroke_width: opt_f32(o, "stroke_width")?,
        dash: a.opt_bool("dash"),
        arrow_start: a.opt_bool("arrow_start"),
        arrow_end: a.opt_bool("arrow_end"),
        text: text("text")?,
        size: opt_f32(o, "size")?,
        bold: a.opt_bool("bold"),
        italic: a.opt_bool("italic"),
        underline: a.opt_bool("underline"),
        color: match o.get("color") {
            None => None,
            Some(v) => Some(color(v)?.unwrap_or(Rgb(0, 0, 0))),
        },
        align: match a.opt_str("align") {
            None => None,
            Some("left") => Some(ShapeAlign::Left),
            Some("center") => Some(ShapeAlign::Center),
            Some("right") => Some(ShapeAlign::Right),
            Some(other) => return Err(bad(format!("unknown alignment '{other}'"))),
        },
        valign: match a.opt_str("valign") {
            None => None,
            Some("top") => Some(VAlign::Top),
            Some("middle") => Some(VAlign::Middle),
            Some("bottom") => Some(VAlign::Bottom),
            Some(other) => return Err(bad(format!("unknown vertical alignment '{other}'"))),
        },
        bullets: a.opt_bool("bullets"),
        source: text("source")?,
    })
}

/// A new shape from `{kind, x, y, w, h, ...patch}`.
fn shape_spec(v: &Value) -> R<Shape> {
    let o = v.as_object().ok_or_else(|| bad("shape must be an object"))?;
    let a = Args(o);
    let kind = match a.str("kind")? {
        "rect" => ShapeKind::Rect,
        "round_rect" => ShapeKind::RoundRect,
        "ellipse" => ShapeKind::Ellipse,
        "triangle" => ShapeKind::Triangle,
        "diamond" => ShapeKind::Diamond,
        "line" => ShapeKind::Line,
        "text_box" => ShapeKind::TextBox,
        "diagram" => ShapeKind::Diagram { source: a.opt_str("source").unwrap_or("").to_owned() },
        "polygon" => {
            let pts = a.get("points")?.as_array().ok_or_else(|| bad("'points' must be a list"))?;
            if pts.len() > office_draw::model::MAX_POINTS {
                return Err(OdfError::limit("too many points"));
            }
            let points = pts
                .iter()
                .map(|p| {
                    let p = p.as_array().filter(|p| p.len() == 2).ok_or_else(|| bad("points are [x, y] pairs"))?;
                    let n = |v: &Value| v.as_f64().map(|v| v as f32).ok_or_else(|| bad("points are numbers"));
                    Ok([n(&p[0])?, n(&p[1])?])
                })
                .collect::<R<Vec<_>>>()?;
            ShapeKind::Polygon { points, closed: a.opt_bool("closed").unwrap_or(true) }
        }
        other => return Err(bad(format!("unknown shape kind '{other}'"))),
    };
    let mut s = Shape::new(kind, f32_arg(&a, "x")?, f32_arg(&a, "y")?, f32_arg(&a, "w")?, f32_arg(&a, "h")?);
    shape_patch(v)?.apply(&mut s)?;
    Ok(s)
}

/// Shapes as JSON; diagrams carry their rendered figure.
fn shapes_json(shapes: &[Shape]) -> Value {
    Value::Array(
        shapes
            .iter()
            .map(|s| {
                let mut v = to_value(s);
                if let (Some(fig), Some(o)) = (s.diagram_figure(), v.as_object_mut()) {
                    o.insert("figure".into(), to_value(&fig));
                }
                if let (ShapeKind::Image { image }, Some(o)) = (&s.kind, v.as_object_mut()) {
                    o.insert("format".into(), json!(image.format()));
                }
                v
            })
            .collect(),
    )
}

fn page_json(d: &Drawing, i: usize) -> Value {
    let p = &d.pages()[i];
    json!({
        "name": p.name, "title": d.page_name(i), "background": p.background.map(Rgb::to_hex),
        "notes": p.notes, "shapes": shapes_json(&p.shapes),
    })
}

/// Writer blocks as JSON, with diagram figures and canvas shapes.
fn blocks_json(blocks: &[Block]) -> Value {
    Value::Array(
        blocks
            .iter()
            .map(|b| {
                let mut v = to_value(b);
                if let Some(o) = v.as_object_mut() {
                    match b {
                        Block::Diagram(d) => {
                            let r = office_draw::mermaid::render(&d.source);
                            o.insert("height".into(), json!(d.width * r.figure.height / r.figure.width.max(1.0)));
                            o.insert("figure".into(), to_value(&r.figure));
                            o.insert("errors".into(), to_value(&r.errors));
                        }
                        Block::Drawing(c) => {
                            o.insert("shapes".into(), shapes_json(&c.shapes));
                        }
                        Block::Image(p) => {
                            o.insert("format".into(), json!(p.image.format()));
                        }
                        _ => {}
                    }
                }
                v
            })
            .collect(),
    )
}

fn zorder(s: &str) -> R<ZOrder> {
    Ok(match s {
        "front" => ZOrder::Front,
        "back" => ZOrder::Back,
        "forward" => ZOrder::Forward,
        "backward" => ZOrder::Backward,
        other => return Err(bad(format!("unknown order '{other}'"))),
    })
}

fn ids_arg(a: &Args, key: &str) -> R<Vec<u32>> {
    a.get(key)?
        .as_array()
        .ok_or_else(|| bad(format!("'{key}' must be a list of shape ids")))?
        .iter()
        .map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad("shape ids are integers")))
        .collect()
}

/// Reads and checks a picture file.
fn read_image(path: &str, limits: &Limits) -> R<Image> {
    let bytes = read_file_limited(Path::new(path), limits)?;
    Image::from_bytes(bytes)
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

    fn draw(&mut self, a: &Args) -> R<&mut Drawing> {
        match self.doc(a)? {
            Doc::Draw(d) => Ok(d),
            _ => Err(bad("not a presentation or drawing")),
        }
    }

    /// Runs a shape edit on a page of a drawing (`page`) or on a Writer
    /// canvas (`block`), as one undo step of that document.
    fn edit_shapes<T>(&mut self, a: &Args, coalesce: bool, f: impl FnOnce(&mut Vec<Shape>, &mut u32) -> R<T>) -> R<T> {
        match self.doc(a)? {
            Doc::Draw(d) => {
                let page = a.usize("page")?;
                d.edit_page(page, f)
            }
            Doc::Writer(d) => {
                let block = a.usize("block")?;
                d.edit_canvas(block, coalesce, f)
            }
            _ => Err(bad("this document has no shapes")),
        }
    }

    /// Loads a document from bytes; `ext` selects the application.
    fn load_bytes(&mut self, bytes: &[u8], ext: &str) -> R<Value> {
        let limits = self.limits.clone();
        let doc = match ext {
            "odt" | "ott" | "txt" | "text" => Doc::Writer(Box::new(writer::load(bytes, &limits)?)),
            "md" | "markdown" => {
                let text = String::from_utf8_lossy(bytes);
                Doc::Writer(Box::new(writer::markdown::from_markdown(&text, &limits)?))
            }
            // Microsoft Office: read only (saving goes to ODF). Old binary
            // .doc/.xls files get a clear "not supported" error.
            "docx" | "docm" | "dotx" | "dotm" | "doc" => Doc::Writer(Box::new(writer::load_docx(bytes, &limits)?)),
            "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" => Doc::Calc(Box::new(calc::load_xlsx(bytes, &limits)?)),
            "ods" | "ots" => Doc::Calc(Box::new(calc::load_ods(bytes, &limits)?)),
            "csv" => Doc::Calc(Box::new(calc::load_csv(bytes, None, &limits)?)),
            "tsv" | "tab" => Doc::Calc(Box::new(calc::load_csv(bytes, Some('\t'), &limits)?)),
            "odf" | "otf" | "mml" | "mathml" => Doc::Math(Box::new(Formula::from_bytes(bytes, &limits)?)),
            "odp" | "otp" | "odg" | "otg" => Doc::Draw(Box::new(office_draw::odg::load(bytes, &limits)?)),
            "ppt" | "pptx" => return Err(OdfError::unsupported("PowerPoint files (save them as .odp first)")),
            other => return Err(OdfError::unsupported(format!("file type '.{other}'"))),
        };
        self.insert(doc)
    }

    fn info(doc: &Doc) -> Value {
        let (modified, undo, redo, title) = match doc {
            Doc::Writer(d) => (d.is_modified(), d.can_undo(), d.can_redo(), d.meta.title.clone()),
            Doc::Calc(d) => (d.is_modified(), d.can_undo(), d.can_redo(), d.meta.title.clone()),
            Doc::Math(d) => (false, false, false, d.meta.title.clone()),
            Doc::Draw(d) => (d.is_modified(), d.can_undo(), d.can_redo(), d.meta.title.clone()),
        };
        json!({ "kind": doc.kind(), "modified": modified, "can_undo": undo, "can_redo": redo, "title": title })
    }

    /// Serialises a document into the format implied by `ext`.
    fn save_bytes(doc: &mut Doc, ext: &str, a: &Args) -> R<Vec<u8>> {
        let bytes = match (doc, ext) {
            (Doc::Writer(d), "odt") => writer::save_odt(d)?,
            (Doc::Writer(d), "docx") => writer::save_docx(d)?,
            (Doc::Writer(d), "txt") => d.plain_text().into_bytes(),
            (Doc::Writer(d), "html" | "htm") => writer::export::to_html(d).into_bytes(),
            (Doc::Writer(d), "md") => writer::export::to_markdown(d).into_bytes(),
            (Doc::Calc(d), "ods") => calc::save_ods(d)?,
            (Doc::Calc(d), "xlsx") => calc::save_xlsx(d)?,
            (Doc::Calc(d), "csv" | "tsv") => {
                let sheet = a.0.get("sheet").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delimiter = if ext == "tsv" { '\t' } else { ',' };
                calc::sheet_to_csv(d, sheet, delimiter, a.opt_bool("sanitize").unwrap_or(true))?.into_bytes()
            }
            (Doc::Math(d), "odf") => d.to_odf_bytes()?,
            (Doc::Math(d), "mml") => d.to_mathml().into_bytes(),
            (Doc::Draw(d), "odp") if d.kind() == DrawingKind::Presentation => office_draw::odg::save(d)?,
            (Doc::Draw(d), "odg") if d.kind() == DrawingKind::Drawing => office_draw::odg::save(d)?,
            (Doc::Draw(d), "txt") => d.plain_text().into_bytes(),
            (Doc::Draw(d), "svg" | "png") => {
                let page = a.0.get("page").and_then(Value::as_u64).unwrap_or(0) as usize;
                let p = d.pages().get(page).ok_or_else(|| bad(format!("no page {page}")))?;
                let (w, h) = d.size();
                let fig = office_draw::model::shapes_to_figure(&p.shapes, w, h, Some(p.background.unwrap_or(Rgb(255, 255, 255))));
                if ext == "svg" { fig.to_svg(None).into_bytes() } else { office_draw::render::figure_to_png(&fig, 2.0, None)? }
            }
            (doc, other) => {
                return Err(OdfError::unsupported(format!("saving a {} document as '.{other}'", doc.kind())))
            }
        };
        Ok(bytes)
    }

    fn mark_saved(doc: &mut Doc, ext: &str) {
        // The native formats, and Word / Excel (which keep everything the
        // model holds), clear "modified"; exports to text, HTML etc. do not.
        match (doc, ext) {
            (Doc::Writer(d), "odt" | "docx") => d.mark_saved(),
            (Doc::Calc(d), "ods" | "xlsx") => d.mark_saved(),
            (Doc::Draw(d), "odp" | "odg") => d.mark_saved(),
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
                    "impress" => Doc::Draw(Box::new(Drawing::new(DrawingKind::Presentation))),
                    "draw" => Doc::Draw(Box::new(Drawing::new(DrawingKind::Drawing))),
                    other => return Err(bad(format!("unknown document kind '{other}'"))),
                };
                self.insert(doc)
            }
            "new_markdown" => {
                let text = a.str("text")?;
                let doc = writer::markdown::from_markdown(text, &self.limits)?;
                self.insert(Doc::Writer(Box::new(doc)))
            }
            "new_envelope" => {
                let page = page_arg(&a)?;
                let doc = Document::envelope(page, a.opt_str("sender").unwrap_or(""), a.opt_str("recipient").unwrap_or(""));
                self.insert(Doc::Writer(Box::new(doc)))
            }
            "new_index_cards" => {
                let page = page_arg(&a)?;
                let count = a.0.get("count").and_then(Value::as_u64).unwrap_or(1).min(500) as usize;
                self.insert(Doc::Writer(Box::new(Document::index_cards(page, count))))
            }
            "new_labels" => {
                let id = a.str("sheet")?;
                let sheet = writer::label_sheet(id).ok_or_else(|| bad(format!("unknown label sheet '{id}'")))?;
                let text = a.opt_str("text").unwrap_or("");
                if text.len() > MAX_TRANSLIT_LEN {
                    return Err(OdfError::limit("label text is too long"));
                }
                let doc = Document::labels(sheet, text, a.opt_bool("same").unwrap_or(true))?;
                self.insert(Doc::Writer(Box::new(doc)))
            }
            "paper_sizes" => Ok(json!({
                "papers": to_value(&writer::PAPER_SIZES),
                "labels": to_value(&writer::LABEL_SHEETS),
            })),
            "pdf_from_pngs" => {
                let out = a.str("path")?.to_owned();
                let (width, height) = (f32_arg(&a, "width")?, f32_arg(&a, "height")?);
                let pngs = a.get("pngs")?.as_array().ok_or_else(|| bad("'pngs' must be a list of paths"))?;
                if pngs.len() > MAX_PDF_PAGES {
                    return Err(OdfError::limit("too many pages"));
                }
                let mut pages = Vec::with_capacity(pngs.len());
                for p in pngs {
                    let path = p.as_str().ok_or_else(|| bad("'pngs' must be a list of paths"))?;
                    let bytes = read_file_limited(Path::new(path), &self.limits)?;
                    pages.push(writer::pdf::decode_png(&bytes)?);
                }
                let pdf = writer::pdf::images_to_pdf(&pages, width, height, a.opt_str("title"))?;
                write_file_atomic(Path::new(&out), &pdf)?;
                Ok(json!(pages.len()))
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
                    (Doc::Draw(d), "undo") => d.undo(),
                    (Doc::Draw(d), _) => d.redo(),
                };
                Ok(json!(done))
            }
            "set_title" => {
                let title = a.opt_str("title").map(str::to_owned).filter(|t| !t.is_empty());
                match self.doc(&a)? {
                    Doc::Writer(d) => d.meta.title = title,
                    Doc::Calc(d) => d.meta.title = title,
                    Doc::Math(d) => d.meta.title = title,
                    Doc::Draw(d) => d.meta.title = title,
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
            "calc.print_area" => {
                let sheet = a.usize("sheet")?;
                let area = *self.calc(&a)?.sheet(sheet)?.print_area();
                Ok(json!({
                    "range": area.range.map(|r| r.to_a1()),
                    "repeat_rows": area.repeat_rows.map(|(x, y)| if x == y { format!("{}", x + 1) } else { format!("{}:{}", x + 1, y + 1) }),
                }))
            }
            "calc.set_print_area" => {
                let sheet = a.usize("sheet")?;
                let text = |key: &str| a.opt_str(key).map(str::trim).filter(|t| !t.is_empty());
                let range = match text("range") {
                    Some(t) => Some(Range::parse(&t.replace('$', "")).filter(Range::is_valid).ok_or_else(|| bad(format!("invalid range '{t}'")))?),
                    None => None,
                };
                let repeat_rows = text("repeat_rows").map(parse_rows).transpose()?;
                self.calc(&a)?.set_print_area(sheet, PrintArea { range, repeat_rows })?;
                Ok(Value::Null)
            }
            "calc.page" => {
                let p = *self.calc(&a)?.page();
                Ok(json!({
                    "width": p.width, "height": p.height, "margin": p.margin, "scale": p.scale,
                    "fit_width": p.fit_width, "grid": p.grid, "header_footer": p.header_footer,
                    "paper": writer::PageSetup { width: p.width, height: p.height, margin: p.margin }.paper().map(|x| x.id),
                }))
            }
            "calc.set_page" => {
                let current = *self.calc(&a)?.page();
                let scale = match a.0.get("scale") {
                    Some(v) => v.as_u64().and_then(|s| u16::try_from(s).ok()).ok_or_else(|| bad("'scale' must be a percentage"))?,
                    None => current.scale,
                };
                let page = CalcPage {
                    width: f32_arg(&a, "width")?,
                    height: f32_arg(&a, "height")?,
                    margin: f32_arg(&a, "margin")?,
                    scale,
                    fit_width: a.opt_bool("fit_width").unwrap_or(current.fit_width),
                    grid: a.opt_bool("grid").unwrap_or(current.grid),
                    header_footer: a.opt_bool("header_footer").unwrap_or(current.header_footer),
                };
                self.calc(&a)?.set_page(page)?;
                Ok(Value::Null)
            }

            // ---- writer ----
            "writer.get" => {
                let d = self.writer(&a)?;
                Ok(json!({
                    "blocks": blocks_json(d.blocks()),
                    "header": to_value(&d.header()),
                    "footer": to_value(&d.footer()),
                    "page": to_value(d.page()),
                    "paper": d.page().paper().map(|p| p.id),
                    "stats": to_value(&d.stats()),
                }))
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
                let align = align_arg(a.str("align")?)?;
                self.writer(&a)?.set_align(r, align)?;
                Ok(Value::Null)
            }
            "writer.indent" => {
                let (r, delta) = (a.para("para")?, a.i32("delta")?);
                self.writer(&a)?.indent(r, delta)?;
                Ok(Value::Null)
            }
            "writer.spacing" => {
                let r = a.para("para")?;
                let (indent, space) = (f32_arg(&a, "indent")?, f32_arg(&a, "space_before")?);
                self.writer(&a)?.set_spacing(r, indent, space)?;
                Ok(Value::Null)
            }
            "writer.field" => {
                let (r, at) = (a.para("para")?, a.usize("at")?);
                let field = match a.str("field")? {
                    "page_number" => Field::PageNumber,
                    "page_count" => Field::PageCount,
                    other => return Err(bad(format!("unknown field '{other}'"))),
                };
                self.writer(&a)?.insert_field(r, at, field)?;
                Ok(Value::Null)
            }
            "writer.page" => {
                let page = page_arg(&a)?;
                self.writer(&a)?.set_page(page)?;
                Ok(Value::Null)
            }
            "writer.header" | "writer.footer" => {
                let on = a.bool("on")?;
                let d = self.writer(&a)?;
                if op == "writer.header" {
                    d.set_header(on)?;
                } else {
                    d.set_footer(on)?;
                }
                Ok(Value::Null)
            }
            "writer.letterhead" => {
                let head = Letterhead {
                    name: a.opt_str("name").unwrap_or("").to_owned(),
                    details: a.opt_str("details").unwrap_or("").to_owned(),
                    contact: a.opt_str("contact").unwrap_or("").to_owned(),
                    align: align_arg(a.opt_str("align").unwrap_or("left"))?,
                    color: match a.0.get("color") {
                        Some(v) => color(v)?,
                        None => None,
                    },
                };
                self.writer(&a)?.set_letterhead(&head)?;
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

            "writer.insert_image" => {
                let at = a.usize("at")?;
                let image = read_image(a.str("path")?, &self.limits)?;
                let d = self.writer(&a)?;
                let page = *d.page();
                let picture = writer::Picture::new(image, (page.width - 2.0 * page.margin).max(72.0));
                d.insert_block(at, Block::Image(picture))?;
                Ok(Value::Null)
            }
            "writer.insert_diagram" => {
                let at = a.usize("at")?;
                let source = a.str("source")?.to_owned();
                if source.len() > office_draw::model::MAX_DIAGRAM_SOURCE {
                    return Err(OdfError::limit("diagram is too long"));
                }
                let d = self.writer(&a)?;
                let page = *d.page();
                let fig = office_draw::mermaid::render(&source).figure;
                let width = fig.width.min((page.width - 2.0 * page.margin).max(72.0));
                d.insert_block(at, Block::Diagram(writer::Diagram { source, width, align: Align::Center }))?;
                Ok(Value::Null)
            }
            "writer.insert_markdown" => {
                let at = a.usize("at")?;
                let text = a.str("text")?;
                if text.len() as u64 > self.limits.max_entry_size {
                    return Err(OdfError::limit("text is too long"));
                }
                let d = self.writer(&a)?;
                let page = *d.page();
                let blocks = writer::markdown::markdown_blocks(text, (page.width - 2.0 * page.margin).max(72.0));
                let n = blocks.len();
                d.insert_blocks(at, blocks)?;
                Ok(json!({ "blocks": n }))
            }
            "writer.insert_drawing" => {
                let at = a.usize("at")?;
                let d = self.writer(&a)?;
                let page = *d.page();
                let text = (page.width - 2.0 * page.margin).max(72.0);
                let width = opt_f32(a.0, "width")?.unwrap_or(text);
                let height = opt_f32(a.0, "height")?.unwrap_or(text * 0.5);
                d.insert_block(at, Block::Drawing(writer::Canvas::new(width, height)))?;
                Ok(Value::Null)
            }
            "writer.frame" => {
                let block = a.usize("block")?;
                let width = opt_f32(a.0, "width")?;
                let height = opt_f32(a.0, "height")?;
                let align = a.opt_str("align").map(align_arg).transpose()?;
                let d = self.writer(&a)?;
                if let Some(w) = width {
                    d.set_frame_size(block, w, height)?;
                }
                if let Some(al) = align {
                    d.set_frame_align(block, al)?;
                }
                if let Some(desc) = a.opt_str("description") {
                    d.set_picture_description(block, desc)?;
                }
                if let Some(src) = a.opt_str("source") {
                    d.set_diagram_source(block, src)?;
                }
                Ok(Value::Null)
            }

            // ---- pictures and diagrams ----
            "set_fonts" => {
                office_draw::render::add_font_dir(a.str("dir")?);
                Ok(Value::Null)
            }
            "image.data" => {
                let id = a.str("id")?.to_owned();
                let raster = a.opt_bool("raster").unwrap_or(true);
                let image = match self.doc(&a)? {
                    Doc::Writer(d) => d.image(&id),
                    Doc::Draw(d) => d.image(&id),
                    _ => None,
                }
                .or_else(|| self.clipboard.iter().find_map(|s| match &s.kind {
                    ShapeKind::Image { image } if image.id() == id => Some(image.clone()),
                    _ => None,
                }))
                .ok_or_else(|| bad(format!("no picture {id}")))?;
                let (w, h) = image.pixel_size();
                let (format, bytes) = if raster && !image.format().is_raster() {
                    ("png", office_draw::render::svg_to_png(image.bytes(), 2.0, 2048, None)?)
                } else {
                    (image.format().extension(), image.bytes().to_vec())
                };
                Ok(json!({ "format": format, "width": w, "height": h, "data": office_draw::image::base64(&bytes) }))
            }
            "mermaid.render" => {
                let source = a.str("source")?;
                if source.len() > office_draw::model::MAX_DIAGRAM_SOURCE {
                    return Err(OdfError::limit("diagram is too long"));
                }
                Ok(to_value(&office_draw::mermaid::render(source)))
            }
            "mermaid.svg" => {
                let source = a.str("source")?;
                if source.len() > office_draw::model::MAX_DIAGRAM_SOURCE {
                    return Err(OdfError::limit("diagram is too long"));
                }
                Ok(json!(office_draw::mermaid::render(source).figure.to_svg(None)))
            }

            // ---- impress and draw ----
            "draw.get" => {
                let d = self.draw(&a)?;
                let (w, h) = d.size();
                let pages: Vec<Value> = (0..d.pages().len()).map(|i| page_json(d, i)).collect();
                let kind = if d.kind() == DrawingKind::Presentation { "impress" } else { "draw" };
                Ok(json!({ "kind": kind, "width": w, "height": h, "pages": pages }))
            }
            "draw.page" => {
                let page = a.usize("page")?;
                let d = self.draw(&a)?;
                if page >= d.pages().len() {
                    return Err(bad(format!("no page {page}")));
                }
                Ok(page_json(d, page))
            }
            "draw.add_page" => {
                let at = a.usize("at")?;
                let layout = a.opt_str("layout").unwrap_or("blank");
                let layout = Layout::parse(layout).ok_or_else(|| bad(format!("unknown layout '{layout}'")))?;
                self.draw(&a)?.add_page(at, layout)?;
                Ok(Value::Null)
            }
            "draw.duplicate_page" => {
                let page = a.usize("page")?;
                self.draw(&a)?.duplicate_page(page)?;
                Ok(Value::Null)
            }
            "draw.remove_page" => {
                let page = a.usize("page")?;
                self.draw(&a)?.remove_page(page)?;
                Ok(Value::Null)
            }
            "draw.move_page" => {
                let (from, to) = (a.usize("from")?, a.usize("to")?);
                self.draw(&a)?.move_page(from, to)?;
                Ok(Value::Null)
            }
            "draw.page_props" => {
                let page = a.usize("page")?;
                let name = a.opt_str("name").map(str::to_owned);
                let background = a.0.get("background").map(color).transpose()?;
                let notes = a.opt_str("notes").map(str::to_owned);
                self.draw(&a)?.set_page_props(page, name.as_deref(), background, notes.as_deref())?;
                Ok(Value::Null)
            }
            "draw.set_size" => {
                let (w, h) = (f32_arg(&a, "width")?, f32_arg(&a, "height")?);
                let scale = a.opt_bool("scale").unwrap_or(false);
                self.draw(&a)?.set_size(w, h, scale)?;
                Ok(Value::Null)
            }
            "draw.break_undo" => {
                match self.doc(&a)? {
                    Doc::Draw(d) => d.break_undo_group(),
                    Doc::Writer(d) => d.break_undo_group(),
                    _ => {}
                }
                Ok(Value::Null)
            }

            // ---- shapes on a slide / page (`page`) or a Writer canvas (`block`) ----
            "shapes.add" => {
                let list = a.get("shapes")?.as_array().ok_or_else(|| bad("'shapes' must be a list"))?;
                if list.len() > office_draw::model::MAX_SHAPES {
                    return Err(OdfError::limit("too many shapes"));
                }
                let shapes = list.iter().map(shape_spec).collect::<R<Vec<_>>>()?;
                Ok(json!(self.edit_shapes(&a, false, |v, next| shape_ops::add(v, shapes, next))?))
            }
            "shapes.update" => {
                let list = a.get("updates")?.as_array().ok_or_else(|| bad("'updates' must be a list"))?;
                let patches = list
                    .iter()
                    .map(|u| {
                        let o = u.as_object().ok_or_else(|| bad("updates are objects"))?;
                        let id = Args(o).u32("id")?;
                        Ok((id, shape_patch(Args(o).get("patch")?)?))
                    })
                    .collect::<R<Vec<_>>>()?;
                let key = a.opt_str("key").map(str::to_owned);
                if let (Some(k), Ok(page)) = (key.as_deref(), a.usize("page")) {
                    if let Doc::Draw(d) = self.doc(&a)? {
                        d.update_shapes(page, &patches, Some(k))?;
                        return Ok(Value::Null);
                    }
                }
                self.edit_shapes(&a, key.is_some(), |v, _| shape_ops::update(v, &patches))?;
                Ok(Value::Null)
            }
            "shapes.remove" => {
                let ids = ids_arg(&a, "ids")?;
                self.edit_shapes(&a, false, |v, _| shape_ops::remove(v, &ids))?;
                Ok(Value::Null)
            }
            "shapes.order" => {
                let (id, to) = (a.u32("id")?, zorder(a.str("to")?)?);
                self.edit_shapes(&a, false, |v, _| shape_ops::reorder(v, id, to))?;
                Ok(Value::Null)
            }
            "shapes.copy" => {
                let ids = ids_arg(&a, "ids")?;
                let copied = self.edit_shapes(&a, false, |v, _| Ok(shape_ops::copy(v, &ids)))?;
                let n = copied.len();
                if n > 0 {
                    self.clipboard = copied;
                }
                Ok(json!(n))
            }
            "shapes.paste" => {
                let (dx, dy) = (opt_f32(a.0, "dx")?.unwrap_or(12.0), opt_f32(a.0, "dy")?.unwrap_or(12.0));
                let mut shapes = self.clipboard.clone();
                for s in &mut shapes {
                    s.x += dx;
                    s.y += dy;
                }
                Ok(json!(self.edit_shapes(&a, false, |v, next| shape_ops::add(v, shapes, next))?))
            }
            "shapes.insert_image" => {
                let image = read_image(a.str("path")?, &self.limits)?;
                let max_w = opt_f32(a.0, "max_w")?.unwrap_or(400.0);
                let max_h = opt_f32(a.0, "max_h")?.unwrap_or(300.0);
                let (w, h) = image.fit_size(max_w, max_h);
                let x = opt_f32(a.0, "x")?.unwrap_or(36.0);
                let y = opt_f32(a.0, "y")?.unwrap_or(36.0);
                let shape = Shape::new(ShapeKind::Image { image }, x, y, w, h);
                Ok(json!(self.edit_shapes(&a, false, |v, next| shape_ops::add(v, vec![shape], next))?.first()))
            }

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

            other => match crate::assist::dispatch(other, obj) {
                Some(result) => result,
                None => Err(bad(format!("unknown operation '{other}'"))),
            },
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
