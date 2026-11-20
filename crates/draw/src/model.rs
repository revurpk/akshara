//! Shapes and pages.
//!
//! A [`Shape`] is a rectangle, ellipse, triangle, diamond, line, polygon,
//! text box, picture or Mermaid diagram in a box measured in points, with
//! a fill, an outline and text. Pages ([`Page`]) hold shapes in painting
//! order (the last one is on top). The same shape lists are used by
//! presentations and drawings ([`crate::Drawing`]) and by drawing canvases
//! inside Writer documents.

use crate::figure::{self, Anchor, Arrow, Figure, Item, Paint};
use crate::image::Image;
use office_odf::{OdfError, Result, Rgb};
use serde::{Serialize, Serializer};

fn ser_rgb<S: Serializer>(value: &Option<Rgb>, s: S) -> std::result::Result<S::Ok, S::Error> {
    match value {
        Some(rgb) => s.serialize_some(&rgb.to_hex()),
        None => s.serialize_none(),
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Largest coordinate magnitude in points (200 inches).
pub const MAX_COORD: f32 = 14_400.0;
/// Most shapes on one page.
pub const MAX_SHAPES: usize = 10_000;
/// Longest text in one shape (bytes).
pub const MAX_SHAPE_TEXT: usize = 100_000;
/// Longest diagram source (bytes).
pub const MAX_DIAGRAM_SOURCE: usize = 64 << 10;
/// Most points of a polygon.
pub const MAX_POINTS: usize = 4096;

/// Geometry of a shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeKind {
    /// Rectangle.
    Rect,
    /// Rectangle with rounded corners.
    RoundRect,
    /// Ellipse (circle when square).
    Ellipse,
    /// Isosceles triangle pointing up.
    Triangle,
    /// Diamond (rhombus).
    Diamond,
    /// Straight line from (x, y) to (x + w, y + h); `w` and `h` may be
    /// negative.
    Line,
    /// Text without fill or outline by default.
    TextBox,
    /// Polygon or polyline with points relative to the box (0..1).
    Polygon {
        /// Points as `[x, y]` fractions of the box.
        points: Vec<[f32; 2]>,
        /// Closed (polygon) or open (polyline).
        closed: bool,
    },
    /// A picture stretched to the box.
    Image {
        /// The picture.
        image: Image,
    },
    /// A Mermaid diagram scaled into the box.
    Diagram {
        /// Mermaid source text.
        source: String,
    },
}

/// Horizontal text alignment inside a shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    /// Left.
    Left,
    /// Centred.
    #[default]
    Center,
    /// Right.
    Right,
}

/// Vertical text alignment inside a shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    /// Top.
    Top,
    /// Middle.
    #[default]
    Middle,
    /// Bottom.
    Bottom,
}

/// Text formatting of a shape (one style for its whole text).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextStyle {
    /// Font size in points.
    pub size: f32,
    /// Bold.
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    /// Italic.
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    /// Underlined.
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    /// Text colour.
    #[serde(serialize_with = "ser_rgb1")]
    pub color: Rgb,
    /// Horizontal alignment.
    pub align: HAlign,
    /// Vertical alignment.
    pub valign: VAlign,
    /// Each line is a bulleted list item.
    #[serde(skip_serializing_if = "is_false")]
    pub bullets: bool,
}

fn ser_rgb1<S: Serializer>(value: &Rgb, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&value.to_hex())
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            size: 18.0,
            bold: false,
            italic: false,
            underline: false,
            color: Rgb(0, 0, 0),
            align: HAlign::Center,
            valign: VAlign::Middle,
            bullets: false,
        }
    }
}

/// Role of a presentation placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Placeholder {
    /// Slide title.
    Title,
    /// Subtitle of a title slide.
    Subtitle,
    /// Bulleted outline (slide body).
    Outline,
}

