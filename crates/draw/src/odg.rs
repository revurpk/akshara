//! OpenDocument Presentation (`.odp`) and Graphics (`.odg`) import and
//! export.
//!
//! Supported: pages with names, backgrounds and speaker notes; rectangles
//! (also rounded), ellipses, lines and connectors with arrowheads,
//! polygons, polylines and paths (flattened), custom shapes of the common
//! types, text frames and presentation placeholders, pictures (embedded in
//! `Pictures/`), groups (flattened), rotation, fill and outline colours
//! (gradients become their start colour), dashes and text formatting.
//! Shapes on master pages are copied onto the pages that use them.
//!
//! Mermaid diagrams are saved as SVG pictures whose description holds the
//! source, so other office suites show the drawing and Akshara restores the
//! editable diagram.

use crate::drawing::{Drawing, DrawingKind};
use crate::image::{base64_decode, Image};
use crate::model::{HAlign, Page, Placeholder, Shape, ShapeKind, TextStyle, VAlign};
use office_odf::util::{format_length_in, parse_length_pt};
use office_odf::{ns, Element, Limits, Metadata, Node, OdfError, Package, Result, Rgb, XmlWriter};
use std::collections::HashMap;

/// Name given to Mermaid diagram frames (and recognised on import).
pub const DIAGRAM_NAME: &str = "Mermaid diagram";

