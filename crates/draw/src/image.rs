//! Embedded pictures: PNG, JPEG, GIF, WebP, BMP and SVG.
//!
//! An [`Image`] holds the original file bytes (never re-encoded, so nothing is
//! lost on a round trip) behind an `Arc`, which keeps undo snapshots and
//! copies cheap. The format is detected from the content, never from a file
//! name, and the pixel size is read from the header without decoding.

use office_odf::{OdfError, Result};
use serde::{Serialize, Serializer};
use std::sync::Arc;

/// Largest accepted picture file (32 MiB).
pub const MAX_IMAGE_BYTES: usize = 32 << 20;
/// Largest accepted picture side in pixels.
pub const MAX_IMAGE_SIDE: u32 = 30_000;
/// Most pixels of a raster picture (decoding needs 4 bytes per pixel).
pub const MAX_IMAGE_PIXELS: u64 = 64 << 20;

/// Refuses SVG with entity declarations: unlike the ODF parser, the SVG
/// parser accepts a DTD, so entity expansion ("billion laughs") is cut off
/// here.
pub fn check_svg(bytes: &[u8]) -> Result<()> {
    if bytes.windows(8).any(|w| w == b"<!ENTITY") {
        return Err(OdfError::unsupported("SVG pictures with entity declarations"));
    }
    Ok(())
}

/// Picture file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF (first frame).
    Gif,
    /// WebP.
    Webp,
    /// Windows bitmap.
    Bmp,
    /// Scalable Vector Graphics.
    Svg,
}

impl ImageFormat {
    /// MIME type.
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Webp => "image/webp",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Svg => "image/svg+xml",
        }
    }

    /// Conventional file extension.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Gif => "gif",
            ImageFormat::Webp => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Svg => "svg",
        }
    }

    /// Whether Flutter (and browsers) can show the format directly; SVG is
    /// rasterised by the core first.
    pub fn is_raster(self) -> bool {
        self != ImageFormat::Svg
    }
}

#[derive(Debug)]
struct ImageData {
    id: String,
    bytes: Vec<u8>,
    format: ImageFormat,
    width: u32,
    height: u32,
}

/// An immutable picture shared by reference.
#[derive(Debug, Clone)]
pub struct Image(Arc<ImageData>);

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || (self.0.id == other.0.id && self.0.bytes == other.0.bytes)
    }
}

impl Serialize for Image {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.id)
    }
}

/// 64-bit FNV-1a.
fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Detects SVG: an XML document whose root element is `svg`.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let Ok(text) = std::str::from_utf8(head).or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()])) else {
        return false;
    };
    let t = text.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<?xml") || t.starts_with("<svg") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE svg"))
        && t.contains("<svg")
}

/// Pixel size of an SVG from its `width`/`height` or `viewBox`.
fn svg_size(bytes: &[u8]) -> Result<(u32, u32)> {
    check_svg(bytes)?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| OdfError::invalid(format!("invalid SVG image: {e}")))?;
    let size = tree.size();
    Ok((size.width().ceil() as u32, size.height().ceil() as u32))
}

impl Image {
    /// Checks and wraps picture bytes. Fails for unknown formats, damaged
    /// headers and absurd sizes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Image> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(OdfError::limit("picture is too large"));
        }
        let (format, (width, height)) = if looks_like_svg(&bytes) {
            (ImageFormat::Svg, svg_size(&bytes)?)
        } else {
            let format = match imagesize::image_type(&bytes) {
                Ok(imagesize::ImageType::Png) => ImageFormat::Png,
                Ok(imagesize::ImageType::Jpeg) => ImageFormat::Jpeg,
                Ok(imagesize::ImageType::Gif) => ImageFormat::Gif,
                Ok(imagesize::ImageType::Webp) => ImageFormat::Webp,
                Ok(imagesize::ImageType::Bmp) => ImageFormat::Bmp,
                _ => return Err(OdfError::unsupported("picture format (use PNG, JPEG, GIF, WebP, BMP or SVG)")),
            };
            let size = imagesize::blob_size(&bytes).map_err(|_| OdfError::invalid("damaged picture"))?;
            let w = u32::try_from(size.width).unwrap_or(u32::MAX);
            let h = u32::try_from(size.height).unwrap_or(u32::MAX);
            (format, (w, h))
        };
        if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
            return Err(OdfError::invalid("picture has an unusable size"));
        }
        if format.is_raster() && u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
            return Err(OdfError::limit("picture has too many pixels"));
        }
        let id = format!("{:016x}{:08x}", fnv1a(&bytes, 0xcbf2_9ce4_8422_2325), bytes.len());
        Ok(Image(Arc::new(ImageData { id, bytes, format, width, height })))
    }

    /// Stable identifier derived from the content.
    pub fn id(&self) -> &str {
        &self.0.id
    }

    /// The original file bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    /// File format.
    pub fn format(&self) -> ImageFormat {
        self.0.format
    }

    /// Size in pixels.
    pub fn pixel_size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    /// Natural size in points (96 pixels per inch), scaled down to fit
    /// `max_w` × `max_h` keeping the aspect ratio.
    pub fn fit_size(&self, max_w: f32, max_h: f32) -> (f32, f32) {
        let (w, h) = (self.0.width as f32 * 0.75, self.0.height as f32 * 0.75);
        let k = (max_w / w).min(max_h / h).min(1.0);
        ((w * k).max(1.0), (h * k).max(1.0))
    }

    /// File name inside a package (`Pictures/<id>.png`).
    pub fn package_name(&self, folder: &str) -> String {
        format!("{folder}/{}.{}", self.0.id, self.0.format.extension())
    }

    /// `data:` URL for HTML and SVG export.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.0.format.mime(), base64(&self.0.bytes))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Decodes standard base64 (whitespace ignored).
pub fn base64_decode(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(OdfError::invalid("invalid base64 data")),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid 2×1 PNG.
    pub(crate) fn tiny_png() -> Vec<u8> {
        let pixmap = resvg::tiny_skia::Pixmap::new(2, 1).expect("pixmap");
        pixmap.encode_png().expect("png")
    }

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"\x00\xff\x10"] {
            assert_eq!(base64_decode(&base64(data)).expect("decode"), data);
        }
        assert_eq!(base64(b"Man"), "TWFu");
    }

    #[test]
    fn detects_svg_and_rejects_junk() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"/>"#.to_vec();
        let img = Image::from_bytes(svg).expect("svg");
        assert_eq!(img.format(), ImageFormat::Svg);
        assert_eq!(img.pixel_size(), (40, 20));
        assert!(Image::from_bytes(b"not a picture".to_vec()).is_err());
        let laughs = br#"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY a "aaaa">]><svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">&a;</svg>"#;
        assert!(Image::from_bytes(laughs.to_vec()).is_err());
        // A PNG header claiming 20000 x 20000 pixels.
        let mut huge = tiny_png();
        huge[16..20].copy_from_slice(&20_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&20_000u32.to_be_bytes());
        assert!(Image::from_bytes(huge).is_err());
        let png = Image::from_bytes(tiny_png()).expect("png");
        assert_eq!((png.format(), png.pixel_size()), (ImageFormat::Png, (2, 1)));
        assert_eq!(png.fit_size(1000.0, 1000.0), (1.5, 1.0));
    }
}
