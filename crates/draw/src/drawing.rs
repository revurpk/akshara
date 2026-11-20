//! Presentation and drawing documents with undo.

use crate::model::{ops, Page, Placeholder, Shape, ShapeKind, ShapePatch, TextStyle, VAlign, HAlign, ZOrder};
use office_odf::{Metadata, OdfError, Result, Rgb};
use serde::Serialize;

/// Most pages (slides) in one document.
pub const MAX_PAGES: usize = 2000;
/// Undo steps kept.
pub const MAX_UNDO: usize = 200;
/// Smallest page side in points (1 inch).
pub const MIN_PAGE_SIDE: f32 = 72.0;
/// Largest page side in points (200 inches).
pub const MAX_PAGE_SIDE: f32 = 14_400.0;

/// Presentation (Impress) or drawing (Draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingKind {
    /// Slides, shown one after another.
    Presentation,
    /// Pages of vector graphics.
    Drawing,
}

/// Slide layouts offered for new slides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Title and subtitle.
    Title,
    /// Title and a bulleted outline.
    Content,
    /// Title and two outlines side by side.
    TwoContent,
    /// Title only.
    TitleOnly,
    /// Nothing.
    Blank,
}

impl Layout {
    /// Parses a layout name (`title`, `content`, `two_content`,
    /// `title_only`, `blank`).
    pub fn parse(name: &str) -> Option<Layout> {
        Some(match name {
            "title" => Layout::Title,
            "content" => Layout::Content,
            "two_content" => Layout::TwoContent,
            "title_only" => Layout::TitleOnly,
            "blank" => Layout::Blank,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Content {
    pages: Vec<Page>,
    width: f32,
    height: f32,
    next_id: u32,
}

/// A presentation or drawing with editing history.
#[derive(Debug, Clone)]
pub struct Drawing {
    kind: DrawingKind,
    c: Content,
    /// Document metadata.
    pub meta: Metadata,
    undo: Vec<Content>,
    redo: Vec<Content>,
    modified: bool,
    /// Coalescing key of the last edit (moving a shape with arrow keys,
    /// typing in one shape): repeated edits with the same key are one step.
    last_key: Option<String>,
}

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

/// 16:9 slides of 28 × 15.75 cm, OpenOffice's default.
pub const SLIDE_SIZE: (f32, f32) = (793.7, 446.46);
/// A4 landscape, the default drawing page.
pub const DRAWING_SIZE: (f32, f32) = (841.89, 595.28);

fn valid_size(w: f32, h: f32) -> bool {
    let side = MIN_PAGE_SIDE..=MAX_PAGE_SIDE;
    side.contains(&w) && side.contains(&h)
}

/// Placeholder shapes of a slide layout for a page of the given size.
pub fn layout_shapes(layout: Layout, width: f32, height: f32) -> Vec<Shape> {
    let m = width * 0.06;
    let title_style = TextStyle { size: 36.0, bold: false, ..TextStyle::default() };
    let outline_style = TextStyle { size: 24.0, align: HAlign::Left, valign: VAlign::Top, bullets: true, ..TextStyle::default() };
    let ph = |role: Placeholder, x: f32, y: f32, w: f32, h: f32, style: TextStyle| Shape {
        placeholder: Some(role),
        ..Shape::text_box(x, y, w, h, "", style)
    };
    let title = |h: f32| ph(Placeholder::Title, m, height * 0.05, width - 2.0 * m, h, title_style);
    let body_top = height * 0.27;
    let body_h = height * 0.66;
    match layout {
        Layout::Title => vec![
            ph(Placeholder::Title, m, height * 0.22, width - 2.0 * m, height * 0.3, TextStyle { size: 44.0, ..title_style }),
            ph(Placeholder::Subtitle, m, height * 0.56, width - 2.0 * m, height * 0.22, TextStyle { size: 28.0, color: Rgb(0x55, 0x55, 0x55), ..TextStyle::default() }),
        ],
        Layout::Content => vec![title(height * 0.18), ph(Placeholder::Outline, m, body_top, width - 2.0 * m, body_h, outline_style)],
        Layout::TwoContent => {
            let half = (width - 3.0 * m) / 2.0;
            vec![
                title(height * 0.18),
                ph(Placeholder::Outline, m, body_top, half, body_h, outline_style),
                ph(Placeholder::Outline, 2.0 * m + half, body_top, half, body_h, outline_style),
            ]
        }
        Layout::TitleOnly => vec![title(height * 0.18)],
        Layout::Blank => Vec::new(),
    }
}

impl Drawing {
    /// A new document with one page (a title slide for presentations).
    pub fn new(kind: DrawingKind) -> Drawing {
        let (width, height) = match kind {
            DrawingKind::Presentation => SLIDE_SIZE,
            DrawingKind::Drawing => DRAWING_SIZE,
        };
        let mut d = Drawing::from_pages(kind, width, height, Vec::new());
        let layout = if kind == DrawingKind::Presentation { Layout::Title } else { Layout::Blank };
        let page = d.make_page(layout);
        d.c.pages = vec![page];
        d
    }

    /// A document from loaded pages. Ids are reassigned so they are
    /// unique; an invalid size is replaced by the default; an empty list
    /// gets one blank page.
    pub fn from_pages(kind: DrawingKind, width: f32, height: f32, mut pages: Vec<Page>) -> Drawing {
        let (width, height) = if valid_size(width, height) {
            (width, height)
        } else if kind == DrawingKind::Presentation {
            SLIDE_SIZE
        } else {
            DRAWING_SIZE
        };
        pages.truncate(MAX_PAGES);
        let mut next_id = 0u32;
        for page in &mut pages {
            page.shapes.truncate(crate::model::MAX_SHAPES);
            for s in &mut page.shapes {
                next_id += 1;
                s.id = next_id;
                s.normalize();
            }
        }
        let mut d = Drawing {
            kind,
            c: Content { pages, width, height, next_id },
            meta: Metadata::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            modified: false,
            last_key: None,
        };
        if d.c.pages.is_empty() {
            let page = d.make_page(Layout::Blank);
            d.c.pages.push(page);
        }
        d
    }

    fn make_page(&mut self, layout: Layout) -> Page {
        let mut shapes = Vec::new();
        let new = layout_shapes(layout, self.c.width, self.c.height);
        // Cannot fail: a fresh page is far below the shape limit.
        let _ = ops::add(&mut shapes, new, &mut self.c.next_id);
        Page { name: String::new(), shapes, background: None, notes: String::new() }
    }

    /// Presentation or drawing.
    pub fn kind(&self) -> DrawingKind {
        self.kind
    }

    /// The pages.
    pub fn pages(&self) -> &[Page] {
        &self.c.pages
    }

    /// Page size in points.
    pub fn size(&self) -> (f32, f32) {
        (self.c.width, self.c.height)
    }

    /// Display name of page `i` (its own name or "Slide n" / "Page n").
    pub fn page_name(&self, i: usize) -> String {
        match self.c.pages.get(i) {
            Some(p) if !p.name.is_empty() => p.name.clone(),
            _ => format!("{} {}", if self.kind == DrawingKind::Presentation { "Slide" } else { "Page" }, i + 1),
        }
    }

    /// Whether there are unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Marks the document as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// Whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts the last change.
    pub fn undo(&mut self) -> bool {
        let Some(before) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.c, before));
        self.modified = true;
        self.last_key = None;
        true
    }

    /// Re-applies the last undone change.
    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.c, after));
        self.modified = true;
        self.last_key = None;
        true
    }

    /// Ends the current group of coalesced edits.
    pub fn break_undo_group(&mut self) {
        self.last_key = None;
    }

    /// Runs `f` on a copy; commits and records undo on success. Edits with
    /// the same `key` in a row are merged into one undo step.
    fn edit<T>(&mut self, key: Option<String>, f: impl FnOnce(&mut Content) -> Result<T>) -> Result<T> {
        let mut c = self.c.clone();
        let out = f(&mut c)?;
        if c == self.c {
            return Ok(out);
        }
        let coalesce = key.is_some() && key == self.last_key && !self.undo.is_empty();
        let before = std::mem::replace(&mut self.c, c);
        if !coalesce {
            self.undo.push(before);
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.modified = true;
        self.last_key = key;
        Ok(out)
    }

    fn page_mut(c: &mut Content, page: usize) -> Result<&mut Page> {
        c.pages.get_mut(page).ok_or_else(|| bad(format!("no page {page}")))
    }

    /// Inserts a new page with a layout before `at` (`at == len` appends).
    pub fn add_page(&mut self, at: usize, layout: Layout) -> Result<()> {
        if at > self.c.pages.len() {
            return Err(bad("page index out of range"));
        }
        if self.c.pages.len() >= MAX_PAGES {
            return Err(OdfError::limit("too many pages"));
        }
        let mut next = self.c.next_id;
        let mut shapes = Vec::new();
        ops::add(&mut shapes, layout_shapes(layout, self.c.width, self.c.height), &mut next)?;
        self.edit(None, |c| {
            c.next_id = next;
            c.pages.insert(at, Page { shapes, ..Page::default() });
            Ok(())
        })
    }

    /// Inserts a copy of page `page` after it.
    pub fn duplicate_page(&mut self, page: usize) -> Result<()> {
        if self.c.pages.len() >= MAX_PAGES {
            return Err(OdfError::limit("too many pages"));
        }
        self.edit(None, |c| {
            let mut copy = Self::page_mut(c, page)?.clone();
            copy.name.clear();
            for s in &mut copy.shapes {
                c.next_id = c.next_id.checked_add(1).ok_or_else(|| OdfError::limit("too many shapes"))?;
                s.id = c.next_id;
            }
            c.pages.insert(page + 1, copy);
            Ok(())
        })
    }

    /// Removes a page; the last page cannot be removed.
    pub fn remove_page(&mut self, page: usize) -> Result<()> {
        if self.c.pages.len() <= 1 {
            return Err(bad("a document keeps at least one page"));
        }
        self.edit(None, |c| {
            Self::page_mut(c, page)?;
            c.pages.remove(page);
            Ok(())
        })
    }

    /// Moves page `from` to position `to`.
    pub fn move_page(&mut self, from: usize, to: usize) -> Result<()> {
        let n = self.c.pages.len();
        if from >= n || to >= n {
            return Err(bad("page index out of range"));
        }
        self.edit(None, |c| {
            let p = c.pages.remove(from);
            c.pages.insert(to, p);
            Ok(())
        })
    }

    /// Changes a page's name, background and notes (`None` = unchanged).
    pub fn set_page_props(&mut self, page: usize, name: Option<&str>, background: Option<Option<Rgb>>, notes: Option<&str>) -> Result<()> {
        let key = notes.map(|_| format!("notes:{page}"));
        self.edit(key, |c| {
            let p = Self::page_mut(c, page)?;
            if let Some(n) = name {
                p.name = n.chars().filter(|c| !c.is_control()).take(200).collect();
            }
            if let Some(b) = background {
                p.background = b;
            }
            if let Some(n) = notes {
                if n.len() > crate::model::MAX_SHAPE_TEXT {
                    return Err(OdfError::limit("notes are too long"));
                }
                p.notes = n.to_owned();
            }
            Ok(())
        })
    }

    /// Changes the page size of all pages. With `scale`, shapes are scaled
    /// with the page.
    pub fn set_size(&mut self, width: f32, height: f32, scale: bool) -> Result<()> {
        if !valid_size(width, height) {
            return Err(bad("page size is out of range"));
        }
        self.edit(None, |c| {
            if scale {
                let (kx, ky) = (width / c.width, height / c.height);
                for p in &mut c.pages {
                    for s in &mut p.shapes {
                        s.x *= kx;
                        s.y *= ky;
                        s.w *= kx;
                        s.h *= ky;
                        s.normalize();
                    }
                }
            }
            c.width = width;
            c.height = height;
            Ok(())
        })
    }

    /// Adds shapes on top of a page; returns their new ids.
    pub fn add_shapes(&mut self, page: usize, shapes: Vec<Shape>) -> Result<Vec<u32>> {
        self.edit(None, |c| {
            let mut next = c.next_id;
            let ids = ops::add(&mut Self::page_mut(c, page)?.shapes, shapes, &mut next)?;
            c.next_id = next;
            Ok(ids)
        })
    }

    /// Updates shapes. `key` merges repeated edits (typing into one shape,
    /// nudging with the keyboard) into one undo step.
    pub fn update_shapes(&mut self, page: usize, patches: &[(u32, ShapePatch)], key: Option<&str>) -> Result<()> {
        self.edit(key.map(|k| format!("{page}:{k}")), |c| ops::update(&mut Self::page_mut(c, page)?.shapes, patches))
    }

    /// Removes shapes.
    pub fn remove_shapes(&mut self, page: usize, ids: &[u32]) -> Result<()> {
        self.edit(None, |c| ops::remove(&mut Self::page_mut(c, page)?.shapes, ids))
    }

    /// Changes the stacking order of a shape.
    pub fn reorder(&mut self, page: usize, id: u32, to: ZOrder) -> Result<()> {
        self.edit(None, |c| ops::reorder(&mut Self::page_mut(c, page)?.shapes, id, to))
    }

    /// Runs `f` on the shapes of a page (with the id counter) as one undo
    /// step; used for edits that are not covered by the methods above.
    pub fn edit_page<T>(&mut self, page: usize, f: impl FnOnce(&mut Vec<Shape>, &mut u32) -> Result<T>) -> Result<T> {
        self.edit(None, |c| {
            let mut next = c.next_id;
            let out = f(&mut Self::page_mut(c, page)?.shapes, &mut next)?;
            c.next_id = next;
            Ok(out)
        })
    }

    /// Finds a shape by id on any page: `(page, index)`.
    pub fn find_shape(&self, id: u32) -> Option<(usize, usize)> {
        self.c.pages.iter().enumerate().find_map(|(p, page)| page.shapes.iter().position(|s| s.id == id).map(|i| (p, i)))
    }

    /// All text of the document (titles, text boxes, notes), page by page.
    pub fn plain_text(&self) -> String {
        let mut out = Vec::new();
        for (i, p) in self.c.pages.iter().enumerate() {
            out.push(format!("— {} —", self.page_name(i)));
            for s in &p.shapes {
                if !s.text.is_empty() {
                    out.push(s.text.clone());
                }
            }
            if !p.notes.is_empty() {
                out.push(p.notes.clone());
            }
            out.push(String::new());
        }
        out.join("\n")
    }

    /// The image of an id used anywhere in the document.
    pub fn image(&self, id: &str) -> Option<crate::Image> {
        self.c.pages.iter().flat_map(|p| p.shapes.iter()).find_map(|s| match &s.kind {
            ShapeKind::Image { image } if image.id() == id => Some(image.clone()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_presentation_has_a_title_slide() {
        let d = Drawing::new(DrawingKind::Presentation);
        assert_eq!(d.pages().len(), 1);
        let roles: Vec<_> = d.pages()[0].shapes.iter().map(|s| s.placeholder).collect();
        assert_eq!(roles, vec![Some(Placeholder::Title), Some(Placeholder::Subtitle)]);
        assert_eq!(d.page_name(0), "Slide 1");
        assert!(!d.is_modified());
    }

    #[test]
    fn pages_and_undo() {
        let mut d = Drawing::new(DrawingKind::Drawing);
        d.add_page(1, Layout::Content).expect("add");
        d.duplicate_page(1).expect("dup");
        assert_eq!(d.pages().len(), 3);
        let ids: Vec<u32> = d.pages().iter().flat_map(|p| p.shapes.iter().map(|s| s.id)).collect();
        let mut sorted = ids.clone();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "ids are unique");
        d.move_page(2, 0).expect("move");
        d.remove_page(0).expect("remove");
        assert!(d.undo());
        assert_eq!(d.pages().len(), 3);
        assert!(d.redo());
        assert_eq!(d.pages().len(), 2);
        assert!(d.remove_page(5).is_err());
    }

    #[test]
    fn typing_coalesces() {
        let mut d = Drawing::new(DrawingKind::Presentation);
        let id = d.pages()[0].shapes[0].id;
        for text in ["H", "He", "Hel"] {
            d.update_shapes(0, &[(id, ShapePatch { text: Some(text.into()), ..Default::default() })], Some("text")).expect("type");
        }
        assert!(d.undo());
        assert_eq!(d.pages()[0].shapes[0].text, "");
        assert!(!d.can_undo());
    }

    #[test]
    fn resize_scales_shapes() {
        let mut d = Drawing::new(DrawingKind::Drawing);
        let id = d.add_shapes(0, vec![Shape::new(ShapeKind::Rect, 100.0, 100.0, 100.0, 50.0)]).expect("add")[0];
        let (w, h) = d.size();
        d.set_size(w * 2.0, h, true).expect("size");
        let (p, i) = d.find_shape(id).expect("shape");
        assert_eq!(d.pages()[p].shapes[i].x, 200.0);
        assert!(d.set_size(10.0, 10.0, false).is_err());
    }
}