fn len(v: f32) -> String {
    format_length_in(f64::from(v))
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Pictures collected while writing, stored under one folder.
#[derive(Debug, Default)]
pub struct Pictures {
    entries: Vec<(String, Vec<u8>)>,
}

impl Pictures {
    /// Registers a file and returns its path inside the package.
    pub fn add(&mut self, name: String, bytes: &[u8]) -> String {
        if !self.entries.iter().any(|(n, _)| *n == name) {
            self.entries.push((name.clone(), bytes.to_vec()));
        }
        name
    }

    /// Adds every collected file to a package under `prefix` (`""` or
    /// `"Object 1/"`).
    pub fn insert_into(self, package: &mut Package, prefix: &str) -> Result<()> {
        for (name, bytes) in self.entries {
            package.insert(format!("{prefix}{name}"), bytes)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct GraphicKey {
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
    stroke_width: f32,
    dash: bool,
    arrow_start: bool,
    arrow_end: bool,
    text: TextStyle,
    frame: bool,
}

/// Automatic graphic styles of one content stream.
#[derive(Debug, Default)]
pub struct Styles {
    graphic: Vec<GraphicKey>,
    pages: Vec<Option<Rgb>>,
    prefix: &'static str,
}

impl Styles {
    /// A style table whose names start with `prefix`.
    pub fn new(prefix: &'static str) -> Styles {
        Styles { prefix, ..Styles::default() }
    }

    fn graphic(&mut self, s: &Shape) -> String {
        let frame = matches!(s.kind, ShapeKind::TextBox | ShapeKind::Image { .. } | ShapeKind::Diagram { .. });
        let key = GraphicKey {
            fill: s.fill,
            stroke: s.stroke,
            stroke_width: s.stroke_width,
            dash: s.dash,
            arrow_start: s.arrow_start,
            arrow_end: s.arrow_end,
            text: s.style,
            frame,
        };
        let i = match self.graphic.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                self.graphic.push(key);
                self.graphic.len() - 1
            }
        };
        format!("{}gr{}", self.prefix, i + 1)
    }

    fn page(&mut self, background: Option<Rgb>) -> String {
        let i = match self.pages.iter().position(|k| *k == background) {
            Some(i) => i,
            None => {
                self.pages.push(background);
                self.pages.len() - 1
            }
        };
        format!("{}dp{}", self.prefix, i + 1)
    }

    /// Writes the collected styles (inside `office:automatic-styles`).
    pub fn write(&self, w: &mut XmlWriter) {
        for (i, k) in self.graphic.iter().enumerate() {
            w.start("style:style").attr("style:name", &format!("{}gr{}", self.prefix, i + 1)).attr("style:family", "graphic");
            if !k.frame {
                w.attr("style:parent-style-name", "standard");
            }
            w.start("style:graphic-properties");
            match k.fill {
                Some(c) => w.attr("draw:fill", "solid").attr("draw:fill-color", &c.to_hex()),
                None => w.attr("draw:fill", "none"),
            };
            match k.stroke {
                Some(c) if k.stroke_width > 0.0 => {
                    w.attr("draw:stroke", if k.dash { "dash" } else { "solid" })
                        .attr("svg:stroke-color", &c.to_hex())
                        .attr("svg:stroke-width", &len(k.stroke_width));
                    if k.dash {
                        w.attr("draw:stroke-dash", "Dash");
                    }
                }
                _ => {
                    w.attr("draw:stroke", "none");
                }
            }
            let marker = len(3.0 + k.stroke_width * 4.0);
            if k.arrow_start {
                w.attr("draw:marker-start", "Arrow").attr("draw:marker-start-width", &marker);
            }
            if k.arrow_end {
                w.attr("draw:marker-end", "Arrow").attr("draw:marker-end-width", &marker);
            }
            let t = &k.text;
            w.attr(
                "draw:textarea-vertical-align",
                match t.valign {
                    VAlign::Top => "top",
                    VAlign::Middle => "middle",
                    VAlign::Bottom => "bottom",
                },
            )
            .attr("draw:auto-grow-height", "false")
            .attr("fo:min-height", "0in")
            .attr("fo:padding", "0.0833in")
            .attr("fo:wrap-option", "wrap");
            w.end();
            w.start("style:paragraph-properties")
                .attr(
                    "fo:text-align",
                    match t.align {
                        HAlign::Left => "start",
                        HAlign::Center => "center",
                        HAlign::Right => "end",
                    },
                )
                .end();
            w.start("style:text-properties")
                .attr("fo:font-size", &format!("{}pt", trim(t.size)))
                .attr("fo:color", &t.color.to_hex())
                .attr("fo:font-weight", if t.bold { "bold" } else { "normal" })
                .attr("fo:font-style", if t.italic { "italic" } else { "normal" });
            if t.underline {
                w.attr("style:text-underline-style", "solid").attr("style:text-underline-width", "auto").attr("style:text-underline-color", "font-color");
            }
            w.end();
            w.end();
        }
        for (i, bg) in self.pages.iter().enumerate() {
            w.start("style:style").attr("style:name", &format!("{}dp{}", self.prefix, i + 1)).attr("style:family", "drawing-page");
            w.start("style:drawing-page-properties");
            match bg {
                Some(c) => w.attr("draw:fill", "solid").attr("draw:fill-color", &c.to_hex()).attr("draw:background-size", "full"),
                None => w.attr("draw:fill", "none"),
            };
            w.end().end();
        }
    }
}

fn trim(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Writes text paragraphs (as a bulleted list when `bullets`).
fn write_text(w: &mut XmlWriter, text: &str, bullets: bool) {
    if text.is_empty() {
        return;
    }
    if bullets {
        w.start("text:list").attr("text:style-name", "L1");
    }
    for para in text.split('\n') {
        if bullets {
            w.start("text:list-item");
        }
        w.start("text:p");
        write_spans(w, para);
        w.end();
        if bullets {
            w.end();
        }
    }
    if bullets {
        w.end();
    }
}

/// Text with tabs and runs of spaces as `text:tab` / `text:s`.
fn write_spans(w: &mut XmlWriter, text: &str) {
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    let mut prev_space = true;
    while let Some(c) = chars.next() {
        match c {
            '\t' => {
                w.text(&std::mem::take(&mut buf));
                w.start("text:tab").end();
                prev_space = false;
            }
            ' ' if prev_space || chars.peek().is_none() => {
                let mut n = 1;
                while chars.peek() == Some(&' ') {
                    chars.next();
                    n += 1;
                }
                w.text(&std::mem::take(&mut buf));
                w.start("text:s");
                if n > 1 {
                    w.attr("text:c", &n.to_string());
                }
                w.end();
                prev_space = false;
            }
            c => {
                buf.push(c);
                prev_space = c == ' ';
            }
        }
    }
    w.text(&buf);
}

/// Placement attributes, with rotation as an ODF transform.
fn geometry(w: &mut XmlWriter, s: &Shape) {
    let (x, y, sw, sh) = s.bounds();
    w.attr("svg:width", &len(sw)).attr("svg:height", &len(sh));
    if s.rotation == 0.0 {
        w.attr("svg:x", &len(x)).attr("svg:y", &len(y));
    } else {
        // Rotate clockwise about the centre: ODF rotates about the origin
        // (counter-clockwise for positive angles) and then translates.
        let t = s.rotation.to_radians();
        let (cx, cy) = (x + sw / 2.0, y + sh / 2.0);
        let (dx, dy) = (-sw / 2.0, -sh / 2.0);
        let tx = cx + dx * t.cos() - dy * t.sin();
        let ty = cy + dx * t.sin() + dy * t.cos();
        w.attr("draw:transform", &format!("rotate ({}) translate ({}pt {}pt)", -t, trim(tx), trim(ty)));
    }
}

/// Writes one shape. Pictures go to `pics` under `Pictures/`.
pub fn write_shape(w: &mut XmlWriter, s: &Shape, styles: &mut Styles, pics: &mut Pictures, page: usize) {
    let style = styles.graphic(s);
    let _ = page;
    match &s.kind {
        ShapeKind::Rect | ShapeKind::RoundRect => {
            w.start("draw:rect").attr("draw:style-name", &style);
            if matches!(s.kind, ShapeKind::RoundRect) {
                w.attr("draw:corner-radius", &len(s.w.min(s.h) * 0.15));
            }
            geometry(w, s);
            write_text(w, &s.text, s.style.bullets);
            w.end();
        }
        ShapeKind::Ellipse => {
            w.start("draw:ellipse").attr("draw:style-name", &style);
            geometry(w, s);
            write_text(w, &s.text, s.style.bullets);
            w.end();
        }
        ShapeKind::Triangle | ShapeKind::Diamond => {
            let (kind, path) = if matches!(s.kind, ShapeKind::Triangle) {
                ("isosceles-triangle", "M 10800 0 L 21600 21600 0 21600 Z N")
            } else {
                ("diamond", "M 10800 0 L 21600 10800 10800 21600 0 10800 Z N")
            };
            w.start("draw:custom-shape").attr("draw:style-name", &style);
            geometry(w, s);
            write_text(w, &s.text, s.style.bullets);
            w.start("draw:enhanced-geometry")
                .attr("svg:viewBox", "0 0 21600 21600")
                .attr("draw:type", kind)
                .attr("draw:enhanced-path", path)
                .end();
            w.end();
        }
        ShapeKind::Line => {
            w.start("draw:line")
                .attr("draw:style-name", &style)
                .attr("svg:x1", &len(s.x))
                .attr("svg:y1", &len(s.y))
                .attr("svg:x2", &len(s.x + s.w))
                .attr("svg:y2", &len(s.y + s.h));
            w.end();
        }
        ShapeKind::Polygon { points, closed } => {
            w.start(if *closed { "draw:polygon" } else { "draw:polyline" }).attr("draw:style-name", &style);
            geometry(w, s);
            w.attr("svg:viewBox", "0 0 10000 10000");
            let pts: Vec<String> = points.iter().map(|p| format!("{},{}", (p[0] * 10000.0).round(), (p[1] * 10000.0).round())).collect();
            w.attr("draw:points", &pts.join(" "));
            write_text(w, &s.text, s.style.bullets);
            w.end();
        }
        ShapeKind::TextBox => {
            w.start("draw:frame").attr("draw:style-name", &style);
            if let Some(role) = s.placeholder {
                w.attr(
                    "presentation:class",
                    match role {
                        Placeholder::Title => "title",
                        Placeholder::Subtitle => "subtitle",
                        Placeholder::Outline => "outline",
                    },
                );
                if s.text.is_empty() {
                    w.attr("presentation:placeholder", "true");
                }
                w.attr("presentation:user-transformed", "true");
            }
            geometry(w, s);
            w.start("draw:text-box");
            write_text(w, &s.text, s.style.bullets);
            w.end();
            w.end();
        }
        ShapeKind::Image { image } => {
            let href = pics.add(image.package_name("Pictures"), image.bytes());
            w.start("draw:frame").attr("draw:style-name", &style);
            geometry(w, s);
            w.start("draw:image")
                .attr("xlink:href", &href)
                .attr("xlink:type", "simple")
                .attr("xlink:show", "embed")
                .attr("xlink:actuate", "onLoad")
                .attr("draw:mime-type", image.format().mime());
            w.end();
            w.end();
        }
        ShapeKind::Diagram { source } => {
            let svg = crate::mermaid::render(source).figure.to_svg(None);
            let name = diagram_file_name(source);
            let href = pics.add(name, svg.as_bytes());
            w.start("draw:frame").attr("draw:style-name", &style).attr("draw:name", DIAGRAM_NAME);
            geometry(w, s);
            w.start("draw:image")
                .attr("xlink:href", &href)
                .attr("xlink:type", "simple")
                .attr("xlink:show", "embed")
                .attr("xlink:actuate", "onLoad")
                .attr("draw:mime-type", "image/svg+xml");
            w.end();
            w.element_text("svg:title", DIAGRAM_NAME);
            w.element_text("svg:desc", source);
            w.end();
        }
    }
}

/// Package file name of a diagram's SVG picture.
pub fn diagram_file_name(source: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("Pictures/mermaid-{h:016x}.svg")
}

const NAMESPACES: &[&str] = &["office", "style", "text", "draw", "fo", "xlink", "svg", "presentation", "dc", "meta"];

/// The streams of a drawing document: `(content.xml, styles.xml,
/// pictures)`.
pub fn streams(kind: DrawingKind, pages: &[Page], page_names: &[String], width: f32, height: f32) -> (String, String, Pictures) {
    let mut styles = Styles::new("");
    let mut pics = Pictures::default();
    let mut body = XmlWriter::fragment();
    let presentation = kind == DrawingKind::Presentation;
    body.start("office:body").start(if presentation { "office:presentation" } else { "office:drawing" });
    for (i, page) in pages.iter().enumerate() {
        let dp = styles.page(page.background);
        body.start("draw:page")
            .attr("draw:name", page_names.get(i).map(String::as_str).unwrap_or(""))
            .attr("draw:style-name", &dp)
            .attr("draw:master-page-name", "Default");
        for s in &page.shapes {
            write_shape(&mut body, s, &mut styles, &mut pics, i);
        }
        if presentation && !page.notes.is_empty() {
            body.start("presentation:notes");
            body.start("draw:page-thumbnail")
                .attr("draw:page-number", &(i + 1).to_string())
                .attr("presentation:class", "page")
                .attr("svg:x", "1.1in")
                .attr("svg:y", "0.9in")
                .attr("svg:width", "6.1in")
                .attr("svg:height", "3.4in")
                .end();
            body.start("draw:frame").attr("presentation:class", "notes").attr("svg:x", "0.9in").attr("svg:y", "4.7in").attr("svg:width", "6.5in").attr("svg:height", "4.5in");
            body.start("draw:text-box");
            write_text(&mut body, &page.notes, false);
            body.end().end();
            body.end();
        }
        body.end();
    }
    body.end().end();
    let body = body.finish();

    let mut w = XmlWriter::new();
    w.start("office:document-content").namespaces(NAMESPACES).attr("office:version", ns::ODF_VERSION);
    w.start("office:automatic-styles");
    styles.write(&mut w);
    w.start("text:list-style").attr("style:name", "L1");
    for level in 1..=3 {
        w.start("text:list-level-style-bullet").attr("text:level", &level.to_string()).attr("text:bullet-char", ["•", "–", "•"][level - 1]);
        w.start("style:list-level-properties").attr("text:space-before", &len(18.0 * (level - 1) as f32)).attr("text:min-label-width", "0.25in").end();
        w.end();
    }
    w.end();
    w.end();
    let mut content = w.finish();
    if let Some(pos) = content.rfind("</office:document-content>") {
        content.insert_str(pos, &body);
    }
    (content, styles_xml(width, height), pics)
}

fn styles_xml(width: f32, height: f32) -> String {
    let mut w = XmlWriter::new();
    w.start("office:document-styles").namespaces(NAMESPACES).attr("office:version", ns::ODF_VERSION);
    w.start("office:styles");
    w.start("draw:marker").attr("draw:name", "Arrow").attr("svg:viewBox", "0 0 20 30").attr("svg:d", "M10 0l-10 30h20z").end();
    w.start("draw:stroke-dash")
        .attr("draw:name", "Dash")
        .attr("draw:style", "rect")
        .attr("draw:dots1", "1")
        .attr("draw:dots1-length", "200%")
        .attr("draw:distance", "150%")
        .end();
    w.start("style:default-style").attr("style:family", "graphic");
    w.start("style:graphic-properties").attr("svg:stroke-color", "#3465a4").attr("draw:fill-color", "#729fcf").end();
    w.start("style:text-properties").attr("fo:font-size", "18pt").attr("fo:font-family", "'Noto Sans'").attr("style:font-family-generic", "swiss").end();
    w.end();
    w.start("style:style").attr("style:name", "standard").attr("style:family", "graphic");
    w.start("style:graphic-properties").attr("draw:stroke", "solid").attr("svg:stroke-width", "0in").attr("draw:fill", "solid").end();
    w.end();
    w.end();
    w.start("office:automatic-styles");
    w.start("style:page-layout").attr("style:name", "PM1");
    w.start("style:page-layout-properties")
        .attr("fo:margin-top", "0in")
        .attr("fo:margin-bottom", "0in")
        .attr("fo:margin-left", "0in")
        .attr("fo:margin-right", "0in")
        .attr("fo:page-width", &len(width))
        .attr("fo:page-height", &len(height))
        .attr("style:print-orientation", if width > height { "landscape" } else { "portrait" })
        .end();
    w.end();
    w.start("style:style").attr("style:name", "Mdp1").attr("style:family", "drawing-page");
    w.start("style:drawing-page-properties").attr("draw:fill", "none").end();
    w.end();
    w.end();
    w.start("office:master-styles");
    w.start("style:master-page").attr("style:name", "Default").attr("style:page-layout-name", "PM1").attr("draw:style-name", "Mdp1").end();
    w.end();
    w.finish()
}

/// Serialises a drawing as `.odp` (presentations) or `.odg` (drawings).
pub fn save(doc: &Drawing) -> Result<Vec<u8>> {
    let names: Vec<String> = (0..doc.pages().len()).map(|i| doc.page_name(i)).collect();
    let (w, h) = doc.size();
    let (content, styles, pics) = streams(doc.kind(), doc.pages(), &names, w, h);
    let mime = if doc.kind() == DrawingKind::Presentation { ns::MIME_ODP } else { ns::MIME_ODG };
    let mut package = Package::new(mime);
    package.insert("content.xml", content.into_bytes())?;
    package.insert("styles.xml", styles.into_bytes())?;
    package.insert("meta.xml", doc.meta.to_xml().into_bytes())?;
    pics.insert_into(&mut package, "")?;
    package.write()
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Resolved formatting of a style.
/// How an area or line is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Paint {
    None,
    Solid,
    Dash,
    /// A gradient, drawn with its start colour.
    Gradient(Option<Rgb>),
}

#[derive(Debug, Clone, Default)]
struct Props {
    fill_kind: Option<Paint>,
    fill_color: Option<Rgb>,
    stroke_kind: Option<Paint>,
    stroke_color: Option<Rgb>,
    stroke_width: Option<f32>,
    marker_start: Option<bool>,
    marker_end: Option<bool>,
    valign: Option<VAlign>,
    align: Option<HAlign>,
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    color: Option<Rgb>,
}

impl Props {
    /// Fills the unset fields from `base`.
    fn or(mut self, base: &Props) -> Props {
        macro_rules! inherit {
            ($($f:ident),*) => { $(if self.$f.is_none() { self.$f = base.$f.clone(); })* };
        }
        inherit!(fill_kind, fill_color, stroke_kind, stroke_color, stroke_width, marker_start, marker_end, valign, align, size, bold, italic, underline, color);
        self
    }

    /// The fill, with `solid_default` deciding unspecified fills.
    fn fill(&self, solid_default: bool) -> Option<Rgb> {
        match self.fill_kind {
            Some(Paint::None) => None,
            Some(Paint::Gradient(c)) => c.or(self.fill_color).or(Some(crate::model::DEFAULT_FILL)),
            Some(_) => Some(self.fill_color.unwrap_or(crate::model::DEFAULT_FILL)),
            None if solid_default => Some(self.fill_color.unwrap_or(crate::model::DEFAULT_FILL)),
            None => None,
        }
    }

    /// The outline colour and whether it is dashed.
    fn stroke(&self, solid_default: bool) -> (Option<Rgb>, bool) {
        match self.stroke_kind {
            Some(Paint::None) => (None, false),
            Some(k) => (Some(self.stroke_color.unwrap_or(crate::model::DEFAULT_STROKE)), k == Paint::Dash),
            None if solid_default => (Some(self.stroke_color.unwrap_or(crate::model::DEFAULT_STROKE)), false),
            None => (None, false),
        }
    }
}

struct RawStyle {
    parent: Option<String>,
    props: Props,
}

/// Styles of a document by (family, name).
struct StyleSheet {
    styles: HashMap<(String, String), RawStyle>,
    defaults: HashMap<String, Props>,
    gradients: HashMap<String, Rgb>,
}

fn parse_props(e: &Element, gradients: &HashMap<String, Rgb>, base_size: f32) -> Props {
    let mut p = Props::default();
    if let Some(g) = e.child("style:graphic-properties").or_else(|| e.child("style:drawing-page-properties")) {
        let color = |a: &str| g.attr(a).and_then(Rgb::parse);
        p.fill_kind = g.attr("draw:fill").map(|k| match k {
            "none" => Paint::None,
            "gradient" => Paint::Gradient(g.attr("draw:fill-gradient-name").and_then(|n| gradients.get(n).copied())),
            _ => Paint::Solid,
        });
        p.fill_color = color("draw:fill-color");
        p.stroke_kind = g.attr("draw:stroke").map(|k| match k {
            "none" => Paint::None,
            "dash" => Paint::Dash,
            _ => Paint::Solid,
        });
        p.stroke_color = color("svg:stroke-color");
        if let Some(wd) = g.attr("svg:stroke-width").and_then(parse_length_pt) {
            p.stroke_width = Some(wd as f32);
        }
        if let Some(m) = g.attr("draw:marker-start") {
            p.marker_start = Some(!m.is_empty());
        }
        if let Some(m) = g.attr("draw:marker-end") {
            p.marker_end = Some(!m.is_empty());
        }
        p.valign = match g.attr("draw:textarea-vertical-align") {
            Some("top") => Some(VAlign::Top),
            Some("middle") => Some(VAlign::Middle),
            Some("bottom") => Some(VAlign::Bottom),
            _ => None,
        };
    }
    if let Some(pp) = e.child("style:paragraph-properties") {
        p.align = match pp.attr("fo:text-align") {
            Some("start" | "left") => Some(HAlign::Left),
            Some("center") => Some(HAlign::Center),
            Some("end" | "right") => Some(HAlign::Right),
            Some("justify") => Some(HAlign::Left),
            _ => None,
        };
    }
    if let Some(t) = e.child("style:text-properties") {
        if let Some(sz) = t.attr("fo:font-size") {
            p.size = if let Some(pct) = sz.strip_suffix('%') {
                pct.trim().parse::<f32>().ok().map(|v| base_size * v / 100.0)
            } else {
                parse_length_pt(sz).map(|v| v as f32)
            };
        }
        if let Some(wt) = t.attr("fo:font-weight") {
            p.bold = Some(wt == "bold" || wt.parse::<u32>().is_ok_and(|n| n >= 600));
        }
        if let Some(st) = t.attr("fo:font-style") {
            p.italic = Some(st == "italic" || st == "oblique");
        }
        if let Some(u) = t.attr("style:text-underline-style") {
            p.underline = Some(u != "none");
        }
        if let Some(c) = t.attr("fo:color").and_then(Rgb::parse) {
            p.color = Some(c);
        }
    }
    p
}

impl StyleSheet {
    fn new() -> StyleSheet {
        StyleSheet { styles: HashMap::new(), defaults: HashMap::new(), gradients: HashMap::new() }
    }

    fn collect(&mut self, container: Option<&Element>) {
        let Some(container) = container else { return };
        for g in container.children_named("draw:gradient") {
            if let (Some(name), Some(c)) = (g.attr("draw:name"), g.attr("draw:start-color").and_then(Rgb::parse)) {
                self.gradients.insert(name.to_owned(), c);
            }
        }
        for e in container.elements() {
            match e.name.as_str() {
                "style:default-style" => {
                    let family = e.attr("style:family").unwrap_or("").to_owned();
                    let props = parse_props(e, &self.gradients, 18.0);
                    self.defaults.insert(family, props);
                }
                "style:style" => {
                    let (Some(name), Some(family)) = (e.attr("style:name"), e.attr("style:family")) else { continue };
                    let props = parse_props(e, &self.gradients, 18.0);
                    self.styles.insert(
                        (family.to_owned(), name.to_owned()),
                        RawStyle { parent: e.attr("style:parent-style-name").map(str::to_owned), props },
                    );
                }
                _ => {}
            }
        }
    }

    /// Resolves a style with its parents and the family default.
    fn resolve(&self, family: &str, name: Option<&str>) -> Props {
        let mut out = Props::default();
        let mut cur = name.map(str::to_owned);
        let mut guard = 0;
        while let Some(n) = cur {
            guard += 1;
            if guard > 32 {
                break;
            }
            match self.styles.get(&(family.to_owned(), n)) {
                Some(s) => {
                    out = out.or(&s.props);
                    cur = s.parent.clone();
                }
                None => break,
            }
        }
        match self.defaults.get(family) {
            Some(d) => out.or(d),
            None => out,
        }
    }
}

struct Reader<'a> {
    sheet: StyleSheet,
    package: &'a Package,
    prefix: String,
    limits: &'a Limits,
    count: usize,
}

/// Text of a shape: paragraphs joined by newlines, plus the first span's
/// style name and whether the text was a list.
fn shape_text(e: &Element) -> (String, Option<String>, Option<String>, bool) {
    let mut lines = Vec::new();
    let mut para_style = None;
    let mut span_style = None;
    let mut list = false;
    // Iterative walk over paragraphs (lists can nest).
    let mut stack: Vec<&Element> = e.elements().collect();
    stack.reverse();
    let mut guard = 0;
    while let Some(el) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        match el.name.as_str() {
            "text:p" | "text:h" => {
                if para_style.is_none() {
                    para_style = el.attr("text:style-name").map(str::to_owned);
                }
                let mut line = String::new();
                inline_text(el, &mut line, &mut span_style, 0);
                lines.push(line);
            }
            "text:list" => {
                list = true;
                let mut kids: Vec<&Element> = el.elements().collect();
                kids.reverse();
                stack.extend(kids);
            }
            "text:list-item" | "text:list-header" | "text:section" => {
                let mut kids: Vec<&Element> = el.elements().collect();
                kids.reverse();
                stack.extend(kids);
            }
            _ => {}
        }
    }
    (lines.join("\n"), para_style, span_style, list)
}

fn inline_text(e: &Element, out: &mut String, span_style: &mut Option<String>, depth: usize) {
    if depth > 32 {
        return;
    }
    for node in &e.children {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Element(c) => match c.name.as_str() {
                "text:s" => {
                    let n = c.attr("text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(1000);
                    out.extend(std::iter::repeat_n(' ', n));
                }
                "text:tab" => out.push('\t'),
                "text:line-break" => out.push(' '),
                "text:span" => {
                    if span_style.is_none() && !c.text_content().trim().is_empty() {
                        *span_style = c.attr("text:style-name").map(str::to_owned);
                    }
                    inline_text(c, out, span_style, depth + 1);
                }
                "text:a" | "text:sequence" | "text:date" | "text:time" | "text:page-number" => inline_text(c, out, span_style, depth + 1),
                _ => {}
            },
        }
    }
}

fn length(e: &Element, attr: &str) -> Option<f32> {
    e.attr(attr).and_then(parse_length_pt).map(|v| v as f32).filter(|v| v.is_finite())
}

/// Parses `rotate (a) translate (x y)` (other parts ignored).
fn parse_transform(t: &str) -> (f32, Option<(f32, f32)>) {
    let mut angle = 0.0;
    let mut translate = None;
    let mut rest = t;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let Some(close) = rest[open..].find(')') else { break };
        let args: Vec<&str> = rest[open + 1..open + close].split([' ', ',']).filter(|s| !s.is_empty()).collect();
        match name {
            "rotate" => angle = args.first().and_then(|a| a.parse::<f32>().ok()).unwrap_or(0.0),
            "translate" => {
                let x = args.first().and_then(|a| parse_length_pt(a)).map(|v| v as f32);
                let y = args.get(1).and_then(|a| parse_length_pt(a)).map(|v| v as f32);
                if let (Some(x), Some(y)) = (x, y) {
                    translate = Some((x, y));
                }
            }
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    (angle, translate)
}

/// Flattens an SVG path into points (curves sampled), returning the
/// points and whether the path was closed.
fn flatten_path(d: &str) -> (Vec<[f32; 2]>, bool) {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            tokens.push(c.to_string());
        } else if c == ',' || c.is_whitespace() || (c == '-' && !cur.is_empty() && !cur.ends_with(['e', 'E'])) {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            if c == '-' {
                cur.push(c);
            }
        } else {
            cur.push(c);
        }
        if tokens.len() > 200_000 {
            break;
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    let mut pts: Vec<[f32; 2]> = Vec::new();
    let mut closed = false;
    let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut cmd = 'M';
    let mut i = 0;
    let num = |i: &mut usize| -> Option<f32> {
        let v = tokens.get(*i)?.parse::<f32>().ok()?;
        *i += 1;
        Some(v)
    };
    while i < tokens.len() && pts.len() < crate::model::MAX_POINTS {
        if let Some(c) = tokens[i].chars().next().filter(|c| c.is_ascii_alphabetic()) {
            cmd = c;
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                closed = true;
                x = sx;
                y = sy;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        let before = i;
        match cmd.to_ascii_uppercase() {
            'M' | 'L' | 'T' => {
                let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else { break };
                x = bx + a;
                y = by + b;
                if cmd.eq_ignore_ascii_case(&'M') {
                    sx = x;
                    sy = y;
                    cmd = if rel { 'l' } else { 'L' };
                }
                pts.push([x, y]);
            }
            'H' => {
                let Some(a) = num(&mut i) else { break };
                x = if rel { x + a } else { a };
                pts.push([x, y]);
            }
            'V' => {
                let Some(a) = num(&mut i) else { break };
                y = if rel { y + a } else { a };
                pts.push([x, y]);
            }
            'C' | 'S' | 'Q' => {
                let n = if cmd.eq_ignore_ascii_case(&'C') { 6 } else { 4 };
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    match num(&mut i) {
                        Some(a) => v.push(a),
                        None => break,
                    }
                }
                if v.len() < n {
                    break;
                }
                let (x0, y0) = (x, y);
                let (ex, ey) = (bx + v[n - 2], by + v[n - 1]);
                let (c1x, c1y) = (bx + v[0], by + v[1]);
                for k in 1..=8 {
                    let t = k as f32 / 8.0;
                    let u = 1.0 - t;
                    let (px, py) = if n == 6 {
                        let (c2x, c2y) = (bx + v[2], by + v[3]);
                        (
                            u * u * u * x0 + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * ex,
                            u * u * u * y0 + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * ey,
                        )
                    } else {
                        (u * u * x0 + 2.0 * u * t * c1x + t * t * ex, u * u * y0 + 2.0 * u * t * c1y + t * t * ey)
                    };
                    pts.push([px, py]);
                }
                x = ex;
                y = ey;
            }
            'A' => {
                let mut v = [0.0f32; 7];
                for slot in &mut v {
                    match num(&mut i) {
                        Some(a) => *slot = a,
                        None => break,
                    }
                }
                x = bx + v[5];
                y = by + v[6];
                pts.push([x, y]);
            }
            _ => i += 1,
        }
        if i == before {
            i += 1;
        }
    }
    (pts, closed)
}

impl Reader<'_> {
    fn props(&self, e: &Element) -> Props {
        let graphic = self.sheet.resolve("graphic", e.attr("draw:style-name"));
        match e.attr("presentation:style-name") {
            Some(p) => graphic.or(&self.sheet.resolve("presentation", Some(p))),
            None => graphic,
        }
    }

    fn image(&self, href: &str) -> Option<Image> {
        let path = href.trim_start_matches("./");
        if path.contains("://") || path.starts_with('/') {
            return None;
        }
        let bytes = self.package.get(&format!("{}{}", self.prefix, path))?;
        Image::from_bytes(bytes.to_vec()).ok()
    }

    fn base_shape(&self, kind: ShapeKind, e: &Element) -> Option<Shape> {
        let (w, h) = (length(e, "svg:width").unwrap_or(0.0), length(e, "svg:height").unwrap_or(0.0));
        let (mut x, mut y) = (length(e, "svg:x").unwrap_or(0.0), length(e, "svg:y").unwrap_or(0.0));
        let mut rotation = 0.0;
        if let Some(t) = e.attr("draw:transform") {
            let (a, tr) = parse_transform(t);
            if let Some((tx, ty)) = tr {
                let r = -a; // clockwise radians
                let (cx, cy) = (tx + (w / 2.0) * r.cos() - (h / 2.0) * r.sin(), ty + (w / 2.0) * r.sin() + (h / 2.0) * r.cos());
                x = cx - w / 2.0;
                y = cy - h / 2.0;
                rotation = r.to_degrees();
            }
        }
        let mut s = Shape::new(kind, x, y, w.max(1.0), h.max(1.0));
        s.rotation = rotation;
        let p = self.props(e);
        let (text, para, span, list) = shape_text(match e.child("draw:text-box") {
            Some(tb) => tb,
            None => e,
        });
        let para_p = self.sheet.resolve("paragraph", para.as_deref());
        let span_p = self.sheet.resolve("text", span.as_deref());
        let t = span_p.or(&para_p).or(&p);
        let shape_like = !matches!(s.kind, ShapeKind::TextBox | ShapeKind::Image { .. } | ShapeKind::Diagram { .. });
        s.fill = p.fill(shape_like);
        let (stroke, dash) = p.stroke(shape_like);
        s.stroke = stroke;
        s.dash = dash;
        s.stroke_width = p.stroke_width.unwrap_or(0.0).max(if s.stroke.is_some() { 0.75 } else { 0.0 });
        s.arrow_start = p.marker_start.unwrap_or(false);
        s.arrow_end = p.marker_end.unwrap_or(false);
        s.text = text;
        let default_align = if matches!(s.kind, ShapeKind::TextBox) { HAlign::Left } else { HAlign::Center };
        let default_valign = if matches!(s.kind, ShapeKind::TextBox) { VAlign::Top } else { VAlign::Middle };
        s.style = TextStyle {
            size: t.size.unwrap_or(18.0),
            bold: t.bold.unwrap_or(false),
            italic: t.italic.unwrap_or(false),
            underline: t.underline.unwrap_or(false),
            color: t.color.unwrap_or(Rgb(0, 0, 0)),
            align: t.align.unwrap_or(default_align),
            valign: p.valign.unwrap_or(default_valign),
            bullets: list,
        };
        Some(s)
    }

    fn shapes(&mut self, parent: &Element, out: &mut Vec<Shape>, depth: usize) -> Result<()> {
        for e in parent.elements() {
            self.shape_element(e, out, depth)?;
        }
        Ok(())
    }

    fn shape_element(&mut self, e: &Element, out: &mut Vec<Shape>, depth: usize) -> Result<()> {
        if depth > 32 {
            return Ok(());
        }
        {
            if self.count >= self.limits.max_blocks {
                return Err(OdfError::limit("document has too many shapes"));
            }
            let shape = match e.name.as_str() {
                "draw:g" => {
                    return self.shapes(e, out, depth + 1);
                }
                "draw:rect" => {
                    let round = length(e, "draw:corner-radius").is_some_and(|r| r > 0.0);
                    self.base_shape(if round { ShapeKind::RoundRect } else { ShapeKind::Rect }, e)
                }
                "draw:ellipse" | "draw:circle" => self.base_shape(ShapeKind::Ellipse, e),
                "draw:line" | "draw:connector" | "draw:measure" => {
                    let (x1, y1) = (length(e, "svg:x1").unwrap_or(0.0), length(e, "svg:y1").unwrap_or(0.0));
                    let (x2, y2) = (length(e, "svg:x2").unwrap_or(0.0), length(e, "svg:y2").unwrap_or(0.0));
                    let p = self.props(e);
                    let mut s = Shape::new(ShapeKind::Line, x1, y1, x2 - x1, y2 - y1);
                    let (stroke, dash) = p.stroke(true);
                    s.stroke = stroke;
                    s.stroke_width = p.stroke_width.unwrap_or(0.75).max(0.75);
                    s.dash = dash;
                    s.arrow_start = p.marker_start.unwrap_or(false);
                    s.arrow_end = p.marker_end.unwrap_or(false);
                    Some(s)
                }
                "draw:polygon" | "draw:polyline" | "draw:path" => {
                    let vb: Vec<f32> = e.attr("svg:viewBox").unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    let (raw, closed) = if e.name == "draw:path" {
                        flatten_path(e.attr("svg:d").unwrap_or(""))
                    } else {
                        let pts: Vec<[f32; 2]> = e
                            .attr("draw:points")
                            .unwrap_or("")
                            .split_whitespace()
                            .take(crate::model::MAX_POINTS)
                            .filter_map(|p| p.split_once(',').and_then(|(a, b)| Some([a.parse().ok()?, b.parse().ok()?])))
                            .collect();
                        (pts, e.name == "draw:polygon")
                    };
                    if raw.len() < 2 {
                        return Ok(());
                    }
                    let (ox, oy, vw, vh) = if vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0 { (vb[0], vb[1], vb[2], vb[3]) } else {
                        let (minx, maxx) = raw.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
                        let (miny, maxy) = raw.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
                        (minx, miny, (maxx - minx).max(1.0), (maxy - miny).max(1.0))
                    };
                    let points = raw.iter().map(|p| [((p[0] - ox) / vw).clamp(0.0, 1.0), ((p[1] - oy) / vh).clamp(0.0, 1.0)]).collect();
                    let mut s = self.base_shape(ShapeKind::Polygon { points, closed }, e);
                    if let Some(s) = s.as_mut() {
                        if !closed {
                            s.fill = None;
                        }
                    }
                    s
                }
                "draw:custom-shape" => {
                    let kind = match e.child("draw:enhanced-geometry").and_then(|g| g.attr("draw:type")).unwrap_or("rectangle") {
                        "round-rectangle" | "ooxml-roundRect" | "flowchart-alternate-process" | "round-quadrat" => ShapeKind::RoundRect,
                        "ellipse" | "circle" | "ooxml-ellipse" | "flowchart-connector" => ShapeKind::Ellipse,
                        "isosceles-triangle" | "ooxml-triangle" | "flowchart-extract" => ShapeKind::Triangle,
                        "diamond" | "ooxml-diamond" | "flowchart-decision" => ShapeKind::Diamond,
                        _ => ShapeKind::Rect,
                    };
                    self.base_shape(kind, e)
                }
                "draw:frame" => self.frame(e),
                _ => return Ok(()),
            };
            if let Some(mut s) = shape {
                s.normalize();
                out.push(s);
                self.count += 1;
            }
        }
        Ok(())
    }

    fn frame(&self, e: &Element) -> Option<Shape> {
        let desc = e.child("svg:desc").map(Element::text_content);
        let is_diagram = e.attr("draw:name").is_some_and(|n| n.starts_with(DIAGRAM_NAME))
            || e.child("svg:title").is_some_and(|t| t.text_content() == DIAGRAM_NAME);
        if let (true, Some(source)) = (is_diagram, desc) {
            return self.base_shape(ShapeKind::Diagram { source }, e);
        }
        for img in e.children_named("draw:image") {
            let image = match img.attr("xlink:href") {
                Some(href) if !href.is_empty() => self.image(href),
                _ => img
                    .child("office:binary-data")
                    .and_then(|b| base64_decode(&b.text_content()).ok())
                    .and_then(|bytes| Image::from_bytes(bytes).ok()),
            };
            if let Some(image) = image {
                return self.base_shape(ShapeKind::Image { image }, e);
            }
        }
        if e.child("draw:text-box").is_some() {
            let mut s = self.base_shape(ShapeKind::TextBox, e)?;
            s.placeholder = match e.attr("presentation:class") {
                Some("title") => Some(Placeholder::Title),
                Some("subtitle") => Some(Placeholder::Subtitle),
                Some("outline") => Some(Placeholder::Outline),
                _ => None,
            };
            if e.attr("presentation:placeholder") == Some("true") && s.text.trim().is_empty() {
                s.text.clear();
            }
            return Some(s);
        }
        None
    }
}

/// Reads shapes embedded in another kind of document (drawings anchored
/// in Writer text), with that document's graphic styles.
pub struct ShapeReader<'a> {
    reader: Reader<'a>,
}

