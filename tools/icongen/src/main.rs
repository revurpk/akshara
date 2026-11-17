//! Generates all application icons from `branding/*.svg`.
//!
//! Run from the repository root: `cargo run -p icongen --release`.
//! Outputs go into the Flutter platform projects and `branding/out/`.

use resvg::{tiny_skia, usvg};
use std::fs;
use std::path::Path;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Variants derived from the master logo.
#[derive(Clone, Copy)]
enum Variant {
    /// Rounded tile, transparent corners (desktop, in-app).
    Rounded,
    /// Full-bleed square, opaque (iOS adds its own mask).
    Square,
    /// Rounded tile with the ~10% margin macOS icons use.
    MacOs,
    /// Artwork only, scaled into Android's adaptive-icon safe zone.
    AdaptiveForeground,
}

fn svg_source(master: &str, v: Variant) -> String {
    match v {
        Variant::Rounded => master.to_owned(),
        Variant::Square => master.replace("rx=\"232\"", "rx=\"0\""),
        Variant::MacOs => {
            // macOS icons sit on a smaller tile with a transparent margin.
            let defs_end = master.find("</defs>").map(|i| i + 7).unwrap_or(0);
            let end = master.rfind("</svg>").unwrap_or(master.len());
            format!(
                "{}<g transform=\"translate(100 100) scale(0.8047)\">{}</g></svg>",
                &master[..defs_end],
                &master[defs_end..end]
            )
        }
        Variant::AdaptiveForeground => {
            // Drop the tile; fit the artwork into the adaptive-icon safe zone.
            let start = master.find("<g filter=").unwrap_or(0);
            let end = master.rfind("</svg>").unwrap_or(master.len());
            let defs_end = master.find("</defs>").map(|i| i + 7).unwrap_or(0);
            format!(
                "{}<g transform=\"translate(512 512) scale(0.78) translate(-512 -512)\">{}</g></svg>",
                &master[..defs_end],
                &master[start..end]
            )
        }
    }
}

fn options() -> usvg::Options<'static> {
    // Only the bundled OFL fonts are used, never system fonts, so the icons
    // come out identical on every machine.
    let mut opt = usvg::Options::default();
    for font in ["NotoSerifDevanagari-Bold.ttf", "NotoSerifDevanagari-Regular.ttf", "NotoSans-Regular.ttf"] {
        let path = format!("app/assets/fonts/{font}");
        match fs::read(&path) {
            Ok(data) => opt.fontdb_mut().load_font_data(data),
            Err(e) => eprintln!("warning: cannot load {path}: {e}"),
        }
    }
    opt
}

fn render(svg: &str, size: u32, opaque_bg: Option<[u8; 3]>) -> Res<tiny_skia::Pixmap> {
    let opt = options();
    let tree = usvg::Tree::from_str(svg, &opt)?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("bad size")?;
    if let Some([r, g, b]) = opaque_bg {
        pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    }
    let scale = size as f32 / tree.size().width();
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap)
}

fn write_png(svg: &str, size: u32, path: &str, opaque_bg: Option<[u8; 3]>) -> Res<Vec<u8>> {
    let png = render(svg, size, opaque_bg)?.encode_png()?;
    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, &png)?;
    println!("wrote {path} ({size}x{size})");
    Ok(png)
}

/// Writes a Windows .ico containing PNG-compressed images.
fn write_ico(svg: &str, sizes: &[u32], path: &str) -> Res<()> {
    let images: Vec<(u32, Vec<u8>)> =
        sizes.iter().map(|&s| Ok((s, render(svg, s, None)?.encode_png()?))).collect::<Res<_>>()?;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, out)?;
    println!("wrote {path} ({} sizes)", sizes.len());
    Ok(())
}

