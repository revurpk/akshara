//! Plain text import, and plain text, HTML and Markdown export.
//!
//! HTML export escapes all text and attribute values, only emits links that
//! passed [`crate::model::sanitize_link`], adds
//! `rel="noopener noreferrer"`, and never emits scripts or event handlers, so
//! the output is safe to open in a browser whatever the document contained.

use crate::document::Document;
use crate::model::*;
use office_odf::{Limits, OdfError, Result};

/// Builds a document from plain text: one paragraph per line.
pub fn from_plain_text(bytes: &[u8], limits: &Limits) -> Result<Document> {
    if bytes.len() as u64 > limits.max_entry_size {
        return Err(OdfError::limit("text file is too large"));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t.to_owned(),
        Err(_) => bytes.iter().map(|b| char::from(*b)).collect(), // Latin-1 fallback
    };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    for line in text.split('\n') {
        if blocks.len() >= limits.max_blocks {
            return Err(OdfError::limit("text file has too many lines"));
        }
        if line == "\u{c}" {
            blocks.push(Block::PageBreak);
        } else {
            let clean: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
            blocks.push(Block::Paragraph(Paragraph::plain(clean)));
        }
    }
    if text.ends_with('\n') && blocks.len() > 1 {
        blocks.pop();
    }
    Ok(Document::from_blocks(blocks))
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' => out.push_str("<br>"),
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

fn html_runs(p: &Paragraph) -> String {
    let mut out = String::new();
    for run in &p.runs {
        if run.text.is_empty() {
            continue;
        }
        let a = &run.attrs;
        let mut open = String::new();
        let mut close = Vec::new();
        let mut tag = |name: &'static str| {
            open.push_str(&format!("<{name}>"));
            close.push(name);
        };
        if a.bold {
            tag("strong");
        }
        if a.italic {
            tag("em");
        }
        if a.underline {
            tag("u");
        }
        if a.strike {
            tag("s");
        }
        if a.code {
            tag("code");
        }
        if a.superscript {
            tag("sup");
        }
        if a.subscript {
            tag("sub");
        }
        let mut css = Vec::new();
        if let Some(c) = a.color {
            css.push(format!("color:{}", c.to_hex()));
        }
        if let Some(h) = a.highlight {
            css.push(format!("background-color:{}", h.to_hex()));
        }
        if let Some(s) = a.size {
            css.push(format!("font-size:{s}pt"));
        }
        if let Some(link) = a.link.as_deref().and_then(sanitize_link) {
            out.push_str(&format!("<a href=\"{}\" rel=\"noopener noreferrer\">", escape(&link)));
        }
        if !css.is_empty() {
            out.push_str(&format!("<span style=\"{}\">", css.join(";")));
        }
        out.push_str(&open);
        out.push_str(&escape(&run.text));
        for name in close.iter().rev() {
            out.push_str(&format!("</{name}>"));
        }
        if !css.is_empty() {
            out.push_str("</span>");
        }
        if a.link.as_deref().and_then(sanitize_link).is_some() {
            out.push_str("</a>");
        }
    }
    out
}

fn html_paragraph(p: &Paragraph) -> String {
    let mut css = Vec::new();
    match p.align {
        Align::Left => {}
        Align::Center => css.push("text-align:center".to_owned()),
        Align::Right => css.push("text-align:right".to_owned()),
        Align::Justify => css.push("text-align:justify".to_owned()),
    }
    if p.indent > 0.0 {
        css.push(format!("margin-left:{}pt", p.indent));
    }
    if p.space_before > 0.0 {
        css.push(format!("margin-top:{}pt", p.space_before));
    }
    let style = if css.is_empty() { String::new() } else { format!(" style=\"{}\"", css.join(";")) };
    let body = html_runs(p);
    match p.kind {
        ParaKind::Heading { level } => format!("<h{level}{style}>{body}</h{level}>"),
        ParaKind::Title => format!("<h1 class=\"title\"{style}>{body}</h1>"),
        ParaKind::Quote => format!("<blockquote{style}>{body}</blockquote>"),
        ParaKind::Code => format!("<pre{style}>{body}</pre>"),
        _ => format!("<p{style}>{body}</p>"),
    }
}

