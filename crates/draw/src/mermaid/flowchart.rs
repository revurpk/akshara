//! Flowcharts: `graph` / `flowchart` with node shapes, labelled links,
//! chains (`A --> B --> C`), `&` groups, subgraphs, `style`, `classDef`
//! and `class`.

use super::layout::{self, Dir, Graph, Outline};
use super::{label_text, theme, Diagnostic, Line, NodeStyle, Output};
use crate::figure::{text_size, Anchor, Arrow, Figure, Item, Paint};
use std::collections::HashMap;

/// Node shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    Asymmetric,
    Diamond,
    Hexagon,
    Parallelogram,
    ParallelogramAlt,
    Trapezoid,
    TrapezoidAlt,
}

impl Shape {
    fn outline(self) -> Outline {
        match self {
            Shape::Circle | Shape::DoubleCircle => Outline::Ellipse,
            Shape::Diamond => Outline::Diamond,
            _ => Outline::Rect,
        }
    }
}

/// How a link is drawn.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkStyle {
    pub dash: bool,
    pub thick: bool,
    pub invisible: bool,
    pub start: Option<Arrow>,
    pub end: Option<Arrow>,
    pub label: Option<String>,
}

/// Draws an edge route with its label.
pub(crate) fn draw_edge(fig: &mut Figure, route: &layout::Route, style: &LinkStyle) {
    if style.invisible || route.points.len() < 2 {
        return;
    }
    let width = if style.thick { 2.6 } else { 1.2 };
    let mut paint = Paint::line(theme::LINE, width);
    paint.dash = style.dash;
    fig.push(Item::Poly { points: route.points.clone(), closed: false, smooth: true, paint, start: style.start, end: style.end });
    if let (Some(label), Some(at)) = (&style.label, route.label) {
        if !label.is_empty() {
            let (w, h) = text_size(label, 12.0, false);
            fig.push(Item::Rect { x: at[0] - w / 2.0 - 3.0, y: at[1] - h / 2.0 - 1.0, w: w + 6.0, h: h + 2.0, r: 2.0, paint: Paint::fill(theme::LABEL_BG) });
            super::text(fig, at[0], at[1], label, 12.0, false, Anchor::Middle, theme::TEXT);
        }
    }
}

/// Label size of a link label.
pub(crate) fn label_size(label: &Option<String>) -> Option<(f32, f32)> {
    label.as_ref().filter(|l| !l.is_empty()).map(|l| {
        let (w, h) = text_size(l, 12.0, false);
        (w + 8.0, h + 4.0)
    })
}

#[derive(Debug, Clone)]
struct Node {
    label: String,
    shape: Shape,
    cluster: Option<usize>,
    classes: Vec<String>,
}

struct Parser {
    nodes: Vec<Node>,
    ids: HashMap<String, usize>,
    links: Vec<(usize, usize, LinkStyle, usize)>,
    clusters: Vec<(String, Option<usize>)>,
    stack: Vec<usize>,
    styles: HashMap<String, NodeStyle>,
    class_defs: HashMap<String, NodeStyle>,
    errors: Vec<Diagnostic>,
    line: usize,
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Cursor<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.rest().starts_with(p) {
            self.pos += p.len();
            true
        } else {
            false
        }
    }
    fn at_end(&self) -> bool {
        self.pos >= self.s.len()
    }
}

/// Node shape delimiters, longest first.
const SHAPES: &[(&str, &str, Shape)] = &[
    ("(((", ")))", Shape::DoubleCircle),
    ("((", "))", Shape::Circle),
    ("([", "])", Shape::Stadium),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Cylinder),
    ("{{", "}}", Shape::Hexagon),
    ("[/", "/]", Shape::Parallelogram),
    ("[/", "\\]", Shape::Trapezoid),
    ("[\\", "\\]", Shape::ParallelogramAlt),
    ("[\\", "/]", Shape::TrapezoidAlt),
    (">", "]", Shape::Asymmetric),
    ("(", ")", Shape::Round),
    ("[", "]", Shape::Rect),
    ("{", "}", Shape::Diamond),
];

impl Parser {
    fn err(&mut self, msg: impl Into<String>) {
        if self.errors.len() < 50 {
            self.errors.push(Diagnostic { line: self.line, message: msg.into() });
        }
    }

