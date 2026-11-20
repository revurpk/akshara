//! Resolution-independent vector figures.
//!
//! A [`Figure`] is a flat list of simple primitives (rectangles, ellipses,
//! polylines with arrowheads, pie wedges and text) in points. Mermaid
//! diagrams are laid out into figures; the Flutter app paints them with its
//! own fonts, and the core turns them into SVG (for ODF, HTML and Word
//! files) or PNG.

use office_odf::Rgb;
use serde::{Serialize, Serializer};
use std::fmt::Write as _;

fn ser_rgb<S: Serializer>(value: &Option<Rgb>, s: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(rgb) => s.serialize_some(&rgb.to_hex()),
        None => s.serialize_none(),
    }
}

fn ser_rgb1<S: Serializer>(value: &Rgb, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&value.to_hex())
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Line ending decorations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrow {
    /// Filled triangle.
    Triangle,
    /// Hollow triangle (UML inheritance).
    Hollow,
    /// Two strokes (open arrow).
    Open,
    /// Filled diamond (UML composition).
    Diamond,
    /// Hollow diamond (UML aggregation).
    HollowDiamond,
    /// Filled circle.
    Circle,
    /// A cross.
    Cross,
}

/// Horizontal anchor of a text item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// The x coordinate is the left edge.
    Start,
    /// The x coordinate is the centre.
    #[default]
    Middle,
    /// The x coordinate is the right edge.
    End,
}

/// Stroke and fill of a primitive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Paint {
    /// Fill colour, `None` = not filled.
    #[serde(serialize_with = "ser_rgb", skip_serializing_if = "Option::is_none")]
    pub fill: Option<Rgb>,
    /// Line colour, `None` = no outline.
    #[serde(serialize_with = "ser_rgb", skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Rgb>,
    /// Line width in points.
    pub width: f32,
    /// Dashed line.
    #[serde(skip_serializing_if = "is_false")]
    pub dash: bool,
}

impl Paint {
    /// Filled and outlined.
    pub const fn both(fill: Rgb, stroke: Rgb, width: f32) -> Paint {
        Paint { fill: Some(fill), stroke: Some(stroke), width, dash: false }
    }

    /// Outline only.
    pub const fn line(stroke: Rgb, width: f32) -> Paint {
        Paint { fill: None, stroke: Some(stroke), width, dash: false }
    }

    /// Fill only.
    pub const fn fill(fill: Rgb) -> Paint {
        Paint { fill: Some(fill), stroke: None, width: 0.0, dash: false }
    }

    /// The same paint, dashed.
    pub const fn dashed(mut self) -> Paint {
        self.dash = true;
        self
    }
}

/// One drawing primitive. Coordinates are points, y grows downwards.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Item {
    /// A rectangle with optionally rounded corners.
    Rect {
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Corner radius.
        r: f32,
        /// Paint.
        #[serde(flatten)]
        paint: Paint,
    },
    /// An ellipse.
    Ellipse {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Horizontal radius.
        rx: f32,
        /// Vertical radius.
        ry: f32,
        /// Paint.
        #[serde(flatten)]
        paint: Paint,
    },
    /// A polyline or polygon, optionally with arrowheads.
    Poly {
        /// Points as `[x, y]`.
        points: Vec<[f32; 2]>,
        /// Closed polygon (can be filled).
        #[serde(skip_serializing_if = "is_false")]
        closed: bool,
        /// Smooth the corners (rounded polyline).
        #[serde(skip_serializing_if = "is_false")]
        smooth: bool,
        /// Paint.
        #[serde(flatten)]
        paint: Paint,
        /// Decoration at the first point.
        #[serde(skip_serializing_if = "Option::is_none")]
        start: Option<Arrow>,
        /// Decoration at the last point.
        #[serde(skip_serializing_if = "Option::is_none")]
        end: Option<Arrow>,
    },
    /// A pie slice. Angles in degrees, clockwise from 12 o'clock.
    Wedge {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Radius.
        r: f32,
        /// Start angle.
        start: f32,
        /// Sweep angle.
        sweep: f32,
        /// Paint.
        #[serde(flatten)]
        paint: Paint,
    },
    /// Text; `\n` separates lines, which are centred vertically on `y`.
    Text {
        /// Anchor x.
        x: f32,
        /// Vertical centre of the text block.
        y: f32,
        /// The text.
        text: String,
        /// Font size in points.
        size: f32,
        /// Bold.
        #[serde(skip_serializing_if = "is_false")]
        bold: bool,
        /// Italic.
        #[serde(skip_serializing_if = "is_false")]
        italic: bool,
        /// Horizontal anchor.
        anchor: Anchor,
        /// Colour.
        #[serde(serialize_with = "ser_rgb1")]
        color: Rgb,
    },
    /// A picture (from a drawing), as a `data:` URL.
    Image {
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Image id (see [`crate::Image::id`]).
        id: String,
        /// `data:` URL used by SVG output.
        #[serde(skip)]
        href: String,
    },
    /// Children rotated by `angle` degrees (clockwise) about (`cx`, `cy`).
    Rotate {
        /// Rotation centre x.
        cx: f32,
        /// Rotation centre y.
        cy: f32,
        /// Angle in degrees, clockwise.
        angle: f32,
        /// Rotated items.
        items: Vec<Item>,
    },
}