/// One shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shape {
    /// Identifier, unique within its document.
    pub id: u32,
    /// Geometry.
    #[serde(flatten)]
    pub kind: ShapeKind,
    /// Left edge in points.
    pub x: f32,
    /// Top edge in points.
    pub y: f32,
    /// Width in points (lines: horizontal extent, may be negative).
    pub w: f32,
    /// Height in points (lines: vertical extent, may be negative).
    pub h: f32,
    /// Clockwise rotation about the centre, in degrees.
    #[serde(skip_serializing_if = "is_zero")]
    pub rotation: f32,
    /// Fill colour; `None` = transparent.
    #[serde(serialize_with = "ser_rgb")]
    pub fill: Option<Rgb>,
    /// Outline colour; `None` = no outline.
    #[serde(serialize_with = "ser_rgb")]
    pub stroke: Option<Rgb>,
    /// Outline width in points.
    pub stroke_width: f32,
    /// Dashed outline.
    #[serde(skip_serializing_if = "is_false")]
    pub dash: bool,
    /// Arrowhead at the start of a line.
    #[serde(skip_serializing_if = "is_false")]
    pub arrow_start: bool,
    /// Arrowhead at the end of a line.
    #[serde(skip_serializing_if = "is_false")]
    pub arrow_end: bool,
    /// Text inside the shape; `\n` separates paragraphs.
    pub text: String,
    /// Text formatting.
    pub style: TextStyle,
    /// Presentation placeholder role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<Placeholder>,
}

/// Default fill of new closed shapes (light blue).
pub const DEFAULT_FILL: Rgb = Rgb(0x72, 0x9f, 0xcf);
/// Default outline (dark blue).
pub const DEFAULT_STROKE: Rgb = Rgb(0x34, 0x65, 0xa4);

impl Shape {
    /// A new shape of the given kind with the default look for that kind.
    /// The id is assigned when the shape is added to a document.
    pub fn new(kind: ShapeKind, x: f32, y: f32, w: f32, h: f32) -> Shape {
        let (fill, stroke) = match kind {
            ShapeKind::Line | ShapeKind::Polygon { closed: false, .. } => (None, Some(Rgb(0, 0, 0))),
            ShapeKind::TextBox | ShapeKind::Image { .. } | ShapeKind::Diagram { .. } => (None, None),
            _ => (Some(DEFAULT_FILL), Some(DEFAULT_STROKE)),
        };
        let style = match kind {
            ShapeKind::TextBox => TextStyle { align: HAlign::Left, valign: VAlign::Top, ..TextStyle::default() },
            _ => TextStyle::default(),
        };
        Shape {
            id: 0,
            kind,
            x,
            y,
            w,
            h,
            rotation: 0.0,
            fill,
            stroke,
            stroke_width: 1.0,
            dash: false,
            arrow_start: false,
            arrow_end: false,
            text: String::new(),
            style,
            placeholder: None,
        }
    }

    /// A text box with the given text.
    pub fn text_box(x: f32, y: f32, w: f32, h: f32, text: &str, style: TextStyle) -> Shape {
        Shape { text: text.to_owned(), style, ..Shape::new(ShapeKind::TextBox, x, y, w, h) }
    }

    /// Whether the shape is a line (its box can have negative sides).
    pub fn is_line(&self) -> bool {
        matches!(self.kind, ShapeKind::Line)
    }

