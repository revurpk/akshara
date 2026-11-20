//! Mermaid diagrams, rendered natively (no JavaScript, no network).
//!
//! [`render`] parses Mermaid source and lays it out into a [`Figure`]. The
//! supported diagram types are flowcharts (`graph` / `flowchart`), state
//! diagrams, class diagrams, sequence diagrams and pie charts, covering the
//! commonly used syntax of each. Like the formula parser, rendering never
//! fails: problems are reported with line numbers and whatever could be
//! understood is still drawn, so a live preview works while typing.

mod class;
mod flowchart;
mod layout;
mod pie;
mod sequence;
mod state;

use crate::figure::{Anchor, Figure, Item, Paint};
use office_odf::Rgb;
use serde::Serialize;

/// A problem found in the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// 1-based line number.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

/// The result of rendering.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rendered {
    /// The diagram (an explanatory message when nothing could be drawn).
    pub figure: Figure,
    /// Diagram type (`flowchart`, `sequence`...), or `null`.
    pub kind: Option<&'static str>,
    /// Problems, in source order.
    pub errors: Vec<Diagnostic>,
}

/// Most statements one diagram may have.
pub const MAX_STATEMENTS: usize = 5000;
/// Most nodes laid out with full crossing reduction.
pub(crate) const MAX_OPTIMIZED_NODES: usize = 400;

/// Colours of the default theme.
pub(crate) mod theme {
    use office_odf::Rgb;
    pub const NODE_FILL: Rgb = Rgb(0xec, 0xec, 0xff);
    pub const NODE_STROKE: Rgb = Rgb(0x93, 0x70, 0xdb);
    pub const LINE: Rgb = Rgb(0x33, 0x33, 0x33);
    pub const TEXT: Rgb = Rgb(0x33, 0x33, 0x33);
    pub const CLUSTER_FILL: Rgb = Rgb(0xff, 0xff, 0xde);
    pub const CLUSTER_STROKE: Rgb = Rgb(0xaa, 0xaa, 0x33);
    pub const NOTE_FILL: Rgb = Rgb(0xff, 0xf5, 0xad);
    pub const NOTE_STROKE: Rgb = Rgb(0xaa, 0xaa, 0x33);
    pub const LABEL_BG: Rgb = Rgb(0xe8, 0xe8, 0xe8);
    pub const FONT: f32 = 14.0;
}

/// One source line with its number, comments removed.
#[derive(Debug, Clone)]
pub(crate) struct Line<'a> {
    pub no: usize,
    pub text: &'a str,
}

/// Splits source into non-empty lines without `%%` comments and the
/// optional `---` front matter (whose `title:` is returned).
pub(crate) fn lines(source: &str) -> (Vec<Line<'_>>, Option<String>) {
    let mut out = Vec::new();
    let mut title = None;
    let mut in_front = false;
    let mut seen_content = false;
    for (i, raw) in source.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed == "---" && !seen_content {
            in_front = !in_front;
            continue;
        }
        if in_front {
            if let Some(t) = trimmed.strip_prefix("title:") {
                title = Some(unquote(t.trim()).to_owned());
            }
            continue;
        }
        let text = match find_unquoted(raw, "%%") {
            Some(pos) => &raw[..pos],
            None => raw,
        }
        .trim();
        if text.is_empty() {
            continue;
        }
        seen_content = true;
        out.push(Line { no: i + 1, text });
    }
    (out, title)
}

/// Byte position of `pat` outside double quotes.
pub(crate) fn find_unquoted(s: &str, pat: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && s[i..].starts_with(pat) {
            return Some(i);
        }
    }
    None
}

