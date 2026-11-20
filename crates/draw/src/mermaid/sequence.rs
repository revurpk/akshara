//! Sequence diagrams: participants and actors, messages of every arrow
//! type, self messages, activations, notes, `autonumber`, and `loop` /
//! `alt` / `else` / `opt` / `par` / `and` / `critical` / `break` / `rect`
//! blocks.

use super::{label_text, theme, Diagnostic, Line, Output};
use crate::figure::{text_size, text_width, Anchor, Arrow, Figure, Item, Paint};
use office_odf::Rgb;

#[derive(Debug, Clone)]
struct Participant {
    id: String,
    label: String,
    actor: bool,
}

#[derive(Debug, Clone)]
enum Event {
    Message { from: usize, to: usize, text: String, dash: bool, end: Option<Arrow>, start: Option<Arrow>, activate: Option<bool> },
    Note { cols: (usize, usize), side: i8, text: String },
    Begin { kind: String, label: String },
    Section { label: String },
    End,
    Activate { who: usize, on: bool },
}

struct Parser {
    parts: Vec<Participant>,
    events: Vec<Event>,
    errors: Vec<Diagnostic>,
    line: usize,
    autonumber: bool,
    title: Option<String>,
    depth: Vec<bool>, // true = drawn block, false = box group
}

const ARROWS: &[(&str, bool, Option<Arrow>, Option<Arrow>)] = &[
    ("<<-->>", true, Some(Arrow::Triangle), Some(Arrow::Triangle)),
    ("<<->>", false, Some(Arrow::Triangle), Some(Arrow::Triangle)),
    ("-->>", true, None, Some(Arrow::Triangle)),
    ("->>", false, None, Some(Arrow::Triangle)),
    ("--x", true, None, Some(Arrow::Cross)),
    ("-x", false, None, Some(Arrow::Cross)),
    ("--)", true, None, Some(Arrow::Open)),
    ("-)", false, None, Some(Arrow::Open)),
    ("-->", true, None, None),
    ("->", false, None, None),
];

impl Parser {
    fn err(&mut self, msg: impl Into<String>) {
        if self.errors.len() < 50 {
            self.errors.push(Diagnostic { line: self.line, message: msg.into() });
        }
    }

    fn part(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.parts.iter().position(|p| p.id == id) {
            return i;
        }
        self.parts.push(Participant { id: id.to_owned(), label: label_text(id), actor: false });
        self.parts.len() - 1
    }