    /// The bounding box `(left, top, width, height)` with positive sides,
    /// ignoring rotation.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let (x0, x1) = if self.w < 0.0 { (self.x + self.w, self.x) } else { (self.x, self.x + self.w) };
        let (y0, y1) = if self.h < 0.0 { (self.y + self.h, self.y) } else { (self.y, self.y + self.h) };
        (x0, y0, x1 - x0, y1 - y0)
    }

    /// Clamps sizes and values into their valid ranges and fixes broken
    /// data (non-finite numbers, too many points, too long text).
    pub fn normalize(&mut self) {
        let fix = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.x = fix(self.x, -MAX_COORD, MAX_COORD, 0.0);
        self.y = fix(self.y, -MAX_COORD, MAX_COORD, 0.0);
        if self.is_line() {
            self.w = fix(self.w, -MAX_COORD, MAX_COORD, 0.0);
            self.h = fix(self.h, -MAX_COORD, MAX_COORD, 0.0);
        } else {
            self.w = fix(self.w, 1.0, MAX_COORD, 72.0);
            self.h = fix(self.h, 1.0, MAX_COORD, 72.0);
        }
        self.rotation = fix(self.rotation, -360_000.0, 360_000.0, 0.0).rem_euclid(360.0);
        if self.rotation > 359.99 {
            self.rotation = 0.0;
        }
        self.stroke_width = fix(self.stroke_width, 0.0, 72.0, 1.0);
        self.style.size = fix(self.style.size, 4.0, 400.0, 18.0);
        if self.text.len() > MAX_SHAPE_TEXT {
            let mut cut = MAX_SHAPE_TEXT;
            while !self.text.is_char_boundary(cut) {
                cut -= 1;
            }
            self.text.truncate(cut);
        }
        self.text.retain(|c| c == '\n' || c == '\t' || !c.is_control());
        match &mut self.kind {
            ShapeKind::Polygon { points, .. } => {
                points.truncate(MAX_POINTS);
                for p in points.iter_mut() {
                    p[0] = fix(p[0], 0.0, 1.0, 0.0);
                    p[1] = fix(p[1], 0.0, 1.0, 0.0);
                }
            }
            ShapeKind::Diagram { source } if source.len() > MAX_DIAGRAM_SOURCE => {
                let mut cut = MAX_DIAGRAM_SOURCE;
                while !source.is_char_boundary(cut) {
                    cut -= 1;
                }
                source.truncate(cut);
            }
            _ => {}
        }
    }

    /// The Mermaid figure of a diagram shape.
    pub fn diagram_figure(&self) -> Option<Figure> {
        match &self.kind {
            ShapeKind::Diagram { source } => Some(crate::mermaid::render(source).figure),
            _ => None,
        }
    }
}

/// A partial update of a shape: only the `Some` fields change.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapePatch {
    /// New left edge.
    pub x: Option<f32>,
    /// New top edge.
    pub y: Option<f32>,
    /// New width.
    pub w: Option<f32>,
    /// New height.
    pub h: Option<f32>,
    /// New rotation.
    pub rotation: Option<f32>,
    /// New fill (`Some(None)` = transparent).
    pub fill: Option<Option<Rgb>>,
    /// New outline colour (`Some(None)` = none).
    pub stroke: Option<Option<Rgb>>,
    /// New outline width.
    pub stroke_width: Option<f32>,
    /// New dash flag.
    pub dash: Option<bool>,
    /// New start arrow flag.
    pub arrow_start: Option<bool>,
    /// New end arrow flag.
    pub arrow_end: Option<bool>,
    /// New text.
    pub text: Option<String>,
    /// New font size.
    pub size: Option<f32>,
    /// New bold flag.
    pub bold: Option<bool>,
    /// New italic flag.
    pub italic: Option<bool>,
    /// New underline flag.
    pub underline: Option<bool>,
    /// New text colour.
    pub color: Option<Rgb>,
    /// New horizontal alignment.
    pub align: Option<HAlign>,
    /// New vertical alignment.
    pub valign: Option<VAlign>,
    /// New bullets flag.
    pub bullets: Option<bool>,
    /// New diagram source (diagram shapes only).
    pub source: Option<String>,
}

impl ShapePatch {
    /// Applies the patch to a shape (then normalises it).
    pub fn apply(&self, s: &mut Shape) -> Result<()> {
        macro_rules! set {
            ($($field:ident => $target:expr),*) => {
                $(if let Some(v) = &self.$field { $target = v.clone(); })*
            };
        }
        set!(x => s.x, y => s.y, w => s.w, h => s.h, rotation => s.rotation, fill => s.fill, stroke => s.stroke,
            stroke_width => s.stroke_width, dash => s.dash, arrow_start => s.arrow_start, arrow_end => s.arrow_end,
            text => s.text, size => s.style.size, bold => s.style.bold, italic => s.style.italic,
            underline => s.style.underline, color => s.style.color, align => s.style.align, valign => s.style.valign,
            bullets => s.style.bullets);
        if let Some(src) = &self.source {
            match &mut s.kind {
                ShapeKind::Diagram { source } => *source = src.clone(),
                _ => return Err(OdfError::invalid("only diagrams have a source")),
            }
        }
        s.normalize();
        Ok(())
    }
}