    fn node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.ids.get(id) {
            return i;
        }
        let cluster = self.stack.last().copied();
        self.nodes.push(Node { label: id.to_owned(), shape: Shape::Rect, cluster, classes: Vec::new() });
        self.ids.insert(id.to_owned(), self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// `id`, optionally followed by a shape with text and `:::class`.
    fn node_ref(&mut self, c: &mut Cursor) -> Option<usize> {
        c.skip_ws();
        let start = c.pos;
        while let Some(ch) = c.peek() {
            let next = c.rest()[ch.len_utf8()..].chars().next();
            let ok = is_id_char(ch) || (ch == '-' && next.is_some_and(|n| n.is_alphanumeric()) && c.pos > start);
            if !ok {
                break;
            }
            c.pos += ch.len_utf8();
        }
        if c.pos == start {
            return None;
        }
        let id = &c.s[start..c.pos];
        let index = self.node(id);
        for (open, close, shape) in SHAPES {
            if !c.rest().starts_with(open) {
                continue;
            }
            let body_start = c.pos + open.len();
            let body = &c.s[body_start..];
            // Quoted text may contain the closing delimiter.
            let end = if body.trim_start().starts_with('"') {
                let q = body.find('"').unwrap_or(0);
                body[q + 1..].find('"').map(|e| q + 1 + e + 1).and_then(|after| body[after..].find(close).map(|k| after + k))
            } else {
                body.find(close)
            };
            if let Some(end) = end {
                let text = &body[..end];
                self.nodes[index].label = label_text(text);
                self.nodes[index].shape = *shape;
                c.pos = body_start + end + close.len();
                break;
            }
        }
        if c.eat(":::") {
            let s = c.pos;
            while c.peek().is_some_and(|ch| is_id_char(ch) || ch == '-') {
                c.pos += c.peek().map_or(1, char::len_utf8);
            }
            let class = c.s[s..c.pos].to_owned();
            self.nodes[index].classes.push(class);
        }
        Some(index)
    }

    fn group(&mut self, c: &mut Cursor) -> Option<Vec<usize>> {
        let mut out = vec![self.node_ref(c)?];
        loop {
            c.skip_ws();
            if c.eat("&") {
                match self.node_ref(c) {
                    Some(n) => out.push(n),
                    None => return Some(out),
                }
            } else {
                return Some(out);
            }
        }
    }

    /// A link token: returns its style and extra length.
    fn link(&mut self, c: &mut Cursor) -> Option<(LinkStyle, usize)> {
        c.skip_ws();
        let mut st = LinkStyle::default();
        let begin = c.pos;
        let marker = |ch: char| match ch {
            '<' => Some(Arrow::Triangle),
            'o' => Some(Arrow::Circle),
            'x' => Some(Arrow::Cross),
            _ => None,
        };
        let first = c.peek()?;
        let second = c.rest().chars().nth(1);
        if let Some(a) = marker(first) {
            if second.is_some_and(|s| matches!(s, '-' | '=' | '.')) {
                st.start = Some(a);
                c.pos += 1;
            }
        }
        if c.eat("~~~") {
            while c.eat("~") {}
            st.invisible = true;
            return Some((st, 0));
        }
        let body_start = c.pos;
        while c.peek().is_some_and(|ch| matches!(ch, '-' | '=' | '.')) {
            c.pos += 1;
        }
        let body = &c.s[body_start..c.pos];
        if body.is_empty() || (body.len() < 2 && !body.contains('.')) {
            c.pos = begin;
            return None;
        }
        // `-- text -->` / `-. text .->` / `== text ==>`
        let text_form = matches!(body, "--" | "==" | "-.") && c.peek().is_some_and(char::is_whitespace);
        let mut body = body.to_owned();
        if text_form {
            let closers: &[&str] = match body.as_str() {
                "--" => &["-->", "---", "--x", "--o"],
                "==" => &["==>", "===", "==x", "==o"],
                _ => &[".->", ".-"],
            };
            let rest = c.rest();
            let found = closers.iter().filter_map(|p| rest.find(p).map(|i| (i, *p))).min_by_key(|(i, _)| *i);
            match found {
                Some((i, _)) => {
                    st.label = Some(label_text(rest[..i].trim()));
                    c.pos += i;
                    let s = c.pos;
                    while c.peek().is_some_and(|ch| matches!(ch, '-' | '=' | '.')) {
                        c.pos += 1;
                    }
                    body = c.s[s..c.pos].to_owned();
                }
                None => {
                    self.err("link text is not closed (expected -->)");
                    return None;
                }
            }
        }
        st.dash = body.contains('.');
        st.thick = body.contains('=');
        match c.peek() {
            Some('>') => {
                c.pos += 1;
                st.end = Some(Arrow::Triangle);
            }
            Some(ch @ ('x' | 'o')) => {
                let after = c.rest()[1..].chars().next();
                if after.is_none_or(|a| a.is_whitespace() || a == '|') {
                    c.pos += 1;
                    st.end = marker(ch);
                }
            }
            _ => {}
        }
        let extra = body.chars().filter(|ch| matches!(ch, '-' | '=' | '.')).count().saturating_sub(if st.end.is_some() { 2 } else { 3 });
        c.skip_ws();
        if c.eat("|") {
            let rest = c.rest();
            match rest.find('|') {
                Some(e) => {
                    st.label = Some(label_text(&rest[..e]));
                    c.pos += e + 1;
                }
                None => self.err("link label is missing its closing |"),
            }
        }
        Some((st, extra))
    }

    fn statement(&mut self, text: &str) {
        let words: Vec<&str> = text.split_whitespace().collect();
        match words.first().copied() {
            Some("subgraph") => {
                let rest = text["subgraph".len()..].trim();
                let (id, title) = match rest.find('[') {
                    Some(b) if rest.ends_with(']') => (rest[..b].trim().to_owned(), label_text(&rest[b + 1..rest.len() - 1])),
                    _ => (rest.to_owned(), label_text(rest)),
                };
                let parent = self.stack.last().copied();
                self.clusters.push((title, parent));
                let index = self.clusters.len() - 1;
                self.stack.push(index);
                // A subgraph id can be used like a node in links; we map it to
                // nothing (links to subgraphs are dropped).
                let _ = id;
                return;
            }
            Some("end") if words.len() == 1 => {
                if self.stack.pop().is_none() {
                    self.err("'end' without subgraph");
                }
                return;
            }
            Some("direction") => return,
            Some("classDef") if words.len() >= 3 => {
                let props = NodeStyle::parse(&words[2..].join(" "));
                for name in words[1].split(',') {
                    self.class_defs.insert(name.to_owned(), props);
                }
                return;
            }
            Some("class") if words.len() >= 3 => {
                let class = words[words.len() - 1].to_owned();
                for id in words[1..words.len() - 1].join("").split(',') {
                    let n = self.node(id.trim());
                    self.nodes[n].classes.push(class.clone());
                }
                return;
            }
            Some("style") if words.len() >= 3 => {
                let props = NodeStyle::parse(&words[2..].join(" "));
                let entry = self.styles.entry(words[1].to_owned()).or_default();
                *entry = entry.merge(props);
                return;
            }
            Some("linkStyle" | "click" | "accTitle" | "accDescr") => return,
            _ => {}
        }
        let mut c = Cursor { s: text, pos: 0 };
        let Some(mut from) = self.group(&mut c) else {
            self.err(format!("cannot read \"{}\"", text.chars().take(40).collect::<String>()));
            return;
        };
        loop {
            c.skip_ws();
            if c.at_end() {
                break;
            }
            let Some((style, extra)) = self.link(&mut c) else {
                self.err(format!("expected a link such as --> near \"{}\"", c.rest().chars().take(20).collect::<String>()));
                break;
            };
            let Some(to) = self.group(&mut c) else {
                self.err("link has no target node");
                break;
            };
            for &a in &from {
                for &b in &to {
                    if self.links.len() < super::MAX_STATEMENTS * 4 {
                        self.links.push((a, b, style.clone(), extra));
                    }
                }
            }
            from = to;
        }
    }
}

/// Splits a line into statements at `;` outside quotes and brackets.
fn statements(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (i, ch) in text.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out.into_iter().map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// Draws a node shape centred at `c`.
pub(crate) fn draw_shape(fig: &mut Figure, shape: Shape, c: [f32; 2], w: f32, h: f32, paint: Paint) {
    let (x, y) = (c[0] - w / 2.0, c[1] - h / 2.0);
    let poly = |pts: Vec<[f32; 2]>| Item::Poly { points: pts, closed: true, smooth: false, paint, start: None, end: None };
    match shape {
        Shape::Rect => fig.push(Item::Rect { x, y, w, h, r: 0.0, paint }),
        Shape::Round => fig.push(Item::Rect { x, y, w, h, r: 6.0, paint }),
        Shape::Stadium => fig.push(Item::Rect { x, y, w, h, r: h / 2.0, paint }),
        Shape::Subroutine => {
            fig.push(Item::Rect { x, y, w, h, r: 0.0, paint });
            let line = Paint { fill: None, ..paint };
            for dx in [7.0, w - 7.0] {
                fig.push(Item::Poly { points: vec![[x + dx, y], [x + dx, y + h]], closed: false, smooth: false, paint: line, start: None, end: None });
            }
        }
        Shape::Cylinder => {
            let ry = (h * 0.12).min(10.0);
            fig.push(Item::Rect { x, y: y + ry, w, h: h - 2.0 * ry, r: 0.0, paint: Paint { stroke: None, ..paint } });
            fig.push(Item::Ellipse { cx: c[0], cy: y + h - ry, rx: w / 2.0, ry, paint });
            let line = Paint { fill: None, ..paint };
            for dx in [0.0, w] {
                fig.push(Item::Poly { points: vec![[x + dx, y + ry], [x + dx, y + h - ry]], closed: false, smooth: false, paint: line, start: None, end: None });
            }
            fig.push(Item::Rect { x: x + paint.width, y: y + h - 2.0 * ry, w: w - 2.0 * paint.width, h: ry, r: 0.0, paint: Paint { stroke: None, ..paint } });
            fig.push(Item::Ellipse { cx: c[0], cy: y + h - ry, rx: w / 2.0, ry, paint: Paint { fill: None, ..paint } });
            fig.push(Item::Ellipse { cx: c[0], cy: y + ry, rx: w / 2.0, ry, paint });
        }
        Shape::Circle => fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: w / 2.0, ry: h / 2.0, paint }),
        Shape::DoubleCircle => {
            fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: w / 2.0, ry: h / 2.0, paint });
            fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: w / 2.0 - 4.0, ry: h / 2.0 - 4.0, paint: Paint { fill: None, ..paint } });
        }
        Shape::Asymmetric => fig.push(poly(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h], [x + h / 2.5, c[1]]])),
        Shape::Diamond => fig.push(poly(vec![[c[0], y], [x + w, c[1]], [c[0], y + h], [x, c[1]]])),
        Shape::Hexagon => {
            let d = h / 2.5;
            fig.push(poly(vec![[x + d, y], [x + w - d, y], [x + w, c[1]], [x + w - d, y + h], [x + d, y + h], [x, c[1]]]));
        }
        Shape::Parallelogram => fig.push(poly(vec![[x + h / 3.0, y], [x + w, y], [x + w - h / 3.0, y + h], [x, y + h]])),
        Shape::ParallelogramAlt => fig.push(poly(vec![[x, y], [x + w - h / 3.0, y], [x + w, y + h], [x + h / 3.0, y + h]])),
        Shape::Trapezoid => fig.push(poly(vec![[x + h / 3.0, y], [x + w - h / 3.0, y], [x + w, y + h], [x, y + h]])),
        Shape::TrapezoidAlt => fig.push(poly(vec![[x, y], [x + w, y], [x + w - h / 3.0, y + h], [x + h / 3.0, y + h]])),
    }
}