/// A vector figure.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Figure {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
    /// Primitives in painting order.
    pub items: Vec<Item>,
}

/// Line height as a multiple of the font size.
pub const LINE_HEIGHT: f32 = 1.25;

/// Approximate advance width of one character, in ems. Used for layout
/// only; the renderer measures exactly. Errs on the wide side.
fn char_em(c: char) -> f32 {
    match c {
        'i' | 'l' | 'j' | '|' | '!' | '.' | ',' | ':' | ';' | '\'' | '`' => 0.28,
        ' ' | 'f' | 't' | 'r' | 'I' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '/' | '\\' | '-' => 0.36,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.86,
        'A'..='Z' | '&' | '#' => 0.68,
        '0'..='9' | 'a'..='z' | '$' | '?' | '*' | '+' | '=' | '<' | '>' | '_' | '~' | '^' => 0.56,
        // Combining marks: Indic vowel signs and viramas, general diacritics.
        '\u{300}'..='\u{36f}' | '\u{200c}' | '\u{200d}' => 0.0,
        c if is_indic_sign(c) => 0.08,
        '\u{900}'..='\u{dff}' => 0.66,
        '\u{1100}'..='\u{11ff}' | '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{ff00}'..='\u{ffef}' => 1.0,
        _ => 0.62,
    }
}

/// Dependent vowel signs, viramas and other marks of the Indic blocks.
fn is_indic_sign(c: char) -> bool {
    let u = c as u32;
    if !(0x900..=0xdff).contains(&u) {
        return false;
    }
    let o = u & 0x7f;
    matches!(o, 0x00..=0x03 | 0x3a..=0x4f | 0x51..=0x57 | 0x62 | 0x63)
}

/// Estimated width of a single line of text in points.
pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let em: f32 = text.chars().map(char_em).sum();
    em * size * if bold { 1.07 } else { 1.0 }
}

/// Estimated size (width, height) of multi-line text in points.
pub fn text_size(text: &str, size: f32, bold: bool) -> (f32, f32) {
    let lines: Vec<&str> = text.split('\n').collect();
    let w = lines.iter().map(|l| text_width(l, size, bold)).fold(0.0, f32::max);
    (w, lines.len() as f32 * size * LINE_HEIGHT)
}