/// Stacking order changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZOrder {
    /// On top of everything.
    Front,
    /// Below everything.
    Back,
    /// One step up.
    Forward,
    /// One step down.
    Backward,
}

/// Validates and stacks shapes. Shared by drawings and Writer canvases.
pub mod ops {
    use super::*;

    fn bad(msg: impl Into<String>) -> OdfError {
        OdfError::Invalid(msg.into())
    }

    fn index(shapes: &[Shape], id: u32) -> Result<usize> {
        shapes.iter().position(|s| s.id == id).ok_or_else(|| bad(format!("no shape {id}")))
    }

    /// Adds shapes on top, assigning fresh ids from `next_id`. Returns the
    /// new ids.
    pub fn add(shapes: &mut Vec<Shape>, new: Vec<Shape>, next_id: &mut u32) -> Result<Vec<u32>> {
        if shapes.len() + new.len() > MAX_SHAPES {
            return Err(OdfError::limit("too many shapes on this page"));
        }
        let mut ids = Vec::with_capacity(new.len());
        for mut s in new {
            *next_id = next_id.checked_add(1).ok_or_else(|| OdfError::limit("too many shapes"))?;
            s.id = *next_id;
            s.normalize();
            ids.push(s.id);
            shapes.push(s);
        }
        Ok(ids)
    }

    /// Applies patches to shapes by id.
    pub fn update(shapes: &mut [Shape], patches: &[(u32, ShapePatch)]) -> Result<()> {
        for (id, patch) in patches {
            let i = index(shapes, *id)?;
            patch.apply(&mut shapes[i])?;
        }
        Ok(())
    }

    /// Removes shapes by id.
    pub fn remove(shapes: &mut Vec<Shape>, ids: &[u32]) -> Result<()> {
        for id in ids {
            index(shapes, *id)?;
        }
        shapes.retain(|s| !ids.contains(&s.id));
        Ok(())
    }

    /// Changes the stacking order of one shape.
    pub fn reorder(shapes: &mut Vec<Shape>, id: u32, to: ZOrder) -> Result<()> {
        let i = index(shapes, id)?;
        let s = shapes.remove(i);
        let at = match to {
            ZOrder::Front => shapes.len(),
            ZOrder::Back => 0,
            ZOrder::Forward => (i + 1).min(shapes.len()),
            ZOrder::Backward => i.saturating_sub(1),
        };
        shapes.insert(at, s);
        Ok(())
    }

    /// Copies of shapes by id, in stacking order.
    pub fn copy(shapes: &[Shape], ids: &[u32]) -> Vec<Shape> {
        shapes.iter().filter(|s| ids.contains(&s.id)).cloned().collect()
    }
}

/// One page (slide) of a drawing or presentation.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Page {
    /// Page name ("Slide 3").
    pub name: String,
    /// Shapes from bottom to top.
    pub shapes: Vec<Shape>,
    /// Background colour; `None` = white.
    #[serde(serialize_with = "ser_rgb")]
    pub background: Option<Rgb>,
    /// Speaker notes.
    pub notes: String,
}

/// Paints shapes into figure primitives, for SVG and PNG output. Text is
/// wrapped with estimated widths; images are embedded as `data:` URLs.
pub fn shapes_to_figure(shapes: &[Shape], width: f32, height: f32, background: Option<Rgb>) -> Figure {
    let mut f = Figure::new(width, height);
    if let Some(bg) = background {
        f.push(Item::Rect { x: 0.0, y: 0.0, w: width, h: height, r: 0.0, paint: Paint::fill(bg) });
    }
    for s in shapes {
        let items = shape_items(s);
        if s.rotation != 0.0 && !s.is_line() {
            f.push(Item::Rotate { cx: s.x + s.w / 2.0, cy: s.y + s.h / 2.0, angle: s.rotation, items });
        } else {
            f.items.extend(items);
        }
    }
    f
}