    fn statement(&mut self, t: &str) {
        let t = t.trim();
        let word = t.split_whitespace().next().unwrap_or("");
        let rest = t[word.len()..].trim();
        match word {
            "participant" | "actor" | "create" | "destroy" => {
                let (rest, actor) = if word == "create" || word == "destroy" {
                    let w2 = rest.split_whitespace().next().unwrap_or("");
                    (rest[w2.len()..].trim(), w2 == "actor")
                } else {
                    (rest, word == "actor")
                };
                if rest.is_empty() {
                    self.err("participant needs a name");
                    return;
                }
                let (id, label) = match rest.split_once(" as ") {
                    Some((id, label)) => (id.trim(), Some(label_text(label))),
                    None => (rest, None),
                };
                let i = self.part(super::unquote(id));
                if let Some(l) = label {
                    self.parts[i].label = l;
                }
                self.parts[i].actor |= actor;
            }
            "autonumber" => self.autonumber = true,
            "title" | "title:" => self.title = Some(label_text(rest.trim_start_matches(':'))),
            "activate" | "deactivate" => {
                let who = self.part(rest);
                self.events.push(Event::Activate { who, on: word == "activate" });
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" => {
                self.depth.push(true);
                self.events.push(Event::Begin { kind: word.to_owned(), label: label_text(rest) });
            }
            "box" => self.depth.push(false),
            "else" | "and" | "option" => {
                if self.depth.last() != Some(&true) {
                    self.err(format!("'{word}' outside a block"));
                }
                self.events.push(Event::Section { label: label_text(rest) });
            }
            "end" => match self.depth.pop() {
                Some(true) => self.events.push(Event::End),
                Some(false) => {}
                None => self.err("'end' without a block"),
            },
            w if w.eq_ignore_ascii_case("note") => {
                let Some((place, text)) = rest.split_once(':') else {
                    self.err("note needs ': text'");
                    return;
                };
                let place = place.trim();
                let (side, who) = if let Some(w) = place.strip_prefix("right of") {
                    (1, w)
                } else if let Some(w) = place.strip_prefix("left of") {
                    (-1, w)
                } else if let Some(w) = place.strip_prefix("over") {
                    (0, w)
                } else {
                    self.err("note must be 'left of', 'right of' or 'over'");
                    return;
                };
                let names: Vec<&str> = who.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                if names.is_empty() {
                    self.err("note needs a participant");
                    return;
                }
                let a = self.part(names[0]);
                let b = names.get(1).map(|n| self.part(n)).unwrap_or(a);
                self.events.push(Event::Note { cols: (a.min(b), a.max(b)), side, text: label_text(text).replace("\\n", "\n") });
            }
            _ => {
                let found = ARROWS.iter().filter_map(|a| t.find(a.0).map(|p| (p, a))).min_by(|x, y| x.0.cmp(&y.0).then(y.1 .0.len().cmp(&x.1 .0.len())));
                let Some((pos, &(tok, dash, start, end))) = found else {
                    self.err(format!("cannot read \"{}\"", t.chars().take(40).collect::<String>()));
                    return;
                };
                let lhs = t[..pos].trim();
                let after = &t[pos + tok.len()..];
                let (rhs, text) = match after.split_once(':') {
                    Some((r, x)) => (r.trim(), label_text(x)),
                    None => (after.trim(), String::new()),
                };
                let (rhs, activate) = match rhs.chars().next() {
                    Some('+') => (&rhs[1..], Some(true)),
                    Some('-') => (&rhs[1..], Some(false)),
                    _ => (rhs, None),
                };
                if lhs.is_empty() || rhs.trim().is_empty() {
                    self.err("message needs a sender and a receiver");
                    return;
                }
                let from = self.part(lhs);
                let to = self.part(rhs);
                self.events.push(Event::Message { from, to, text, dash, start, end, activate });
            }
        }
    }
}

/// An open block: (kind, label, top, sections, first and last column).
type Frame = (String, String, f32, Vec<(f32, String)>, usize, usize);

const BOX_H: f32 = 38.0;
const MSG_FONT: f32 = 13.0;

