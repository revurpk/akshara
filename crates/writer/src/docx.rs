//! Microsoft Word (`.docx`) import.
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
//! The text of text boxes follows the paragraph they are anchored in.
//! Images, footnotes, comments and other fields are not modelled; text
//! inside them is skipped (except field results, which are kept as text).
//! Documents are only read: saving goes to `.odt`.

use crate::document::Document;
use crate::model::*;
use office_odf::ooxml::{self, twips, Relationship};
use office_odf::xml::Element;
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
        if style.is_some() {
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
        let align = own.align.or(style_para.align).unwrap_or_default();
        let is_list = matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. });
        let indent = if is_list { 0.0 } else { own.indent.or(style_para.indent).unwrap_or(0.0) };
        let space_before = own.space_before.or(style_para.space_before).unwrap_or(0.0);
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
        (n.starts_with("word/") && (n.ends_with(".xml") || n.ends_with(".rels")) && !n.starts_with("word/media/"))
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
    let mut importer = Importer { limits, styles, numbering, rels: rels.clone(), blocks: 0, in_text_box: false };
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