fn list_tag(kind: ParaKind) -> Option<(&'static str, u8)> {
    match kind {
        ParaKind::Bullet { level } => Some(("ul", level)),
        ParaKind::Numbered { level } => Some(("ol", level)),
        _ => None,
    }
}

fn html_paragraphs(paragraphs: &[Paragraph], out: &mut String) {
    // Stack of open list tags.
    let mut open: Vec<&'static str> = Vec::new();
    for p in paragraphs {
        match list_tag(p.kind) {
            Some((tag, level)) => {
                let want = usize::from(level) + 1;
                while open.len() > want {
                    out.push_str(&format!("</li></{}>", open.pop().unwrap_or("ul")));
                }
                if open.len() == want && open.last() != Some(&tag) {
                    out.push_str(&format!("</li></{}>", open.pop().unwrap_or("ul")));
                }
                if open.len() == want {
                    out.push_str("</li>");
                }
                while open.len() < want {
                    out.push_str(&format!("<{tag}>"));
                    open.push(tag);
                    if open.len() < want {
                        out.push_str("<li>");
                    }
                }
                out.push_str(&format!("<li>{}", html_runs(p)));
            }
            None => {
                while let Some(tag) = open.pop() {
                    out.push_str(&format!("</li></{tag}>"));
                }
                out.push_str(&html_paragraph(p));
                out.push('\n');
            }
        }
    }
    while let Some(tag) = open.pop() {
        out.push_str(&format!("</li></{tag}>"));
    }
}

/// Exports the document as a standalone HTML5 page.
pub fn to_html(doc: &Document) -> String {
    let title = doc.meta.title.clone().unwrap_or_else(|| "Document".into());
    let mut out = format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">\n\
         <title>{}</title>\n<style>body{{font-family:serif;max-width:48em;margin:2em auto;line-height:1.4}}\
         table{{border-collapse:collapse}}td{{border:1px solid #999;padding:4px 8px;vertical-align:top}}\
         .title{{text-align:center}}table.plain td{{border:none}}\
         header,footer{{color:#555;font-size:90%}}footer{{margin-top:2em}}</style>\n</head>\n<body>\n",
        escape(&title)
    );
    if !doc.header().is_empty() {
        out.push_str("<header>\n");
        html_paragraphs(doc.header(), &mut out);
        out.push_str("</header>\n");
    }
    let mut pending: Vec<Paragraph> = Vec::new();
    for block in doc.blocks() {
        match block {
            Block::Paragraph(p) => pending.push(p.clone()),
            other => {
                html_paragraphs(&pending, &mut out);
                pending.clear();
                match other {
                    Block::Table(t) => {
                        out.push_str(if t.borders { "<table>\n" } else { "<table class=\"plain\">\n" });
                        for (r, row) in t.rows.iter().enumerate() {
                            match t.row_heights.get(r) {
                                Some(h) => out.push_str(&format!("<tr style=\"height:{h}pt\">")),
                                None => out.push_str("<tr>"),
                            }
                            for (c, cell) in row.iter().enumerate() {
                                match t.col_widths.get(c) {
                                    Some(w) => out.push_str(&format!("<td style=\"width:{w}pt\">")),
                                    None => out.push_str("<td>"),
                                }
                                html_paragraphs(cell, &mut out);
                                out.push_str("</td>");
                            }
                            out.push_str("</tr>\n");
                        }
                        out.push_str("</table>\n");
                    }
                    Block::Image(p) => {
                        out.push_str(&format!(
                            "<p style=\"text-align:{}\"><img src=\"{}\" style=\"width:{}pt;height:{}pt\" alt=\"{}\"></p>\n",
                            css_align(p.align),
                            p.image.data_url(),
                            p.width,
                            p.height,
                            escape(&p.description)
                        ));
                    }
                    Block::Diagram(d) => {
                        let fig = d.figure();
                        out.push_str(&format!("<figure class=\"diagram\" style=\"text-align:{};margin:1em 0\">\n", css_align(d.align)));
                        out.push_str(&fig.to_svg_sized(d.width, d.height(), None));
                        out.push_str("</figure>\n");
                    }
                    Block::Drawing(c) => {
                        out.push_str(&format!("<figure class=\"drawing\" style=\"text-align:{};margin:1em 0\">\n", css_align(c.align)));
                        out.push_str(&canvas_figure(c).to_svg(None));
                        out.push_str("</figure>\n");
                    }
                    _ => out.push_str("<hr style=\"page-break-after:always\">\n"),
                }
            }
        }
    }
    html_paragraphs(&pending, &mut out);
    if !doc.footer().is_empty() {
        out.push_str("<footer>\n");
        html_paragraphs(doc.footer(), &mut out);
        out.push_str("</footer>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

fn css_align(a: Align) -> &'static str {
    match a {
        Align::Left | Align::Justify => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

/// The shapes of a canvas as a vector figure.
pub fn canvas_figure(c: &Canvas) -> office_draw::Figure {
    office_draw::model::shapes_to_figure(&c.shapes, c.width, c.height, None)
}

fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') {
            out.push('\\');
        }
        if c == '\n' {
            out.push_str("  \n");
        } else {
            out.push(c);
        }
    }
    out
}