/// Removes one pair of surrounding double quotes.
pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Converts label markup: `<br>` line breaks, `#quot;`-style entity codes,
/// HTML entities, Markdown emphasis markers and backtick strings.
pub(crate) fn label_text(s: &str) -> String {
    let s = unquote(s).trim();
    let s = s.strip_prefix('`').and_then(|x| x.strip_suffix('`')).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let lower = rest.get(..6.min(rest.len())).map(str::to_ascii_lowercase).unwrap_or_default();
        if lower.starts_with("<br") {
            if let Some(end) = rest.find('>') {
                out.push('\n');
                rest = &rest[end + 1..];
                continue;
            }
        }
        if c == '#' || c == '&' {
            if let Some(end) = rest[1..].find(';').map(|e| e + 1).filter(|e| *e <= 10) {
                let code = &rest[1..end];
                let decoded = match code {
                    "quot" => Some('"'),
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "nbsp" => Some(' '),
                    "apos" => Some('\''),
                    _ => code
                        .strip_prefix('#')
                        .unwrap_or(code)
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|ch| !ch.is_control()),
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        if rest.starts_with("**") || rest.starts_with("__") {
            rest = &rest[2..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    // Font Awesome icons (`fa:fa-car`) cannot be drawn offline: drop them.
    let out: Vec<String> = out
        .split('\n')
        .map(|line| line.split(' ').filter(|w| !(w.starts_with("fa:fa-") || w.starts_with("fab:fa-"))).collect::<Vec<_>>().join(" "))
        .collect();
    out.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n")
}

/// Parses a CSS-like colour: `#rgb`, `#rrggbb`, `rgb(r,g,b)` or a few
/// common names.
pub(crate) fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_end_matches(';').trim();
    if let Some(hex) = s.strip_prefix('#') {
        let v = |h: &str| u8::from_str_radix(h, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let d: Vec<u8> = hex.chars().take(3).map(|c| c.to_digit(16).map(|x| x as u8 * 17)).collect::<Option<_>>()?;
                Some(Rgb(d[0], d[1], d[2]))
            }
            6 | 8 => Some(Rgb(v(&hex[0..2])?, v(&hex[2..4])?, v(&hex[4..6])?)),
            _ => None,
        };
    }
    if let Some(args) = s.strip_prefix("rgb(").or_else(|| s.strip_prefix("rgba(")).and_then(|r| r.strip_suffix(')')) {
        let n: Vec<u8> = args.split(',').take(3).map(|p| p.trim().parse::<f32>().ok().map(|v| v.clamp(0.0, 255.0) as u8)).collect::<Option<_>>()?;
        return (n.len() == 3).then(|| Rgb(n[0], n[1], n[2]));
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "black" => Rgb(0, 0, 0),
        "white" => Rgb(255, 255, 255),
        "red" => Rgb(255, 0, 0),
        "green" => Rgb(0, 128, 0),
        "blue" => Rgb(0, 0, 255),
        "yellow" => Rgb(255, 255, 0),
        "orange" => Rgb(255, 165, 0),
        "purple" => Rgb(128, 0, 128),
        "pink" => Rgb(255, 192, 203),
        "gray" | "grey" => Rgb(128, 128, 128),
        "lightgray" | "lightgrey" => Rgb(211, 211, 211),
        "lightblue" => Rgb(173, 216, 230),
        "lightgreen" => Rgb(144, 238, 144),
        "lightyellow" => Rgb(255, 255, 224),
        "cyan" | "aqua" => Rgb(0, 255, 255),
        "magenta" | "fuchsia" => Rgb(255, 0, 255),
        "brown" => Rgb(165, 42, 42),
        "navy" => Rgb(0, 0, 128),
        "teal" => Rgb(0, 128, 128),
        "olive" => Rgb(128, 128, 0),
        "maroon" => Rgb(128, 0, 0),
        "gold" => Rgb(255, 215, 0),
        "silver" => Rgb(192, 192, 192),
        "lime" => Rgb(0, 255, 0),
        "transparent" | "none" => return None,
        _ => return None,
    })
}

/// Style properties from `style` / `classDef` statements.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct NodeStyle {
    pub fill: Option<Rgb>,
    pub stroke: Option<Rgb>,
    pub color: Option<Rgb>,
    pub width: Option<f32>,
    pub dash: bool,
    pub no_fill: bool,
}

impl NodeStyle {
    /// Parses `fill:#f9f,stroke:#333,stroke-width:4px,color:#fff`.
    pub fn parse(props: &str) -> NodeStyle {
        let mut s = NodeStyle::default();
        for prop in props.split(',') {
            let Some((k, v)) = prop.split_once(':') else { continue };
            let v = v.trim();
            match k.trim() {
                "fill" => {
                    s.fill = parse_color(v);
                    s.no_fill = s.fill.is_none() && matches!(v, "none" | "transparent");
                }
                "stroke" => s.stroke = parse_color(v),
                "color" => s.color = parse_color(v),
                "stroke-width" => s.width = v.trim_end_matches("px").trim().parse::<f32>().ok().map(|w| w.clamp(0.0, 20.0) * 0.75),
                "stroke-dasharray" => s.dash = true,
                _ => {}
            }
        }
        s
    }

    /// Later values override earlier ones.
    pub fn merge(self, other: NodeStyle) -> NodeStyle {
        NodeStyle {
            fill: other.fill.or(if other.no_fill { None } else { self.fill }),
            stroke: other.stroke.or(self.stroke),
            color: other.color.or(self.color),
            width: other.width.or(self.width),
            dash: self.dash || other.dash,
            no_fill: other.no_fill || (self.no_fill && other.fill.is_none()),
        }
    }

    /// The paint for a node with the given default fill and stroke.
    pub fn paint(&self, fill: Rgb, stroke: Rgb, width: f32) -> Paint {
        Paint {
            fill: if self.no_fill { None } else { Some(self.fill.unwrap_or(fill)) },
            stroke: Some(self.stroke.unwrap_or(stroke)),
            width: self.width.unwrap_or(width),
            dash: self.dash,
        }
    }
}

/// Adds a text item.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text(fig: &mut Figure, x: f32, y: f32, s: &str, size: f32, bold: bool, anchor: Anchor, color: Rgb) {
    if s.is_empty() {
        return;
    }
    fig.push(Item::Text { x, y, text: s.to_owned(), size, bold, italic: false, anchor, color });
}