impl<'a> ShapeReader<'a> {
    /// A reader using the styles of `docs` (the `content.xml` and
    /// `styles.xml` roots) and pictures from `package`.
    pub fn new(package: &'a Package, docs: &[&Element], limits: &'a Limits) -> ShapeReader<'a> {
        let mut sheet = StyleSheet::new();
        for d in docs {
            sheet.collect(d.child("office:styles"));
            sheet.collect(d.child("office:automatic-styles"));
        }
        ShapeReader { reader: Reader { sheet, package, prefix: String::new(), limits, count: 0 } }
    }

    /// Reads the given elements as shapes (groups are flattened; unknown
    /// elements are skipped).
    pub fn shapes(&mut self, elements: &[&Element]) -> Result<Vec<Shape>> {
        let mut out = Vec::new();
        for e in elements {
            self.reader.shape_element(e, &mut out, 0)?;
        }
        Ok(out)
    }
}

/// Whether an element is a drawing shape (or group) that
/// [`ShapeReader::shapes`] understands.
pub fn is_shape_element(name: &str) -> bool {
    matches!(
        name,
        "draw:g" | "draw:rect" | "draw:ellipse" | "draw:circle" | "draw:line" | "draw:connector" | "draw:polygon" | "draw:polyline" | "draw:path" | "draw:custom-shape"
    )
}