fn main() -> Res<()> {
    let master = fs::read_to_string("branding/logo.svg")?;
    let favicon = fs::read_to_string("branding/favicon.svg")?;
    let rounded = svg_source(&master, Variant::Rounded);
    let square = svg_source(&master, Variant::Square);
    let macos = svg_source(&master, Variant::MacOs);
    let foreground = svg_source(&master, Variant::AdaptiveForeground);

    // Shared outputs.
    for size in [16, 32, 48, 64, 128, 256, 512, 1024] {
        write_png(&rounded, size, &format!("branding/out/akshara-{size}.png"), None)?;
    }
    write_png(&rounded, 512, "app/assets/branding/logo.png", None)?;

    // Favicons (docs site, any web page about the project).
    for size in [16, 32, 48] {
        write_png(&favicon, size, &format!("branding/out/favicon-{size}.png"), None)?;
    }
    write_png(&square, 180, "branding/out/apple-touch-icon.png", Some([0xE0, 0x6A, 0x12]))?;
    write_ico(&favicon, &[16, 32, 48], "branding/out/favicon.ico")?;
    fs::copy("branding/favicon.svg", "branding/out/favicon.svg")?;

    // Windows.
    write_ico(&rounded, &[16, 24, 32, 48, 64, 128, 256], "app/windows/runner/resources/app_icon.ico")?;

    // Linux (hicolor theme sizes, installed next to the binary and by packages).
    for size in [16, 32, 48, 64, 128, 256, 512] {
        write_png(&rounded, size, &format!("packaging/linux/icons/hicolor/{size}x{size}/apps/org.akshara.Office.png"), None)?;
    }
    write_png(&rounded, 256, "app/linux/runner/resources/app_icon.png", None)?;

    // macOS.
    for size in [16, 32, 64, 128, 256, 512, 1024] {
        write_png(&macos, size, &format!("app/macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_{size}.png"), None)?;
    }

    // iOS (opaque, full bleed).
    let ios: &[(&str, f32, u32)] = &[
        ("20x20", 20.0, 1), ("20x20", 20.0, 2), ("20x20", 20.0, 3), ("29x29", 29.0, 1), ("29x29", 29.0, 2),
        ("29x29", 29.0, 3), ("40x40", 40.0, 1), ("40x40", 40.0, 2), ("40x40", 40.0, 3), ("60x60", 60.0, 2),
        ("60x60", 60.0, 3), ("76x76", 76.0, 1), ("76x76", 76.0, 2), ("83.5x83.5", 83.5, 2), ("1024x1024", 1024.0, 1),
    ];
    for (name, points, scale) in ios {
        let px = (points * *scale as f32).round() as u32;
        write_png(
            &square,
            px,
            &format!("app/ios/Runner/Assets.xcassets/AppIcon.appiconset/Icon-App-{name}@{scale}x.png"),
            Some([0xE0, 0x6A, 0x12]),
        )?;
    }

    // Android: legacy launcher icons and adaptive-icon foregrounds.
    let res = "app/android/app/src/main/res";
    for (density, legacy, adaptive) in
        [("mdpi", 48, 108), ("hdpi", 72, 162), ("xhdpi", 96, 216), ("xxhdpi", 144, 324), ("xxxhdpi", 192, 432)]
    {
        write_png(&rounded, legacy, &format!("{res}/mipmap-{density}/ic_launcher.png"), None)?;
        write_png(&foreground, adaptive, &format!("{res}/mipmap-{density}/ic_launcher_foreground.png"), None)?;
    }
    fs::create_dir_all(format!("{res}/mipmap-anydpi-v26"))?;
    fs::write(
        format!("{res}/mipmap-anydpi-v26/ic_launcher.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\">\n    \
         <background android:drawable=\"@color/ic_launcher_background\"/>\n    \
         <foreground android:drawable=\"@mipmap/ic_launcher_foreground\"/>\n    \
         <monochrome android:drawable=\"@mipmap/ic_launcher_foreground\"/>\n</adaptive-icon>\n",
    )?;
    fs::write(
        format!("{res}/values/ic_launcher_background.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n    <color name=\"ic_launcher_background\">#E06A12</color>\n</resources>\n",
    )?;
    println!("done");
    Ok(())
}