/// Size of a node for its label.
fn node_size(shape: Shape, label: &str) -> (f32, f32) {
    let (tw, th) = text_size(label, theme::FONT, false);
    let (w, h) = (tw + 30.0, th + 20.0);
    match shape {
        Shape::Circle | Shape::DoubleCircle => {
            let d = tw.max(th) + 28.0;
            (d, d)
        }
        Shape::Diamond => {
            let s = (tw + th) * 1.0 + 24.0;
            (s.max(60.0), (s * 0.7).max(50.0))
        }
        Shape::Hexagon | Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt | Shape::Asymmetric => (w + h * 0.7, h),
        Shape::Cylinder => (w, h + 16.0),
        Shape::Stadium => (w + h * 0.4, h),
        _ => (w.max(40.0), h),
    }
}

pub(crate) fn render(lines: &[Line]) -> Output {
    let header: Vec<&str> = lines[0].text.split_whitespace().collect();
    let dir = header.get(1).and_then(|d| Dir::parse(d.trim_end_matches(';'))).unwrap_or(Dir::TB);
    let mut p = Parser {
        nodes: Vec::new(),
        ids: HashMap::new(),
        links: Vec::new(),
        clusters: Vec::new(),
        stack: Vec::new(),
        styles: HashMap::new(),
        class_defs: HashMap::new(),
        errors: Vec::new(),
        line: lines[0].no,
    };
    // A header may carry a first statement after `graph TD;`.
    let first_rest = lines[0].text.split_once(';').map(|(_, r)| r.trim()).unwrap_or("");
    let rest_lines = std::iter::once((lines[0].no, first_rest)).chain(lines[1..].iter().map(|l| (l.no, l.text)));
    for (no, text) in rest_lines {
        p.line = no;
        for st in statements(text) {
            p.statement(st);
        }
    }
    if !p.stack.is_empty() {
        p.line = lines.last().map_or(1, |l| l.no);
        p.err("subgraph is not closed with 'end'");
    }

    let mut g = Graph::new(dir);
    let sizes: Vec<(f32, f32)> = p.nodes.iter().map(|n| node_size(n.shape, &n.label)).collect();
    for (n, &(w, h)) in p.nodes.iter().zip(&sizes) {
        g.nodes.push(layout::Node { w, h, outline: n.shape.outline(), cluster: n.cluster });
    }
    for (title, parent) in &p.clusters {
        let (w, h) = text_size(title, 13.0, false);
        g.clusters.push(layout::Cluster { parent: *parent, title: (w + 10.0, h + 6.0) });
    }
    for (a, b, st, extra) in &p.links {
        g.edges.push(layout::Edge { from: *a, to: *b, label: label_size(&st.label), extra: *extra });
    }
    let l = g.layout();
    let mut fig = Figure::new(l.width, l.height);
    // Clusters, outermost first.
    for (i, (title, _)) in p.clusters.iter().enumerate() {
        let b = l.clusters[i];
        if b[2] <= 0.0 {
            continue;
        }
        fig.push(Item::Rect { x: b[0], y: b[1], w: b[2], h: b[3], r: 0.0, paint: Paint::both(theme::CLUSTER_FILL, theme::CLUSTER_STROKE, 1.0) });
        super::text(&mut fig, b[0] + b[2] / 2.0, b[1] + 12.0, title, 13.0, false, Anchor::Middle, theme::TEXT);
    }
    for (i, (_, _, st, _)) in p.links.iter().enumerate() {
        draw_edge(&mut fig, &l.routes[i], st);
    }
    for (i, n) in p.nodes.iter().enumerate() {
        let mut style = NodeStyle::default();
        for class in std::iter::once("default").chain(n.classes.iter().map(String::as_str)) {
            if let Some(s) = p.class_defs.get(class) {
                style = style.merge(*s);
            }
        }
        let id = p.ids.iter().find(|(_, v)| **v == i).map(|(k, _)| k.clone()).unwrap_or_default();
        if let Some(s) = p.styles.get(&id) {
            style = style.merge(*s);
        }
        let (w, h) = sizes[i];
        draw_shape(&mut fig, n.shape, l.pos[i], w, h, style.paint(theme::NODE_FILL, theme::NODE_STROKE, 1.0));
        super::text(&mut fig, l.pos[i][0], l.pos[i][1], &n.label, theme::FONT, false, Anchor::Middle, style.color.unwrap_or(theme::TEXT));
    }
    (fig, p.errors, None)
}

