//! Microsoft Word (`.docx`) import and export.
//!
//! Read: paragraphs and runs (bold, italic, underline, strike-through,
//! monospace fonts, super- and subscript, colour, highlight and shading,
//! size), character and paragraph styles (headings, title, quotations,
//! preformatted text), alignment, left indent and space above, bulleted and
//! numbered lists (from the numbering definitions), tables (column widths,
//! exact row heights, merged cells as empty cells, nested tables flattened),
//! hyperlinks, line, column and page breaks, page number and page count
//! fields, tracked insertions (deletions are dropped), page size and margin,
//! the default header and footer, and the title.
//!
//! The text of text boxes follows the paragraph they are anchored in, and
//! so do pictures (inline or anchored; the SVG version is preferred when
//! a picture has one). Footnotes, comments and other fields are not
//! modelled; text inside them is skipped (except field results, which are
//! kept as text).
//!
//! Pictures are saved as Word pictures. Mermaid diagrams and drawings are
//! saved as SVG pictures with a PNG fallback for older Word versions; a
//! diagram keeps its source in the picture's description, so it stays an
//! editable diagram when the file is opened again.
//!
//! [`save_docx`] writes everything the model holds, with Word's own styles
//! (Normal, Heading 1-6, Title, Quote, HTML Preformatted, Hyperlink, Table
//! Grid) so the document looks right and edits naturally in Word.

use crate::document::Document;
use crate::model::*;
use office_odf::ooxml::{self, twips, Relationship};
use office_odf::xml::{Element, XmlWriter};
use office_odf::{Limits, Metadata, OdfError, Package, Result, Rgb};
use std::collections::HashMap;

const MONO_FONTS: &[&str] = &["courier", "mono", "consolas", "menlo", "lucida console", "liberation mono"];

/// Run properties as optionals, so styles can be layered.
#[derive(Debug, Clone, Default)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    code: Option<bool>,
    superscript: Option<bool>,
    subscript: Option<bool>,
    color: Option<Option<Rgb>>,
    highlight: Option<Option<Rgb>>,
    size: Option<f32>,
}

impl RunProps {
    fn over(&mut self, o: &RunProps) {
        macro_rules! take {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        take!(bold, italic, underline, strike, code, superscript, subscript, color, highlight, size);
    }

    fn apply(&self, a: &mut TextAttrs) {
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(v) = self.$f { a.$f = v; } )* };
        }
        set!(bold, italic, underline, strike, code, superscript, subscript);
        if let Some(c) = self.color {
            a.color = c;
        }
        if let Some(h) = self.highlight {
            a.highlight = h;
        }
        if let Some(s) = self.size {
            a.size = Some(s);
        }
    }
}

/// `<w:b/>` is on; `w:val` of `0`, `false` or `off` turns it off.
fn toggle(e: &Element) -> bool {
    !matches!(e.attr("w:val"), Some("0" | "false" | "off" | "none"))
}

fn hex_color(v: Option<&str>) -> Option<Option<Rgb>> {
    let v = v?;
    if v.eq_ignore_ascii_case("auto") {
        return Some(None);
    }
    Rgb::parse(&format!("#{v}")).map(Some)
}

/// Word's named highlight colours.
fn highlight_color(name: &str) -> Option<Rgb> {
    let (r, g, b) = match name {
        "yellow" => (0xFF, 0xFF, 0x00),
        "green" => (0x00, 0xFF, 0x00),
        "cyan" => (0x00, 0xFF, 0xFF),
        "magenta" => (0xFF, 0x00, 0xFF),
        "blue" => (0x00, 0x00, 0xFF),
        "red" => (0xFF, 0x00, 0x00),
        "darkBlue" => (0x00, 0x00, 0x80),
        "darkCyan" => (0x00, 0x80, 0x80),
        "darkGreen" => (0x00, 0x80, 0x00),
        "darkMagenta" => (0x80, 0x00, 0x80),
        "darkRed" => (0x80, 0x00, 0x00),
        "darkYellow" => (0x80, 0x80, 0x00),
        "darkGray" => (0x80, 0x80, 0x80),
        "lightGray" => (0xC0, 0xC0, 0xC0),
        "black" => (0x00, 0x00, 0x00),
        "white" => (0xFF, 0xFF, 0xFF),
        _ => return None,
    };
    Some(Rgb(r, g, b))
}

fn parse_rpr(rpr: Option<&Element>) -> RunProps {
    let mut p = RunProps::default();
    let Some(rpr) = rpr else { return p };
    for e in rpr.elements() {
        match e.name.as_str() {
            "w:b" => p.bold = Some(toggle(e)),
            "w:i" => p.italic = Some(toggle(e)),
            "w:u" => p.underline = Some(toggle(e)),
            "w:strike" | "w:dstrike" => p.strike = Some(toggle(e)),
            "w:vertAlign" => {
                let v = e.attr("w:val").unwrap_or("baseline");
                p.superscript = Some(v == "superscript");
                p.subscript = Some(v == "subscript");
            }
            "w:color" => p.color = hex_color(e.attr("w:val")),
            "w:highlight" => p.highlight = Some(e.attr("w:val").and_then(highlight_color)),
            "w:shd" => {
                if p.highlight.is_none() {
                    if let Some(Some(fill)) = hex_color(e.attr("w:fill")) {
                        // White shading is only a background, not a highlight.
                        if fill != Rgb(0xFF, 0xFF, 0xFF) {
                            p.highlight = Some(Some(fill));
                        }
                    }
                }
            }
            "w:sz" => {
                p.size = e
                    .attr("w:val")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|half| half / 2.0)
                    .filter(|s| (4.0..=400.0).contains(s));
            }
            "w:rFonts" => {
                if let Some(font) = e.attr("w:ascii").or_else(|| e.attr("w:hAnsi")) {
                    let lower = font.to_lowercase();
                    p.code = Some(MONO_FONTS.iter().any(|m| lower.contains(m)));
                }
            }
            _ => {}
        }
    }
    p
}

/// Paragraph properties of a style or a paragraph.
#[derive(Debug, Clone, Default)]
struct ParaProps {
    align: Option<Align>,
    indent: Option<f32>,
    space_before: Option<f32>,
    outline: Option<u8>,
    /// `(numId, level)`; numId "0" switches numbering off.
    num: Option<(String, u8)>,
    page_break_before: bool,
}

fn parse_ppr(ppr: Option<&Element>) -> ParaProps {
    let mut p = ParaProps::default();
    let Some(ppr) = ppr else { return p };
    for e in ppr.elements() {
        match e.name.as_str() {
            "w:jc" => {
                p.align = match e.attr("w:val") {
                    Some("center") => Some(Align::Center),
                    Some("right" | "end") => Some(Align::Right),
                    Some("both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida") => Some(Align::Justify),
                    Some("left" | "start") => Some(Align::Left),
                    _ => None,
                }
            }
            "w:ind" => {
                p.indent = twips(e.attr("w:left").or_else(|| e.attr("w:start"))).map(|v| v.clamp(0.0, MAX_PARA_SPACING));
            }
            "w:spacing" => {
                if e.attr("w:beforeAutospacing").is_none_or(|v| v == "0" || v == "false") {
                    p.space_before = twips(e.attr("w:before")).map(|v| v.clamp(0.0, MAX_PARA_SPACING));
                }
            }
            "w:outlineLvl" => p.outline = e.attr("w:val").and_then(|v| v.parse::<u8>().ok()).filter(|l| *l < 9),
            "w:numPr" => {
                let id = e.child("w:numId").and_then(|n| n.attr("w:val")).unwrap_or("0");
                let level = e.child("w:ilvl").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
                p.num = Some((id.to_owned(), level.min(9)));
            }
            "w:pageBreakBefore" => p.page_break_before = toggle(e),
            _ => {}
        }
    }
    p
}

