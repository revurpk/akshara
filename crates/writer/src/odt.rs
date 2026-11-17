//! OpenDocument Text (`.odt`) import and export.
//!
//! Supported: paragraphs, headings, bulleted and numbered lists (nested),
//! tables (nested tables are flattened to text), page breaks, character
//! attributes (bold, italic, underline, strike-through, monospace, super- and
//! subscript, colour, highlight, size), links, alignment, metadata and page
//! size. Images, frames, fields, footnotes, comments and tracked changes are
//! not modelled; their text content is kept where it is part of the flow.

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
        let mut p = Paragraph { kind: kind.normalized(), align: own.align.unwrap_or_default(), runs };
        p.normalize();
        if style.break_before && !self.blocks.is_empty() {
            self.blocks.push(Block::PageBreak);
        }
        Ok(p)
    }

    /// A top-level paragraph; an empty body paragraph that only carries a
    /// page break becomes just the break (this is how breaks are saved).
    fn top_paragraph(&mut self, e: &Element) -> Result<()> {
        let p = self.paragraph(e, None)?;
        let only_break = p.kind == ParaKind::Normal
            && p.text().is_empty()
            && matches!(self.blocks.last(), Some(Block::PageBreak))
            && self.resolved(e.attr("text:style-name")).break_before;
        if !only_break {
            self.blocks.push(Block::Paragraph(p));
        }
        Ok(())
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
        for row in table_rows(e) {
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
            }
        }
        let mut table = Table { rows };
        table.normalize();
        Ok(table)
    }

    fn body(&mut self, parent: &Element, depth: usize) -> Result<()> {
        for child in parent.elements() {
            match child.name.as_str() {
                "text:p" | "text:h" => self.top_paragraph(child)?,
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
    let package =
        Package::read_filtered(bytes, limits, |n| matches!(n, "content.xml" | "styles.xml" | "meta.xml"))?;
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

    let mut importer =
        Importer { styles: HashMap::new(), list_styles: HashMap::new(), limits, blocks: Vec::new(), block_count: 0 };
    let mut page = PageSetup::default();
    if let Some(styles_doc) = &styles_doc {
        importer.collect_styles(styles_doc.child("office:styles"), &font_faces, false);
        importer.collect_styles(styles_doc.child("office:automatic-styles"), &font_faces, true);
        if let Some(layout) = styles_doc
            .child("office:automatic-styles")
            .and_then(|s| s.children_named("style:page-layout").next())
            .and_then(|l| l.child("style:page-layout-properties"))
        {
            let len = |attr: &str| layout.attr(attr).and_then(parse_length_pt).map(|v| v as f32);
            if let (Some(w), Some(h)) = (len("fo:page-width"), len("fo:page-height")) {
                if (72.0..=14_400.0).contains(&w) && (72.0..=14_400.0).contains(&h) {
                    page.width = w;
                    page.height = h;
                }
            }
            if let Some(m) = len("fo:margin-left").filter(|m| (0.0..=page.width / 3.0).contains(m)) {
                page.margin = m;
            }
        }
    }
    importer.collect_styles(content.child("office:automatic-styles"), &font_faces, true);

    let text = content
        .descend(&["office:body", "office:text"])
        .ok_or_else(|| OdfError::invalid("document has no text body"))?;
    importer.body(text, 0)?;

    let mut doc = Document::from_blocks(importer.blocks);
    doc.page = page;
    if let Some(meta) = package.get_xml("meta.xml", limits)? {
        doc.meta = Metadata::from_xml(&meta);
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Assigns automatic style names to distinct attribute sets.
#[derive(Default)]
struct StyleTable {
    text: Vec<TextAttrs>,
    para: Vec<Align>,
}

impl StyleTable {
    fn text_style(&mut self, attrs: &TextAttrs) -> Option<String> {
        let mut key = attrs.clone();
        key.link = None;
        if key == TextAttrs::default() {
            return None;
        }
        let idx = match self.text.iter().position(|a| *a == key) {
            Some(i) => i,
            None => {
                self.text.push(key);
                self.text.len() - 1
            }
        };
        Some(format!("T{}", idx + 1))
    }

    fn para_style(&mut self, kind: ParaKind, align: Align) -> String {
        let base = base_style(kind);
        if align == Align::Left {
            return base.to_owned();
        }
        let idx = match self.para.iter().position(|a| *a == align) {
            Some(i) => i,
            None => {
                self.para.push(align);
                self.para.len() - 1
            }
        };
        // Alignment styles are generic; the kind is carried by the parent.
        format!("P{}_{}", idx + 1, base)
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
        write_flow_text(w, &run.text);
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
    let style = styles.para_style(p.kind, p.align);
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
    w.start("table:table").attr("table:name", &format!("Table{}", index + 1)).attr("table:style-name", "Tbl");
    w.start("table:table-column").attr("table:number-columns-repeated", &t.cols().max(1).to_string()).end();
    for row in &t.rows {
        w.start("table:table-row");
        for cell in row {
            w.start("table:table-cell").attr("table:style-name", "TblCell").attr("office:value-type", "string");
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

fn body_xml(doc: &Document, styles: &mut StyleTable) -> String {
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
                write_table(&mut w, t, tables, styles);
                tables += 1;
            }
            Block::PageBreak => {
                flush(&mut w, &mut pending, styles);
                page_break_next = true;
            }
        }
    }
    flush(&mut w, &mut pending, styles);
    if page_break_next {
        w.start("text:p").attr("text:style-name", "PageBreak").end();
    }
    w.finish()
}

fn content_xml(doc: &Document) -> String {
    let mut styles = StyleTable::default();
    let body = body_xml(doc, &mut styles);
    let mut w = XmlWriter::new();
    w.start("office:document-content")
        .namespaces(&["office", "style", "text", "table", "fo", "xlink", "svg"])
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
    for (i, attrs) in styles.text.iter().enumerate() {
        w.start("style:style").attr("style:name", &format!("T{}", i + 1)).attr("style:family", "text");
        write_text_props(&mut w, attrs);
        w.end();
    }
    for (i, align) in styles.para.iter().enumerate() {
        for base in [
            "Standard", "Heading_20_1", "Heading_20_2", "Heading_20_3", "Heading_20_4", "Heading_20_5",
            "Heading_20_6", "Quotations", "Preformatted_20_Text", "Title",
        ] {
            let name = format!("P{}_{}", i + 1, base);
            if !body.contains(&format!("\"{name}\"")) {
                continue;
            }
            w.start("style:style")
                .attr("style:name", &name)
                .attr("style:family", "paragraph")
                .attr("style:parent-style-name", base);
            let value = match align {
                Align::Left => "start",
                Align::Center => "center",
                Align::Right => "end",
                Align::Justify => "justify",
            };
            w.start("style:paragraph-properties").attr("fo:text-align", value).end();
            w.end();
        }
    }
    w.start("style:style").attr("style:name", "PageBreak").attr("style:family", "paragraph").attr("style:parent-style-name", "Standard");
    w.start("style:paragraph-properties").attr("fo:break-before", "page").end();
    w.end();
    w.start("style:style").attr("style:name", "Tbl").attr("style:family", "table");
    w.start("style:table-properties").attr("table:align", "margins").end();
    w.end();
    w.start("style:style").attr("style:name", "TblCell").attr("style:family", "table-cell");
    w.start("style:table-cell-properties").attr("fo:padding", "0.05in").attr("fo:border", "0.5pt solid #000000").end();
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
    let mut w = XmlWriter::new();
    w.start("office:document-styles")
        .namespaces(&["office", "style", "text", "table", "fo", "svg"])
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
    w.start("style:page-layout").attr("style:name", "pm1");
    w.start("style:page-layout-properties")
        .attr("fo:page-width", &format_length_in(f64::from(doc.page.width)))
        .attr("fo:page-height", &format_length_in(f64::from(doc.page.height)))
        .attr("style:print-orientation", if doc.page.width > doc.page.height { "landscape" } else { "portrait" });
    for side in ["top", "bottom", "left", "right"] {
        w.attr(&format!("fo:margin-{side}"), &format_length_in(f64::from(doc.page.margin)));
    }
    w.end().end().end();
    w.start("office:master-styles");
    w.start("style:master-page").attr("style:name", "Standard").attr("style:page-layout-name", "pm1").end();
    w.finish()
}

/// Serialises a document to the bytes of an `.odt` file.
pub fn save_odt(doc: &Document) -> Result<Vec<u8>> {
    let mut package = Package::new(ns::MIME_ODT);
    package.insert("content.xml", content_xml(doc).into_bytes())?;
    package.insert("styles.xml", styles_xml(doc).into_bytes())?;
    package.insert("meta.xml", doc.meta.to_xml().into_bytes())?;
    package.write()
}