#[cfg(test)]
mod tests {
    use super::super::render as mermaid;
    use crate::figure::Item;

    fn texts(src: &str) -> Vec<String> {
        mermaid(src).figure.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect()
    }

    #[test]
    fn basic_flowchart() {
        let r = mermaid("graph TD\n  A[Start] --> B{Is it?}\n  B -->|Yes| C[OK]\n  C --> D[Rethink]\n  D --> B\n  B ---->|No| E[End]");
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.kind, Some("flowchart"));
        let t = texts("graph TD\n  A[Start] --> B{Is it?}\n  B -->|Yes| C[OK]");
        for want in ["Start", "Is it?", "Yes", "OK"] {
            assert!(t.iter().any(|x| x == want), "{want} missing from {t:?}");
        }
    }

    #[test]
    fn chains_groups_text_links_and_shapes() {
        let r = mermaid("flowchart LR\n a & b --> c --> d\n c -- some text --> e((circle))\n e -. dotted .-> f[(db)]\n f == thick ==> g{{hex}}\n g --o h>flag] --x i([stadium])\n i <--> j[/para/]");
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let polys = r.figure.items.iter().filter(|i| matches!(i, Item::Poly { end: Some(_), .. })).count();
        assert!(polys >= 7, "{polys}");
        let t = texts("flowchart LR\n c -- some text --> e((circle))");
        assert!(t.contains(&"some text".to_owned()));
    }

    #[test]
    fn subgraphs_styles_and_errors() {
        let r = mermaid("graph TB\nsubgraph one [First]\n a1-->a2\nend\nsubgraph two\n b1-->b2\nend\na2-->b1\nclassDef hot fill:#f96\nclass a1 hot\nstyle b2 fill:#9f6,stroke:#333\nc:::hot --> a1");
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let bad = mermaid("graph TD\nA --> \nsubgraph x\nB");
        assert_eq!(bad.errors.len(), 2, "{:?}", bad.errors);
    }

    #[test]
    fn quoted_labels_may_contain_brackets() {
        let t = texts("graph TD\nA[\"a [b] (c)\"] --> B(\"x)y\")");
        assert!(t.contains(&"a [b] (c)".to_owned()), "{t:?}");
        assert!(t.contains(&"x)y".to_owned()), "{t:?}");
    }
}
