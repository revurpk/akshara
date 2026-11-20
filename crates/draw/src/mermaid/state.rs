//! State diagrams: `[*]` start and end states, transitions with labels,
//! `state "Description" as Id`, `Id : description`, composite states,
//! `<<choice>>` / `<<fork>>` / `<<join>>` and notes.

use super::flowchart::{draw_edge, label_size, LinkStyle};
use super::layout::{self, Dir, Graph, Outline};
use super::{label_text, theme, Diagnostic, Line, Output};
use crate::figure::{text_size, Anchor, Arrow, Figure, Item, Paint};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    State,
    Start,
    End,
    Choice,
    Bar,
    Note,
}

#[derive(Debug, Clone)]
struct Node {
    label: String,
    description: Vec<String>,
    kind: Kind,
    cluster: Option<usize>,
}

struct Parser {
    nodes: Vec<Node>,
    ids: HashMap<String, usize>,
    edges: Vec<(usize, usize, LinkStyle)>,
    clusters: Vec<(String, Option<usize>)>,
    /// Cluster stack (with the composite state's id).
    stack: Vec<usize>,
    /// Composite state id -> cluster.
    composite: HashMap<String, usize>,
    errors: Vec<Diagnostic>,
    line: usize,
    dir: Dir,
    pending_note: Option<(String, usize, String)>,
}

impl Parser {
    fn err(&mut self, msg: impl Into<String>) {
        if self.errors.len() < 50 {
            self.errors.push(Diagnostic { line: self.line, message: msg.into() });
        }
    }