#[derive(Debug, Clone, Default)]
struct Style {
    /// Lower-case display name ("heading 1", "title"...).
    name: String,
    based_on: Option<String>,
    run: RunProps,
    para: ParaProps,
    /// The default paragraph style ("Normal").
    default: bool,
}

struct Importer<'a> {
    limits: &'a Limits,
    styles: HashMap<String, Style>,
    /// numId -> per-level "is a bullet".
    numbering: HashMap<String, Vec<bool>>,
    rels: HashMap<String, Relationship>,
    blocks: usize,
    /// Text box nesting (boxes inside boxes are not followed).
    in_text_box: bool,
    /// The package, for pictures.
    package: &'a Package,
}

/// `w:drawing` elements in a paragraph's runs (not inside text boxes or
/// compatibility fallbacks).
fn drawings<'a>(e: &'a Element, out: &mut Vec<&'a Element>, depth: usize) {
    if depth > 24 {
        return;
    }
    for child in e.elements() {
        if child.name == "w:drawing" {
            out.push(child);
        } else if !child.name.ends_with("}Fallback") && child.name != "w:txbxContent" && child.name != "w:p" {
            drawings(child, out, depth + 1);
        }
    }
}

/// English Metric Units per point.
const EMU: f32 = 12_700.0;

fn find_descendant<'a>(e: &'a Element, name: &str, depth: usize) -> Option<&'a Element> {
    if depth > 16 {
        return None;
    }
    for c in e.elements() {
        if c.name == name {
            return Some(c);
        }
        if let Some(found) = find_descendant(c, name, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// State of a complex field (`w:fldChar`) while walking runs.
#[derive(Debug, Default)]
struct FieldState {
    /// Nesting depth of open fields.
    depth: usize,
    /// Instruction text of the outermost field.
    instr: String,
    /// In the result part of the outermost field.
    in_result: bool,
    /// The result text of a PAGE / NUMPAGES field.
    field: Option<Field>,
    result: String,
    attrs: TextAttrs,
}

fn field_of(instr: &str) -> Option<Field> {
    match instr.split_whitespace().next().map(str::to_ascii_uppercase).as_deref() {
        Some("PAGE") => Some(Field::PageNumber),
        Some("NUMPAGES" | "SECTIONPAGES") => Some(Field::PageCount),
        _ => None,
    }
}

fn push_text(runs: &mut Vec<Run>, text: &str, attrs: &TextAttrs) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.attrs == *attrs && last.attrs.field.is_none() => last.text.push_str(text),
        _ => runs.push(Run { text: text.to_owned(), attrs: attrs.clone() }),
    }
}

/// Children named `name`, also inside content controls (`w:sdt`), which
/// Word allows around table rows and cells.
fn children_through_sdt<'a>(e: &'a Element, name: &str, out: &mut Vec<&'a Element>, depth: usize) {
    for child in e.elements() {
        if child.name == name {
            out.push(child);
        } else if child.name == "w:sdt" && depth < 8 {
            if let Some(content) = child.child("w:sdtContent") {
                children_through_sdt(content, name, out, depth + 1);
            }
        }
    }
}

fn rows_of(tbl: &Element) -> Vec<&Element> {
    let mut out = Vec::new();
    children_through_sdt(tbl, "w:tr", &mut out, 0);
    out
}

fn cells_of(tr: &Element) -> Vec<&Element> {
    let mut out = Vec::new();
    children_through_sdt(tr, "w:tc", &mut out, 0);
    out
}

/// The text boxes (`w:txbxContent`) anchored in a paragraph. Word stores
/// each box twice (a DrawingML choice and a VML fallback): fallbacks are
/// skipped so the text appears once.
fn text_boxes<'a>(e: &'a Element, out: &mut Vec<&'a Element>, depth: usize) {
    if depth > 24 {
        return;
    }
    for child in e.elements() {
        if child.name == "w:txbxContent" {
            out.push(child);
        } else if !child.name.ends_with("}Fallback") && child.name != "w:p" {
            text_boxes(child, out, depth + 1);
        }
    }
}

/// Marks a page break inside a paragraph's runs (split afterwards).
const PAGE_MARK: &str = "\u{c}";

