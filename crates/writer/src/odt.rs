//! OpenDocument Text (`.odt`) import and export.
//!
//! Supported: paragraphs, headings, bulleted and numbered lists (nested),
//! tables (nested tables are flattened to text; fixed column widths, exact
//! row heights and borderless cells are kept), page breaks, character
//! attributes (bold, italic, underline, strike-through, monospace, super- and
//! subscript, colour, highlight, size), links, alignment, left indent and
//! space above paragraphs, page number and page count fields, metadata, page
//! size and margin, and the page header and footer of the default master
//! page. Pictures, Mermaid diagrams (SVG pictures with their source in the
//! description) and drawings (embedded OpenDocument Graphics objects, or
//! shapes anchored in a paragraph) are kept as blocks of their own; text
//! frames contribute their paragraphs. Other fields, footnotes, comments
//! and tracked changes are not modelled; their text content is kept where
//! it is part of the flow.

use crate::document::Document;
use crate::model::*;
use office_odf::util::{format_length_in, parse_length_pt};
use office_odf::xml::{Element, Node, XmlWriter};
use office_odf::{ns, Limits, Metadata, OdfError, Package, Result, Rgb};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct StyleInfo {
    parent: Option<String>,
    attrs: TextAttrsPatchLite,
    align: Option<Align>,
    heading_level: Option<u8>,
    break_before: bool,
    list_style: Option<String>,
    monospace_font: bool,
    indent: Option<f32>,
    space_before: Option<f32>,
    /// Table column width / exact row height (column and row styles).
    width: Option<f32>,
    row_height: Option<f32>,
    /// Cell style without borders.
    no_border: bool,
    /// Automatic styles are per-document formatting; common (named) styles
    /// define what a paragraph *is* (heading, quote...).
    automatic: bool,
}

/// Text attributes as optionals, so styles can inherit.
#[derive(Debug, Clone, Default)]
struct TextAttrsPatchLite {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    code: Option<bool>,
    superscript: Option<bool>,
    subscript: Option<bool>,
    color: Option<Rgb>,
    highlight: Option<Rgb>,
    size: Option<f32>,
}

impl TextAttrsPatchLite {
    fn apply(&self, a: &mut TextAttrs) {
        if let Some(v) = self.bold {
            a.bold = v;
        }
        if let Some(v) = self.italic {
            a.italic = v;
        }
        if let Some(v) = self.underline {
            a.underline = v;
        }
        if let Some(v) = self.strike {
            a.strike = v;
        }
        if let Some(v) = self.code {
            a.code = v;
        }
        if let Some(v) = self.superscript {
            a.superscript = v;
        }
        if let Some(v) = self.subscript {
            a.subscript = v;
        }
        if self.color.is_some() {
            a.color = self.color;
        }
        if self.highlight.is_some() {
            a.highlight = self.highlight;
        }
        if self.size.is_some() {
            a.size = self.size;
        }
    }
}

const MONO_FONTS: &[&str] = &["courier", "mono", "consolas", "menlo", "liberation mono", "dejavu sans mono"];

fn parse_style(e: &Element, font_faces: &HashMap<String, String>) -> StyleInfo {
    let mut info = StyleInfo {
        parent: e.attr("style:parent-style-name").map(str::to_owned),
        list_style: e.attr("style:list-style-name").map(str::to_owned),
        heading_level: e.attr("style:default-outline-level").and_then(|v| v.parse().ok()),
        ..Default::default()
    };
    if let Some(t) = e.child("style:text-properties") {
        let a = &mut info.attrs;
        match t.attr("fo:font-weight") {
            Some("bold") | Some("600") | Some("700") | Some("800") | Some("900") => a.bold = Some(true),
            Some(_) => a.bold = Some(false),
            None => {}
        }
        if let Some(v) = t.attr("fo:font-style") {
            a.italic = Some(v == "italic" || v == "oblique");
        }
        if let Some(v) = t.attr("style:text-underline-style") {
            a.underline = Some(v != "none");
        }
        if let Some(v) = t.attr("style:text-line-through-style") {
            a.strike = Some(v != "none");
        }
        if let Some(v) = t.attr("style:text-position") {
            let first = v.split_whitespace().next().unwrap_or("0");
            let raised = first == "super" || first.trim_end_matches('%').parse::<f32>().is_ok_and(|p| p > 0.0);
            let lowered = first == "sub" || first.trim_end_matches('%').parse::<f32>().is_ok_and(|p| p < 0.0);
            a.superscript = Some(raised);
            a.subscript = Some(lowered);
        }
        a.color = t.attr("fo:color").and_then(Rgb::parse);
        a.highlight = t.attr("fo:background-color").and_then(Rgb::parse);
        a.size = t
            .attr("fo:font-size")
            .and_then(parse_length_pt)
            .filter(|s| (4.0..=400.0).contains(s))
            .map(|s| s as f32);
        let font = t
            .attr("style:font-name")
            .and_then(|n| font_faces.get(n).cloned().or_else(|| Some(n.to_owned())))
            .or_else(|| t.attr("fo:font-family").map(str::to_owned));
        if let Some(font) = font {
            let lower = font.to_lowercase();
            info.monospace_font = MONO_FONTS.iter().any(|m| lower.contains(m));
            if info.monospace_font {
                a.code = Some(true);
            }
        }
    }
    if let Some(p) = e.child("style:paragraph-properties") {
        info.align = match p.attr("fo:text-align") {
            Some("center") => Some(Align::Center),
            Some("end" | "right") => Some(Align::Right),
            Some("justify") => Some(Align::Justify),
            Some("start" | "left") => Some(Align::Left),
            _ => None,
        };
        info.break_before = p.attr("fo:break-before") == Some("page");
        let len = |attr: &str| {
            p.attr(attr).and_then(parse_length_pt).map(|v| v as f32).filter(|v| (0.0..=MAX_PARA_SPACING).contains(v))
        };
        info.indent = len("fo:margin-left");
        info.space_before = len("fo:margin-top");
    }
    let side = |v: Option<&str>| v.and_then(parse_length_pt).map(|v| v as f32).filter(|v| (0.0..=MAX_PAGE_SIDE).contains(v));
    if let Some(c) = e.child("style:table-column-properties") {
        info.width = side(c.attr("style:column-width"));
    }
    if let Some(r) = e.child("style:table-row-properties") {
        info.row_height = side(r.attr("style:row-height"));
    }
    if let Some(c) = e.child("style:table-cell-properties") {
        let none = |v: Option<&str>| v.is_none_or(|b| b == "none" || b.starts_with("0pt") || b.starts_with("0in"));
        info.no_border = c.attr("fo:border").map_or_else(
            || {
                ["fo:border-top", "fo:border-bottom", "fo:border-left", "fo:border-right"]
                    .iter()
                    .all(|a| none(c.attr(a)))
            },
            |b| none(Some(b)),
        );
    }
    info
}

struct Importer<'a> {
    styles: HashMap<String, StyleInfo>,
    /// List style name → per-level "is numbered".
    list_styles: HashMap<String, Vec<bool>>,
    limits: &'a Limits,
    blocks: Vec<Block>,
    block_count: usize,
    /// The package (pictures, embedded drawings) and a reader for shapes
    /// anchored in the text.
    package: Option<&'a Package>,
    shapes: Option<office_draw::odg::ShapeReader<'a>>,
    /// Width available for pictures without a size.
    text_width: f32,
}

