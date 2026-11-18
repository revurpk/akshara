//! Minimal PDF output for printing: one full-page image per page.
//!
//! The editor renders each page itself (so text shaping, fonts and Indic
//! scripts look exactly as on screen) and hands the page images to
//! [`images_to_pdf`], which wraps them into a PDF that any print system or
//! viewer accepts. Images are composited onto white and compressed with
//! Flate.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use office_odf::{OdfError, Result};
use std::io::Write;

/// Largest page image side in pixels (about 600 dpi on A3).
pub const MAX_IMAGE_SIDE: u32 = 10_000;
/// Most pages one PDF may hold.
pub const MAX_PAGES: usize = 2_000;

/// An RGB page image, 8 bits per channel, rows top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes.
    pub rgb: Vec<u8>,
}

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

/// Decodes a PNG (any colour type) into RGB on a white background.
pub fn decode_png(bytes: &[u8]) -> Result<PageImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let side = MAX_IMAGE_SIDE as usize;
    decoder.set_limits(png::Limits { bytes: side * side * 4 / 4 });
    let mut reader = decoder.read_info().map_err(|e| bad(format!("invalid PNG: {e}")))?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Err(OdfError::limit("page image is too large"));
    }
    let size = reader.output_buffer_size().ok_or_else(|| OdfError::limit("page image is too large"))?;
    let mut buf = vec![0u8; size];
    let frame = reader.next_frame(&mut buf).map_err(|e| bad(format!("invalid PNG: {e}")))?;
    let channels = frame.color_type.samples();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for row in buf.chunks(frame.line_size).take(height as usize) {
        for px in row.chunks(channels).take(width as usize) {
            let (r, g, b, a) = match *px {
                [l] => (l, l, l, 255),
                [l, a] => (l, l, l, a),
                [r, g, b] => (r, g, b, 255),
                [r, g, b, a] => (r, g, b, a),
                _ => (255, 255, 255, 255),
            };
            let over = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
            rgb.extend_from_slice(&[over(r), over(g), over(b)]);
        }
    }
    if rgb.len() != width as usize * height as usize * 3 {
        return Err(bad("truncated PNG"));
    }
    Ok(PageImage { width, height, rgb })
}

/// A PDF text string: plain ASCII escaped, anything else as UTF-16BE hex.
fn pdf_string(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        let mut out = String::from("(");
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for unit in s.encode_utf16() {
            out.push_str(&format!("{unit:04X}"));
        }
        out.push('>');
        out
    }
}

/// Builds a PDF whose pages are `width_pt` × `height_pt` points, each
/// showing one image stretched over the whole page.
pub fn images_to_pdf(pages: &[PageImage], width_pt: f32, height_pt: f32, title: Option<&str>) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(bad("nothing to print"));
    }
    if pages.len() > MAX_PAGES {
        return Err(OdfError::limit("too many pages"));
    }
    let side = 36.0..=14_400.0;
    if !side.contains(&width_pt) || !side.contains(&height_pt) {
        return Err(bad("page size out of range"));
    }
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let object = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, head: String, stream: Option<&[u8]>| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{head}\n", offsets.len()).as_bytes());
        if let Some(data) = stream {
            out.extend_from_slice(b"stream\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream\n");
        }
        out.extend_from_slice(b"endobj\n");
    };
    // Objects: 1 catalog, 2 page tree, 3 info, then per page: page,
    // content stream, image.
    let page_id = |i: usize| 4 + 3 * i;
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", page_id(i))).collect();
    object(&mut out, &mut offsets, "<< /Type /Catalog /Pages 2 0 R >>".into(), None);
    object(
        &mut out,
        &mut offsets,
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()),
        None,
    );
    let title = title.map(|t| format!(" /Title {}", pdf_string(&t.chars().take(500).collect::<String>()))).unwrap_or_default();
    object(&mut out, &mut offsets, format!("<< /Producer (Akshara Office){title} >>"), None);
    for (i, page) in pages.iter().enumerate() {
        let expected = page.width as usize * page.height as usize * 3;
        if page.width == 0 || page.height == 0 || page.rgb.len() != expected {
            return Err(bad(format!("page {} has an invalid image", i + 1)));
        }
        let id = page_id(i);
        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width_pt:.2} {height_pt:.2}] \
                 /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>",
                id + 2,
                id + 1
            ),
            None,
        );
        let content = format!("q {width_pt:.2} 0 0 {height_pt:.2} 0 0 cm /Im0 Do Q");
        object(&mut out, &mut offsets, format!("<< /Length {} >>", content.len()), Some(content.as_bytes()));
        let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
        z.write_all(&page.rgb).map_err(OdfError::from)?;
        let data = z.finish().map_err(OdfError::from)?;
        object(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                page.width,
                page.height,
                data.len()
            ),
            Some(&data),
        );
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1)
            .as_bytes(),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(w: u32, h: u32) -> PageImage {
        PageImage { width: w, height: h, rgb: vec![255; (w * h * 3) as usize] }
    }

    #[test]
    fn pdf_has_one_page_per_image_and_a_valid_xref() {
        let pdf = images_to_pdf(&[white(10, 14), white(10, 14)], 595.28, 841.89, Some("Notes – నోట్స్")).expect("pdf");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 2"));
        assert_eq!(text.matches("/Type /Page ").count(), 2);
        assert!(text.contains("/Title <FEFF"));
        // Every xref offset points at the start of its object.
        let start: usize = text.rsplit("startxref\n").next().and_then(|t| t.lines().next()).and_then(|n| n.parse().ok()).expect("startxref");
        assert!(pdf[start..].starts_with(b"xref\n"));
        let table = String::from_utf8_lossy(&pdf[start..]).into_owned();
        let mut count = 0;
        for (n, line) in table.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let off: usize = line[..10].parse().expect("offset");
            assert!(pdf[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "object {}", n + 1);
            count += 1;
        }
        assert_eq!(count, 3 + 3 * 2);
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(images_to_pdf(&[], 100.0, 100.0, None).is_err());
        let broken = PageImage { width: 2, height: 2, rgb: vec![0; 5] };
        assert!(images_to_pdf(&[broken], 100.0, 100.0, None).is_err());
        assert!(images_to_pdf(&[white(1, 1)], 1.0, 100.0, None).is_err());
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn png_alpha_is_composited_on_white() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("header");
            w.write_image_data(&[0, 0, 0, 0, 255, 0, 0, 255]).expect("data");
        }
        let img = decode_png(&png_bytes).expect("decode");
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgb, vec![255, 255, 255, 255, 0, 0]);
    }
}