    fn node(&mut self, id: &str, as_target: bool) -> usize {
        let scope = self.stack.last().copied();
        let key = if id == "[*]" { format!("[*]{}{:?}", if as_target { "end" } else { "start" }, scope) } else { id.to_owned() };
        if let Some(&i) = self.ids.get(&key) {
            return i;
        }
        let kind = match id {
            "[*]" if as_target => Kind::End,
            "[*]" => Kind::Start,
            _ => Kind::State,
        };
        self.nodes.push(Node { label: if kind == Kind::State { id.to_owned() } else { String::new() }, description: Vec::new(), kind, cluster: scope });
        self.ids.insert(key, self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// The node standing for a state id; a composite state is represented
    /// by its own start node... simplified: by a hidden anchor node.
    fn endpoint(&mut self, id: &str, as_target: bool) -> usize {
        self.node(id.trim(), as_target)
    }

    fn statement(&mut self, text: &str) {
        if let Some((target, idx, mut body)) = self.pending_note.take() {
            if text.trim() == "end note" {
                self.add_note(&target, idx, &body);
            } else {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(text.trim());
                self.pending_note = Some((target, idx, body));
            }
            return;
        }
        let t = text.trim();
        if t == "}" {
            if self.stack.pop().is_none() {
                self.err("unexpected }");
            }
            return;
        }
        if t == "--" || t.starts_with("hide empty") || t.starts_with("classDef") || t.starts_with("class ") || t.starts_with("scale ") {
            return;
        }
        if let Some(d) = t.strip_prefix("direction ") {
            if let Some(d) = Dir::parse(d) {
                if self.stack.is_empty() {
                    self.dir = d;
                }
            }
            return;
        }
        if let Some(rest) = t.strip_prefix("note ") {
            // note right of X : text | note left of X (multi-line until "end note")
            let rest = rest.trim();
            let rest = rest.strip_prefix("right of").or_else(|| rest.strip_prefix("left of")).unwrap_or(rest).trim();
            let (target, body) = match rest.split_once(':') {
                Some((a, b)) => (a.trim().to_owned(), Some(b.trim().to_owned())),
                None => (rest.to_owned(), None),
            };
            let idx = self.endpoint(&target, false);
            match body {
                Some(b) => self.add_note(&target, idx, &b),
                None => self.pending_note = Some((target, idx, String::new())),
            }
            return;
        }
        if let Some(rest) = t.strip_prefix("state ") {
            let rest = rest.trim();
            let opens = rest.ends_with('{');
            let rest = rest.trim_end_matches('{').trim();
            // state "Long name" as Id
            let (id, label) = if let Some(quoted) = rest.strip_prefix('"') {
                match quoted.find('"') {
                    Some(q) => {
                        let label = label_text(&quoted[..q]);
                        let after = quoted[q + 1..].trim();
                        (after.strip_prefix("as").map(str::trim).unwrap_or(after).to_owned(), Some(label))
                    }
                    None => (rest.to_owned(), None),
                }
            } else if let Some((id, label)) = rest.split_once(" as ") {
                // state Id as "Label" is not Mermaid, but `state A as B` names B.
                (label.trim().to_owned(), Some(label_text(id)))
            } else {
                (rest.to_owned(), None)
            };
            let (id, stereo) = match id.split_once("<<") {
                Some((a, b)) => (a.trim().to_owned(), Some(b.trim_end_matches(">>").trim().to_owned())),
                None => (id, None),
            };
            if id.is_empty() {
                self.err("state needs a name");
                return;
            }
            if opens {
                let parent = self.stack.last().copied();
                self.clusters.push((label.clone().unwrap_or_else(|| id.clone()), parent));
                let c = self.clusters.len() - 1;
                self.composite.insert(id.clone(), c);
                self.stack.push(c);
                return;
            }
            let n = self.node(&id, false);
            if let Some(l) = label {
                self.nodes[n].label = l;
            }
            match stereo.as_deref() {
                Some("choice") => self.nodes[n].kind = Kind::Choice,
                Some("fork" | "join") => self.nodes[n].kind = Kind::Bar,
                _ => {}
            }
            return;
        }
        // Transition: A --> B : label
        if let Some(pos) = t.find("-->") {
            let (a, rest) = (&t[..pos], &t[pos + 3..]);
            let (b, label) = match rest.split_once(':') {
                Some((b, l)) => (b, Some(label_text(l))),
                None => (rest, None),
            };
            let (a, b) = (a.trim(), b.trim());
            if a.is_empty() || b.is_empty() {
                self.err("transition needs two states");
                return;
            }
            let from = self.endpoint(a, false);
            let to = self.endpoint(b, true);
            self.edges.push((from, to, LinkStyle { end: Some(Arrow::Triangle), label, ..LinkStyle::default() }));
            return;
        }
        // Description: A : text
        if let Some((id, desc)) = t.split_once(':') {
            let n = self.node(id.trim(), false);
            self.nodes[n].description.push(label_text(desc));
            return;
        }
        // A bare state name.
        if t.split_whitespace().count() == 1 {
            self.node(t, false);
        } else {
            self.err(format!("cannot read \"{}\"", t.chars().take(40).collect::<String>()));
        }
    }

    fn add_note(&mut self, _target: &str, idx: usize, body: &str) {
        let cluster = self.nodes[idx].cluster;
        self.nodes.push(Node { label: label_text(body), description: Vec::new(), kind: Kind::Note, cluster });
        let note = self.nodes.len() - 1;
        self.edges.push((idx, note, LinkStyle { dash: true, ..LinkStyle::default() }));
    }
}

fn size(n: &Node) -> (f32, f32) {
    match n.kind {
        Kind::Start => (16.0, 16.0),
        Kind::End => (20.0, 20.0),
        Kind::Choice => (28.0, 28.0),
        Kind::Bar => (70.0, 8.0),
        Kind::Note => {
            let (w, h) = text_size(&n.label, 12.0, false);
            (w + 20.0, h + 14.0)
        }
        Kind::State => {
            let (w, h) = text_size(&n.label, theme::FONT, false);
            let desc = n.description.join("\n");
            let (dw, dh) = if desc.is_empty() { (0.0, 0.0) } else { text_size(&desc, 12.0, false) };
            ((w.max(dw) + 30.0).max(60.0), h + 18.0 + if desc.is_empty() { 0.0 } else { dh + 10.0 })
        }
    }
}

pub(crate) fn render(lines: &[Line]) -> Output {
    let mut p = Parser {
        nodes: Vec::new(),
        ids: HashMap::new(),
        edges: Vec::new(),
        clusters: Vec::new(),
        stack: Vec::new(),
        composite: HashMap::new(),
        errors: Vec::new(),
        line: lines[0].no,
        dir: Dir::TB,
        pending_note: None,
    };
    for l in &lines[1..] {
        p.line = l.no;
        p.statement(l.text);
    }
    if !p.stack.is_empty() {
        p.err("composite state is not closed with }");
    }
    // Transitions to a composite state go to its start (or first) node.
    let composite = p.composite.clone();
    for (id, &c) in &composite {
        if let Some(&n) = p.ids.get(id) {
            let target = p
                .nodes
                .iter()
                .position(|x| x.cluster == Some(c) && x.kind == Kind::Start)
                .or_else(|| p.nodes.iter().position(|x| x.cluster == Some(c)));
            if let Some(t) = target {
                for e in &mut p.edges {
                    if e.0 == n {
                        e.0 = t;
                    }
                    if e.1 == n {
                        e.1 = t;
                    }
                }
                p.nodes[n].kind = Kind::Note; // hidden below
                p.nodes[n].label.clear();
                p.nodes[n].description.clear();
            }
        }
    }
    let hidden: Vec<bool> = p.nodes.iter().map(|n| n.kind == Kind::Note && n.label.is_empty()).collect();

    let mut g = Graph::new(p.dir);
    g.node_sep = 34.0;
    let sizes: Vec<(f32, f32)> = p.nodes.iter().map(size).collect();
    for (i, n) in p.nodes.iter().enumerate() {
        let (w, h) = if hidden[i] { (1.0, 1.0) } else { sizes[i] };
        let outline = match n.kind {
            Kind::Start | Kind::End => Outline::Ellipse,
            Kind::Choice => Outline::Diamond,
            _ => Outline::Rect,
        };
        g.nodes.push(layout::Node { w, h, outline, cluster: n.cluster });
    }
    for (title, parent) in &p.clusters {
        let (w, h) = text_size(title, 13.0, true);
        g.clusters.push(layout::Cluster { parent: *parent, title: (w + 10.0, h + 6.0) });
    }
    for (a, b, st) in &p.edges {
        g.edges.push(layout::Edge { from: *a, to: *b, label: label_size(&st.label), extra: 0 });
    }
    let l = g.layout();
    let mut fig = Figure::new(l.width, l.height);
    for (i, (title, _)) in p.clusters.iter().enumerate() {
        let b = l.clusters[i];
        if b[2] > 0.0 {
            fig.push(Item::Rect { x: b[0], y: b[1], w: b[2], h: b[3], r: 8.0, paint: Paint::both(rgb(0xff, 0xff, 0xff), theme::NODE_STROKE, 1.0) });
            super::text(&mut fig, b[0] + b[2] / 2.0, b[1] + 12.0, title, 13.0, true, Anchor::Middle, theme::TEXT);
        }
    }
    for (i, (_, _, st)) in p.edges.iter().enumerate() {
        draw_edge(&mut fig, &l.routes[i], st);
    }
    let node_paint = Paint::both(theme::NODE_FILL, theme::NODE_STROKE, 1.0);
    for (i, n) in p.nodes.iter().enumerate() {
        if hidden[i] {
            continue;
        }
        let c = l.pos[i];
        let (w, h) = sizes[i];
        match n.kind {
            Kind::Start => fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: 7.0, ry: 7.0, paint: Paint::fill(theme::LINE) }),
            Kind::End => {
                fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: 9.0, ry: 9.0, paint: Paint::both(rgb(255, 255, 255), theme::LINE, 1.2) });
                fig.push(Item::Ellipse { cx: c[0], cy: c[1], rx: 5.5, ry: 5.5, paint: Paint::fill(theme::LINE) });
            }
            Kind::Choice => super::flowchart::draw_shape(&mut fig, super::flowchart::Shape::Diamond, c, w, h, node_paint),
            Kind::Bar => fig.push(Item::Rect { x: c[0] - w / 2.0, y: c[1] - h / 2.0, w, h, r: 2.0, paint: Paint::fill(theme::LINE) }),
            Kind::Note => {
                fig.push(Item::Rect { x: c[0] - w / 2.0, y: c[1] - h / 2.0, w, h, r: 0.0, paint: Paint::both(theme::NOTE_FILL, theme::NOTE_STROKE, 1.0) });
                super::text(&mut fig, c[0], c[1], &n.label, 12.0, false, Anchor::Middle, theme::TEXT);
            }
            Kind::State => {
                let (x, y) = (c[0] - w / 2.0, c[1] - h / 2.0);
                fig.push(Item::Rect { x, y, w, h, r: 8.0, paint: node_paint });
                if n.description.is_empty() {
                    super::text(&mut fig, c[0], c[1], &n.label, theme::FONT, false, Anchor::Middle, theme::TEXT);
                } else {
                    let (_, th) = text_size(&n.label, theme::FONT, false);
                    super::text(&mut fig, c[0], y + 9.0 + th / 2.0, &n.label, theme::FONT, false, Anchor::Middle, theme::TEXT);
                    let sep = y + th + 16.0;
                    fig.push(Item::Poly { points: vec![[x, sep], [x + w, sep]], closed: false, smooth: false, paint: Paint::line(theme::NODE_STROKE, 1.0), start: None, end: None });
                    let desc = n.description.join("\n");
                    let (_, dh) = text_size(&desc, 12.0, false);
                    super::text(&mut fig, x + 10.0, sep + 5.0 + dh / 2.0, &desc, 12.0, false, Anchor::Start, theme::TEXT);
                }
            }
        }
    }
    (fig, p.errors, None)
}

const fn rgb(r: u8, g: u8, b: u8) -> office_odf::Rgb {
    office_odf::Rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::super::render as mermaid;

    #[test]
    fn state_diagram() {
        let r = mermaid(
            "stateDiagram-v2\n[*] --> Still\nStill --> [*]\nStill --> Moving : push\nMoving --> Crash\nCrash --> [*]\nstate \"Moving fast\" as Moving\nMoving : speed > 0\nstate Choice <<choice>>\nCrash --> Choice\nnote right of Crash : ouch\nstate Busy {\n  [*] --> Work\n  Work --> [*]\n}\nStill --> Busy",
        );
        assert_eq!(r.kind, Some("state"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.figure.items.len() > 20);
    }

    #[test]
    fn multi_line_notes_and_errors() {
        let r = mermaid("stateDiagram\nA --> B\nnote left of A\n  line one\n  line two\nend note\nstate X {\nthis is wrong\n");
        assert_eq!(r.errors.len(), 2, "{:?}", r.errors);
    }
}
