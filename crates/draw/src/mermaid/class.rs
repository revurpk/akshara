//! Class diagrams: classes with attributes and methods, stereotypes,
//! generics, relations (inheritance, composition, aggregation,
//! association, dependency, realisation) with labels and cardinalities,
//! and notes.

use super::flowchart::{draw_edge, label_size, LinkStyle};
use super::layout::{self, Dir, Graph, Outline};
use super::{label_text, theme, Diagnostic, Line, Output};
use crate::figure::{text_size, Anchor, Arrow, Figure, Item, Paint};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct Class {
    name: String,
    stereotype: Option<String>,
    attributes: Vec<String>,
    methods: Vec<String>,
    note: bool,
}

struct Rel {
    from: usize,
    to: usize,
    style: LinkStyle,
    card_from: Option<String>,
    card_to: Option<String>,
}

/// A relation operator: (start, end, left arrow, dashed, right arrow).
type Relation = (usize, usize, Option<Arrow>, bool, Option<Arrow>);

struct Parser {
    classes: Vec<Class>,
    ids: HashMap<String, usize>,
    rels: Vec<Rel>,
    errors: Vec<Diagnostic>,
    line: usize,
    dir: Dir,
    open: Option<usize>,
}

fn display_name(id: &str) -> String {
    // Generics: Class~T~ -> Class<T>
    let mut s = id.to_owned();
    if let Some(a) = s.find('~') {
        if let Some(b) = s[a + 1..].find('~') {
            s = format!("{}<{}>{}", &s[..a], &s[a + 1..a + 1 + b], &s[a + 2 + b..]);
        }
    }
    s
}

fn base_id(id: &str) -> &str {
    id.split('~').next().unwrap_or(id).trim()
}

impl Parser {
    fn err(&mut self, msg: impl Into<String>) {
        if self.errors.len() < 50 {
            self.errors.push(Diagnostic { line: self.line, message: msg.into() });
        }
    }

    fn class(&mut self, id: &str) -> usize {
        let key = base_id(id).to_owned();
        if let Some(&i) = self.ids.get(&key) {
            if id.contains('~') {
                self.classes[i].name = display_name(id);
            }
            return i;
        }
        self.classes.push(Class { name: display_name(id), ..Class::default() });
        self.ids.insert(key, self.classes.len() - 1);
        self.classes.len() - 1
    }

    fn member(&mut self, c: usize, m: &str) {
        let m = m.trim();
        if m.is_empty() {
            return;
        }
        if let Some(st) = m.strip_prefix("<<").and_then(|x| x.strip_suffix(">>")) {
            self.classes[c].stereotype = Some(st.trim().to_owned());
        } else if m.contains('(') {
            self.classes[c].methods.push(display_name(m));
        } else {
            self.classes[c].attributes.push(display_name(m));
        }
    }

    /// Finds a relation operator: returns (start, end, left arrow, dashed, right arrow).
    fn relation(t: &str) -> Option<Relation> {
        let bytes = t.as_bytes();
        let pos = t.find("--").map(|p| (p, false)).into_iter().chain(t.find("..").map(|p| (p, true))).min_by_key(|(p, _)| *p)?;
        let (mid, dashed) = pos;
        let mut start = mid;
        let mut left = None;
        for (pat, arrow) in [("<|", Arrow::Hollow), ("*", Arrow::Diamond), ("o", Arrow::HollowDiamond), ("<", Arrow::Open)] {
            if t[..mid].ends_with(pat) {
                let s = mid - pat.len();
                // 'o' must be a separate token, not the end of a class name.
                if pat == "o" && s > 0 && !bytes[s - 1].is_ascii_whitespace() {
                    continue;
                }
                start = s;
                left = Some(arrow);
                break;
            }
        }
        let mut end = mid + 2;
        let mut right = None;
        for (pat, arrow) in [("|>", Arrow::Hollow), ("*", Arrow::Diamond), ("o", Arrow::HollowDiamond), (">", Arrow::Open)] {
            if t[end..].starts_with(pat) {
                let e = end + pat.len();
                if pat == "o" && e < t.len() && !bytes[e].is_ascii_whitespace() {
                    continue;
                }
                end = e;
                right = Some(arrow);
                break;
            }
        }
        Some((start, end, left, dashed, right))
    }

