//! Pie charts: `pie [showData]`, `title ...` and `"Label" : value` lines.

use super::{label_text, theme, Diagnostic, Line, Output};
use crate::figure::{text_size, Anchor, Figure, Item, Paint};
use office_odf::Rgb;

/// Slice colours (accessible on white, distinguishable in order).
const PALETTE: [Rgb; 12] = [
    Rgb(0x4e, 0x79, 0xa7),
    Rgb(0xf2, 0x8e, 0x2b),
    Rgb(0xe1, 0x57, 0x59),
    Rgb(0x76, 0xb7, 0xb2),
    Rgb(0x59, 0xa1, 0x4f),
    Rgb(0xed, 0xc9, 0x48),
    Rgb(0xb0, 0x7a, 0xa1),
    Rgb(0xff, 0x9d, 0xa7),
    Rgb(0x9c, 0x75, 0x5f),
    Rgb(0xba, 0xb0, 0xac),
    Rgb(0x2f, 0x4b, 0x7c),
    Rgb(0xa0, 0xcb, 0xe8),
];

pub(crate) fn render(lines: &[Line]) -> Output {
    let mut errors = Vec::new();
    let mut title = None;
    let mut show_data = lines[0].text.split_whitespace().any(|w| w == "showData");
    let mut slices: Vec<(String, f64)> = Vec::new();
    // `pie title X` on the first line.
    if let Some(t) = lines[0].text.split_once("title") {
        title = Some(label_text(t.1));
    }
    for l in &lines[1..] {
        let t = l.text.trim();
        if let Some(rest) = t.strip_prefix("title") {
            title = Some(label_text(rest));
        } else if t == "showData" {
            show_data = true;
        } else if t.starts_with("accTitle") || t.starts_with("accDescr") {
        } else if let Some(pos) = t.rfind(':') {
            let label = label_text(&t[..pos]);
            match t[pos + 1..].trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v >= 0.0 => slices.push((label, v)),
                _ => errors.push(Diagnostic { line: l.no, message: "slice value must be a positive number".into() }),
            }
        } else {
            errors.push(Diagnostic { line: l.no, message: "expected \"Label\" : value".into() });
        }
    }
    let total: f64 = slices.iter().map(|s| s.1).sum();
    if slices.is_empty() || total <= 0.0 {
        return (Figure::default(), errors, title);
    }
    let r = 130.0f32;
    let (cx, cy) = (r + 20.0, r + 20.0);
    let legend_x = cx + r + 40.0;
    let mut legend_w: f32 = 0.0;
    let mut fig = Figure::new(0.0, 0.0);
    let mut angle = 0.0f32;
    for (i, (label, v)) in slices.iter().enumerate() {
        let sweep = (*v / total * 360.0) as f32;
        let color = PALETTE[i % PALETTE.len()];
        fig.push(Item::Wedge { cx, cy, r, start: angle, sweep, paint: Paint::both(color, Rgb(255, 255, 255), 1.5) });
        if sweep > 12.0 {
            let mid = (angle + sweep / 2.0 - 90.0).to_radians();
            let pct = format!("{:.0}%", v / total * 100.0);
            super::text(&mut fig, cx + r * 0.65 * mid.cos(), cy + r * 0.65 * mid.sin(), &pct, 13.0, true, Anchor::Middle, Rgb(255, 255, 255));
        }
        angle += sweep;
        let entry = if show_data { format!("{label} [{}]", trim_num(*v)) } else { label.clone() };
        let ly = 30.0 + i as f32 * 24.0;
        fig.push(Item::Rect { x: legend_x, y: ly - 8.0, w: 16.0, h: 16.0, r: 2.0, paint: Paint::fill(color) });
        super::text(&mut fig, legend_x + 24.0, ly, &entry, 13.0, false, Anchor::Start, theme::TEXT);
        legend_w = legend_w.max(text_size(&entry, 13.0, false).0 + 30.0);
    }
    fig.width = legend_x + legend_w + 20.0;
    fig.height = (2.0 * r + 40.0).max(30.0 + slices.len() as f32 * 24.0 + 10.0);
    (fig, errors, title)
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::super::render as mermaid;
    use crate::figure::Item;

    #[test]
    fn pie_chart() {
        let r = mermaid("pie showData\ntitle Pets adopted\n\"Dogs\" : 386\n\"Cats\" : 85.5\n\"Rats\" : 15\n\"Bad\" : x");
        assert_eq!(r.kind, Some("pie"));
        assert_eq!(r.errors.len(), 1);
        let wedges: f32 = r.figure.items.iter().filter_map(|i| if let Item::Wedge { sweep, .. } = i { Some(*sweep) } else { None }).sum();
        assert!((wedges - 360.0).abs() < 0.01);
        let texts: Vec<String> = r.figure.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect();
        assert!(texts.contains(&"Pets adopted".to_owned()));
        assert!(texts.contains(&"Cats [85.5]".to_owned()), "{texts:?}");
    }
}