pub(crate) fn render(lines: &[Line]) -> Output {
    let mut p = Parser { parts: Vec::new(), events: Vec::new(), errors: Vec::new(), line: lines[0].no, autonumber: false, title: None, depth: Vec::new() };
    for l in &lines[1..] {
        p.line = l.no;
        p.statement(l.text);
    }
    if !p.depth.is_empty() {
        p.err("block is not closed with 'end'");
    }
    let n = p.parts.len();
    if n == 0 {
        return (Figure::default(), p.errors, p.title);
    }
    // Column widths and gaps.
    let widths: Vec<f32> = p.parts.iter().map(|x| (text_size(&x.label, theme::FONT, false).0 + 30.0).max(80.0)).collect();
    let mut gaps: Vec<f32> = (0..n.saturating_sub(1)).map(|i| widths[i] / 2.0 + widths[i + 1] / 2.0 + 40.0).collect();
    let mut self_room = vec![0.0f32; n];
    let mut number = 0;
    for e in &p.events {
        match e {
            Event::Message { from, to, text, .. } => {
                number += 1;
                let label = if p.autonumber { format!("{number} {text}") } else { text.clone() };
                let w = text.split('\n').map(|l| text_width(l, MSG_FONT, false)).fold(0.0, f32::max).max(text_width(&label, MSG_FONT, false));
                if from == to {
                    self_room[*from] = self_room[*from].max(w + 40.0);
                } else {
                    let (a, b) = ((*from).min(*to), (*from).max(*to));
                    let have: f32 = gaps[a..b].iter().sum();
                    if have < w + 30.0 {
                        gaps[b - 1] += w + 30.0 - have;
                    }
                }
            }
            Event::Note { cols: (a, b), side, text } => {
                let w = text_size(text, 12.0, false).0 + 20.0;
                if *side != 0 {
                    self_room[*a] = self_room[*a].max(w + 20.0);
                } else if a != b {
                    let have: f32 = gaps[*a..*b].iter().sum();
                    if have < w {
                        gaps[b - 1] += w - have;
                    }
                }
            }
            _ => {}
        }
    }
    for i in 0..n.saturating_sub(1) {
        gaps[i] = gaps[i].max(widths[i] / 2.0 + self_room[i] + 10.0);
    }
    let mut xs = vec![widths[0] / 2.0 + 20.0 + self_room.first().map_or(0.0, |_| 0.0)];
    for g in &gaps {
        let last = xs[xs.len() - 1];
        xs.push(last + g);
    }
    let left_note = p.events.iter().any(|e| matches!(e, Event::Note { side: -1, cols: (0, _), .. }));
    if left_note {
        let extra = p.events.iter().filter_map(|e| match e { Event::Note { side: -1, cols: (0, _), text } => Some(text_size(text, 12.0, false).0 + 30.0), _ => None }).fold(0.0, f32::max);
        for x in &mut xs {
            *x += extra;
        }
    }
    let width = xs[n - 1] + widths[n - 1] / 2.0 + 20.0 + self_room[n - 1];

    // Rows.
    let mut items: Vec<Item> = Vec::new();
    let mut frames: Vec<Item> = Vec::new();
    let mut y = 20.0 + BOX_H + 24.0;
    let line = Paint::line(theme::LINE, 1.2);
    let mut open: Vec<Frame> = Vec::new();
    let mut active: Vec<Vec<f32>> = vec![Vec::new(); n];
    let mut bars: Vec<(usize, f32, f32, usize)> = Vec::new();
    let mut number = 0;
    let touch = |open: &mut Vec<Frame>, a: usize, b: usize| {
        for f in open.iter_mut() {
            f.4 = f.4.min(a);
            f.5 = f.5.max(b);
        }
    };
    let offset = |active: &Vec<Vec<f32>>, i: usize| active[i].len().min(4) as f32 * 4.0;
    for e in &p.events {
        match e {
            Event::Message { from, to, text, dash, end, start, activate } => {
                number += 1;
                let label = if p.autonumber { format!("{number}. {text}") } else { text.clone() };
                let (_, th) = if label.is_empty() { (0.0, 0.0) } else { text_size(&label, MSG_FONT, false) };
                y += th;
                let mut paint = line;
                paint.dash = *dash;
                if from == to {
                    let x = xs[*from] + offset(&active, *from);
                    let pts = vec![[x, y], [x + 32.0, y], [x + 32.0, y + 18.0], [x + 2.0, y + 18.0]];
                    items.push(Item::Poly { points: pts, closed: false, smooth: false, paint, start: *start, end: *end });
                    super::text_item(&mut items, x + 38.0, y - th / 2.0 + 2.0, &label, MSG_FONT, Anchor::Start);
                    y += 18.0;
                } else {
                    let dir = if to > from { 1.0 } else { -1.0 };
                    let x1 = xs[*from] + dir * offset(&active, *from);
                    let x2 = xs[*to] - dir * (offset(&active, *to) + if activate == &Some(true) { 4.0 } else { 0.0 });
                    items.push(Item::Poly { points: vec![[x1, y], [x2, y]], closed: false, smooth: false, paint, start: *start, end: *end });
                    super::text_item(&mut items, (x1 + x2) / 2.0, y - th / 2.0 - 3.0, &label, MSG_FONT, Anchor::Middle);
                }
                touch(&mut open, (*from).min(*to), (*from).max(*to));
                match activate {
                    Some(true) => active[*to].push(y),
                    Some(false) => {
                        if let Some(s) = active[*from].pop() {
                            bars.push((*from, s, y, active[*from].len()));
                        }
                    }
                    None => {}
                }
                y += 26.0;
            }
            Event::Activate { who, on } => {
                if *on {
                    active[*who].push(y - 10.0);
                } else if let Some(s) = active[*who].pop() {
                    bars.push((*who, s, y - 10.0, active[*who].len()));
                }
            }
            Event::Note { cols: (a, b), side, text } => {
                let (tw, th) = text_size(text, 12.0, false);
                let (w, h) = (tw + 20.0, th + 12.0);
                let (x, w) = match side {
                    1 => (xs[*a] + 10.0, w),
                    -1 => (xs[*a] - 10.0 - w, w),
                    _ if a == b => (xs[*a] - w / 2.0, w),
                    _ => {
                        let span = xs[*b] - xs[*a] + 40.0;
                        let w = w.max(span);
                        ((xs[*a] + xs[*b]) / 2.0 - w / 2.0, w)
                    }
                };
                items.push(Item::Rect { x, y: y - 6.0, w, h, r: 0.0, paint: Paint::both(theme::NOTE_FILL, theme::NOTE_STROKE, 1.0) });
                super::text_item(&mut items, x + w / 2.0, y - 6.0 + h / 2.0, text, 12.0, Anchor::Middle);
                touch(&mut open, *a, *b);
                y += h + 14.0;
            }
            Event::Begin { kind, label } => {
                open.push((kind.clone(), label.clone(), y - 8.0, Vec::new(), usize::MAX, 0));
                y += 30.0;
            }
            Event::Section { label } => {
                if let Some(f) = open.last_mut() {
                    f.3.push((y - 6.0, label.clone()));
                }
                y += 26.0;
            }
            Event::End => {
                if let Some((kind, label, top, sections, a, b)) = open.pop() {
                    let (a, b) = if a == usize::MAX { (0, n - 1) } else { (a, b) };
                    let depth = open.len() as f32;
                    let x0 = xs[a] - widths[a] / 2.0 - 4.0 + depth * 6.0;
                    let x1 = (xs[b] + widths[b] / 2.0 + 4.0 - depth * 6.0).max(x0 + 120.0);
                    let bottom = y;
                    if kind == "rect" {
                        let color = super::parse_color(&label).unwrap_or(Rgb(0xf0, 0xf0, 0xf8));
                        frames.push(Item::Rect { x: x0, y: top, w: x1 - x0, h: bottom - top, r: 0.0, paint: Paint::fill(color) });
                    } else {
                        let frame = Paint::line(Rgb(0x88, 0x88, 0x88), 1.0);
                        items.push(Item::Rect { x: x0, y: top, w: x1 - x0, h: bottom - top, r: 0.0, paint: frame });
                        let kw = text_width(&kind, 12.0, true) + 14.0;
                        items.push(Item::Poly {
                            points: vec![[x0, top], [x0 + kw, top], [x0 + kw, top + 14.0], [x0 + kw - 6.0, top + 20.0], [x0, top + 20.0]],
                            closed: true,
                            smooth: false,
                            paint: Paint::both(theme::NODE_FILL, Rgb(0x88, 0x88, 0x88), 1.0),
                            start: None,
                            end: None,
                        });
                        super::text_item_bold(&mut items, x0 + kw / 2.0, top + 10.0, &kind, 12.0);
                        if !label.is_empty() {
                            super::text_item(&mut items, x0 + kw + 8.0, top + 10.0, &format!("[{label}]"), 12.0, Anchor::Start);
                        }
                        for (sy, sl) in sections {
                            items.push(Item::Poly { points: vec![[x0, sy], [x1, sy]], closed: false, smooth: false, paint: frame.dashed(), start: None, end: None });
                            if !sl.is_empty() {
                                super::text_item(&mut items, x0 + 8.0, sy + 10.0, &format!("[{sl}]"), 12.0, Anchor::Start);
                            }
                        }
                    }
                    y += 14.0;
                }
            }
        }
    }
    for (who, list) in active.iter().enumerate() {
        for (k, s) in list.iter().enumerate() {
            bars.push((who, *s, y, k));
        }
    }
    let bottom_top = y + 10.0;
    let height = bottom_top + BOX_H + 20.0;
    let mut fig = Figure::new(width, height);
    fig.items.extend(frames);
    for (i, part) in p.parts.iter().enumerate() {
        fig.push(Item::Poly { points: vec![[xs[i], 20.0 + BOX_H], [xs[i], bottom_top]], closed: false, smooth: false, paint: Paint::line(Rgb(0x99, 0x99, 0x99), 1.0).dashed(), start: None, end: None });
        for top in [20.0, bottom_top] {
            participant(&mut fig, part, xs[i], top, widths[i]);
        }
    }
    for (who, s, e, depth) in bars {
        let x = xs[who] - 5.0 + depth as f32 * 4.0;
        fig.push(Item::Rect { x, y: s, w: 10.0, h: (e - s).max(8.0), r: 0.0, paint: Paint::both(Rgb(0xf4, 0xf4, 0xf4), Rgb(0x66, 0x66, 0x66), 1.0) });
    }
    fig.items.extend(items);
    (fig, p.errors, p.title)
}