fn md_runs(p: &Paragraph) -> String {
    let mut out = String::new();
    for run in &p.runs {
        if run.text.is_empty() {
            continue;
        }
        let a = &run.attrs;
        let mut text = if a.code { format!("`{}`", run.text.replace('`', "'")) } else { md_escape(&run.text) };
        if a.strike {
            text = format!("~~{text}~~");
        }
        if a.italic {
            text = format!("*{text}*");
        }
        if a.bold {
            text = format!("**{text}**");
        }
        if let Some(link) = a.link.as_deref().and_then(sanitize_link) {
            text = format!("[{text}]({})", link.replace(')', "%29").replace(' ', "%20"));
        }
        out.push_str(&text);
    }
    out
}

/// Exports the document as CommonMark-flavoured Markdown.
pub fn to_markdown(doc: &Document) -> String {
    let mut out = String::new();
    let mut counters = [0usize; 10];
    for block in doc.blocks() {
        match block {
            Block::Paragraph(p) => {
                if !matches!(p.kind, ParaKind::Numbered { .. }) {
                    counters = [0; 10];
                }
                match p.kind {
                    ParaKind::Heading { level } => {
                        out.push_str(&format!("{} {}\n\n", "#".repeat(usize::from(level)), md_runs(p)));
                    }
                    ParaKind::Title => out.push_str(&format!("# {}\n\n", md_runs(p))),
                    ParaKind::Bullet { level } => {
                        out.push_str(&format!("{}- {}\n", "  ".repeat(usize::from(level)), md_runs(p)));
                    }
                    ParaKind::Numbered { level } => {
                        let l = usize::from(level);
                        counters[l] += 1;
                        counters[l + 1..].iter_mut().for_each(|c| *c = 0);
                        out.push_str(&format!("{}{}. {}\n", "   ".repeat(l), counters[l], md_runs(p)));
                    }
                    ParaKind::Quote => out.push_str(&format!("> {}\n\n", md_runs(p))),
                    ParaKind::Code => out.push_str(&format!("```\n{}\n```\n\n", p.text().replace("```", "'''"))),
                    ParaKind::Normal => {
                        if out.ends_with("\n") && !out.ends_with("\n\n") && !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&format!("{}\n\n", md_runs(p)));
                    }
                }
            }
            Block::Table(t) => {
                out.push('\n');
                for (i, row) in t.rows.iter().enumerate() {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|cell| cell.iter().map(md_runs).collect::<Vec<_>>().join(" ").replace('\n', " "))
                        .collect();
                    out.push_str(&format!("| {} |\n", cells.join(" | ")));
                    if i == 0 {
                        out.push_str(&format!("|{}\n", " --- |".repeat(row.len())));
                    }
                }
                out.push('\n');
            }
            Block::PageBreak => out.push_str("\n---\n\n"),
            Block::Image(p) => {
                let alt = p.description.replace(['[', ']', '\n'], " ");
                out.push_str(&format!("![{alt}]({})\n\n", p.image.data_url()));
            }
            Block::Diagram(d) => out.push_str(&format!("```mermaid\n{}\n```\n\n", d.source.trim_end().replace("```", "'''"))),
            Block::Drawing(c) => {
                let svg = canvas_figure(c).to_svg(None);
                out.push_str(&format!("![Drawing](data:image/svg+xml;base64,{})\n\n", office_draw::image::base64(svg.as_bytes())));
            }
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_import() {
        let doc = from_plain_text(b"line one\r\nline two\n\x0c\nend\n", &Limits::default()).expect("load");
        assert_eq!(doc.blocks().len(), 4);
        assert!(matches!(doc.blocks()[2], Block::PageBreak));
        assert_eq!(doc.plain_text(), "line one\nline two\n\u{c}\nend");
        let doc = from_plain_text(b"caf\xe9", &Limits::default()).expect("latin1");
        assert_eq!(doc.plain_text(), "café");
        let limits = Limits { max_blocks: 2, ..Limits::default() };
        assert!(from_plain_text(b"a\nb\nc", &limits).is_err());
    }

    #[test]
    fn html_is_escaped_and_links_are_sanitised() {
        let mut p = Paragraph::plain("");
        p.runs = vec![
            Run { text: "<script>alert(1)</script>".into(), attrs: TextAttrs::default() },
            Run { text: "click".into(), attrs: TextAttrs { link: Some("javascript:alert(1)".into()), ..Default::default() } },
            Run { text: "ok".into(), attrs: TextAttrs { link: Some("https://example.org/?a=1&b=\"2\"".into()), bold: true, ..Default::default() } },
        ];
        let doc = Document::from_blocks(vec![Block::Paragraph(p)]);
        let html = to_html(&doc);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("<a href=\"https://example.org/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer\"><strong>ok</strong></a>"));
        assert!(html.contains("Content-Security-Policy"));
    }

    #[test]
    fn html_lists_nest() {
        let doc = Document::from_blocks(vec![
            Block::Paragraph(Paragraph::with_kind(ParaKind::Bullet { level: 0 }, "a")),
            Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 1 }, "b")),
            Block::Paragraph(Paragraph::with_kind(ParaKind::Bullet { level: 0 }, "c")),
            Block::Paragraph(Paragraph::plain("after")),
        ]);
        let html = to_html(&doc);
        assert!(html.contains("<ul><li>a<ol><li>b</li></ol></li><li>c</li></ul><p>after</p>"), "{html}");
    }

    #[test]
    fn markdown_export() {
        let mut bold = Paragraph::plain("");
        bold.runs = vec![
            Run { text: "a*b ".into(), attrs: TextAttrs::default() },
            Run { text: "bold".into(), attrs: TextAttrs { bold: true, ..Default::default() } },
        ];
        let doc = Document::from_blocks(vec![
            Block::Paragraph(Paragraph::with_kind(ParaKind::Heading { level: 2 }, "Title")),
            Block::Paragraph(bold),
            Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 0 }, "one")),
            Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 0 }, "two")),
            Block::Table(Table::from_rows(vec![vec![vec![Paragraph::plain("h")]], vec![vec![Paragraph::plain("v")]]])),
        ]);
        assert_eq!(
            to_markdown(&doc),
            "## Title\n\na\\*b **bold**\n\n1. one\n2. two\n\n| h |\n| --- |\n| v |\n"
        );
    }
}