/// Breaks text into lines no wider than `max` points (at spaces; very long
/// words are kept whole).
pub fn wrap_text(text: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if !line.is_empty() && text_width(&candidate, size, bold) > max {
                out.push(std::mem::take(&mut line));
                line = word.to_owned();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

fn num(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

fn paint_attrs(p: &Paint) -> String {
    let mut s = format!(" fill=\"{}\"", p.fill.map(Rgb::to_hex).unwrap_or_else(|| "none".into()));
    match p.stroke {
        Some(c) if p.width > 0.0 => {
            let _ = write!(s, " stroke=\"{}\" stroke-width=\"{}\"", c.to_hex(), num(p.width));
            if p.dash {
                let _ = write!(s, " stroke-dasharray=\"{} {}\"", num(p.width * 4.0), num(p.width * 3.0));
            }
        }
        _ => s.push_str(" stroke=\"none\""),
    }
    s
}

/// The outline of an arrowhead at `tip` pointing along (`dx`, `dy`) (unit
/// vector), as polygon points, and whether it is filled. Shared by the SVG
/// writer and the Flutter painter (which mirrors this geometry).
pub fn arrow_points(kind: Arrow, tip: [f32; 2], dx: f32, dy: f32, width: f32) -> (Vec<[f32; 2]>, bool) {
    let s = 3.0 + width * 3.0;
    let (px, py) = (-dy, dx);
    let at = |along: f32, side: f32| [tip[0] - dx * along + px * side, tip[1] - dy * along + py * side];
    match kind {
        Arrow::Triangle => (vec![tip, at(s * 1.6, s * 0.7), at(s * 1.6, -s * 0.7)], true),
        Arrow::Hollow => (vec![tip, at(s * 2.0, s * 1.0), at(s * 2.0, -s * 1.0)], false),
        Arrow::Open => (vec![at(s * 1.6, s * 0.8), tip, at(s * 1.6, -s * 0.8)], false),
        Arrow::Diamond => (vec![tip, at(s * 1.3, s * 0.7), at(s * 2.6, 0.0), at(s * 1.3, -s * 0.7)], true),
        Arrow::HollowDiamond => (vec![tip, at(s * 1.3, s * 0.7), at(s * 2.6, 0.0), at(s * 1.3, -s * 0.7)], false),
        Arrow::Circle => {
            let r = s * 0.6;
            let c = at(r, 0.0);
            ((0..12).map(|i| {
                let a = i as f32 * std::f32::consts::PI / 6.0;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            }).collect(), true)
        }
        Arrow::Cross => (vec![at(s * 1.4, s * 0.7), at(0.0, -s * 0.7), at(s * 0.7, 0.0), at(0.0, s * 0.7), at(s * 1.4, -s * 0.7)], false),
    }
}

/// How far the line must stop short of the tip so it does not poke through
/// a filled arrowhead.
pub fn arrow_inset(kind: Arrow, width: f32) -> f32 {
    let s = 3.0 + width * 3.0;
    match kind {
        Arrow::Triangle => s * 1.4,
        Arrow::Hollow => s * 2.0,
        Arrow::Diamond | Arrow::HollowDiamond => s * 2.6,
        Arrow::Circle => s * 1.2,
        Arrow::Open | Arrow::Cross => 0.0,
    }
}

fn unit(a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 { (1.0, 0.0) } else { (dx / len, dy / len) }
}

fn svg_item(out: &mut String, item: &Item) {
    match item {
        Item::Rect { x, y, w, h, r, paint } => {
            let _ = write!(out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", num(*x), num(*y), num(*w), num(*h));
            if *r > 0.0 {
                let _ = write!(out, " rx=\"{}\"", num(*r));
            }
            let _ = writeln!(out, "{}/>", paint_attrs(paint));
        }
        Item::Ellipse { cx, cy, rx, ry, paint } => {
            let _ = writeln!(out, "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}/>", num(*cx), num(*cy), num(*rx), num(*ry), paint_attrs(paint));
        }
        Item::Poly { points, closed, smooth, paint, start, end } => {
            if points.len() < 2 {
                return;
            }
            let mut pts = points.clone();
            let n = pts.len();
            if let Some(a) = start {
                let (dx, dy) = unit(pts[1], pts[0]);
                let inset = arrow_inset(*a, paint.width);
                pts[0] = [pts[0][0] - dx * inset, pts[0][1] - dy * inset];
            }
            if let Some(a) = end {
                let (dx, dy) = unit(pts[n - 2], pts[n - 1]);
                let inset = arrow_inset(*a, paint.width);
                pts[n - 1] = [pts[n - 1][0] - dx * inset, pts[n - 1][1] - dy * inset];
            }
            let d = if *smooth && n > 2 && !*closed { smooth_path(&pts) } else {
                let mut d = String::new();
                for (i, p) in pts.iter().enumerate() {
                    let _ = write!(d, "{}{} {} ", if i == 0 { 'M' } else { 'L' }, num(p[0]), num(p[1]));
                }
                if *closed {
                    d.push('Z');
                }
                d
            };
            let mut paint = *paint;
            if !*closed {
                paint.fill = None;
            }
            let _ = writeln!(out, "<path d=\"{}\"{} stroke-linejoin=\"round\"/>", d.trim_end(), paint_attrs(&paint));
            let color = paint.stroke.unwrap_or(Rgb(0, 0, 0));
            for (kind, tip, from) in [(start, points[0], points[1]), (end, points[n - 1], points[n - 2])] {
                if let Some(kind) = kind {
                    let (dx, dy) = unit(from, tip);
                    let (poly, filled) = arrow_points(*kind, tip, dx, dy, paint.width);
                    let fill = if filled { color.to_hex() } else if matches!(kind, Arrow::Hollow | Arrow::HollowDiamond) { "#ffffff".into() } else { "none".into() };
                    let closed = !matches!(kind, Arrow::Open | Arrow::Cross);
                    let mut d = String::new();
                    for (i, p) in poly.iter().enumerate() {
                        let _ = write!(d, "{}{} {} ", if i == 0 { 'M' } else { 'L' }, num(p[0]), num(p[1]));
                    }
                    if closed {
                        d.push('Z');
                    }
                    let _ = writeln!(out, "<path d=\"{}\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"/>", d.trim_end(), color.to_hex(), num(paint.width.max(1.0)));
                }
            }
        }
        Item::Wedge { cx, cy, r, start, sweep, paint } => {
            let pt = |deg: f32| {
                let a = (deg - 90.0).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            };
            if *sweep >= 359.99 {
                let _ = writeln!(out, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>", num(*cx), num(*cy), num(*r), paint_attrs(paint));
            } else {
                let (x0, y0) = pt(*start);
                let (x1, y1) = pt(start + sweep);
                let large = if *sweep > 180.0 { 1 } else { 0 };
                let _ = writeln!(
                    out,
                    "<path d=\"M{} {} L{} {} A{} {} 0 {large} 1 {} {} Z\"{}/>",
                    num(*cx), num(*cy), num(x0), num(y0), num(*r), num(*r), num(x1), num(y1), paint_attrs(paint)
                );
            }
        }
        Item::Text { x, y, text, size, bold, italic, anchor, color } => {
            let lines: Vec<&str> = text.split('\n').collect();
            let lh = size * LINE_HEIGHT;
            let top = y - lh * lines.len() as f32 / 2.0;
            let anchor = match anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let _ = write!(
                out,
                "<text font-family=\"Noto Sans, Liberation Sans, Arial, sans-serif\" font-size=\"{}\" text-anchor=\"{anchor}\" fill=\"{}\"",
                num(*size),
                color.to_hex()
            );
            if *bold {
                out.push_str(" font-weight=\"bold\"");
            }
            if *italic {
                out.push_str(" font-style=\"italic\"");
            }
            out.push('>');
            for (i, line) in lines.iter().enumerate() {
                // Baseline at about 78% of the line box.
                let base = top + lh * i as f32 + lh * 0.5 + size * 0.35;
                let _ = write!(out, "<tspan x=\"{}\" y=\"{}\">{}</tspan>", num(*x), num(base), esc(line));
            }
            out.push_str("</text>\n");
        }
        Item::Image { x, y, w, h, href, .. } => {
            let _ = writeln!(
                out,
                "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"{}\"/>",
                num(*x), num(*y), num(*w), num(*h), esc(href)
            );
        }
        Item::Rotate { cx, cy, angle, items } => {
            let _ = writeln!(out, "<g transform=\"rotate({} {} {})\">", num(*angle), num(*cx), num(*cy));
            for item in items {
                svg_item(out, item);
            }
            out.push_str("</g>\n");
        }
    }
}

/// A polyline with rounded corners as an SVG path.
fn smooth_path(p: &[[f32; 2]]) -> String {
    let mut d = format!("M{} {} ", num(p[0][0]), num(p[0][1]));
    for i in 1..p.len() - 1 {
        let (a, b, c) = (p[i - 1], p[i], p[i + 1]);
        let r = 8.0f32;
        let (d1, d2) = (dist(a, b), dist(b, c));
        let k1 = (r / d1.max(1e-3)).min(0.5);
        let k2 = (r / d2.max(1e-3)).min(0.5);
        let s = [b[0] + (a[0] - b[0]) * k1, b[1] + (a[1] - b[1]) * k1];
        let e = [b[0] + (c[0] - b[0]) * k2, b[1] + (c[1] - b[1]) * k2];
        let _ = write!(d, "L{} {} Q{} {} {} {} ", num(s[0]), num(s[1]), num(b[0]), num(b[1]), num(e[0]), num(e[1]));
    }
    let l = p[p.len() - 1];
    let _ = write!(d, "L{} {}", num(l[0]), num(l[1]));
    d
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

impl Figure {
    /// An empty figure of the given size.
    pub fn new(width: f32, height: f32) -> Figure {
        Figure { width, height, items: Vec::new() }
    }

    /// Adds a primitive.
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    /// Standalone SVG document. `background` paints the whole canvas.
    pub fn to_svg(&self, background: Option<Rgb>) -> String {
        self.to_svg_sized(self.width, self.height, background)
    }

    /// SVG document displayed at `width` × `height` points.
    pub fn to_svg_sized(&self, width: f32, height: f32, background: Option<Rgb>) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}pt\" height=\"{}pt\" viewBox=\"0 0 {w} {h}\">",
            num(width.max(1.0)),
            num(height.max(1.0)),
            w = num(self.width.max(1.0)),
            h = num(self.height.max(1.0))
        );
        if let Some(bg) = background {
            let _ = writeln!(out, "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>", bg.to_hex());
        }
        for item in &self.items {
            svg_item(&mut out, item);
        }
        out.push_str("</svg>\n");
        out
    }

    /// A figure showing an error message (used for diagrams that cannot be
    /// drawn).
    pub fn message(text: &str) -> Figure {
        let lines = wrap_text(text, 11.0, false, 320.0).join("\n");
        let (w, h) = text_size(&lines, 11.0, false);
        let (w, h) = (w + 32.0, h + 24.0);
        let mut f = Figure::new(w, h);
        f.push(Item::Rect { x: 1.0, y: 1.0, w: w - 2.0, h: h - 2.0, r: 4.0, paint: Paint::both(Rgb(0xff, 0xf4, 0xe5), Rgb(0xd9, 0x77, 0x06), 1.0).dashed() });
        f.push(Item::Text { x: w / 2.0, y: h / 2.0, text: lines, size: 11.0, bold: false, italic: false, anchor: Anchor::Middle, color: Rgb(0x7c, 0x2d, 0x12) });
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_is_well_formed_and_escaped() {
        let mut f = Figure::new(100.0, 50.0);
        f.push(Item::Text { x: 50.0, y: 25.0, text: "a<b & \"c\"".into(), size: 12.0, bold: true, italic: false, anchor: Anchor::Middle, color: Rgb(0, 0, 0) });
        f.push(Item::Poly { points: vec![[0.0, 0.0], [50.0, 0.0], [50.0, 40.0]], closed: false, smooth: true, paint: Paint::line(Rgb(0, 0, 0), 1.0), start: None, end: Some(Arrow::Triangle) });
        f.push(Item::Wedge { cx: 20.0, cy: 20.0, r: 10.0, start: 0.0, sweep: 90.0, paint: Paint::fill(Rgb(1, 2, 3)) });
        let svg = f.to_svg(None);
        assert!(svg.contains("a&lt;b &amp; &quot;c&quot;"));
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default());
        assert!(tree.is_ok(), "{svg}");
    }

    #[test]
    fn text_estimates_are_sane() {
        assert!(text_width("WWW", 10.0, false) > text_width("iii", 10.0, false));
        // Indic vowel signs add little width.
        assert!(text_width("कि", 10.0, false) < text_width("कक", 10.0, false));
        assert_eq!(wrap_text("aa bb cc", 10.0, false, 20.0), vec!["aa", "bb", "cc"]);
        assert_eq!(text_size("a\nb", 10.0, false).1, 25.0);
    }
}