/// Frames and shapes anchored in a paragraph (inside spans and links too).
fn anchored<'e>(e: &'e Element, out: &mut Vec<&'e Element>, depth: usize) {
    if depth > 16 {
        return;
    }
    for child in e.elements() {
        match child.name.as_str() {
            "draw:frame" => out.push(child),
            n if office_draw::odg::is_shape_element(n) => out.push(child),
            "text:span" | "draw:a" | "text:a" => anchored(child, out, depth + 1),
            _ => {}
        }
    }
}

fn frame_align(e: &Element, para_align: Align) -> Align {
    let _ = e;
    para_align
}

impl Importer<'_> {
    fn collect_styles(&mut self, container: Option<&Element>, font_faces: &HashMap<String, String>, automatic: bool) {
        let Some(container) = container else { return };
        for e in container.elements() {
            match e.name.as_str() {
                "style:style" => {
                    if let Some(name) = e.attr("style:name") {
                        let mut info = parse_style(e, font_faces);
                        info.automatic = automatic;
                        self.styles.insert(name.to_owned(), info);
                    }
                }
                "text:list-style" => {
                    if let Some(name) = e.attr("style:name") {
                        let mut levels = vec![false; 10];
                        for level in e.elements() {
                            let idx = level.attr("text:level").and_then(|l| l.parse::<usize>().ok()).unwrap_or(1);
                            if (1..=10).contains(&idx) {
                                levels[idx - 1] = level.name == "text:list-level-style-number";
                            }
                        }
                        self.list_styles.insert(name.to_owned(), levels);
                    }
                }
                _ => {}
            }
        }
    }

    /// Resolves a style with its parents (bounded to avoid loops). With
    /// `automatic_only`, the chain stops at the first common style.
    fn resolved_with(&self, name: Option<&str>, automatic_only: bool) -> StyleInfo {
        let mut chain = Vec::new();
        let mut names = Vec::new();
        let mut current = name.map(str::to_owned);
        while let Some(n) = current {
            if chain.len() >= 16 {
                break;
            }
            let Some(style) = self.styles.get(&n) else {
                names.push(n);
                break;
            };
            if automatic_only && !style.automatic {
                break;
            }
            chain.push(style.clone());
            names.push(n);
            current = style.parent.clone();
        }
        let mut out = StyleInfo::default();
        for style in chain.iter().rev() {
            let a = &style.attrs;
            let o = &mut out.attrs;
            o.bold = a.bold.or(o.bold);
            o.italic = a.italic.or(o.italic);
            o.underline = a.underline.or(o.underline);
            o.strike = a.strike.or(o.strike);
            o.code = a.code.or(o.code);
            o.superscript = a.superscript.or(o.superscript);
            o.subscript = a.subscript.or(o.subscript);
            o.color = a.color.or(o.color);
            o.highlight = a.highlight.or(o.highlight);
            o.size = a.size.or(o.size);
            out.align = style.align.or(out.align);
            out.heading_level = style.heading_level.or(out.heading_level);
            out.list_style = style.list_style.clone().or(out.list_style.take());
            out.break_before |= style.break_before;
            out.indent = style.indent.or(out.indent);
            out.space_before = style.space_before.or(out.space_before);
        }
        // Recognise the standard heading style names anywhere in the chain,
        // whatever their properties (innermost wins).
        if !automatic_only && out.heading_level.is_none() {
            out.heading_level = names.iter().find_map(|n| {
                n.replace("_20_", " ").to_lowercase().strip_prefix("heading ").and_then(|l| l.parse::<u8>().ok())
            });
        }
        out
    }

    fn resolved(&self, name: Option<&str>) -> StyleInfo {
        self.resolved_with(name, false)
    }

    fn style_kind(&self, name: Option<&str>) -> Option<ParaKind> {
        let mut current = name.map(str::to_owned);
        for _ in 0..16 {
            let n = current?;
            let display = n.replace("_20_", " ").to_lowercase();
            match display.as_str() {
                "title" => return Some(ParaKind::Title),
                "quotations" | "quote" | "block quote" => return Some(ParaKind::Quote),
                "preformatted text" | "source text" | "code" => return Some(ParaKind::Code),
                _ => {}
            }
            current = self.styles.get(&n).and_then(|s| s.parent.clone());
        }
        None
    }

    fn count_block(&mut self) -> Result<()> {
        self.block_count += 1;
        if self.block_count > self.limits.max_blocks {
            return Err(OdfError::limit(format!("document has more than {} paragraphs", self.limits.max_blocks)));
        }
        Ok(())
    }

    /// Collects the runs of a paragraph-like element.
    fn inline(&self, e: &Element, attrs: &TextAttrs, runs: &mut Vec<Run>, depth: usize) {
        if depth > 64 {
            return;
        }
        for child in &e.children {
            match child {
                Node::Text(t) => push_text(runs, t, attrs),
                Node::Element(el) => match el.name.as_str() {
                    "text:span" => {
                        let mut a = attrs.clone();
                        self.resolved(el.attr("text:style-name")).attrs.apply(&mut a);
                        self.inline(el, &a, runs, depth + 1);
                    }
                    "text:a" => {
                        let mut a = attrs.clone();
                        a.link = el.attr("xlink:href").and_then(sanitize_link);
                        self.resolved(el.attr("text:style-name")).attrs.apply(&mut a);
                        self.inline(el, &a, runs, depth + 1);
                    }
                    "text:s" => {
                        let n = el.attr("text:c").and_then(|c| c.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1024);
                        push_text(runs, &" ".repeat(n), attrs);
                    }
                    "text:tab" => push_text(runs, "\t", attrs),
                    "text:page-number" | "text:page-count" => {
                        let field = if el.name == "text:page-number" { Field::PageNumber } else { Field::PageCount };
                        let a = TextAttrs { field: Some(field), ..attrs.clone() };
                        let mut inner = Vec::new();
                        self.inline(el, &a, &mut inner, depth + 1);
                        let text: String = inner.iter().map(|r| r.text.as_str()).collect();
                        let text = if text.trim().is_empty() { "1".to_owned() } else { text.chars().take(16).collect() };
                        // Never merged with a neighbouring field.
                        runs.push(Run { text, attrs: a });
                    }
                    "text:line-break" => push_text(runs, "\n", attrs),
                    // Annotations, notes' citations bodies and change-tracking
                    // metadata are not part of the visible text flow.
                    "office:annotation" | "office:annotation-end" | "text:note" | "text:tracked-changes"
                    | "text:bookmark" | "text:bookmark-start" | "text:bookmark-end" | "text:reference-mark"
                    | "text:soft-page-break" | "draw:frame" | "draw:a" => {}
                    // Fields and other inline containers: keep their text.
                    _ => self.inline(el, attrs, runs, depth + 1),
                },
            }
        }
    }

    fn paragraph(&mut self, e: &Element, list: Option<(bool, u8)>) -> Result<Paragraph> {
        self.count_block()?;
        let style_name = e.attr("text:style-name");
        let style = self.resolved(style_name);
        // Run formatting and alignment come from the automatic part of the
        // style chain only: the look of headings, titles, quotes etc. is
        // implied by the paragraph kind.
        let own = self.resolved_with(style_name, true);
        let mut attrs = TextAttrs::default();
        own.attrs.apply(&mut attrs);
        let mut runs = Vec::new();
        self.inline(e, &attrs, &mut runs, 0);
        let kind = if e.name == "text:h" {
            let level = e
                .attr("text:outline-level")
                .and_then(|l| l.parse::<u8>().ok())
                .or(style.heading_level)
                .unwrap_or(1);
            ParaKind::Heading { level }
        } else if let Some((numbered, level)) = list {
            if numbered { ParaKind::Numbered { level } } else { ParaKind::Bullet { level } }
        } else if let Some(level) = style.heading_level {
            ParaKind::Heading { level }
        } else {
            self.style_kind(style_name).unwrap_or(if own.attrs.code == Some(true) && own.monospace_font {
                ParaKind::Code
            } else {
                ParaKind::Normal
            })
        };
        // A paragraph-level monospace style already says "code block"; do
        // not repeat it on every run.
        if kind == ParaKind::Code {
            for run in &mut runs {
                run.attrs.code = false;
            }
        }
        let is_list = matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. });
        let mut p = Paragraph {
            kind: kind.normalized(),
            align: own.align.unwrap_or_default(),
            indent: if is_list { 0.0 } else { own.indent.unwrap_or(0.0) },
            space_before: own.space_before.unwrap_or(0.0),
            runs,
        };
        p.normalize();
        if style.break_before && !self.blocks.is_empty() {
            self.blocks.push(Block::PageBreak);
        }
        Ok(p)
    }

    /// A top-level paragraph; an empty body paragraph that only carries a
    /// page break becomes just the break (this is how breaks are saved).
    fn top_paragraph(&mut self, e: &Element, depth: usize) -> Result<()> {
        let p = self.paragraph(e, None)?;
        let mut frames = Vec::new();
        anchored(e, &mut frames, 0);
        let only_break = p.kind == ParaKind::Normal
            && p.text().is_empty()
            && matches!(self.blocks.last(), Some(Block::PageBreak))
            && self.resolved(e.attr("text:style-name")).break_before;
        // A paragraph that only holds pictures becomes just the pictures.
        let only_frames = !frames.is_empty() && p.kind == ParaKind::Normal && p.text().trim().is_empty();
        let align = p.align;
        if !only_break && !only_frames {
            self.blocks.push(Block::Paragraph(p));
        }
        self.frames(&frames, align, depth)
    }

    /// Turns anchored frames and shapes into blocks: pictures, diagrams,
    /// drawings (loose shapes are gathered into one canvas) and the
    /// paragraphs of text frames.
    fn frames(&mut self, frames: &[&Element], align: Align, depth: usize) -> Result<()> {
        let mut loose: Vec<&Element> = Vec::new();
        for f in frames {
            if f.name != "draw:frame" {
                loose.push(f);
                continue;
            }
            if let Some(block) = self.frame_block(f, align)? {
                self.count_block()?;
                self.blocks.push(block);
            } else if let Some(tb) = f.child("draw:text-box") {
                if depth < 8 {
                    self.body(tb, depth + 1)?;
                }
            }
        }
        if loose.is_empty() {
            return Ok(());
        }
        let Some(reader) = self.shapes.as_mut() else { return Ok(()) };
        let mut shapes = reader.shapes(&loose)?;
        if shapes.is_empty() {
            return Ok(());
        }
        // Shapes are positioned relative to their anchor: move them into a
        // canvas of their bounding box.
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for s in &shapes {
            let (x, y, w, h) = s.bounds();
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + w);
            y1 = y1.max(y + h);
        }
        for s in &mut shapes {
            s.x -= x0;
            s.y -= y0;
        }
        let mut canvas = Canvas::new((x1 - x0).max(MIN_FRAME_SIDE), (y1 - y0).max(MIN_FRAME_SIDE));
        canvas.align = align;
        canvas.shapes = shapes;
        canvas.normalize();
        self.count_block()?;
        self.blocks.push(Block::Drawing(canvas));
        Ok(())
    }

    /// A picture, diagram or embedded drawing frame as a block.
    fn frame_block(&mut self, f: &Element, align: Align) -> Result<Option<Block>> {
        let length = |a: &str| f.attr(a).and_then(parse_length_pt).map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0);
        let (width, height) = (length("svg:width"), length("svg:height"));
        let align = frame_align(f, align);
        let Some(package) = self.package else { return Ok(None) };
        let desc = f.child("svg:desc").map(Element::text_content);
        let is_diagram = f.attr("draw:name").is_some_and(|n| n.starts_with(office_draw::odg::DIAGRAM_NAME))
            || f.child("svg:title").is_some_and(|t| t.text_content() == office_draw::odg::DIAGRAM_NAME);
        if let (true, Some(source)) = (is_diagram, desc.clone()) {
            let mut d = Diagram { source, width: width.unwrap_or(300.0), align };
            d.normalize();
            return Ok(Some(Block::Diagram(d)));
        }
        if let Some(obj) = f.child("draw:object") {
            let dir = obj.attr("xlink:href").unwrap_or("").trim_start_matches("./").trim_end_matches('/');
            if !dir.is_empty() && !dir.contains("..") && package.get(&format!("{dir}/content.xml")).is_some() {
                if let Ok((office_draw::DrawingKind::Drawing, w, h, pages, _)) = office_draw::odg::load_package(package, &format!("{dir}/"), self.limits) {
                    let mut canvas = Canvas::new(width.unwrap_or(w), height.unwrap_or(h));
                    // Scale the object's page to the frame.
                    let (kx, ky) = (canvas.width / w, canvas.height / h);
                    canvas.shapes = pages.into_iter().next().map(|p| p.shapes).unwrap_or_default();
                    for s in &mut canvas.shapes {
                        s.x *= kx;
                        s.y *= ky;
                        s.w *= kx;
                        s.h *= ky;
                    }
                    canvas.align = align;
                    canvas.normalize();
                    return Ok(Some(Block::Drawing(canvas)));
                }
            }
        }
        for img in f.children_named("draw:image") {
            let image = match img.attr("xlink:href") {
                Some(href) if !href.contains("://") && !href.starts_with('/') => {
                    package.get(href.trim_start_matches("./")).and_then(|b| office_draw::Image::from_bytes(b.to_vec()).ok())
                }
                Some(_) => None,
                None => img
                    .child("office:binary-data")
                    .and_then(|b| office_draw::image::base64_decode(&b.text_content()).ok())
                    .and_then(|b| office_draw::Image::from_bytes(b).ok()),
            };
            if let Some(image) = image {
                let mut p = Picture::new(image, self.text_width);
                if let (Some(w), Some(h)) = (width, height) {
                    p.width = w;
                    p.height = h;
                }
                p.align = align;
                p.description = desc.clone().or_else(|| f.child("svg:title").map(Element::text_content)).unwrap_or_default();
                p.normalize();
                return Ok(Some(Block::Image(p)));
            }
        }
        Ok(None)
    }

    fn list(&mut self, e: &Element, style: Option<&str>, level: u8, out: &mut Vec<Paragraph>) -> Result<()> {
        let style = e.attr("text:style-name").or(style).map(str::to_owned);
        let numbered = style
            .as_deref()
            .and_then(|s| self.list_styles.get(s))
            .and_then(|levels| levels.get(usize::from(level)).copied())
            .unwrap_or(false);
        for item in e.elements() {
            if item.name != "text:list-item" && item.name != "text:list-header" {
                continue;
            }
            for child in item.elements() {
                match child.name.as_str() {
                    "text:p" | "text:h" => {
                        let list = (item.name == "text:list-item").then_some((numbered, level.min(9)));
                        out.push(self.paragraph(child, list)?);
                    }
                    "text:list" if level < 9 => self.list(child, style.as_deref(), level + 1, out)?,
                    "text:list" => self.list(child, style.as_deref(), level, out)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Paragraphs of a table cell (nested structures are flattened).
    fn cell_paragraphs(&mut self, cell: &Element, out: &mut Vec<Paragraph>, depth: usize) -> Result<()> {
        for child in cell.elements() {
            match child.name.as_str() {
                "text:p" | "text:h" => out.push(self.paragraph(child, None)?),
                "text:list" => self.list(child, None, 0, out)?,
                "table:table" if depth < 8 => {
                    for row in table_rows(child) {
                        for nested in row.elements() {
                            self.cell_paragraphs(nested, out, depth + 1)?;
                        }
                    }
                }
                "text:section" if depth < 8 => self.cell_paragraphs(child, out, depth + 1)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn table(&mut self, e: &Element) -> Result<Table> {
        let mut rows = Vec::new();
        let mut col_widths = Vec::new();
        let mut row_heights = Vec::new();
        let mut borders = true;
        for col in e.elements().flat_map(|c| {
            if c.name == "table:table-columns" || c.name == "table:table-header-columns" || c.name == "table:table-column-group" {
                c.children_named("table:table-column").collect::<Vec<_>>()
            } else if c.name == "table:table-column" {
                vec![c]
            } else {
                Vec::new()
            }
        }) {
            let repeat = col.attr("table:number-columns-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
            let width = self.styles.get(col.attr("table:style-name").unwrap_or("")).and_then(|s| s.width);
            for _ in 0..repeat {
                if col_widths.len() < 64 {
                    col_widths.push(width);
                }
            }
        }
        for row in table_rows(e) {
            let height = self.styles.get(row.attr("table:style-name").unwrap_or("")).and_then(|s| s.row_height);
            let repeat = row.attr("table:number-rows-repeated").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 100);
            let mut cells = Vec::new();
            for cell in row.elements() {
                if cell.name != "table:table-cell" && cell.name != "table:covered-table-cell" {
                    continue;
                }
                let span = cell
                    .attr("table:number-columns-repeated")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                if rows.is_empty() && cells.is_empty() {
                    borders = !self.styles.get(cell.attr("table:style-name").unwrap_or("")).is_some_and(|s| s.no_border);
                }
                let mut paragraphs = Vec::new();
                self.cell_paragraphs(cell, &mut paragraphs, 0)?;
                for _ in 0..span {
                    if cells.len() < 64 {
                        cells.push(paragraphs.clone());
                    }
                }
            }
            for _ in 0..repeat {
                self.count_block()?;
                rows.push(cells.clone());
                row_heights.push(height);
            }
        }
        // Sizes are kept only when every column / row has one.
        let all = |v: Vec<Option<f32>>| -> Vec<f32> { v.iter().copied().collect::<Option<Vec<f32>>>().unwrap_or_default() };
        let mut table = Table { rows, col_widths: all(col_widths), row_heights: all(row_heights), borders };
        table.normalize();
        Ok(table)
    }

    /// Paragraphs of a header or footer (lists and tables flattened). A
    /// hidden header (`style:display="false"`) counts as none.
    fn area(&mut self, e: Option<&Element>) -> Result<Vec<Paragraph>> {
        let Some(e) = e.filter(|e| e.attr("style:display") != Some("false")) else { return Ok(Vec::new()) };
        let saved = std::mem::take(&mut self.blocks);
        let mut out = Vec::new();
        let result = self.cell_paragraphs(e, &mut out, 0);
        self.blocks = saved;
        result?;
        Ok(out)
    }

    fn body(&mut self, parent: &Element, depth: usize) -> Result<()> {
        for child in parent.elements() {
            match child.name.as_str() {
                "text:p" | "text:h" => self.top_paragraph(child, depth)?,
                "draw:frame" | "draw:a" => {
                    let mut frames = Vec::new();
                    if child.name == "draw:frame" {
                        frames.push(child);
                    } else {
                        anchored(child, &mut frames, 0);
                    }
                    self.frames(&frames, Align::Center, depth)?;
                }
                n if office_draw::odg::is_shape_element(n) => self.frames(&[child], Align::Center, depth)?,
                "text:list" => {
                    let mut out = Vec::new();
                    self.list(child, None, 0, &mut out)?;
                    self.blocks.extend(out.into_iter().map(Block::Paragraph));
                }
                "table:table" => {
                    let t = self.table(child)?;
                    self.blocks.push(Block::Table(t));
                }
                "text:section" | "text:index-body" | "text:table-of-content" | "text:alphabetical-index"
                | "text:illustration-index" | "text:bibliography"
                    if depth < 16 =>
                {
                    self.body(child, depth + 1)?
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn table_rows(table: &Element) -> Vec<&Element> {
    let mut out = Vec::new();
    for child in table.elements() {
        match child.name.as_str() {
            "table:table-row" => out.push(child),
            "table:table-header-rows" | "table:table-rows" | "table:table-row-group" => {
                out.extend(child.children_named("table:table-row"));
            }
            _ => {}
        }
    }
    out
}

fn push_text(runs: &mut Vec<Run>, text: &str, attrs: &TextAttrs) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.attrs == *attrs => last.text.push_str(text),
        _ => runs.push(Run { text: text.to_owned(), attrs: attrs.clone() }),
    }
}

/// Loads a document from the bytes of an `.odt` file.
pub fn load_odt(bytes: &[u8], limits: &Limits) -> Result<Document> {
    // Everything but previews and editor settings: pictures and embedded
    // drawings are needed too.
    let package = Package::read_filtered(bytes, limits, |n| {
        !(n.starts_with("Thumbnails/") || n.starts_with("Configurations2/") || n == "settings.xml" || n.starts_with("ObjectReplacements/"))
    })?;
    package.expect_mimetype(&[ns::MIME_ODT])?;
    let content = package.require_xml("content.xml", limits)?;
    let styles_doc = package.get_xml("styles.xml", limits)?;

    let mut font_faces = HashMap::new();
    for doc in [Some(&content), styles_doc.as_ref()].into_iter().flatten() {
        if let Some(decls) = doc.child("office:font-face-decls") {
            for face in decls.children_named("style:font-face") {
                if let (Some(name), Some(family)) = (face.attr("style:name"), face.attr("svg:font-family")) {
                    font_faces.insert(name.to_owned(), family.trim_matches('\'').to_owned());
                }
            }
        }
    }

    let docs: Vec<&Element> = [Some(&content), styles_doc.as_ref()].into_iter().flatten().collect();
    let mut importer = Importer {
        styles: HashMap::new(),
        list_styles: HashMap::new(),
        limits,
        blocks: Vec::new(),
        block_count: 0,
        package: Some(&package),
        shapes: Some(office_draw::odg::ShapeReader::new(&package, &docs, limits)),
        text_width: 480.0,
    };
    let mut page = PageSetup::default();
    let (mut header, mut footer) = (Vec::new(), Vec::new());
    if let Some(styles_doc) = &styles_doc {
        importer.collect_styles(styles_doc.child("office:styles"), &font_faces, false);
        importer.collect_styles(styles_doc.child("office:automatic-styles"), &font_faces, true);
        // The default master page ("Standard", else the first one) gives the
        // page layout and the header and footer.
        let master = styles_doc.child("office:master-styles").and_then(|m| {
            m.children_named("style:master-page")
                .find(|p| p.attr("style:name") == Some("Standard"))
                .or_else(|| m.children_named("style:master-page").next())
        });
        if let Some(master) = master {
            // Header paragraphs use the styles of styles.xml, so read them
            // before content.xml's automatic styles shadow those names.
            header = importer.area(master.child("style:header"))?;
            footer = importer.area(master.child("style:footer"))?;
        }
        let layouts = || styles_doc.child("office:automatic-styles").into_iter().flat_map(|s| s.children_named("style:page-layout"));
        let wanted = master.and_then(|m| m.attr("style:page-layout-name"));
        if let Some(layout) = layouts()
            .find(|l| wanted.is_some() && l.attr("style:name") == wanted)
            .or_else(|| layouts().next())
            .and_then(|l| l.child("style:page-layout-properties"))
        {
            let len = |attr: &str| layout.attr(attr).and_then(parse_length_pt).map(|v| v as f32);
            if let (Some(w), Some(h)) = (len("fo:page-width"), len("fo:page-height")) {
                if (72.0..=14_400.0).contains(&w) && (72.0..=14_400.0).contains(&h) {
                    page.width = w;
                    page.height = h;
                }
            }
            if let Some(m) = len("fo:margin-left").filter(|m| (0.0..=page.width.min(page.height) / 3.0).contains(m)) {
                page.margin = m;
            }
        }
    }
    importer.collect_styles(content.child("office:automatic-styles"), &font_faces, true);
    importer.text_width = (page.width - 2.0 * page.margin).max(72.0);

    let text = content
        .descend(&["office:body", "office:text"])
        .ok_or_else(|| OdfError::invalid("document has no text body"))?;
    importer.body(text, 0)?;

    let mut doc = Document::from_blocks(importer.blocks).with_page(page, header, footer);
    if let Some(meta) = package.get_xml("meta.xml", limits)? {
        doc.meta = Metadata::from_xml(&meta);
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Paragraph formatting carried by automatic paragraph styles.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParaFmt {
    align: Align,
    indent: f32,
    space_before: f32,
}

/// Assigns automatic style names to distinct attribute sets. Styles for
/// content.xml and styles.xml live in separate tables with different name
/// prefixes, since the two files have separate automatic styles.
#[derive(Default)]
struct StyleTable {
    prefix: &'static str,
    text: Vec<TextAttrs>,
    para: Vec<ParaFmt>,
    /// (style name, column width) and (style name, row height).
    cols: Vec<f32>,
    rows: Vec<f32>,
    /// Paragraph style names actually used, so unused ones are not written.
    used_para: Vec<(usize, &'static str)>,
}

fn intern<T: PartialEq + Clone>(list: &mut Vec<T>, key: &T) -> usize {
    match list.iter().position(|a| a == key) {
        Some(i) => i,
        None => {
            list.push(key.clone());
            list.len() - 1
        }
    }
}

impl StyleTable {
    fn with_prefix(prefix: &'static str) -> StyleTable {
        StyleTable { prefix, ..Default::default() }
    }

    fn text_style(&mut self, attrs: &TextAttrs) -> Option<String> {
        let mut key = attrs.clone();
        key.link = None;
        key.field = None;
        if key == TextAttrs::default() {
            return None;
        }
        Some(format!("{}T{}", self.prefix, intern(&mut self.text, &key) + 1))
    }

    fn para_style(&mut self, p: &Paragraph) -> String {
        let base = base_style(p.kind);
        let indent = if list_info(p.kind).is_some() { 0.0 } else { p.indent };
        let fmt = ParaFmt { align: p.align, indent, space_before: p.space_before };
        if fmt == (ParaFmt { align: Align::Left, indent: 0.0, space_before: 0.0 }) {
            return base.to_owned();
        }
        let idx = intern(&mut self.para, &fmt);
        if !self.used_para.contains(&(idx, base)) {
            self.used_para.push((idx, base));
        }
        // Formatting styles are generic; the kind is carried by the parent.
        format!("{}P{}_{}", self.prefix, idx + 1, base)
    }

    fn col_style(&mut self, width: f32) -> String {
        format!("{}Co{}", self.prefix, intern(&mut self.cols, &width) + 1)
    }

    fn row_style(&mut self, height: f32) -> String {
        format!("{}Ro{}", self.prefix, intern(&mut self.rows, &height) + 1)
    }

    /// Writes the automatic styles collected so far.
    fn write(&self, w: &mut XmlWriter) {
        // Points in shortest round-trip form, so values survive exactly.
        let pt = |v: f32| format!("{v}pt");
        for (i, attrs) in self.text.iter().enumerate() {
            w.start("style:style").attr("style:name", &format!("{}T{}", self.prefix, i + 1)).attr("style:family", "text");
            write_text_props(w, attrs);
            w.end();
        }
        let mut used = self.used_para.clone();
        used.sort_unstable();
        for (i, base) in used {
            let fmt = self.para[i];
            w.start("style:style")
                .attr("style:name", &format!("{}P{}_{}", self.prefix, i + 1, base))
                .attr("style:family", "paragraph")
                .attr("style:parent-style-name", base);
            w.start("style:paragraph-properties");
            if fmt.align != Align::Left {
                let value = match fmt.align {
                    Align::Left => "start",
                    Align::Center => "center",
                    Align::Right => "end",
                    Align::Justify => "justify",
                };
                w.attr("fo:text-align", value);
            }
            if fmt.indent > 0.0 {
                w.attr("fo:margin-left", &pt(fmt.indent));
            }
            if fmt.space_before > 0.0 {
                w.attr("fo:margin-top", &pt(fmt.space_before));
            }
            w.end();
            w.end();
        }
        for (i, width) in self.cols.iter().enumerate() {
            w.start("style:style").attr("style:name", &format!("{}Co{}", self.prefix, i + 1)).attr("style:family", "table-column");
            w.start("style:table-column-properties").attr("style:column-width", &pt(*width)).end();
            w.end();
        }
        for (i, height) in self.rows.iter().enumerate() {
            w.start("style:style").attr("style:name", &format!("{}Ro{}", self.prefix, i + 1)).attr("style:family", "table-row");
            w.start("style:table-row-properties").attr("style:row-height", &pt(*height)).end();
            w.end();
        }
    }
}

fn base_style(kind: ParaKind) -> &'static str {
    match kind {
        ParaKind::Normal | ParaKind::Bullet { .. } | ParaKind::Numbered { .. } => "Standard",
        ParaKind::Heading { level: 1 } => "Heading_20_1",
        ParaKind::Heading { level: 2 } => "Heading_20_2",
        ParaKind::Heading { level: 3 } => "Heading_20_3",
        ParaKind::Heading { level: 4 } => "Heading_20_4",
        ParaKind::Heading { level: 5 } => "Heading_20_5",
        ParaKind::Heading { .. } => "Heading_20_6",
        ParaKind::Quote => "Quotations",
        ParaKind::Code => "Preformatted_20_Text",
        ParaKind::Title => "Title",
    }
}

fn write_text_props(w: &mut XmlWriter, a: &TextAttrs) {
    w.start("style:text-properties");
    if a.bold {
        w.attr("fo:font-weight", "bold");
    }
    if a.italic {
        w.attr("fo:font-style", "italic");
    }
    if a.underline {
        w.attr("style:text-underline-style", "solid")
            .attr("style:text-underline-width", "auto")
            .attr("style:text-underline-color", "font-color");
    }
    if a.strike {
        w.attr("style:text-line-through-style", "solid");
    }
    if a.code {
        w.attr("style:font-name", "Liberation Mono");
    }
    if a.superscript {
        w.attr("style:text-position", "super 58%");
    } else if a.subscript {
        w.attr("style:text-position", "sub 58%");
    }
    if let Some(c) = a.color {
        w.attr("fo:color", &c.to_hex());
    }
    if let Some(h) = a.highlight {
        w.attr("fo:background-color", &h.to_hex());
    }
    if let Some(s) = a.size {
        w.attr("fo:font-size", &format!("{s}pt"));
    }
    w.end();
}

fn write_runs(w: &mut XmlWriter, p: &Paragraph, styles: &mut StyleTable) {
    for run in &p.runs {
        if run.text.is_empty() {
            continue;
        }
        let style = styles.text_style(&run.attrs);
        if let Some(link) = &run.attrs.link {
            w.start("text:a").attr("xlink:type", "simple").attr("xlink:href", link);
        }
        if let Some(style) = &style {
            w.start("text:span").attr("text:style-name", style);
        }
        match run.attrs.field {
            Some(Field::PageNumber) => {
                w.start("text:page-number").attr("text:select-page", "current").text(&run.text).end();
            }
            Some(Field::PageCount) => {
                w.start("text:page-count").text(&run.text).end();
            }
            None => write_flow_text(w, &run.text),
        }
        if style.is_some() {
            w.end();
        }
        if run.attrs.link.is_some() {
            w.end();
        }
    }
}

/// Writes text using `text:s`, `text:tab` and `text:line-break` so that
/// whitespace survives (XML collapses runs of spaces in ODF paragraphs).
fn write_flow_text(w: &mut XmlWriter, text: &str) {
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    let mut prev_space = true; // a leading space must be encoded too
    while let Some(c) = chars.next() {
        match c {
            '\t' | '\n' => {
                w.text(&buf);
                buf.clear();
                w.start(if c == '\t' { "text:tab" } else { "text:line-break" }).end();
                prev_space = true;
            }
            ' ' => {
                let mut count = 1;
                while chars.peek() == Some(&' ') {
                    chars.next();
                    count += 1;
                }
                let at_end = chars.peek().is_none();
                if !prev_space && !at_end {
                    buf.push(' ');
                    count -= 1;
                }
                if count > 0 {
                    w.text(&buf);
                    buf.clear();
                    w.start("text:s");
                    if count > 1 {
                        w.attr("text:c", &count.to_string());
                    }
                    w.end();
                }
                prev_space = true;
            }
            other => {
                buf.push(other);
                prev_space = false;
            }
        }
    }
    w.text(&buf);
}

fn write_paragraph(w: &mut XmlWriter, p: &Paragraph, styles: &mut StyleTable) {
    let style = styles.para_style(p);
    match p.kind {
        ParaKind::Heading { level } => {
            w.start("text:h").attr("text:style-name", &style).attr("text:outline-level", &level.to_string());
        }
        _ => {
            w.start("text:p").attr("text:style-name", &style);
        }
    }
    write_runs(w, p, styles);
    w.end();
}

fn list_info(kind: ParaKind) -> Option<(bool, u8)> {
    match kind {
        ParaKind::Bullet { level } => Some((false, level)),
        ParaKind::Numbered { level } => Some((true, level)),
        _ => None,
    }
}

/// Writes a run of consecutive list paragraphs as nested `text:list`s.
///
/// Each nesting level is a `text:list-item` holding a `text:list`; a deeper
/// item nests inside the previous item. Every list carries the style (bullet
/// or numbered) of the first item that opened it.
fn write_list(w: &mut XmlWriter, items: &[&Paragraph], styles: &mut StyleTable) {
    let Some((numbered, _)) = items.first().and_then(|p| list_info(p.kind)) else { return };
    let style_of = |numbered: bool| if numbered { "L2" } else { "L1" };
    w.start("text:list").attr("text:style-name", style_of(numbered));
    let mut depth = 0u8;
    // List type of the open list at each depth.
    let mut types: Vec<bool> = vec![numbered];
    for (i, p) in items.iter().enumerate() {
        let (item_numbered, level) = list_info(p.kind).unwrap_or((numbered, 0));
        if i == 0 || level > depth {
            if i > 0 {
                // Nest inside the previous (still open) item.
                w.start("text:list").attr("text:style-name", style_of(item_numbered));
                types.push(item_numbered);
                depth += 1;
            }
            while depth < level {
                w.start("text:list-item");
                w.start("text:list").attr("text:style-name", style_of(item_numbered));
                types.push(item_numbered);
                depth += 1;
            }
        } else {
            w.end(); // previous list-item
            while depth > level {
                w.end().end(); // nested list, its parent list-item
                types.pop();
                depth -= 1;
            }
            if depth > 0 && types[usize::from(depth)] != item_numbered {
                // Same level, other list type: start a sibling list in the
                // same parent item.
                w.end();
                w.start("text:list").attr("text:style-name", style_of(item_numbered));
                types[usize::from(depth)] = item_numbered;
            }
        }
        w.start("text:list-item");
        write_paragraph(w, p, styles);
    }
    w.end(); // last list-item
    while depth > 0 {
        w.end().end();
        depth -= 1;
    }
    w.end(); // top-level list
}

fn write_table(w: &mut XmlWriter, t: &Table, index: usize, styles: &mut StyleTable) {
    let fixed = !t.col_widths.is_empty();
    w.start("table:table")
        .attr("table:name", &format!("Table{}", index + 1))
        .attr("table:style-name", if fixed { "TblFixed" } else { "Tbl" });
    if fixed {
        for width in &t.col_widths {
            let style = styles.col_style(*width);
            w.start("table:table-column").attr("table:style-name", &style).end();
        }
    } else {
        w.start("table:table-column").attr("table:number-columns-repeated", &t.cols().max(1).to_string()).end();
    }
    let cell_style = if t.borders { "TblCell" } else { "TblCellPlain" };
    for (r, row) in t.rows.iter().enumerate() {
        w.start("table:table-row");
        if let Some(h) = t.row_heights.get(r) {
            let style = styles.row_style(*h);
            w.attr("table:style-name", &style);
        }
        for cell in row {
            w.start("table:table-cell").attr("table:style-name", cell_style).attr("office:value-type", "string");
            write_blocks(w, cell.iter(), styles);
            w.end();
        }
        w.end();
    }
    w.end();
}

fn write_blocks<'a>(w: &mut XmlWriter, paragraphs: impl Iterator<Item = &'a Paragraph>, styles: &mut StyleTable) {
    let paragraphs: Vec<&Paragraph> = paragraphs.collect();
    let mut i = 0;
    while i < paragraphs.len() {
        if let Some((numbered, _)) = list_info(paragraphs[i].kind) {
            let mut j = i + 1;
            // A list continues while items follow; a top-level item of the
            // other type starts a new list.
            while j < paragraphs.len()
                && list_info(paragraphs[j].kind).is_some_and(|(n, level)| n == numbered || level > 0)
            {
                j += 1;
            }
            write_list(w, &paragraphs[i..j], styles);
            i = j;
        } else {
            write_paragraph(w, paragraphs[i], styles);
            i += 1;
        }
    }
}

/// Pictures and embedded drawing objects written with the text.
#[derive(Default)]
struct Media {
    pictures: office_draw::odg::Pictures,
    /// (folder, content.xml, styles.xml, pictures) of embedded drawings.
    objects: Vec<(String, String, String, office_draw::odg::Pictures)>,
    frames: usize,
}

fn align_style(a: Align) -> &'static str {
    match a {
        Align::Left | Align::Justify => "FrameL",
        Align::Center => "FrameC",
        Align::Right => "FrameR",
    }
}

/// Opens `<text:p><draw:frame ...>` for an as-character frame.
fn start_frame(w: &mut XmlWriter, media: &mut Media, align: Align, name: &str, width: f32, height: f32) {
    media.frames += 1;
    w.start("text:p").attr("text:style-name", align_style(align));
    w.start("draw:frame")
        .attr("draw:style-name", "FramePic")
        .attr("draw:name", &format!("{name} {}", media.frames))
        .attr("text:anchor-type", "as-char")
        .attr("svg:width", &format_length_in(f64::from(width)))
        .attr("svg:height", &format_length_in(f64::from(height)))
        .attr("draw:z-index", "0");
}

fn write_image_ref(w: &mut XmlWriter, href: &str, mime: &str) {
    w.start("draw:image")
        .attr("xlink:href", href)
        .attr("xlink:type", "simple")
        .attr("xlink:show", "embed")
        .attr("xlink:actuate", "onLoad")
        .attr("draw:mime-type", mime)
        .end();
}

fn write_frame_block(w: &mut XmlWriter, block: &Block, media: &mut Media) {
    match block {
        Block::Image(p) => {
            start_frame(w, media, p.align, "Image", p.width, p.height);
            let href = media.pictures.add(p.image.package_name("Pictures"), p.image.bytes());
            write_image_ref(w, &href, p.image.format().mime());
            if !p.description.is_empty() {
                w.element_text("svg:desc", &p.description);
            }
            w.end().end();
        }
        Block::Diagram(d) => {
            media.frames += 1;
            w.start("text:p").attr("text:style-name", align_style(d.align));
            w.start("draw:frame")
                .attr("draw:style-name", "FramePic")
                .attr("draw:name", &format!("{} {}", office_draw::odg::DIAGRAM_NAME, media.frames))
                .attr("text:anchor-type", "as-char")
                .attr("svg:width", &format_length_in(f64::from(d.width)))
                .attr("svg:height", &format_length_in(f64::from(d.height())))
                .attr("draw:z-index", "0");
            let svg = d.figure().to_svg(None);
            let href = media.pictures.add(office_draw::odg::diagram_file_name(&d.source), svg.as_bytes());
            write_image_ref(w, &href, "image/svg+xml");
            w.element_text("svg:title", office_draw::odg::DIAGRAM_NAME);
            w.element_text("svg:desc", &d.source);
            w.end().end();
        }
        Block::Drawing(c) => {
            start_frame(w, media, c.align, "Drawing", c.width, c.height);
            let dir = format!("Object {}", media.objects.len() + 1);
            let page = office_draw::Page { shapes: c.shapes.clone(), ..office_draw::Page::default() };
            let (content, styles, pics) = office_draw::odg::streams(office_draw::DrawingKind::Drawing, &[page], &[String::new()], c.width, c.height);
            w.start("draw:object")
                .attr("xlink:href", &format!("./{dir}"))
                .attr("xlink:type", "simple")
                .attr("xlink:show", "embed")
                .attr("xlink:actuate", "onLoad")
                .end();
            // A picture of the drawing for applications that cannot edit it.
            let svg = crate::export::canvas_figure(c).to_svg(None);
            let replacement = media.pictures.add(format!("Pictures/drawing-{}.svg", media.objects.len() + 1), svg.as_bytes());
            write_image_ref(w, &replacement, "image/svg+xml");
            w.end().end();
            media.objects.push((dir, content, styles, pics));
        }
        _ => {}
    }
}

fn body_xml(doc: &Document, styles: &mut StyleTable, media: &mut Media) -> String {
    let mut w = XmlWriter::fragment();
    w.start("office:body").start("office:text");
    let mut page_break_next = false;
    let mut pending: Vec<&Paragraph> = Vec::new();
    let mut tables = 0;
    let flush = |w: &mut XmlWriter, pending: &mut Vec<&Paragraph>, styles: &mut StyleTable| {
        write_blocks(w, pending.drain(..), styles);
    };
    for block in doc.blocks() {
        match block {
            Block::Paragraph(p) => {
                if page_break_next {
                    flush(&mut w, &mut pending, styles);
                    w.start("text:p").attr("text:style-name", "PageBreak").end();
                    page_break_next = false;
                }
                pending.push(p);
            }
            Block::Table(t) => {
                flush(&mut w, &mut pending, styles);
                if page_break_next {
                    // The break must come before the table, not after it.
                    w.start("text:p").attr("text:style-name", "PageBreak").end();
                    page_break_next = false;
                }
                write_table(&mut w, t, tables, styles);
                tables += 1;
            }
            Block::PageBreak => {
                flush(&mut w, &mut pending, styles);
                page_break_next = true;
            }
            frame @ (Block::Image(_) | Block::Diagram(_) | Block::Drawing(_)) => {
                flush(&mut w, &mut pending, styles);
                if page_break_next {
                    w.start("text:p").attr("text:style-name", "PageBreak").end();
                    page_break_next = false;
                }
                write_frame_block(&mut w, frame, media);
            }
        }
    }
    flush(&mut w, &mut pending, styles);
    if page_break_next {
        w.start("text:p").attr("text:style-name", "PageBreak").end();
    }
    w.finish()
}

fn content_xml(doc: &Document, media: &mut Media) -> String {
    let mut styles = StyleTable::with_prefix("");
    let body = body_xml(doc, &mut styles, media);
    let mut w = XmlWriter::new();
    w.start("office:document-content")
        .namespaces(&["office", "style", "text", "table", "fo", "xlink", "svg", "draw"])
        .attr("office:version", ns::ODF_VERSION);
    w.start("office:font-face-decls");
    w.start("style:font-face")
        .attr("style:name", "Liberation Mono")
        .attr("svg:font-family", "'Liberation Mono'")
        .attr("style:font-family-generic", "modern")
        .attr("style:font-pitch", "fixed")
        .end();
    w.end();
    w.start("office:automatic-styles");
    styles.write(&mut w);
    w.start("style:style").attr("style:name", "PageBreak").attr("style:family", "paragraph").attr("style:parent-style-name", "Standard");
    w.start("style:paragraph-properties").attr("fo:break-before", "page").end();
    w.end();
    for (name, align) in [("FrameL", "start"), ("FrameC", "center"), ("FrameR", "end")] {
        w.start("style:style").attr("style:name", name).attr("style:family", "paragraph").attr("style:parent-style-name", "Standard");
        w.start("style:paragraph-properties").attr("fo:text-align", align).end();
        w.end();
    }
    w.start("style:style").attr("style:name", "FramePic").attr("style:family", "graphic");
    w.start("style:graphic-properties")
        .attr("style:vertical-pos", "top")
        .attr("style:vertical-rel", "baseline")
        .attr("draw:fill", "none")
        .attr("draw:stroke", "none")
        .attr("fo:border", "none")
        .end();
    w.end();
    w.start("style:style").attr("style:name", "Tbl").attr("style:family", "table");
    w.start("style:table-properties").attr("table:align", "margins").end();
    w.end();
    w.start("style:style").attr("style:name", "TblFixed").attr("style:family", "table");
    let width: f32 = doc
        .blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Table(t) if !t.col_widths.is_empty() => Some(t.col_widths.iter().sum::<f32>()),
            _ => None,
        })
        .fold(0.0, f32::max);
    w.start("style:table-properties").attr("table:align", "left");
    if width > 0.0 {
        w.attr("style:width", &format_length_in(f64::from(width)));
    }
    w.end();
    w.end();
    w.start("style:style").attr("style:name", "TblCell").attr("style:family", "table-cell");
    w.start("style:table-cell-properties").attr("fo:padding", "0.05in").attr("fo:border", "0.5pt solid #000000").end();
    w.end();
    w.start("style:style").attr("style:name", "TblCellPlain").attr("style:family", "table-cell");
    w.start("style:table-cell-properties").attr("fo:padding", "0.08in").attr("fo:border", "none").end();
    w.end();
    for (name, numbered) in [("L1", false), ("L2", true)] {
        w.start("text:list-style").attr("style:name", name);
        for level in 1..=10 {
            let indent = format_length_in(18.0 * f64::from(level));
            if numbered {
                w.start("text:list-level-style-number")
                    .attr("text:level", &level.to_string())
                    .attr("style:num-suffix", ".")
                    .attr("style:num-format", "1");
            } else {
                let bullet = ["•", "◦", "▪"][(level as usize - 1) % 3];
                w.start("text:list-level-style-bullet").attr("text:level", &level.to_string()).attr("text:bullet-char", bullet);
            }
            w.start("style:list-level-properties")
                .attr("text:list-level-position-and-space-mode", "label-alignment");
            w.start("style:list-level-label-alignment")
                .attr("text:label-followed-by", "listtab")
                .attr("fo:text-indent", "-0.25in")
                .attr("fo:margin-left", &indent)
                .end();
            w.end().end();
        }
        w.end();
    }
    w.end(); // automatic-styles
    let mut out = w.finish();
    // Splice the pre-rendered body before the closing root tag.
    if let Some(pos) = out.rfind("</office:document-content>") {
        out.insert_str(pos, &body);
    }
    out
}

fn styles_xml(doc: &Document) -> String {
    // Header and footer paragraphs are rendered first so their automatic
    // styles can be written into this file's automatic styles.
    let mut area_styles = StyleTable::with_prefix("M");
    let area = |name: &str, paragraphs: &[Paragraph], styles: &mut StyleTable| -> Option<String> {
        if paragraphs.is_empty() {
            return None;
        }
        let mut w = XmlWriter::fragment();
        w.start(name);
        write_blocks(&mut w, paragraphs.iter(), styles);
        w.end();
        Some(w.finish())
    };
    let header = area("style:header", doc.header(), &mut area_styles);
    let footer = area("style:footer", doc.footer(), &mut area_styles);
    let page = doc.page();
    let mut w = XmlWriter::new();
    w.start("office:document-styles")
        .namespaces(&["office", "style", "text", "table", "fo", "svg", "xlink"])
        .attr("office:version", ns::ODF_VERSION);
    w.start("office:font-face-decls");
    for (name, generic, pitch) in [("Liberation Serif", "roman", "variable"), ("Liberation Sans", "swiss", "variable"), ("Liberation Mono", "modern", "fixed")] {
        w.start("style:font-face")
            .attr("style:name", name)
            .attr("svg:font-family", &format!("'{name}'"))
            .attr("style:font-family-generic", generic)
            .attr("style:font-pitch", pitch)
            .end();
    }
    w.end();
    w.start("office:styles");
    w.start("style:default-style").attr("style:family", "paragraph");
    w.start("style:text-properties").attr("style:font-name", "Liberation Serif").attr("fo:font-size", "12pt").end();
    w.end();
    let para = |w: &mut XmlWriter, name: &str, display: &str, parent: Option<&str>, f: &dyn Fn(&mut XmlWriter)| {
        w.start("style:style").attr("style:name", name).attr("style:display-name", display).attr("style:family", "paragraph");
        if let Some(parent) = parent {
            w.attr("style:parent-style-name", parent);
        }
        f(w);
        w.end();
    };
    para(&mut w, "Standard", "Default Paragraph Style", None, &|w| {
        w.start("style:paragraph-properties").attr("fo:margin-bottom", "0.0972in").end();
    });
    para(&mut w, "Heading", "Heading", Some("Standard"), &|w| {
        w.start("style:paragraph-properties").attr("fo:margin-top", "0.1665in").attr("fo:keep-with-next", "always").end();
        w.start("style:text-properties").attr("style:font-name", "Liberation Sans").attr("fo:font-weight", "bold").end();
    });
    for (level, size) in [(1, "130%"), (2, "115%"), (3, "101%"), (4, "95%"), (5, "85%"), (6, "85%")] {
        para(&mut w, &format!("Heading_20_{level}"), &format!("Heading {level}"), Some("Heading"), &|w| {
            w.attr("style:default-outline-level", &level.to_string());
            w.start("style:text-properties").attr("fo:font-size", size).end();
        });
    }
    para(&mut w, "Title", "Title", Some("Heading"), &|w| {
        w.start("style:paragraph-properties").attr("fo:text-align", "center").end();
        w.start("style:text-properties").attr("fo:font-size", "28pt").end();
    });
    para(&mut w, "Quotations", "Quotations", Some("Standard"), &|w| {
        w.start("style:paragraph-properties").attr("fo:margin-left", "0.3937in").attr("fo:margin-right", "0.3937in").end();
        w.start("style:text-properties").attr("fo:font-style", "italic").end();
    });
    para(&mut w, "Preformatted_20_Text", "Preformatted Text", Some("Standard"), &|w| {
        w.start("style:paragraph-properties").attr("fo:margin-bottom", "0in").end();
        w.start("style:text-properties").attr("style:font-name", "Liberation Mono").attr("fo:font-size", "10pt").end();
    });
    w.end(); // office:styles
    w.start("office:automatic-styles");
    area_styles.write(&mut w);
    w.start("style:page-layout").attr("style:name", "pm1");
    w.start("style:page-layout-properties")
        .attr("fo:page-width", &format_length_in(f64::from(page.width)))
        .attr("fo:page-height", &format_length_in(f64::from(page.height)))
        .attr("style:print-orientation", if page.is_landscape() { "landscape" } else { "portrait" });
    for side in ["top", "bottom", "left", "right"] {
        w.attr(&format!("fo:margin-{side}"), &format_length_in(f64::from(page.margin)));
    }
    w.end();
    // Header and footer sit inside the page margins with a small gap to the
    // text, as in the editor.
    for (style, props, gap) in [("style:header-style", "style:header-properties", "fo:margin-bottom"), ("style:footer-style", "style:footer-properties", "fo:margin-top")] {
        w.start(style).start(props).attr("fo:min-height", "0in").attr(gap, "0.1in").end().end();
    }
    w.end().end();
    w.start("office:master-styles");
    w.start("style:master-page").attr("style:name", "Standard").attr("style:page-layout-name", "pm1");
    w.end();
    let mut out = w.finish();
    // Splice the pre-rendered header and footer into the master page.
    let parts = format!("{}{}", header.unwrap_or_default(), footer.unwrap_or_default());
    if !parts.is_empty() {
        let empty = "style:page-layout-name=\"pm1\"/>";
        if let Some(pos) = out.rfind(empty) {
            out.replace_range(pos..pos + empty.len(), &format!("style:page-layout-name=\"pm1\">{parts}</style:master-page>"));
        }
    }
    out
}

/// Serialises a document to the bytes of an `.odt` file.
pub fn save_odt(doc: &Document) -> Result<Vec<u8>> {
    let mut package = Package::new(ns::MIME_ODT);
    let mut media = Media::default();
    package.insert("content.xml", content_xml(doc, &mut media).into_bytes())?;
    media.pictures.insert_into(&mut package, "")?;
    for (dir, content, styles, pics) in media.objects {
        package.add_sub_document(&dir, ns::MIME_ODG)?;
        package.insert(format!("{dir}/content.xml"), content.into_bytes())?;
        package.insert(format!("{dir}/styles.xml"), styles.into_bytes())?;
        pics.insert_into(&mut package, &format!("{dir}/"))?;
    }
    package.insert("styles.xml", styles_xml(doc).into_bytes())?;
    package.insert("meta.xml", doc.meta.to_xml().into_bytes())?;
    package.write()
}