fn shape_items(s: &Shape) -> Vec<Item> {
    let paint = Paint { fill: s.fill, stroke: s.stroke, width: s.stroke_width, dash: s.dash };
    let (x, y, w, h) = (s.x, s.y, s.w, s.h);
    let mut out = Vec::new();
    let poly = |pts: Vec<[f32; 2]>, closed: bool| Item::Poly { points: pts, closed, smooth: false, paint, start: None, end: None };
    match &s.kind {
        ShapeKind::Rect | ShapeKind::TextBox => out.push(Item::Rect { x, y, w, h, r: 0.0, paint }),
        ShapeKind::RoundRect => out.push(Item::Rect { x, y, w, h, r: w.min(h) * 0.15, paint }),
        ShapeKind::Ellipse => out.push(Item::Ellipse { cx: x + w / 2.0, cy: y + h / 2.0, rx: w / 2.0, ry: h / 2.0, paint }),
        ShapeKind::Triangle => out.push(poly(vec![[x + w / 2.0, y], [x + w, y + h], [x, y + h]], true)),
        ShapeKind::Diamond => out.push(poly(vec![[x + w / 2.0, y], [x + w, y + h / 2.0], [x + w / 2.0, y + h], [x, y + h / 2.0]], true)),
        ShapeKind::Polygon { points, closed } => {
            out.push(poly(points.iter().map(|p| [x + p[0] * w, y + p[1] * h]).collect(), *closed));
        }
        ShapeKind::Line => out.push(Item::Poly {
            points: vec![[x, y], [x + w, y + h]],
            closed: false,
            smooth: false,
            paint: Paint { fill: None, ..paint },
            start: s.arrow_start.then_some(Arrow::Triangle),
            end: s.arrow_end.then_some(Arrow::Triangle),
        }),
        ShapeKind::Image { image } => {
            out.push(Item::Image { x, y, w, h, id: image.id().to_owned(), href: image.data_url() });
            if paint.stroke.is_some() {
                out.push(Item::Rect { x, y, w, h, r: 0.0, paint: Paint { fill: None, ..paint } });
            }
        }
        ShapeKind::Diagram { source } => {
            let fig = crate::mermaid::render(source).figure;
            out.extend(place_figure(&fig, x, y, w, h));
        }
    }
    if !s.text.is_empty() && !s.is_line() {
        let st = &s.style;
        let pad = 6.0;
        let bullet = if st.bullets { "• " } else { "" };
        let mut lines = Vec::new();
        for para in s.text.split('\n') {
            let wrapped = figure::wrap_text(&format!("{bullet}{para}"), st.size, st.bold, (w - 2.0 * pad).max(10.0));
            lines.extend(wrapped);
        }
        let lh = st.size * figure::LINE_HEIGHT;
        let block = lh * lines.len() as f32;
        let top = match st.valign {
            VAlign::Top => y + pad,
            VAlign::Middle => y + (h - block) / 2.0,
            VAlign::Bottom => y + h - pad - block,
        };
        let (tx, anchor) = match st.align {
            HAlign::Left => (x + pad, Anchor::Start),
            HAlign::Center => (x + w / 2.0, Anchor::Middle),
            HAlign::Right => (x + w - pad, Anchor::End),
        };
        out.push(Item::Text {
            x: tx,
            y: top + block / 2.0,
            text: lines.join("\n"),
            size: st.size,
            bold: st.bold,
            italic: st.italic,
            anchor,
            color: st.color,
        });
    }
    out
}

/// Scales and centres a figure into a box (keeping its aspect ratio).
pub fn place_figure(fig: &Figure, x: f32, y: f32, w: f32, h: f32) -> Vec<Item> {
    let k = (w / fig.width.max(1.0)).min(h / fig.height.max(1.0));
    let ox = x + (w - fig.width * k) / 2.0;
    let oy = y + (h - fig.height * k) / 2.0;
    fig.items.iter().map(|i| transform(i, ox, oy, k)).collect()
}