    fn statement(&mut self, text: &str) {
        let t = text.trim();
        if let Some(c) = self.open {
            if t == "}" {
                self.open = None;
            } else {
                self.member(c, t);
            }
            return;
        }
        if let Some(d) = t.strip_prefix("direction ") {
            if let Some(d) = Dir::parse(d) {
                self.dir = d;
            }
            return;
        }
        if t.starts_with("classDef") || t.starts_with("cssClass") || t.starts_with("style ") || t.starts_with("click ") || t.starts_with("link ") || t.starts_with("callback ") {
            return;
        }
        if let Some(rest) = t.strip_prefix("note") {
            let rest = rest.trim();
            let (target, body) = match rest.strip_prefix("for ") {
                Some(r) => match r.trim().split_once(char::is_whitespace) {
                    Some((id, body)) => (Some(id.to_owned()), body),
                    None => (None, r),
                },
                None => (None, rest),
            };
            self.classes.push(Class { name: label_text(body).replace("\\n", "\n"), note: true, ..Class::default() });
            let note = self.classes.len() - 1;
            if let Some(id) = target {
                let c = self.class(&id);
                self.rels.push(Rel { from: note, to: c, style: LinkStyle { dash: true, ..LinkStyle::default() }, card_from: None, card_to: None });
            }
            return;
        }
        if let Some(rest) = t.strip_prefix("class ") {
            let rest = rest.trim();
            let opens = rest.ends_with('{');
            let body = rest.trim_end_matches('{').trim();
            let (id, label) = match body.split_once('[') {
                Some((id, l)) => (id.trim(), Some(label_text(l.trim_end_matches(']')))),
                None => (body, None),
            };
            let (id, stereo) = match id.split_once("<<") {
                Some((a, b)) => (a.trim(), Some(b.trim_end_matches(">>").trim().to_owned())),
                None => (id, None),
            };
            if id.is_empty() {
                self.err("class needs a name");
                return;
            }
            let c = self.class(id);
            if let Some(l) = label {
                self.classes[c].name = l;
            }
            if stereo.is_some() {
                self.classes[c].stereotype = stereo;
            }
            if opens {
                self.open = Some(c);
            }
            return;
        }
        if let Some(st) = t.strip_prefix("<<") {
            if let Some((stereo, id)) = st.split_once(">>") {
                let c = self.class(id.trim());
                self.classes[c].stereotype = Some(stereo.trim().to_owned());
                return;
            }
        }
        if let Some((start, end, left, dashed, right)) = Self::relation(t) {
            let lhs = t[..start].trim();
            let (rhs, label) = match t[end..].split_once(':') {
                Some((r, l)) => (r.trim(), Some(label_text(l))),
                None => (t[end..].trim(), None),
            };
            let split_card = |side: &str, card_last: bool| -> (String, Option<String>) {
                let side = side.trim();
                if card_last {
                    if let Some(q) = side.rfind('"') {
                        if let Some(p) = side[..q].rfind('"') {
                            return (side[..p].trim().to_owned(), Some(side[p + 1..q].to_owned()));
                        }
                    }
                } else if let Some(stripped) = side.strip_prefix('"') {
                    if let Some(q) = stripped.find('"') {
                        return (stripped[q + 1..].trim().to_owned(), Some(stripped[..q].to_owned()));
                    }
                }
                (side.to_owned(), None)
            };
            let (a, card_a) = split_card(lhs, true);
            let (b, card_b) = split_card(rhs, false);
            if a.is_empty() || b.is_empty() {
                self.err("relation needs two classes");
                return;
            }
            let (ia, ib) = (self.class(&a), self.class(&b));
            // Draw from b to a when only the left end is decorated, so
            // inheritance arrows point at the parent above.
            let style = LinkStyle { dash: dashed, start: left, end: right, label, ..LinkStyle::default() };
            self.rels.push(Rel { from: ia, to: ib, style, card_from: card_a, card_to: card_b });
            return;
        }
        if let Some((id, member)) = t.split_once(':') {
            let c = self.class(id.trim());
            self.member(c, member);
            return;
        }
        if t.split_whitespace().count() == 1 {
            self.class(t);
            return;
        }
        self.err(format!("cannot read \"{}\"", t.chars().take(40).collect::<String>()));
    }
}

const NAME: f32 = 14.0;
const MEMBER: f32 = 12.0;

fn sections(c: &Class) -> (String, String) {
    (c.attributes.join("\n"), c.methods.join("\n"))
}

fn size(c: &Class) -> (f32, f32) {
    if c.note {
        let (w, h) = text_size(&c.name, MEMBER, false);
        return (w + 20.0, h + 14.0);
    }
    let (nw, nh) = text_size(&c.name, NAME, true);
    let stereo = c.stereotype.as_ref().map(|s| text_size(&format!("«{s}»"), MEMBER, false)).unwrap_or((0.0, 0.0));
    let (a, m) = sections(c);
    let sa = if a.is_empty() { (0.0, MEMBER) } else { text_size(&a, MEMBER, false) };
    let sm = if m.is_empty() { (0.0, MEMBER) } else { text_size(&m, MEMBER, false) };
    let w = nw.max(stereo.0).max(sa.0).max(sm.0) + 24.0;
    (w.max(80.0), nh + stereo.1 + 12.0 + sa.1 + 10.0 + sm.1 + 10.0)
}

