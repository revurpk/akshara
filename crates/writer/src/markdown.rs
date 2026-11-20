//! Markdown import: the CommonMark/GitHub subset that chat assistants and
//! note apps produce. Headings (ATX and setext), paragraphs, bulleted,
//! numbered and task lists with nesting, block quotes, fenced code, pipe
//! tables, and inline bold, italic, strikethrough, code and links.
//! ```` ```mermaid ```` fences become diagrams. Raw HTML is kept as text.

use crate::model::{sanitize_link, Align, Block, Diagram, PageSetup, ParaKind, Paragraph, Run, Table, TextAttrs};
use crate::Document;
use office_odf::{Limits, OdfError, Result};

/// Builds a document from Markdown text.
pub fn from_markdown(text: &str, limits: &Limits) -> Result<Document> {
    if text.len() as u64 > limits.max_entry_size {
        return Err(OdfError::limit("Markdown text is too large"));
    }
    let page = PageSetup::default();
    let blocks = markdown_blocks(text, (page.width - 2.0 * page.margin).max(72.0));
    if blocks.len() > limits.max_blocks {
        return Err(OdfError::limit("Markdown text has too many paragraphs"));
    }
    Ok(Document::from_blocks(blocks))
}

/// Converts Markdown to blocks. Diagrams are at most `max_width` wide.
pub fn markdown_blocks(text: &str, max_width: f32) -> Vec<Block> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    // Indent (in columns) of each open list level.
    let mut list_indents: Vec<usize> = Vec::new();
    let mut i = 0;

    let flush = |para: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            let joined = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            out.push(Block::Paragraph(inline(ParaKind::Normal, &joined)));
            para.clear();
        }
    };

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }

        // Fenced code.
        if let Some((fence, info)) = fence_open(trimmed) {
            flush(&mut para, &mut out);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !is_fence_close(lines[i].trim(), fence) {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            if info.eq_ignore_ascii_case("mermaid") {
                let source = body.join("\n");
                let fig = office_draw::mermaid::render(&source).figure;
                out.push(Block::Diagram(Diagram { source, width: fig.width.min(max_width).max(72.0), align: Align::Center }));
            } else {
                if body.is_empty() {
                    body.push("");
                }
                for l in body {
                    out.push(Block::Paragraph(Paragraph::with_kind(ParaKind::Code, l.replace('\t', "    "))));
                }
            }
            continue;
        }

        // Setext heading underline for the paragraph so far.
        if !para.is_empty() && (is_rule_of(trimmed, '=') || (is_rule_of(trimmed, '-') && !trimmed.contains(' '))) {
            let level = if trimmed.starts_with('=') { 1 } else { 2 };
            let joined = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            para.clear();
            out.push(Block::Paragraph(inline(ParaKind::Heading { level }, &joined)));
            i += 1;
            continue;
        }

        // Thematic break.
        if is_break(trimmed) {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }

        // ATX heading.
        if let Some((level, rest)) = atx(trimmed) {
            flush(&mut para, &mut out);
            out.push(Block::Paragraph(inline(ParaKind::Heading { level }, rest)));
            i += 1;
            continue;
        }

        // Pipe table: a row followed by a delimiter row.
        if trimmed.contains('|') && i + 1 < lines.len() && is_table_delimiter(lines[i + 1].trim()) {
            flush(&mut para, &mut out);
            let mut rows = vec![cells(trimmed)];
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(cells(lines[i].trim()));
                i += 1;
            }
            let rows: Vec<Vec<Vec<Paragraph>>> = rows
                .into_iter()
                .enumerate()
                .map(|(r, row)| {
                    row.into_iter()
                        .map(|c| {
                            let mut p = inline(ParaKind::Normal, &c);
                            if r == 0 {
                                p.runs.iter_mut().for_each(|run| run.attrs.bold = true);
                            }
                            vec![p]
                        })
                        .collect()
                })
                .collect();
            out.push(Block::Table(Table::from_rows(rows)));
            continue;
        }

        // Block quote (one level; nested markers are dropped).
        if let Some(rest) = trimmed.strip_prefix('>') {
            flush(&mut para, &mut out);
            let mut quote = vec![rest.trim_start_matches('>').trim()];
            i += 1;
            while i < lines.len() {
                let t = lines[i].trim();
                match t.strip_prefix('>') {
                    Some(r) => quote.push(r.trim_start_matches('>').trim()),
                    None => break,
                }
                i += 1;
            }
            for chunk in quote.split(|l| l.is_empty()).filter(|c| !c.is_empty()) {
                let joined = chunk.join(" ");
                out.push(Block::Paragraph(match atx(&joined) {
                    Some((_, rest)) => inline(ParaKind::Quote, rest),
                    None => inline(ParaKind::Quote, &joined),
                }));
            }
            continue;
        }

        // List item.
        if let Some((numbered, indent, rest)) = list_item(line) {
            flush(&mut para, &mut out);
            while list_indents.last().is_some_and(|&d| indent < d) {
                list_indents.pop();
            }
            if list_indents.last().is_none_or(|&d| indent > d) {
                list_indents.push(indent);
            }
            let level = (list_indents.len() - 1).min(9) as u8;
            let kind = if numbered { ParaKind::Numbered { level } } else { ParaKind::Bullet { level } };
            let mut text = rest.to_string();
            // Lazy continuation lines belong to the item.
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let t = next.trim();
                if t.is_empty() || list_item(next).is_some() || fence_open(t).is_some() || atx(t).is_some() || t.starts_with('>') || is_break(t) {
                    break;
                }
                text.push(' ');
                text.push_str(t);
                i += 1;
            }
            let text = task_box(&text);
            out.push(Block::Paragraph(inline(kind, &text)));
            continue;
        }
        list_indents.clear();

        para.push(line);
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}