impl Importer<'_> {
    /// A picture (or Mermaid diagram) from a `w:drawing`.
    fn picture_block(&self, drawing: &Element, align: Align) -> Option<Block> {
        let frame = drawing.child("wp:inline").or_else(|| drawing.child("wp:anchor"))?;
        let extent = frame.child("wp:extent");
        let size = |a: &str| extent.and_then(|e| e.attr(a)).and_then(|v| v.parse::<f32>().ok()).map(|v| v / EMU).filter(|v| v.is_finite() && *v > 0.0);
        let (width, height) = (size("cx"), size("cy"));
        let doc_pr = frame.child("wp:docPr");
        let name = doc_pr.and_then(|d| d.attr("name")).unwrap_or("");
        let descr = doc_pr.and_then(|d| d.attr("descr")).unwrap_or("");
        if name.starts_with(office_draw::odg::DIAGRAM_NAME) && !descr.trim().is_empty() {
            let mut d = Diagram { source: descr.to_owned(), width: width.unwrap_or(300.0), align };
            d.normalize();
            return Some(Block::Diagram(d));
        }
        let blip = find_descendant(frame, "a:blip", 0)?;
        let svg = find_descendant(blip, "asvg:svgBlip", 0).and_then(|b| b.attr("r:embed"));
        let image = [svg, blip.attr("r:embed")].into_iter().flatten().find_map(|id| {
            let rel = self.rels.get(id).filter(|r| !r.external)?;
            office_draw::Image::from_bytes(self.package.get(&rel.target)?.to_vec()).ok()
        })?;
        let mut p = Picture::new(image, 480.0);
        if let (Some(w), Some(h)) = (width, height) {
            p.width = w;
            p.height = h;
        }
        p.align = align;
        p.description = descr.to_owned();
        p.normalize();
        Some(Block::Image(p))
    }

    fn count(&mut self) -> Result<()> {
        self.blocks += 1;
        if self.blocks > self.limits.max_blocks {
            return Err(OdfError::limit(format!("document has more than {} paragraphs", self.limits.max_blocks)));
        }
        Ok(())
    }

    /// A style with its `basedOn` chain resolved (bounded against loops).
    /// With `skip_default`, the chain stops at the default paragraph style.
    fn style_chain(&self, id: Option<&str>, skip_default: bool) -> (RunProps, ParaProps, Vec<String>) {
        let mut chain = Vec::new();
        let mut current = id.map(str::to_owned);
        while let Some(name) = current {
            if chain.len() >= 16 {
                break;
            }
            let Some(style) = self.styles.get(&name) else { break };
            if skip_default && style.default {
                break;
            }
            chain.push(style);
            current = style.based_on.clone();
        }
        let (mut run, mut para) = (RunProps::default(), ParaProps::default());
        let mut names = Vec::new();
        for style in chain.iter().rev() {
            run.over(&style.run);
            let p = &style.para;
            para.align = p.align.or(para.align);
            para.indent = p.indent.or(para.indent);
            para.space_before = p.space_before.or(para.space_before);
            para.outline = p.outline.or(para.outline);
            para.num = p.num.clone().or(para.num.take());
            para.page_break_before |= p.page_break_before;
        }
        for style in &chain {
            names.push(style.name.clone());
        }
        (run, para, names)
    }

    fn kind_from_style(names: &[String], outline: Option<u8>) -> Option<ParaKind> {
        for name in names {
            if let Some(level) = name.strip_prefix("heading ").and_then(|l| l.trim().parse::<u8>().ok()) {
                return Some(ParaKind::Heading { level });
            }
            match name.as_str() {
                "title" => return Some(ParaKind::Title),
                "quote" | "intense quote" | "block text" => return Some(ParaKind::Quote),
                "html preformatted" | "plain text" | "code" | "source code" => return Some(ParaKind::Code),
                _ => {}
            }
        }
        outline.filter(|l| *l < 6).map(|l| ParaKind::Heading { level: l + 1 })
    }

    /// Collects runs of a paragraph-like element.
    fn inline(&self, e: &Element, base: &TextAttrs, runs: &mut Vec<Run>, field: &mut FieldState, depth: usize) {
        if depth > 32 {
            return;
        }
        for child in e.elements() {
            match child.name.as_str() {
                "w:r" => self.run(child, base, runs, field),
                "w:hyperlink" => {
                    let mut a = base.clone();
                    a.link = child
                        .attr("r:id")
                        .and_then(|id| self.rels.get(id))
                        .filter(|r| r.external)
                        .and_then(|r| sanitize_link(&r.target));
                    self.inline(child, &a, runs, field, depth + 1);
                }
                "w:fldSimple" => {
                    let kind = child.attr("w:instr").and_then(field_of);
                    let mut inner = Vec::new();
                    let mut nested = FieldState::default();
                    self.inline(child, base, &mut inner, &mut nested, depth + 1);
                    match kind {
                        Some(f) => {
                            let text: String = inner.iter().map(|r| r.text.as_str()).collect();
                            let attrs = TextAttrs { field: Some(f), ..inner.first().map(|r| r.attrs.clone()).unwrap_or_else(|| base.clone()) };
                            let text = if text.trim().is_empty() { "1".to_owned() } else { text.chars().take(16).collect() };
                            runs.push(Run { text, attrs });
                        }
                        None => {
                            for r in inner {
                                push_text(runs, &r.text, &r.attrs);
                            }
                        }
                    }
                }
                // Tracked insertions and moved-to text are part of the
                // document; deletions and moved-from text are not.
                "w:ins" | "w:moveTo" | "w:smartTag" | "w:customXml" | "w:dir" | "w:bdo" => {
                    self.inline(child, base, runs, field, depth + 1)
                }
                "w:sdt" => {
                    if let Some(content) = child.child("w:sdtContent") {
                        self.inline(content, base, runs, field, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn run(&self, r: &Element, base: &TextAttrs, runs: &mut Vec<Run>, field: &mut FieldState) {
        let rpr = r.child("w:rPr");
        let mut attrs = base.clone();
        let style = rpr.and_then(|p| p.child("w:rStyle")).and_then(|s| s.attr("w:val"));
        // Link styling comes from the link itself in Akshara.
        let link_style = matches!(style, Some("Hyperlink" | "FollowedHyperlink")) && base.link.is_some();
        if style.is_some() && !link_style {
            self.style_chain(style, false).0.apply(&mut attrs);
        }
        parse_rpr(rpr).apply(&mut attrs);
        let emit = |text: &str, runs: &mut Vec<Run>, field: &mut FieldState| {
            if field.depth > 0 && !field.in_result {
                return; // instruction part
            }
            if field.depth > 0 && field.field.is_some() {
                if field.result.is_empty() {
                    field.attrs = attrs.clone();
                }
                field.result.push_str(text);
            } else {
                push_text(runs, text, &attrs);
            }
        };
        for c in r.elements() {
            match c.name.as_str() {
                "w:t" => emit(&c.text_content(), runs, field),
                "w:tab" | "w:ptab" => emit("\t", runs, field),
                "w:br" => match c.attr("w:type") {
                    Some("page") => emit(PAGE_MARK, runs, field),
                    _ => emit("\n", runs, field),
                },
                "w:cr" => emit("\n", runs, field),
                // Ruby (furigana): the base text; the reading aid above it
                // is not modelled.
                "w:ruby" => {
                    if let Some(base_text) = c.child("w:rubyBase") {
                        for r in base_text.children_named("w:r") {
                            self.run(r, &attrs, runs, field);
                        }
                    }
                }
                "w:noBreakHyphen" => emit("\u{2011}", runs, field),
                "w:sym" => {
                    let ch = c.attr("w:char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32);
                    // Symbol-font code points (F0xx) mean nothing outside that font.
                    if let Some(ch) = ch.filter(|c| !('\u{F000}'..='\u{F0FF}').contains(c)) {
                        emit(&ch.to_string(), runs, field);
                    }
                }
                "w:instrText" => {
                    if field.depth == 1 && !field.in_result {
                        field.instr.push_str(&c.text_content());
                    }
                }
                "w:fldChar" => match c.attr("w:fldCharType") {
                    Some("begin") => {
                        field.depth += 1;
                        if field.depth == 1 {
                            *field = FieldState { depth: 1, ..Default::default() };
                        }
                    }
                    Some("separate") if field.depth == 1 => {
                        field.in_result = true;
                        field.field = field_of(&field.instr);
                    }
                    Some("end") if field.depth > 0 => {
                        field.depth -= 1;
                        if field.depth == 0 {
                            if let Some(f) = field.field.take() {
                                let text = if field.result.trim().is_empty() { "1".to_owned() } else { field.result.chars().take(16).collect() };
                                let attrs = TextAttrs { field: Some(f), ..field.attrs.clone() };
                                runs.push(Run { text, attrs });
                            }
                            *field = FieldState::default();
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// A `w:p`: one or more paragraphs (split at page breaks) and breaks.
    fn paragraph(&mut self, p: &Element, out: &mut Vec<Block>) -> Result<()> {
        self.count()?;
        let ppr = p.child("w:pPr");
        let style_id = ppr.and_then(|e| e.child("w:pStyle")).and_then(|e| e.attr("w:val"));
        let (_, style_para, names) = self.style_chain(style_id, false);
        let own = parse_ppr(ppr);
        let outline = own.outline.or(style_para.outline);
        let num = own.num.clone().or(style_para.num.clone()).filter(|(id, _)| id != "0");
        let kind = match &num {
            Some((id, level)) => {
                let bullet = self.numbering.get(id).and_then(|l| l.get(usize::from(*level))).copied().unwrap_or(true);
                if bullet { ParaKind::Bullet { level: *level } } else { ParaKind::Numbered { level: *level } }
            }
            None => Self::kind_from_style(&names, outline).unwrap_or(ParaKind::Normal),
        };
        // Character formatting of the paragraph style applies to body text
        // (headings and titles get their look from the paragraph kind).
        let mut base = TextAttrs::default();
        if kind == ParaKind::Normal || matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. }) {
            self.style_chain(style_id, true).0.apply(&mut base);
        }
        let mut runs = Vec::new();
        let mut field = FieldState::default();
        self.inline(p, &base, &mut runs, &mut field, 0);
        if kind == ParaKind::Code {
            for r in &mut runs {
                r.attrs.code = false;
            }
        }
        // Headings, titles, quotations and code get their look from the kind;
        // only body text takes layout from its (custom) paragraph style.
        let is_list = matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. });
        let style_layout = if kind == ParaKind::Normal { style_para.clone() } else { ParaProps::default() };
        let align = own.align.or(style_layout.align).unwrap_or_default();
        let indent = if is_list { 0.0 } else { own.indent.or(style_layout.indent).unwrap_or(0.0) };
        let space_before = own.space_before.or(style_layout.space_before).unwrap_or(0.0);
        if (own.page_break_before || style_para.page_break_before) && !out.is_empty() {
            out.push(Block::PageBreak);
        }
        // Split at page breaks inside the paragraph.
        let mut pieces: Vec<Vec<Run>> = vec![Vec::new()];
        for run in runs {
            let mut parts = run.text.split(PAGE_MARK).peekable();
            while let Some(part) = parts.next() {
                if let (false, Some(piece)) = (part.is_empty(), pieces.last_mut()) {
                    piece.push(Run { text: part.to_owned(), attrs: run.attrs.clone() });
                }
                if parts.peek().is_some() {
                    pieces.push(Vec::new());
                }
            }
        }
        let count = pieces.len();
        // An empty body paragraph holding only a page break is the break.
        if count == 2 && kind == ParaKind::Normal && pieces.iter().all(Vec::is_empty) {
            out.push(Block::PageBreak);
            return Ok(());
        }
        let mut found = Vec::new();
        for run in p.elements() {
            if run.name != "w:pPr" {
                drawings(run, &mut found, 0);
            }
        }
        let pictures: Vec<Block> = found.iter().filter_map(|d| self.picture_block(d, align)).collect();
        // A paragraph that only holds pictures becomes just the pictures.
        let only_pictures = !pictures.is_empty() && count == 1 && kind == ParaKind::Normal && pieces[0].iter().all(|r| r.text.trim().is_empty());
        let pieces = if only_pictures { Vec::new() } else { pieces };
        for (i, piece) in pieces.into_iter().enumerate() {
            if i > 0 {
                out.push(Block::PageBreak);
                // A break at the very end leaves no text after it.
                if i == count - 1 && piece.is_empty() {
                    break;
                }
            }
            let mut para = Paragraph {
                kind: kind.normalized(),
                align,
                indent,
                space_before: if i == 0 { space_before } else { 0.0 },
                runs: piece,
            };
            para.normalize();
            out.push(Block::Paragraph(para));
        }
        for picture in pictures {
            self.count()?;
            out.push(picture);
        }
        // Text boxes anchored here: their paragraphs follow this one.
        if !self.in_text_box {
            let mut boxes = Vec::new();
            for run in p.elements() {
                if run.name != "w:pPr" {
                    text_boxes(run, &mut boxes, 0);
                }
            }
            self.in_text_box = true;
            let mut paras = Vec::new();
            let result = boxes.iter().try_for_each(|b| self.flat_paragraphs(b, &mut paras, 0));
            self.in_text_box = false;
            result?;
            out.extend(paras.into_iter().filter(|p| !p.text().trim().is_empty()).map(Block::Paragraph));
        }
        // A section break inside the paragraph starts a new page.
        if let Some(sect) = ppr.and_then(|e| e.child("w:sectPr")) {
            let continuous = sect.child("w:type").and_then(|t| t.attr("w:val")) == Some("continuous");
            if !continuous {
                out.push(Block::PageBreak);
            }
        }
        Ok(())
    }

    /// Paragraphs of a container where tables are flattened (table cells,
    /// headers and footers).
    fn flat_paragraphs(&mut self, e: &Element, out: &mut Vec<Paragraph>, depth: usize) -> Result<()> {
        if depth > 16 {
            return Ok(());
        }
        for child in e.elements() {
            match child.name.as_str() {
                "w:p" => {
                    let mut blocks = Vec::new();
                    self.paragraph(child, &mut blocks)?;
                    out.extend(blocks.into_iter().filter_map(|b| match b {
                        Block::Paragraph(p) => Some(p),
                        _ => None,
                    }));
                }
                "w:tbl" => {
                    for row in rows_of(child) {
                        for cell in cells_of(row) {
                            self.flat_paragraphs(cell, out, depth + 1)?;
                        }
                    }
                }
                "w:sdt" => {
                    if let Some(c) = child.child("w:sdtContent") {
                        self.flat_paragraphs(c, out, depth + 1)?;
                    }
                }
                "w:customXml" => self.flat_paragraphs(child, out, depth + 1)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn table(&mut self, tbl: &Element) -> Result<Table> {
        let grid: Vec<Option<f32>> = tbl
            .child("w:tblGrid")
            .map(|g| g.children_named("w:gridCol").map(|c| twips(c.attr("w:w")).filter(|w| *w > 0.0)).collect())
            .unwrap_or_default();
        let tbl_pr = tbl.child("w:tblPr");
        // Full-width percentage tables share the width (no fixed columns).
        let full_width = tbl_pr.and_then(|p| p.child("w:tblW")).is_some_and(|w| w.attr("w:type") == Some("pct"));
        let grid = if full_width { Vec::new() } else { grid };
        let borders = match tbl_pr.and_then(|p| p.child("w:tblBorders")) {
            Some(b) => b.elements().any(|e| !matches!(e.attr("w:val"), Some("nil" | "none"))),
            // Without explicit borders, a table style (e.g. "Table Grid")
            // usually draws them; Word's plain "Normal Table" does not.
            None => tbl_pr.and_then(|p| p.child("w:tblStyle")).is_some(),
        };
        let mut rows = Vec::new();
        let mut heights = Vec::new();
        for tr in rows_of(tbl) {
            self.count()?;
            let height = tr
                .child("w:trPr")
                .and_then(|p| p.child("w:trHeight"))
                .filter(|h| h.attr("w:hRule") == Some("exact"))
                .and_then(|h| twips(h.attr("w:val")));
            let mut cells = Vec::new();
            for tc in cells_of(tr) {
                let pr = tc.child("w:tcPr");
                let span = pr
                    .and_then(|p| p.child("w:gridSpan"))
                    .and_then(|g| g.attr("w:val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                // A vertically merged continuation cell shows nothing.
                let continued = pr.and_then(|p| p.child("w:vMerge")).is_some_and(|v| v.attr("w:val") != Some("restart"));
                let mut paras = Vec::new();
                if !continued {
                    self.flat_paragraphs(tc, &mut paras, 0)?;
                }
                cells.push(paras);
                for _ in 1..span {
                    cells.push(Vec::new());
                }
            }
            cells.truncate(64);
            rows.push(cells);
            heights.push(height);
        }
        let all = |v: Vec<Option<f32>>| v.into_iter().collect::<Option<Vec<f32>>>().unwrap_or_default();
        let mut table = Table { rows, col_widths: all(grid), row_heights: all(heights), borders };
        table.normalize();
        Ok(table)
    }

    fn body(&mut self, parent: &Element, out: &mut Vec<Block>, depth: usize) -> Result<()> {
        if depth > 16 {
            return Ok(());
        }
        for child in parent.elements() {
            match child.name.as_str() {
                "w:p" => self.paragraph(child, out)?,
                "w:tbl" => {
                    let t = self.table(child)?;
                    out.push(Block::Table(t));
                }
                "w:sdt" => {
                    if let Some(c) = child.child("w:sdtContent") {
                        self.body(c, out, depth + 1)?;
                    }
                }
                "w:customXml" | "w:ins" | "w:moveTo" => self.body(child, out, depth + 1)?,
                _ => {}
            }
        }
        Ok(())
    }
}

fn read_styles(xml: &Element) -> HashMap<String, Style> {
    let mut out = HashMap::new();
    for s in xml.children_named("w:style") {
        let Some(id) = s.attr("w:styleId") else { continue };
        out.insert(
            id.to_owned(),
            Style {
                name: s.child("w:name").and_then(|n| n.attr("w:val")).unwrap_or(id).to_lowercase(),
                based_on: s.child("w:basedOn").and_then(|b| b.attr("w:val")).map(str::to_owned),
                run: parse_rpr(s.child("w:rPr")),
                para: parse_ppr(s.child("w:pPr")),
                default: s.attr("w:type") == Some("paragraph") && matches!(s.attr("w:default"), Some("1" | "true")),
            },
        );
    }
    out
}

fn read_numbering(xml: &Element) -> HashMap<String, Vec<bool>> {
    let mut abstracts: HashMap<&str, Vec<bool>> = HashMap::new();
    for a in xml.children_named("w:abstractNum") {
        let Some(id) = a.attr("w:abstractNumId") else { continue };
        let mut levels = vec![true; 10];
        for lvl in a.children_named("w:lvl") {
            let index = lvl.attr("w:ilvl").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let fmt = lvl.child("w:numFmt").and_then(|f| f.attr("w:val")).unwrap_or("bullet");
            if index < 10 {
                levels[index] = matches!(fmt, "bullet" | "none");
            }
        }
        abstracts.insert(id, levels);
    }
    let mut out = HashMap::new();
    for n in xml.children_named("w:num") {
        let (Some(id), Some(abs)) = (n.attr("w:numId"), n.child("w:abstractNumId").and_then(|a| a.attr("w:val"))) else {
            continue;
        };
        if let Some(levels) = abstracts.get(abs) {
            out.insert(id.to_owned(), levels.clone());
        }
    }
    out
}

/// Loads a document from the bytes of a `.docx` file.
pub fn load_docx(bytes: &[u8], limits: &Limits) -> Result<Document> {
    ooxml::reject_binary_office(bytes)?;
    let package = Package::read_filtered(bytes, limits, |n| {
        (n.starts_with("word/") && (n.ends_with(".xml") || n.ends_with(".rels")))
            || n.starts_with("word/media/")
            || n == "_rels/.rels"
            || n == "docProps/core.xml"
    })?;
    let main = ooxml::main_part(&package, "word/document.xml", limits)?;
    let document = package
        .get_xml(&main, limits)?
        .ok_or_else(|| OdfError::invalid("not a Word document (no word/document.xml)"))?;
    let body = document.child("w:body").ok_or_else(|| OdfError::invalid("Word document has no body"))?;
    let rels = ooxml::relationships(&package, &main, limits)?;
    let part = |kind: &str| rels.values().find(|r| r.kind.ends_with(kind) && !r.external).map(|r| r.target.clone());
    let styles = match part("/styles") {
        Some(p) => package.get_xml(&p, limits)?.map(|x| read_styles(&x)).unwrap_or_default(),
        None => HashMap::new(),
    };
    let numbering = match part("/numbering") {
        Some(p) => package.get_xml(&p, limits)?.map(|x| read_numbering(&x)).unwrap_or_default(),
        None => HashMap::new(),
    };
    let mut importer = Importer { limits, styles, numbering, rels: rels.clone(), blocks: 0, in_text_box: false, package: &package };
    let mut blocks = Vec::new();
    importer.body(body, &mut blocks, 0)?;
    // Drop a trailing page break left by the last section.
    while matches!(blocks.last(), Some(Block::PageBreak)) {
        blocks.pop();
    }

    // Page size, margin, header and footer from the last section.
    let mut page = PageSetup::default();
    let (mut header, mut footer) = (Vec::new(), Vec::new());
    if let Some(sect) = body.child("w:sectPr") {
        if let Some(size) = sect.child("w:pgSz") {
            if let (Some(w), Some(h)) = (twips(size.attr("w:w")), twips(size.attr("w:h"))) {
                page.width = w;
                page.height = h;
            }
        }
        if let Some(m) = sect.child("w:pgMar").and_then(|m| twips(m.attr("w:left").or_else(|| m.attr("w:start")))) {
            page.margin = m;
        }
        for (element, out) in [("w:headerReference", &mut header), ("w:footerReference", &mut footer)] {
            let reference = sect
                .children_named(element)
                .find(|r| r.attr("w:type").is_none_or(|t| t == "default"))
                .and_then(|r| r.attr("r:id"))
                .and_then(|id| rels.get(id));
            if let Some(rel) = reference {
                let part_rels = ooxml::relationships(&package, &rel.target, limits)?;
                if let Some(xml) = package.get_xml(&rel.target, limits)? {
                    let saved = std::mem::replace(&mut importer.rels, part_rels);
                    importer.flat_paragraphs(&xml, out, 0)?;
                    importer.rels = saved;
                }
            }
        }
    }
    // Empty header or footer paragraphs only take space.
    for area in [&mut header, &mut footer] {
        if area.iter().all(|p| p.text().trim().is_empty()) {
            area.clear();
        }
    }

    let mut doc = Document::from_blocks(blocks).with_page(page, header, footer);
    if let Some(core) = package.get_xml("docProps/core.xml", limits)? {
        let text = |name: &str| core.child(name).map(Element::text_content).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
        doc.meta = Metadata { title: text("dc:title"), subject: text("dc:subject"), creator: text("dc:creator"), ..Default::default() };
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DOC_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WML: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

/// Points to twentieths of a point.
fn tw(pt: f32) -> String {
    format!("{}", (pt * 20.0).round() as i64)
}

fn hex(c: Rgb) -> String {
    c.to_hex().trim_start_matches('#').to_uppercase()
}

struct Writer {
    /// External link targets, by relationship id order (rIdL1...).
    links: Vec<String>,
    /// Pictures: (relationship id, file name in word/media, bytes).
    media: Vec<(String, String, Vec<u8>)>,
    /// Pictures written (for unique drawing ids).
    pictures: usize,
    /// numId of each numbered list (all use abstract numbering 1).
    numbered_lists: usize,
    /// numId for the numbered list being written.
    current_list: Option<usize>,
}

const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const SVG_EXT: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";

impl Writer {
    fn add_media(&mut self, ext: &str, bytes: Vec<u8>) -> String {
        let id = format!("rIdM{}", self.media.len() + 1);
        let name = format!("image{}.{ext}", self.media.len() + 1);
        self.media.push((id.clone(), name, bytes));
        id
    }

    /// An inline picture in its own paragraph; `svg` adds the vector
    /// version (Word 2016 and later show it, others the raster).
    #[allow(clippy::too_many_arguments)]
    fn picture(&mut self, w: &mut XmlWriter, page_break: bool, align: Align, width: f32, height: f32, name: &str, descr: &str, raster: (Vec<u8>, &str), svg: Option<Vec<u8>>) {
        self.pictures += 1;
        let n = self.pictures.to_string();
        let raster_id = self.add_media(raster.1, raster.0);
        let svg_id = svg.map(|b| self.add_media("svg", b));
        let (cx, cy) = (((width * EMU) as i64).to_string(), ((height * EMU) as i64).to_string());
        w.start("w:p");
        w.start("w:pPr");
        if page_break {
            w.start("w:pageBreakBefore").end();
        }
        w.start("w:jc").attr("w:val", match align {
            Align::Left | Align::Justify => "left",
            Align::Center => "center",
            Align::Right => "right",
        }).end();
        w.end();
        w.start("w:r").start("w:drawing");
        w.start("wp:inline").attr("xmlns:wp", WP_NS).attr("distT", "0").attr("distB", "0").attr("distL", "0").attr("distR", "0");
        w.start("wp:extent").attr("cx", &cx).attr("cy", &cy).end();
        w.start("wp:docPr").attr("id", &n).attr("name", &format!("{name} {n}"));
        if !descr.is_empty() {
            w.attr("descr", descr);
        }
        w.end();
        w.start("wp:cNvGraphicFramePr").start("a:graphicFrameLocks").attr("xmlns:a", A_NS).attr("noChangeAspect", "1").end().end();
        w.start("a:graphic").attr("xmlns:a", A_NS);
        w.start("a:graphicData").attr("uri", PIC_NS);
        w.start("pic:pic").attr("xmlns:pic", PIC_NS);
        w.start("pic:nvPicPr");
        w.start("pic:cNvPr").attr("id", &n).attr("name", &format!("{name} {n}")).end();
        w.start("pic:cNvPicPr").end();
        w.end();
        w.start("pic:blipFill");
        w.start("a:blip").attr("r:embed", &raster_id);
        if let Some(svg_id) = &svg_id {
            w.start("a:extLst").start("a:ext").attr("uri", SVG_EXT);
            w.start("asvg:svgBlip").attr("xmlns:asvg", "http://schemas.microsoft.com/office/drawing/2016/SVG/main").attr("r:embed", svg_id).end();
            w.end().end();
        }
        w.end();
        w.start("a:stretch").start("a:fillRect").end().end();
        w.end();
        w.start("pic:spPr");
        w.start("a:xfrm").start("a:off").attr("x", "0").attr("y", "0").end().start("a:ext").attr("cx", &cx).attr("cy", &cy).end().end();
        w.start("a:prstGeom").attr("prst", "rect").start("a:avLst").end().end();
        w.end();
        w.end(); // pic:pic
        w.end().end(); // graphicData, graphic
        w.end(); // wp:inline
        w.end().end(); // w:drawing, w:r
        w.end(); // w:p
    }

    fn link_id(&mut self, target: &str) -> String {
        let i = self.links.iter().position(|l| l == target).unwrap_or_else(|| {
            self.links.push(target.to_owned());
            self.links.len() - 1
        });
        format!("rIdL{}", i + 1)
    }

    fn run_props(w: &mut XmlWriter, a: &TextAttrs) {
        let any = a.bold || a.italic || a.underline || a.strike || a.code || a.superscript || a.subscript
            || a.color.is_some() || a.highlight.is_some() || a.size.is_some() || a.link.is_some();
        if !any {
            return;
        }
        // Children in the order the schema requires.
        w.start("w:rPr");
        if a.link.is_some() {
            w.start("w:rStyle").attr("w:val", "Hyperlink").end();
        }
        if a.code {
            w.start("w:rFonts").attr("w:ascii", "Courier New").attr("w:hAnsi", "Courier New").attr("w:cs", "Courier New").end();
        }
        if a.bold {
            w.start("w:b").end();
        }
        if a.italic {
            w.start("w:i").end();
        }
        if a.strike {
            w.start("w:strike").end();
        }
        if let Some(c) = a.color {
            w.start("w:color").attr("w:val", &hex(c)).end();
        }
        if let Some(size) = a.size {
            let half = format!("{}", (size * 2.0).round() as i64);
            w.start("w:sz").attr("w:val", &half).end();
            w.start("w:szCs").attr("w:val", &half).end();
        }
        if a.underline {
            w.start("w:u").attr("w:val", "single").end();
        }
        if let Some(h) = a.highlight {
            w.start("w:shd").attr("w:val", "clear").attr("w:color", "auto").attr("w:fill", &hex(h)).end();
        }
        if a.superscript {
            w.start("w:vertAlign").attr("w:val", "superscript").end();
        } else if a.subscript {
            w.start("w:vertAlign").attr("w:val", "subscript").end();
        }
        w.end();
    }

    fn run(w: &mut XmlWriter, run: &Run) {
        if let Some(field) = run.attrs.field {
            let instr = match field {
                Field::PageNumber => " PAGE ",
                Field::PageCount => " NUMPAGES ",
            };
            w.start("w:fldSimple").attr("w:instr", instr).start("w:r");
            Self::run_props(w, &run.attrs);
            w.start("w:t").text(&run.text).end();
            w.end().end();
            return;
        }
        w.start("w:r");
        Self::run_props(w, &run.attrs);
        let mut first = true;
        for line in run.text.split('\n') {
            if !first {
                w.start("w:br").end();
            }
            first = false;
            let mut first_tab = true;
            for part in line.split('\t') {
                if !first_tab {
                    w.start("w:tab").end();
                }
                first_tab = false;
                if !part.is_empty() {
                    w.start("w:t").attr("xml:space", "preserve").text(part).end();
                }
            }
        }
        w.end();
    }

    fn runs(&mut self, w: &mut XmlWriter, p: &Paragraph) {
        let mut i = 0;
        while i < p.runs.len() {
            let run = &p.runs[i];
            if run.text.is_empty() {
                i += 1;
                continue;
            }
            match run.attrs.link.as_deref().and_then(sanitize_link) {
                Some(link) => {
                    // Consecutive runs with the same target share one link.
                    let id = self.link_id(&link);
                    w.start("w:hyperlink").attr("r:id", &id);
                    while i < p.runs.len() && p.runs[i].attrs.link.as_deref() == run.attrs.link.as_deref() {
                        Self::run(w, &p.runs[i]);
                        i += 1;
                    }
                    w.end();
                }
                None => {
                    Self::run(w, run);
                    i += 1;
                }
            }
        }
    }

    fn paragraph(&mut self, w: &mut XmlWriter, p: &Paragraph, break_before: bool) {
        w.start("w:p");
        let style = match p.kind {
            ParaKind::Heading { level } => Some(format!("Heading{}", level.clamp(1, 6))),
            ParaKind::Title => Some("Title".into()),
            ParaKind::Quote => Some("Quote".into()),
            ParaKind::Code => Some("HTMLPreformatted".into()),
            _ => None,
        };
        let num = match p.kind {
            ParaKind::Bullet { level } => Some((1, level)),
            ParaKind::Numbered { level } => {
                let id = *self.current_list.get_or_insert_with(|| {
                    self.numbered_lists += 1;
                    1 + self.numbered_lists
                });
                Some((id, level))
            }
            _ => None,
        };
        let has_props = style.is_some() || break_before || num.is_some() || p.space_before > 0.0 || p.indent > 0.0 || p.align != Align::Left;
        if has_props {
            w.start("w:pPr");
            if let Some(style) = style {
                w.start("w:pStyle").attr("w:val", &style).end();
            }
            if break_before {
                w.start("w:pageBreakBefore").end();
            }
            if let Some((id, level)) = num {
                w.start("w:numPr");
                w.start("w:ilvl").attr("w:val", &level.min(8).to_string()).end();
                w.start("w:numId").attr("w:val", &id.to_string()).end();
                w.end();
            }
            if p.space_before > 0.0 {
                w.start("w:spacing").attr("w:before", &tw(p.space_before)).end();
            }
            if p.indent > 0.0 && num.is_none() {
                w.start("w:ind").attr("w:left", &tw(p.indent)).end();
            }
            let jc = match p.align {
                Align::Left => None,
                Align::Center => Some("center"),
                Align::Right => Some("right"),
                Align::Justify => Some("both"),
            };
            if let Some(jc) = jc {
                w.start("w:jc").attr("w:val", jc).end();
            }
            w.end();
        }
        self.runs(w, p);
        w.end();
    }

    fn table(&mut self, w: &mut XmlWriter, t: &Table, text_width: f32) {
        let cols = t.cols().max(1);
        let fixed = t.col_widths.len() == cols;
        let widths: Vec<f32> = if fixed { t.col_widths.clone() } else { vec![text_width / cols as f32; cols] };
        w.start("w:tbl").start("w:tblPr");
        if t.borders {
            w.start("w:tblStyle").attr("w:val", "TableGrid").end();
        }
        if fixed {
            w.start("w:tblW").attr("w:w", &tw(widths.iter().sum())).attr("w:type", "dxa").end();
        } else {
            w.start("w:tblW").attr("w:w", "5000").attr("w:type", "pct").end();
        }
        if !t.borders {
            w.start("w:tblBorders");
            for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
                w.start(&format!("w:{side}")).attr("w:val", "nil").end();
            }
            w.end();
        }
        if fixed {
            w.start("w:tblLayout").attr("w:type", "fixed").end();
        }
        w.end();
        w.start("w:tblGrid");
        for width in &widths {
            w.start("w:gridCol").attr("w:w", &tw(*width)).end();
        }
        w.end();
        for (r, row) in t.rows.iter().enumerate() {
            w.start("w:tr");
            if let Some(h) = t.row_heights.get(r) {
                w.start("w:trPr").start("w:trHeight").attr("w:val", &tw(*h)).attr("w:hRule", "exact").end().end();
            }
            for (c, cell) in row.iter().enumerate() {
                w.start("w:tc").start("w:tcPr").start("w:tcW").attr("w:w", &tw(widths[c.min(widths.len() - 1)])).attr("w:type", "dxa").end().end();
                self.current_list = None;
                for p in cell {
                    self.paragraph(w, p, false);
                }
                if cell.is_empty() {
                    w.start("w:p").end();
                }
                w.end();
            }
            w.end();
        }
        w.end();
    }

    /// A header or footer part and its link targets (each part has its
    /// own relationships).
    fn area(&mut self, root: &str, paragraphs: &[Paragraph]) -> (String, Vec<String>) {
        let saved = std::mem::take(&mut self.links);
        let mut w = XmlWriter::new();
        w.start(root).attr("xmlns:w", W_NS).attr("xmlns:r", R_NS);
        self.current_list = None;
        for p in paragraphs {
            self.paragraph(&mut w, p, false);
        }
        let links = std::mem::replace(&mut self.links, saved);
        (w.finish(), links)
    }
}

/// A relationships part listing styles/numbering/areas plus external links.
fn rels_xml(fixed: &[(&str, &str, &str)], links: &[String]) -> String {
    let mut w = XmlWriter::new();
    w.start("Relationships").attr("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships");
    for (id, kind, target) in fixed {
        w.start("Relationship").attr("Id", id).attr("Type", &format!("{DOC_REL}/{kind}")).attr("Target", target).end();
    }
    for (i, link) in links.iter().enumerate() {
        w.start("Relationship")
            .attr("Id", &format!("rIdL{}", i + 1))
            .attr("Type", &format!("{DOC_REL}/hyperlink"))
            .attr("Target", link)
            .attr("TargetMode", "External")
            .end();
    }
    w.finish()
}

fn styles_xml() -> String {
    let mut w = XmlWriter::new();
    w.start("w:styles").attr("xmlns:w", W_NS);
    w.start("w:docDefaults");
    w.start("w:rPrDefault").start("w:rPr");
    w.start("w:rFonts").attr("w:ascii", "Times New Roman").attr("w:hAnsi", "Times New Roman").attr("w:eastAsia", "Times New Roman").attr("w:cs", "Times New Roman").end();
    w.start("w:sz").attr("w:val", "24").end();
    w.start("w:szCs").attr("w:val", "24").end();
    w.end().end();
    w.start("w:pPrDefault").start("w:pPr").start("w:spacing").attr("w:after", "140").end().end().end();
    w.end();
    let style = |w: &mut XmlWriter, kind: &str, id: &str, name: &str, body: &dyn Fn(&mut XmlWriter)| {
        w.start("w:style").attr("w:type", kind).attr("w:styleId", id);
        if id == "Normal" {
            w.attr("w:default", "1");
        }
        w.start("w:name").attr("w:val", name).end();
        body(w);
        w.end();
    };
    style(&mut w, "paragraph", "Normal", "Normal", &|_| {});
    for (level, size) in [(1, 40), (2, 34), (3, 29), (4, 26), (5, 24), (6, 24)] {
        style(&mut w, "paragraph", &format!("Heading{level}"), &format!("heading {level}"), &|w| {
            w.start("w:basedOn").attr("w:val", "Normal").end();
            w.start("w:next").attr("w:val", "Normal").end();
            w.start("w:qFormat").end();
            w.start("w:pPr");
            w.start("w:keepNext").end();
            w.start("w:spacing").attr("w:before", "240").attr("w:after", "80").end();
            w.start("w:outlineLvl").attr("w:val", &(level - 1).to_string()).end();
            w.end();
            w.start("w:rPr");
            w.start("w:rFonts").attr("w:ascii", "Arial").attr("w:hAnsi", "Arial").end();
            w.start("w:b").end();
            if level >= 5 {
                w.start("w:i").end();
            }
            w.start("w:sz").attr("w:val", &size.to_string()).end();
            w.end();
        });
    }
    style(&mut w, "paragraph", "Title", "Title", &|w| {
        w.start("w:basedOn").attr("w:val", "Normal").end();
        w.start("w:next").attr("w:val", "Normal").end();
        w.start("w:qFormat").end();
        w.start("w:rPr");
        w.start("w:rFonts").attr("w:ascii", "Arial").attr("w:hAnsi", "Arial").end();
        w.start("w:b").end();
        w.start("w:sz").attr("w:val", "56").end();
        w.end();
    });
    style(&mut w, "paragraph", "Quote", "Quote", &|w| {
        w.start("w:basedOn").attr("w:val", "Normal").end();
        w.start("w:qFormat").end();
        w.start("w:pPr").start("w:ind").attr("w:left", "567").attr("w:right", "567").end().end();
        w.start("w:rPr").start("w:i").end().end();
    });
    style(&mut w, "paragraph", "HTMLPreformatted", "HTML Preformatted", &|w| {
        w.start("w:basedOn").attr("w:val", "Normal").end();
        w.start("w:pPr").start("w:spacing").attr("w:after", "0").end().end();
        w.start("w:rPr");
        w.start("w:rFonts").attr("w:ascii", "Courier New").attr("w:hAnsi", "Courier New").attr("w:cs", "Courier New").end();
        w.start("w:sz").attr("w:val", "20").end();
        w.end();
    });
    style(&mut w, "character", "Hyperlink", "Hyperlink", &|w| {
        w.start("w:rPr").start("w:color").attr("w:val", "0563C1").end().start("w:u").attr("w:val", "single").end().end();
    });
    style(&mut w, "table", "TableGrid", "Table Grid", &|w| {
        w.start("w:tblPr").start("w:tblBorders");
        for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
            w.start(&format!("w:{side}")).attr("w:val", "single").attr("w:sz", "4").attr("w:space", "0").attr("w:color", "auto").end();
        }
        w.end();
        w.start("w:tblCellMar");
        for (side, v) in [("left", "72"), ("right", "72")] {
            w.start(&format!("w:{side}")).attr("w:w", v).attr("w:type", "dxa").end();
        }
        w.end();
        w.end();
    });
    w.finish()
}

fn numbering_xml(numbered_lists: usize) -> String {
    let mut w = XmlWriter::new();
    w.start("w:numbering").attr("xmlns:w", W_NS);
    for (abstract_id, bullet) in [(0, true), (1, false)] {
        w.start("w:abstractNum").attr("w:abstractNumId", &abstract_id.to_string());
        w.start("w:multiLevelType").attr("w:val", "hybridMultilevel").end();
        for level in 0..9u32 {
            w.start("w:lvl").attr("w:ilvl", &level.to_string());
            w.start("w:start").attr("w:val", "1").end();
            w.start("w:numFmt").attr("w:val", if bullet { "bullet" } else { "decimal" }).end();
            let text = if bullet { ["•", "◦", "▪"][(level % 3) as usize].to_owned() } else { format!("%{}.", level + 1) };
            w.start("w:lvlText").attr("w:val", &text).end();
            w.start("w:lvlJc").attr("w:val", "left").end();
            w.start("w:pPr").start("w:ind").attr("w:left", &(360 * (level + 2)).to_string()).attr("w:hanging", "360").end().end();
            w.end();
        }
        w.end();
    }
    // Bullets share numId 1; every numbered list has its own numId so its
    // numbering starts at 1, as in the editor.
    w.start("w:num").attr("w:numId", "1").start("w:abstractNumId").attr("w:val", "0").end().end();
    for i in 0..numbered_lists {
        w.start("w:num").attr("w:numId", &(i + 2).to_string());
        w.start("w:abstractNumId").attr("w:val", "1").end();
        for level in 0..9 {
            w.start("w:lvlOverride").attr("w:ilvl", &level.to_string()).start("w:startOverride").attr("w:val", "1").end().end();
        }
        w.end();
    }
    w.finish()
}

fn core_xml(meta: &Metadata) -> String {
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

/// Serialises a document to the bytes of a `.docx` file.
pub fn save_docx(doc: &Document) -> Result<Vec<u8>> {
    let page = doc.page();
    let text_width = (page.width - 2.0 * page.margin).max(72.0);
    let mut out = Writer { links: Vec::new(), media: Vec::new(), pictures: 0, numbered_lists: 0, current_list: None };

    let mut w = XmlWriter::new();
    w.start("w:document").attr("xmlns:w", W_NS).attr("xmlns:r", R_NS);
    w.start("w:body");
    let mut break_next = false;
    for block in doc.blocks() {
        match block {
            Block::Paragraph(p) => {
                if !matches!(p.kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. }) {
                    out.current_list = None;
                }
                out.paragraph(&mut w, p, break_next);
                break_next = false;
            }
            Block::Table(t) => {
                out.current_list = None;
                if break_next {
                    w.start("w:p").start("w:r").start("w:br").attr("w:type", "page").end().end().end();
                    break_next = false;
                }
                out.table(&mut w, t, text_width);
                out.current_list = None;
            }
            Block::PageBreak => {
                out.current_list = None;
                break_next = true;
            }
            Block::Image(p) => {
                out.current_list = None;
                let (raster, svg) = if p.image.format().is_raster() {
                    ((p.image.bytes().to_vec(), p.image.format().extension()), None)
                } else {
                    let png = office_draw::render::svg_to_png(p.image.bytes(), 2.0, 4096, None)?;
                    ((png, "png"), Some(p.image.bytes().to_vec()))
                };
                out.picture(&mut w, break_next, p.align, p.width, p.height, "Picture", &p.description, raster, svg);
                break_next = false;
            }
            Block::Diagram(d) => {
                out.current_list = None;
                let fig = d.figure();
                let png = office_draw::render::figure_to_png(&fig, 2.0, None)?;
                let svg = fig.to_svg(None).into_bytes();
                out.picture(&mut w, break_next, d.align, d.width, d.height(), office_draw::odg::DIAGRAM_NAME, &d.source, (png, "png"), Some(svg));
                break_next = false;
            }
            Block::Drawing(c) => {
                out.current_list = None;
                let fig = crate::export::canvas_figure(c);
                let png = office_draw::render::figure_to_png(&fig, 2.0, None)?;
                let svg = fig.to_svg(None).into_bytes();
                out.picture(&mut w, break_next, c.align, c.width, c.height, "Drawing", "", (png, "png"), Some(svg));
                break_next = false;
            }
        }
    }
    if break_next {
        w.start("w:p").start("w:r").start("w:br").attr("w:type", "page").end().end().end();
    }
    let header = (!doc.header().is_empty()).then(|| out.area("w:hdr", doc.header()));
    let footer = (!doc.footer().is_empty()).then(|| out.area("w:ftr", doc.footer()));
    w.start("w:sectPr");
    if header.is_some() {
        w.start("w:headerReference").attr("w:type", "default").attr("r:id", "rIdH1").end();
    }
    if footer.is_some() {
        w.start("w:footerReference").attr("w:type", "default").attr("r:id", "rIdF1").end();
    }
    w.start("w:pgSz").attr("w:w", &tw(page.width)).attr("w:h", &tw(page.height));
    if page.is_landscape() {
        w.attr("w:orient", "landscape");
    }
    w.end();
    let m = tw(page.margin);
    w.start("w:pgMar")
        .attr("w:top", &m)
        .attr("w:right", &m)
        .attr("w:bottom", &m)
        .attr("w:left", &m)
        .attr("w:header", &tw((page.margin / 2.0).min(36.0)))
        .attr("w:footer", &tw((page.margin / 2.0).min(36.0)))
        .attr("w:gutter", "0")
        .end();
    w.end(); // sectPr
    let document = w.finish();

    let mut package = Package::new("");
    let mut types = XmlWriter::new();
    types.start("Types").attr("xmlns", "http://schemas.openxmlformats.org/package/2006/content-types");
    types.start("Default").attr("Extension", "rels").attr("ContentType", "application/vnd.openxmlformats-package.relationships+xml").end();
    types.start("Default").attr("Extension", "xml").attr("ContentType", "application/xml").end();
    let mut over = |part: &str, kind: &str| {
        types.start("Override").attr("PartName", part).attr("ContentType", kind).end();
    };
    over("/word/document.xml", &format!("{WML}.document.main+xml"));
    over("/word/styles.xml", &format!("{WML}.styles+xml"));
    over("/word/numbering.xml", &format!("{WML}.numbering+xml"));
    if header.is_some() {
        over("/word/header1.xml", &format!("{WML}.header+xml"));
    }
    if footer.is_some() {
        over("/word/footer1.xml", &format!("{WML}.footer+xml"));
    }
    over("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml");
    let mut exts: Vec<&str> = out.media.iter().filter_map(|(_, name, _)| name.rsplit_once('.').map(|(_, e)| e)).collect();
    exts.sort_unstable();
    exts.dedup();
    for ext in exts {
        let mime = match ext {
            "png" => "image/png",
            "jpg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "webp" => "image/webp",
            _ => "image/svg+xml",
        };
        types.start("Default").attr("Extension", ext).attr("ContentType", mime).end();
    }
    package.insert("[Content_Types].xml", types.finish().into_bytes())?;

    let mut rels = XmlWriter::new();
    rels.start("Relationships").attr("xmlns", "http://schemas.openxmlformats.org/package/2006/relationships");
    rels.start("Relationship").attr("Id", "rId1").attr("Type", &format!("{DOC_REL}/officeDocument")).attr("Target", "word/document.xml").end();
    rels.start("Relationship")
        .attr("Id", "rId2")
        .attr("Type", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties")
        .attr("Target", "docProps/core.xml")
        .end();
    package.insert("_rels/.rels", rels.finish().into_bytes())?;

    let mut fixed = vec![("rIdS1", "styles", "styles.xml"), ("rIdN1", "numbering", "numbering.xml")];
    if header.is_some() {
        fixed.push(("rIdH1", "header", "header1.xml"));
    }
    if footer.is_some() {
        fixed.push(("rIdF1", "footer", "footer1.xml"));
    }
    let media_rels: Vec<(String, String)> = out.media.iter().map(|(id, name, _)| (id.clone(), format!("media/{name}"))).collect();
    let mut fixed: Vec<(&str, &str, &str)> = fixed;
    for (id, target) in &media_rels {
        fixed.push((id, "image", target));
    }
    package.insert("word/_rels/document.xml.rels", rels_xml(&fixed, &out.links).into_bytes())?;
    for (_, name, bytes) in std::mem::take(&mut out.media) {
        package.insert(format!("word/media/{name}"), bytes)?;
    }
    package.insert("word/document.xml", document.into_bytes())?;
    package.insert("word/styles.xml", styles_xml().into_bytes())?;
    package.insert("word/numbering.xml", numbering_xml(out.numbered_lists).into_bytes())?;
    for (name, area) in [("header1.xml", header), ("footer1.xml", footer)] {
        if let Some((xml, links)) = area {
            package.insert(format!("word/{name}"), xml.into_bytes())?;
            if !links.is_empty() {
                package.insert(format!("word/_rels/{name}.rels"), rels_xml(&[], &links).into_bytes())?;
            }
        }
    }
    package.insert("docProps/core.xml", core_xml(&doc.meta).into_bytes())?;
    package.write_plain()
}