fn participant(fig: &mut Figure, p: &Participant, cx: f32, top: f32, w: f32) {
    if p.actor {
        let paint = Paint::line(theme::NODE_STROKE, 1.5);
        let head = top + 6.0;
        fig.push(Item::Ellipse { cx, cy: head, rx: 5.0, ry: 5.0, paint: Paint::both(theme::NODE_FILL, theme::NODE_STROKE, 1.5) });
        let seg = |a: [f32; 2], b: [f32; 2]| Item::Poly { points: vec![a, b], closed: false, smooth: false, paint, start: None, end: None };
        fig.push(seg([cx, head + 5.0], [cx, head + 17.0]));
        fig.push(seg([cx - 9.0, head + 9.0], [cx + 9.0, head + 9.0]));
        fig.push(seg([cx, head + 17.0], [cx - 8.0, head + 25.0]));
        fig.push(seg([cx, head + 17.0], [cx + 8.0, head + 25.0]));
        super::text(fig, cx, top + BOX_H - 2.0, &p.label, 12.0, false, Anchor::Middle, theme::TEXT);
    } else {
        fig.push(Item::Rect { x: cx - w / 2.0, y: top, w, h: BOX_H, r: 3.0, paint: Paint::both(theme::NODE_FILL, theme::NODE_STROKE, 1.0) });
        super::text(fig, cx, top + BOX_H / 2.0, &p.label, theme::FONT, false, Anchor::Middle, theme::TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::super::render as mermaid;
    use crate::figure::{Arrow, Item};

    #[test]
    fn sequence_diagram() {
        let r = mermaid(
            "sequenceDiagram\ntitle Greeting\nautonumber\nparticipant A as Alice\nactor B as Bob\nA->>+B: Hello Bob, how are you?\nB-->>-A: Great!\nA-)B: async\nA-xB: lost\nB->>B: think\nloop Every minute\n  A->>B: ping\nend\nalt is sick\n  B->>A: not so good\nelse is well\n  B->>A: fine\nend\nNote right of B: Bob thinks\nNote over A,B: shared\nrect rgb(200, 220, 255)\nA->>B: inside\nend",
        );
        assert_eq!(r.kind, Some("sequence"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let crosses = r.figure.items.iter().filter(|i| matches!(i, Item::Poly { end: Some(Arrow::Cross), .. })).count();
        assert_eq!(crosses, 1);
        let texts: Vec<String> = r.figure.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect();
        assert!(texts.iter().any(|t| t == "Greeting"));
        assert!(texts.iter().any(|t| t == "1. Hello Bob, how are you?"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "[is well]"));
    }

    #[test]
    fn errors_are_reported() {
        let r = mermaid("sequenceDiagram\nA->>B: hi\nelse\nend\nloop forever\nA->>B");
        assert_eq!(r.errors.len(), 3, "{:?}", r.errors);
    }
}