fn transform(item: &Item, ox: f32, oy: f32, k: f32) -> Item {
    let p = |v: f32, o: f32| o + v * k;
    let paint = |pt: &Paint| Paint { width: pt.width * k, ..*pt };
    match item {
        Item::Rect { x, y, w, h, r, paint: pt } => Item::Rect { x: p(*x, ox), y: p(*y, oy), w: w * k, h: h * k, r: r * k, paint: paint(pt) },
        Item::Ellipse { cx, cy, rx, ry, paint: pt } => Item::Ellipse { cx: p(*cx, ox), cy: p(*cy, oy), rx: rx * k, ry: ry * k, paint: paint(pt) },
        Item::Poly { points, closed, smooth, paint: pt, start, end } => Item::Poly {
            points: points.iter().map(|q| [p(q[0], ox), p(q[1], oy)]).collect(),
            closed: *closed,
            smooth: *smooth,
            paint: paint(pt),
            start: *start,
            end: *end,
        },
        Item::Wedge { cx, cy, r, start, sweep, paint: pt } => Item::Wedge { cx: p(*cx, ox), cy: p(*cy, oy), r: r * k, start: *start, sweep: *sweep, paint: paint(pt) },
        Item::Text { x, y, text, size, bold, italic, anchor, color } => Item::Text {
            x: p(*x, ox),
            y: p(*y, oy),
            text: text.clone(),
            size: size * k,
            bold: *bold,
            italic: *italic,
            anchor: *anchor,
            color: *color,
        },
        Item::Image { x, y, w, h, id, href } => Item::Image { x: p(*x, ox), y: p(*y, oy), w: w * k, h: h * k, id: id.clone(), href: href.clone() },
        Item::Rotate { cx, cy, angle, items } => Item::Rotate {
            cx: p(*cx, ox),
            cy: p(*cy, oy),
            angle: *angle,
            items: items.iter().map(|i| transform(i, ox, oy, k)).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_assign_ids_and_reorder() {
        let mut shapes = Vec::new();
        let mut next = 0;
        let ids = ops::add(&mut shapes, vec![Shape::new(ShapeKind::Rect, 0.0, 0.0, 10.0, 10.0); 3], &mut next).expect("add");
        assert_eq!(ids, vec![1, 2, 3]);
        ops::reorder(&mut shapes, 1, ZOrder::Front).expect("front");
        assert_eq!(shapes.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2, 3, 1]);
        ops::reorder(&mut shapes, 1, ZOrder::Backward).expect("back");
        assert_eq!(shapes.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2, 1, 3]);
        ops::update(&mut shapes, &[(3, ShapePatch { w: Some(-5.0), fill: Some(None), ..Default::default() })]).expect("update");
        assert_eq!(shapes[2].w, 1.0, "boxes keep a positive size");
        assert_eq!(shapes[2].fill, None);
        assert!(ops::update(&mut shapes, &[(99, ShapePatch::default())]).is_err());
        ops::remove(&mut shapes, &[2]).expect("remove");
        assert_eq!(shapes.len(), 2);
    }

    #[test]
    fn normalize_fixes_nonsense() {
        let mut s = Shape::new(ShapeKind::Line, f32::NAN, 0.0, -50.0, f32::INFINITY);
        s.rotation = -90.0;
        s.normalize();
        assert_eq!((s.x, s.w, s.h, s.rotation), (0.0, -50.0, 0.0, 270.0));
        assert_eq!(s.bounds(), (-50.0, 0.0, 50.0, 0.0));
    }

    #[test]
    fn figure_contains_text_and_rotation() {
        let mut s = Shape::new(ShapeKind::Rect, 10.0, 10.0, 100.0, 50.0);
        s.text = "Hello".into();
        s.rotation = 45.0;
        let f = shapes_to_figure(&[s], 200.0, 100.0, Some(Rgb(255, 255, 255)));
        assert!(matches!(f.items[1], Item::Rotate { .. }));
        assert!(f.to_svg(None).contains("Hello"));
    }
}