/// Loads drawing pages from a package; `prefix` selects an embedded
/// document (`"Object 1/"`), `""` the package itself. Returns the kind,
/// page size, pages with names, and metadata.
pub fn load_package(package: &Package, prefix: &str, limits: &Limits) -> Result<(DrawingKind, f32, f32, Vec<Page>, Metadata)> {
    let content = package.require_xml(&format!("{prefix}content.xml"), limits)?;
    let styles_doc = package.get_xml(&format!("{prefix}styles.xml"), limits)?;
    let mut sheet = StyleSheet::new();
    if let Some(sd) = &styles_doc {
        sheet.collect(sd.child("office:styles"));
        sheet.collect(sd.child("office:automatic-styles"));
    }
    sheet.collect(content.child("office:styles"));
    sheet.collect(content.child("office:automatic-styles"));
    let body = content.child("office:body").ok_or_else(|| OdfError::invalid("document has no body"))?;
    let (kind, root) = if let Some(p) = body.child("office:presentation") {
        (DrawingKind::Presentation, p)
    } else if let Some(d) = body.child("office:drawing") {
        (DrawingKind::Drawing, d)
    } else {
        return Err(OdfError::invalid("not a presentation or drawing"));
    };

    // Page size and master pages (background, decorations).
    let (mut width, mut height) = match kind {
        DrawingKind::Presentation => crate::SLIDE_SIZE,
        DrawingKind::Drawing => crate::DRAWING_SIZE,
    };
    let mut masters: HashMap<String, (Option<Rgb>, Vec<Shape>)> = HashMap::new();
    let mut reader = Reader { sheet, package, prefix: prefix.to_owned(), limits, count: 0 };
    if let Some(sd) = &styles_doc {
        let layouts: Vec<&Element> = sd.child("office:automatic-styles").into_iter().flat_map(|s| s.children_named("style:page-layout")).collect();
        let mut first_layout = None;
        if let Some(ms) = sd.child("office:master-styles") {
            for m in ms.children_named("style:master-page") {
                let name = m.attr("style:name").unwrap_or("").to_owned();
                if first_layout.is_none() {
                    first_layout = m.attr("style:page-layout-name").map(str::to_owned);
                }
                let bg = reader.sheet.resolve("drawing-page", m.attr("draw:style-name")).fill(false);
                let mut shapes = Vec::new();
                reader.shapes(m, &mut shapes, 0)?;
                // Placeholders on masters are templates, not content.
                shapes.retain(|s| s.placeholder.is_none() && !(matches!(s.kind, ShapeKind::TextBox) && s.text.is_empty()));
                masters.insert(name, (bg, shapes));
            }
        }
        let layout = layouts.iter().find(|l| first_layout.is_some() && l.attr("style:name") == first_layout.as_deref()).or(layouts.first());
        if let Some(props) = layout.and_then(|l| l.child("style:page-layout-properties")) {
            if let (Some(w), Some(h)) = (length(props, "fo:page-width"), length(props, "fo:page-height")) {
                if (72.0..=14_400.0).contains(&w) && (72.0..=14_400.0).contains(&h) {
                    width = w;
                    height = h;
                }
            }
        }
    }

    let mut pages = Vec::new();
    for p in root.children_named("draw:page") {
        if pages.len() >= crate::MAX_PAGES {
            return Err(OdfError::limit("document has too many pages"));
        }
        let master = p.attr("draw:master-page-name").and_then(|m| masters.get(m));
        let own = reader.sheet.resolve("drawing-page", p.attr("draw:style-name"));
        let background = match own.fill_kind {
            Some(_) => own.fill(false),
            None => master.and_then(|m| m.0),
        }
        .filter(|c| *c != Rgb(255, 255, 255));
        let mut shapes: Vec<Shape> = master.map(|m| m.1.clone()).unwrap_or_default();
        reader.shapes(p, &mut shapes, 0)?;
        let notes = p
            .child("presentation:notes")
            .and_then(|n| n.elements().find(|f| f.name == "draw:frame" && f.attr("presentation:class") == Some("notes")))
            .map(|f| shape_text(f.child("draw:text-box").unwrap_or(f)).0)
            .unwrap_or_default();
        let name = p.attr("draw:name").unwrap_or("").to_owned();
        pages.push(Page { name, shapes, background, notes });
    }
    let meta = match package.get_xml(&format!("{prefix}meta.xml"), limits)? {
        Some(m) => Metadata::from_xml(&m),
        None => Metadata::default(),
    };
    Ok((kind, width, height, pages, meta))
}