fn fence_open(t: &str) -> Option<(&str, &str)> {
    for fence in ["```", "~~~"] {
        if let Some(rest) = t.strip_prefix(fence) {
            let ch = fence.as_bytes()[0] as char;
            let extra = rest.chars().take_while(|&c| c == ch).count();
            let marker = &t[..fence.len() + extra];
            let info = rest[extra..].trim();
            if ch == '`' && info.contains('`') {
                return None;
            }
            return Some((marker, info.split_whitespace().next().unwrap_or("")));
        }
    }
    None
}

fn is_fence_close(t: &str, fence: &str) -> bool {
    let ch = fence.as_bytes()[0] as char;
    t.len() >= fence.len() && t.chars().all(|c| c == ch)
}

fn is_rule_of(t: &str, ch: char) -> bool {
    !t.is_empty() && t.chars().all(|c| c == ch)
}

fn is_break(t: &str) -> bool {
    ['-', '*', '_'].iter().any(|&ch| {
        let n = t.chars().filter(|&c| c == ch).count();
        n >= 3 && t.chars().all(|c| c == ch || c == ' ')
    })
}

fn atx(t: &str) -> Option<(u8, &str)> {
    let level = t.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let rest = rest.trim();
    // Optional closing run of #.
    let rest = rest.trim_end_matches('#').trim_end();
    Some((level as u8, rest))
}

/// `(numbered, indent columns, text)` for a list item line.
fn list_item(line: &str) -> Option<(bool, usize, &str)> {
    let indent: usize = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let t = line.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            if is_break(t) {
                return None;
            }
            return Some((false, indent, rest.trim()));
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = &t[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((true, indent, rest.trim()));
        }
    }
    None
}

fn task_box(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("[ ] ") {
        format!("☐ {rest}")
    } else if let Some(rest) = text.strip_prefix("[x] ").or_else(|| text.strip_prefix("[X] ")) {
        format!("☑ {rest}")
    } else {
        text.to_string()
    }
}

fn is_table_delimiter(t: &str) -> bool {
    t.contains('-')
        && t.contains('|') | t.starts_with(':') | t.starts_with('-')
        && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
        && cells(t).iter().all(|c| c.trim_matches(':').chars().all(|ch| ch == '-') && c.contains('-'))
}