pub(crate) fn render(lines: &[Line]) -> Output {
    let mut p = Parser { classes: Vec::new(), ids: HashMap::new(), rels: Vec::new(), errors: Vec::new(), line: lines[0].no, dir: Dir::TB, open: None };
    for l in &lines[1..] {
        p.line = l.no;
        p.statement(l.text);
    }
    if p.open.is_some() {
        p.err("class body is not closed with }");
    }
    let mut g = Graph::new(p.dir);
    g.rank_sep = 60.0;
    let sizes: Vec<(f32, f32)> = p.classes.iter().map(size).collect();
    for &(w, h) in &sizes {
        g.nodes.push(layout::Node { w, h, outline: Outline::Rect, cluster: None });
    }
    // Inheritance and composition: the decorated end is the "parent" and
    // goes above, so lay those edges out from the decorated side.
    let flip: Vec<bool> = p.rels.iter().map(|r| r.style.start.is_some() && r.style.end.is_none()).collect();
    for (i, r) in p.rels.iter().enumerate() {
        let (from, to) = if flip[i] { (r.to, r.from) } else { (r.from, r.to) };
        g.edges.push(layout::Edge { from, to, label: label_size(&r.style.label), extra: 0 });
    }
    let l = g.layout();
    let mut fig = Figure::new(l.width, l.height);
    for (i, r) in p.rels.iter().enumerate() {
        let mut route = l.routes[i].clone();
        if flip[i] {
            route.points.reverse();
        }
        draw_edge(&mut fig, &route, &r.style);
        let n = route.points.len();
        if n >= 2 {
            for (card, at, from) in [(&r.card_from, route.points[0], route.points[1]), (&r.card_to, route.points[n - 1], route.points[n - 2])] {
                if let Some(card) = card {
                    let (dx, dy) = (from[0] - at[0], from[1] - at[1]);
                    let len = (dx * dx + dy * dy).sqrt().max(1.0);
                    let (ux, uy) = (dx / len, dy / len);
                    let pos = [at[0] + ux * 16.0 - uy * 10.0, at[1] + uy * 16.0 + ux * 10.0];
                    super::text(&mut fig, pos[0], pos[1], card, 11.0, false, Anchor::Middle, theme::TEXT);
                }
            }
        }
    }
    let line = Paint::line(theme::NODE_STROKE, 1.0);
    for (i, c) in p.classes.iter().enumerate() {
        let (w, h) = sizes[i];
        let (x, y) = (l.pos[i][0] - w / 2.0, l.pos[i][1] - h / 2.0);
        if c.note {
            fig.push(Item::Rect { x, y, w, h, r: 0.0, paint: Paint::both(theme::NOTE_FILL, theme::NOTE_STROKE, 1.0) });
            super::text(&mut fig, x + w / 2.0, y + h / 2.0, &c.name, MEMBER, false, Anchor::Middle, theme::TEXT);
            continue;
        }
        fig.push(Item::Rect { x, y, w, h, r: 0.0, paint: Paint::both(theme::NODE_FILL, theme::NODE_STROKE, 1.0) });
        let mut cy = y + 6.0;
        if let Some(st) = &c.stereotype {
            let s = format!("«{st}»");
            let (_, sh) = text_size(&s, MEMBER, false);
            super::text(&mut fig, x + w / 2.0, cy + sh / 2.0, &s, MEMBER, false, Anchor::Middle, theme::TEXT);
            cy += sh;
        }
        let (_, nh) = text_size(&c.name, NAME, true);
        super::text(&mut fig, x + w / 2.0, cy + nh / 2.0, &c.name, NAME, true, Anchor::Middle, theme::TEXT);
        cy += nh + 6.0;
        let (attrs, methods) = sections(c);
        for body in [attrs, methods] {
            fig.push(Item::Poly { points: vec![[x, cy], [x + w, cy]], closed: false, smooth: false, paint: line, start: None, end: None });
            let bh = if body.is_empty() { MEMBER } else { text_size(&body, MEMBER, false).1 };
            super::text(&mut fig, x + 10.0, cy + 5.0 + bh / 2.0, &body, MEMBER, false, Anchor::Start, theme::TEXT);
            cy += bh + 10.0;
        }
    }
    (fig, p.errors, None)
}

#[cfg(test)]
mod tests {
    use super::super::render as mermaid;
    use crate::figure::{Arrow, Item};

    #[test]
    fn class_diagram() {
        let r = mermaid(
            "classDiagram\nAnimal <|-- Duck : is a\nAnimal <|-- Fish\nAnimal : +int age\nAnimal: +isMammal()\nclass Duck{\n  +String beakColor\n  +swim()\n}\nclass Shape~T~\n<<interface>> Shape\nCar \"1\" *-- \"4\" Wheel : has\nnote for Duck \"can fly\"\nZoo o-- Animal",
        );
        assert_eq!(r.kind, Some("class"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let hollow = r.figure.items.iter().filter(|i| matches!(i, Item::Poly { start: Some(Arrow::Hollow), .. })).count();
        assert_eq!(hollow, 2);
        let texts: Vec<String> = r.figure.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect();
        assert!(texts.contains(&"Shape<T>".to_owned()), "{texts:?}");
        assert!(texts.contains(&"«interface»".to_owned()));
        assert!(texts.contains(&"4".to_owned()));
    }
}