/// Loads an `.odp` / `.odg` file (and their templates).
pub fn load(bytes: &[u8], limits: &Limits) -> Result<Drawing> {
    let package = Package::read(bytes, limits)?;
    package.expect_mimetype(&[ns::MIME_ODP, ns::MIME_ODG])?;
    let (kind, width, height, mut pages, meta) = load_package(&package, "", limits)?;
    // Default names ("page1", "Slide 1") are not kept, so new pages number
    // themselves consistently.
    for (i, p) in pages.iter_mut().enumerate() {
        let n = p.name.to_ascii_lowercase();
        if n == format!("page{}", i + 1) || n == format!("slide {}", i + 1) || n == format!("page {}", i + 1) {
            p.name.clear();
        }
    }
    let mut doc = Drawing::from_pages(kind, width, height, pages);
    doc.meta = meta;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_round_trip() {
        let mut s = Shape::new(ShapeKind::Rect, 100.0, 50.0, 80.0, 40.0);
        s.rotation = 30.0;
        let mut w = XmlWriter::fragment();
        w.start("x");
        geometry(&mut w, &s);
        w.end();
        let xml = w.finish();
        let t = xml.split("draw:transform=\"").nth(1).and_then(|r| r.split('"').next()).expect("transform");
        let (a, tr) = parse_transform(t);
        let (tx, ty) = tr.expect("translate");
        let r = -a;
        let cx = tx + 40.0 * r.cos() - 20.0 * r.sin();
        let cy = ty + 40.0 * r.sin() + 20.0 * r.cos();
        assert!((cx - 140.0).abs() < 0.05 && (cy - 70.0).abs() < 0.05, "{cx} {cy}");
        assert!((r.to_degrees() - 30.0).abs() < 0.01);
    }

    #[test]
    fn paths_flatten() {
        let (pts, closed) = flatten_path("M0 0 L10 0 l0 10 h-10 z");
        assert_eq!(pts, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        assert!(closed);
        let (pts, _) = flatten_path("M0,0 C 0,10 10,10 10,0");
        assert_eq!(pts.len(), 9);
        let (pts, _) = flatten_path("M1-2-3-4");
        assert_eq!(pts, vec![[1.0, -2.0], [-3.0, -4.0]]);
    }
}