fn cells(t: &str) -> Vec<String> {
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    let mut in_code = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '`' => {
                in_code = !in_code;
                cur.push(c);
            }
            '|' if !in_code => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Parses inline Markdown into a paragraph of the given kind.
fn inline(kind: ParaKind, text: &str) -> Paragraph {
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut attrs = TextAttrs::default();
    let mut buf = String::new();
    let mut i = 0;

    let push = |runs: &mut Vec<Run>, buf: &mut String, attrs: &TextAttrs| {
        if !buf.is_empty() {
            runs.push(Run { text: std::mem::take(buf), attrs: attrs.clone() });
        }
    };

    while i < chars.len() {
        let c = chars[i];
        // Escapes.
        if c == '\\' && i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() {
            buf.push(chars[i + 1]);
            i += 2;
            continue;
        }
        // Code span.
        if c == '`' {
            let n = chars[i..].iter().take_while(|&&ch| ch == '`').count();
            if let Some(end) = find_run(&chars, i + n, '`', n) {
                push(&mut runs, &mut buf, &attrs);
                let code: String = chars[i + n..end].iter().collect();
                let code = if code.starts_with(' ') && code.ends_with(' ') && code.trim() != "" { code[1..code.len() - 1].to_string() } else { code };
                runs.push(Run { text: code, attrs: TextAttrs { code: true, ..attrs.clone() } });
                i = end + n;
                continue;
            }
            buf.extend(&chars[i..i + n]);
            i += n;
            continue;
        }
        // Images: keep the alt text.
        if c == '!' && chars.get(i + 1) == Some(&'[') {
            if let Some((label, _, end)) = link_at(&chars, i + 1) {
                buf.push_str(&label);
                i = end;
                continue;
            }
        }
        // Links.
        if c == '[' {
            if let Some((label, url, end)) = link_at(&chars, i) {
                push(&mut runs, &mut buf, &attrs);
                let link = sanitize_link(&url);
                let inner = inline(ParaKind::Normal, &label);
                for mut r in inner.runs {
                    r.attrs.bold |= attrs.bold;
                    r.attrs.italic |= attrs.italic;
                    r.attrs.strike |= attrs.strike;
                    r.attrs.link = link.clone();
                    runs.push(r);
                }
                i = end;
                continue;
            }
        }
        // Autolinks.
        if c == '<' {
            if let Some(end) = chars[i + 1..].iter().position(|&ch| ch == '>' || ch == ' ') {
                let url: String = chars[i + 1..i + 1 + end].iter().collect();
                if chars[i + 1 + end] == '>' && (url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
                    push(&mut runs, &mut buf, &attrs);
                    runs.push(Run { text: url.clone(), attrs: TextAttrs { link: sanitize_link(&url), ..attrs.clone() } });
                    i += end + 2;
                    continue;
                }
            }
        }
        // Emphasis delimiters.
        if c == '*' || c == '_' || c == '~' {
            let n = chars[i..].iter().take_while(|&&ch| ch == c).count();
            let prev = if i == 0 { ' ' } else { chars[i - 1] };
            let next = chars.get(i + n).copied().unwrap_or(' ');
            let mut handled = false;
            if c == '~' {
                if n == 2 {
                    if attrs.strike && !prev.is_whitespace() {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.strike = false;
                        handled = true;
                    } else if !attrs.strike && !next.is_whitespace() && find_closer(&chars, i + 2, c, 2) {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.strike = true;
                        handled = true;
                    }
                }
            } else {
                // Intraword underscores are literal.
                let word = |ch: char| ch.is_alphanumeric();
                let can_open = !next.is_whitespace() && !(c == '_' && word(prev));
                let can_close = !prev.is_whitespace() && !(c == '_' && word(next));
                let mut k = n.min(3);
                let mut changed = false;
                // Closing first (innermost), then opening.
                if can_close {
                    if k >= 2 && attrs.bold {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.bold = false;
                        k -= 2;
                        changed = true;
                    }
                    if k >= 1 && attrs.italic {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.italic = false;
                        k -= 1;
                        changed = true;
                    }
                }
                if can_open && k > 0 && !changed {
                    if k >= 2 && !attrs.bold && find_closer(&chars, i + n, c, 2) {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.bold = true;
                        k -= 2;
                        changed = true;
                    }
                    if k >= 1 && !attrs.italic && find_closer(&chars, i + n, c, 1) {
                        push(&mut runs, &mut buf, &attrs);
                        attrs.italic = true;
                        k -= 1;
                        changed = true;
                    }
                }
                if changed {
                    // Any surplus delimiter characters stay literal.
                    for _ in 0..k {
                        buf.push(c);
                    }
                    i += n;
                    continue;
                }
            }
            if handled {
                i += n;
                continue;
            }
            buf.extend(&chars[i..i + n]);
            i += n;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    push(&mut runs, &mut buf, &attrs);
    let mut p = Paragraph { kind: kind.normalized(), runs, ..Default::default() };
    p.normalize();
    p
}

/// Index of the next run of exactly `n` `ch` characters at or after `from`.
fn find_run(chars: &[char], from: usize, ch: char, n: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == ch {
            let m = chars[j..].iter().take_while(|&&c| c == ch).count();
            if m == n {
                return Some(j);
            }
            j += m;
        } else {
            j += 1;
        }
    }
    None
}

/// Whether a closing delimiter of at least `n` `ch` follows `from`, preceded
/// by a non-space character.
fn find_closer(chars: &[char], from: usize, ch: char, n: usize) -> bool {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == '`' {
            // Skip code spans.
            let m = chars[j..].iter().take_while(|&&c| c == '`').count();
            match find_run(chars, j + m, '`', m) {
                Some(end) => j = end + m,
                None => j += m,
            }
            continue;
        }
        if chars[j] == ch {
            let m = chars[j..].iter().take_while(|&&c| c == ch).count();
            let next = chars.get(j + m).copied().unwrap_or(' ');
            let close_ok = ch != '_' || !next.is_alphanumeric();
            if m >= n && j > from && !chars[j - 1].is_whitespace() && close_ok {
                return true;
            }
            j += m;
        } else {
            j += 1;
        }
    }
    false
}

/// `[label](url)` starting at `chars[at] == '['`: label, url, index after.
fn link_at(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let mut depth = 0;
    let mut j = at;
    let close = loop {
        match chars.get(j)? {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break j;
                }
            }
            '\\' => j += 1,
            _ => {}
        }
        j += 1;
    };
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = close + 2 + chars[close + 2..].iter().position(|&c| c == ')')?;
    let label: String = chars[at + 1..close].iter().collect();
    let target: String = chars[close + 2..end].iter().collect();
    // Drop an optional "title".
    let url = target.split_whitespace().next().unwrap_or("").trim_matches(|c| c == '<' || c == '>').to_string();
    Some((label, url, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paras(blocks: &[Block]) -> Vec<(ParaKind, String)> {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some((p.kind, p.text())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn structure() {
        let md = "# Title\n\nSome *text*\nwrapped.\n\n- one\n- two\n  - nested\n1. first\n2. second\n\n> quoted\n\nSub\n---\n\n```rust\nfn x() {}\n```\n";
        let b = markdown_blocks(md, 400.0);
        assert_eq!(
            paras(&b),
            [
                (ParaKind::Heading { level: 1 }, "Title".into()),
                (ParaKind::Normal, "Some text wrapped.".into()),
                (ParaKind::Bullet { level: 0 }, "one".into()),
                (ParaKind::Bullet { level: 0 }, "two".into()),
                (ParaKind::Bullet { level: 1 }, "nested".into()),
                (ParaKind::Numbered { level: 0 }, "first".into()),
                (ParaKind::Numbered { level: 0 }, "second".into()),
                (ParaKind::Quote, "quoted".into()),
                (ParaKind::Heading { level: 2 }, "Sub".into()),
                (ParaKind::Code, "fn x() {}".into()),
            ]
        );
    }

    #[test]
    fn inline_styles() {
        let p = inline(ParaKind::Normal, "a **bold** and *it* `co*de*` ~~gone~~ [link](https://x.org \"t\") snake_case_name");
        let find = |t: &str| p.runs.iter().find(|r| r.text == t).map(|r| r.attrs.clone()).unwrap_or_default();
        assert!(find("bold").bold);
        assert!(find("it").italic);
        assert!(find("co*de*").code);
        assert!(find("gone").strike);
        assert_eq!(find("link").link.as_deref(), Some("https://x.org"));
        assert_eq!(p.text(), "a bold and it co*de* gone link snake_case_name");
        // Unclosed markers stay literal.
        assert_eq!(inline(ParaKind::Normal, "5 * 3 = 15, **oops").text(), "5 * 3 = 15, **oops");
        assert!(inline(ParaKind::Normal, "***both***").runs[0].attrs.bold);
        assert_eq!(inline(ParaKind::Normal, "***both***").text(), "both");
    }

    #[test]
    fn tables_and_diagrams() {
        let md = "| Name | Qty |\n|:-----|----:|\n| Tea | 2 |\n| `a|b` | 3 |\n\n```mermaid\ngraph TD; A-->B\n```";
        let b = markdown_blocks(md, 300.0);
        let Block::Table(t) = &b[0] else { panic!("table expected: {b:?}") };
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[0][1][0].text(), "Qty");
        assert!(t.rows[0][0][0].runs[0].attrs.bold);
        assert_eq!(t.rows[2][0][0].text(), "a|b");
        let Block::Diagram(d) = &b[1] else { panic!("diagram expected") };
        assert!(d.source.contains("A-->B"));
        assert!(d.width <= 300.0);
    }

    #[test]
    fn tasks_and_rules() {
        let b = markdown_blocks("- [ ] todo\n- [x] done\n\n---\n\ntext", 300.0);
        assert_eq!(
            paras(&b),
            [
                (ParaKind::Bullet { level: 0 }, "☐ todo".into()),
                (ParaKind::Bullet { level: 0 }, "☑ done".into()),
                (ParaKind::Normal, "text".into()),
            ]
        );
    }
}
