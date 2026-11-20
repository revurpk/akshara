//! Raster output: SVG and figures to PNG, through resvg (pure Rust).
//!
//! Fonts come from the directories registered with [`add_font_dir`] (the
//! app registers its bundled Noto fonts) and from the system font folders,
//! loaded once on first use. External files referenced by an SVG are never
//! read.

use crate::figure::Figure;
use office_odf::{OdfError, Result, Rgb};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Largest rendered side in pixels.
pub const MAX_RENDER_SIDE: u32 = 8192;

struct Fonts {
    dirs: Vec<PathBuf>,
    db: Option<Arc<resvg::usvg::fontdb::Database>>,
}

fn fonts() -> &'static Mutex<Fonts> {
    static FONTS: std::sync::OnceLock<Mutex<Fonts>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| Mutex::new(Fonts { dirs: Vec::new(), db: None }))
}

/// Registers a folder of font files for rendering text (call before the
/// first render; later calls rebuild the font list).
pub fn add_font_dir(dir: impl Into<PathBuf>) {
    let mut f = fonts().lock().unwrap_or_else(|p| p.into_inner());
    let dir = dir.into();
    if !f.dirs.contains(&dir) {
        f.dirs.push(dir);
        f.db = None;
    }
}

fn font_db() -> Arc<resvg::usvg::fontdb::Database> {
    let mut f = fonts().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(db) = &f.db {
        return db.clone();
    }
    let mut db = resvg::usvg::fontdb::Database::new();
    for dir in &f.dirs {
        db.load_fonts_dir(dir);
    }
    db.load_system_fonts();
    for (family, name) in [("Noto Sans", "sans-serif"), ("Noto Serif", "serif"), ("Noto Sans Mono", "monospace")] {
        if db.faces().any(|face| face.families.iter().any(|(n, _)| n == family)) {
            match name {
                "sans-serif" => db.set_sans_serif_family(family),
                "serif" => db.set_serif_family(family),
                _ => db.set_monospace_family(family),
            }
        }
    }
    let db = Arc::new(db);
    f.db = Some(db.clone());
    db
}

/// Renders SVG bytes to PNG, scaled so the longer side is at most
/// `max_side` pixels (and by `scale` otherwise).
pub fn svg_to_png(svg: &[u8], scale: f32, max_side: u32, background: Option<Rgb>) -> Result<Vec<u8>> {
    crate::image::check_svg(svg)?;
    let mut opt = resvg::usvg::Options { fontdb: font_db(), ..resvg::usvg::Options::default() };
    // Never touch the file system for <image href="file">.
    opt.resources_dir = None;
    opt.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = resvg::usvg::Tree::from_data(svg, &opt).map_err(|e| OdfError::invalid(format!("invalid SVG: {e}")))?;
    let size = tree.size();
    let max_side = max_side.clamp(1, MAX_RENDER_SIDE) as f32;
    let k = scale.max(0.01).min(max_side / size.width().max(size.height()).max(1.0));
    let (w, h) = ((size.width() * k).ceil().max(1.0) as u32, (size.height() * k).ceil().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| OdfError::limit("picture is too large to render"))?;
    if let Some(Rgb(r, g, b)) = background {
        pixmap.fill(resvg::tiny_skia::Color::from_rgba8(r, g, b, 255));
    }
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| OdfError::invalid(format!("cannot encode PNG: {e}")))
}

/// Renders a figure to PNG at `scale` pixels per point.
pub fn figure_to_png(fig: &Figure, scale: f32, background: Option<Rgb>) -> Result<Vec<u8>> {
    svg_to_png(fig.to_svg(None).as_bytes(), scale, MAX_RENDER_SIDE, background)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_figures_and_ignores_external_files() {
        let fig = crate::mermaid::render("graph LR\nA-->B").figure;
        let png = figure_to_png(&fig, 2.0, Some(Rgb(255, 255, 255))).expect("png");
        let img = crate::Image::from_bytes(png).expect("valid png");
        assert!(img.pixel_size().0 > 100);
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="/etc/passwd" width="10" height="10"/></svg>"#;
        assert!(svg_to_png(svg, 1.0, 100, None).is_ok());
    }
}