/// Pushes a text item onto a list.
pub(crate) fn text_item(items: &mut Vec<Item>, x: f32, y: f32, s: &str, size: f32, anchor: Anchor) {
    if !s.is_empty() {
        items.push(Item::Text { x, y, text: s.to_owned(), size, bold: false, italic: false, anchor, color: theme::TEXT });
    }
}

/// Pushes a bold, centred text item onto a list.
pub(crate) fn text_item_bold(items: &mut Vec<Item>, x: f32, y: f32, s: &str, size: f32) {
    if !s.is_empty() {
        items.push(Item::Text { x, y, text: s.to_owned(), size, bold: true, italic: false, anchor: Anchor::Middle, color: theme::TEXT });
    }
}

/// Adds a diagram title above the figure (shifting everything down).
pub(crate) fn add_title(fig: &mut Figure, title: Option<&str>) {
    let Some(title) = title.filter(|t| !t.is_empty()) else { return };
    let size = 18.0;
    let (w, h) = crate::figure::text_size(title, size, true);
    let shift = h + 12.0;
    for item in &mut fig.items {
        shift_item(item, 0.0, shift);
    }
    fig.width = fig.width.max(w + 20.0);
    fig.height += shift;
    let cx = fig.width / 2.0;
    fig.items.insert(0, Item::Text { x: cx, y: 6.0 + h / 2.0, text: title.to_owned(), size, bold: true, italic: false, anchor: Anchor::Middle, color: theme::TEXT });
}

pub(crate) fn shift_item(item: &mut Item, dx: f32, dy: f32) {
    match item {
        Item::Rect { x, y, .. } | Item::Text { x, y, .. } | Item::Image { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
        Item::Ellipse { cx, cy, .. } | Item::Wedge { cx, cy, .. } => {
            *cx += dx;
            *cy += dy;
        }
        Item::Poly { points, .. } => {
            for p in points {
                p[0] += dx;
                p[1] += dy;
            }
        }
        Item::Rotate { cx, cy, items, .. } => {
            *cx += dx;
            *cy += dy;
            for i in items {
                shift_item(i, dx, dy);
            }
        }
    }
}

/// Renders Mermaid source.
pub fn render(source: &str) -> Rendered {
    let (lines, title) = lines(source);
    let Some(first) = lines.first() else {
        return Rendered { figure: Figure::message("Empty diagram"), kind: None, errors: Vec::new() };
    };
    if lines.len() > MAX_STATEMENTS {
        return Rendered {
            figure: Figure::message("This diagram is too large to draw."),
            kind: None,
            errors: vec![Diagnostic { line: 1, message: "too many statements".into() }],
        };
    }
    let keyword = first.text.split(|c: char| c.is_whitespace()).next().unwrap_or("");
    let (kind, result) = match keyword {
        "graph" | "flowchart" | "flowchart-elk" => ("flowchart", flowchart::render(&lines)),
        "stateDiagram" | "stateDiagram-v2" => ("state", state::render(&lines)),
        "classDiagram" | "classDiagram-v2" => ("class", class::render(&lines)),
        "sequenceDiagram" => ("sequence", sequence::render(&lines)),
        "pie" => ("pie", pie::render(&lines)),
        other => {
            let msg = format!("Unsupported diagram type \"{other}\". Supported: flowchart, sequenceDiagram, classDiagram, stateDiagram, pie.");
            return Rendered {
                figure: Figure::message(&msg),
                kind: None,
                errors: vec![Diagnostic { line: first.no, message: msg.clone() }],
            };
        }
    };
    let (mut figure, errors, own_title) = result;
    add_title(&mut figure, title.as_deref().or(own_title.as_deref()));
    if figure.items.is_empty() {
        figure = Figure::message("Empty diagram");
    }
    Rendered { figure, kind: Some(kind), errors }
}

/// What a diagram renderer returns: the figure, problems, and a title from
/// the diagram body (pie charts and sequence diagrams have one).
pub(crate) type Output = (Figure, Vec<Diagnostic>, Option<String>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_cleaned() {
        assert_eq!(label_text("\"a<br/>b\""), "a\nb");
        assert_eq!(label_text("x #quot;y#quot; #35;"), "x \"y\" #");
        assert_eq!(label_text("**bold** text"), "bold text");
        assert_eq!(parse_color("#f9f"), Some(Rgb(255, 153, 255)));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
        assert_eq!(NodeStyle::parse("fill:#fff,stroke-width:4px").width, Some(3.0));
    }

    #[test]
    fn front_matter_title_and_comments() {
        let (lines, title) = lines("---\ntitle: Hello\n---\ngraph TD\n%% comment\nA-->B %% trailing\n");
        assert_eq!(title.as_deref(), Some("Hello"));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "A-->B");
        assert_eq!(lines[1].no, 6);
    }

    #[test]
    fn unknown_type_is_reported() {
        let r = render("gitGraph\ncommit");
        assert!(r.kind.is_none());
        assert_eq!(r.errors.len(), 1);
        assert!(!r.figure.items.is_empty());
        assert!(render("").errors.is_empty());
    }
}
