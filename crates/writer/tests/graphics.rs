//! Pictures, Mermaid diagrams and drawings in text documents.

use office_draw::{model::ops, Image, Shape, ShapeKind, ShapePatch};
use office_odf::{Limits, Package};
use office_writer::{export, load_docx, load_odt, save_docx, save_odt, Align, Block, Canvas, Diagram, Document, Paragraph, Picture};

fn png() -> Image {
    let bytes = resvg::tiny_skia::Pixmap::new(8, 4).expect("pixmap").encode_png().expect("png");
    Image::from_bytes(bytes).expect("image")
}

fn svg() -> Image {
    Image::from_bytes(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#.to_vec()).expect("svg")
}

fn sample() -> Document {
    let mut picture = Picture::new(png(), 400.0);
    picture.width = 120.0;
    picture.height = 60.0;
    picture.align = Align::Right;
    picture.description = "A red box".into();
    let diagram = Diagram { source: "graph TD\nA[Start] --> B[End]".into(), width: 200.0, align: Align::Center };
    let mut canvas = Canvas::new(300.0, 150.0);
    let mut rect = Shape::new(ShapeKind::Ellipse, 10.0, 10.0, 100.0, 50.0);
    rect.text = "Hi".into();
    let line = Shape::new(ShapeKind::Line, 0.0, 0.0, 200.0, 100.0);
    canvas.shapes = vec![rect, line, Shape::new(ShapeKind::Image { image: png() }, 150.0, 20.0, 40.0, 20.0)];
    Document::from_blocks(vec![
        Block::Paragraph(Paragraph::plain("before")),
        Block::Image(picture),
        Block::Diagram(diagram),
        Block::Drawing(canvas),
        Block::Image(Picture::new(svg(), 400.0)),
        Block::Paragraph(Paragraph::plain("after")),
    ])
}

#[test]
fn odt_round_trip() {
    let doc = sample();
    let bytes = save_odt(&doc).expect("save");
    let pkg = Package::read(&bytes, &Limits::default()).expect("pkg");
    let names: Vec<&str> = pkg.entry_names().collect();
    assert!(names.contains(&"Object 1/content.xml"), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("Object 1/Pictures/")), "drawing pictures live in the object");
    assert!(names.iter().any(|n| n.starts_with("Pictures/mermaid-")));
    // The manifest is generated on write; check it in the raw ZIP.
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    assert!(raw.contains("META-INF/manifest.xml"));

    let back = load_odt(&bytes, &Limits::default()).expect("load");
    let b = back.blocks();
    assert_eq!(b.len(), 6, "{b:#?}");
    match &b[1] {
        Block::Image(p) => {
            assert_eq!((p.width.round(), p.height.round(), p.align), (120.0, 60.0, Align::Right));
            assert_eq!(p.description, "A red box");
            assert_eq!(p.image.pixel_size(), (8, 4));
        }
        other => panic!("{other:?}"),
    }
    match &b[2] {
        Block::Diagram(d) => assert_eq!((d.source.as_str(), d.width.round()), ("graph TD\nA[Start] --> B[End]", 200.0)),
        other => panic!("{other:?}"),
    }
    match &b[3] {
        Block::Drawing(c) => {
            assert_eq!((c.width.round(), c.height.round()), (300.0, 150.0));
            assert_eq!(c.shapes.len(), 3);
            assert_eq!(c.shapes[0].text, "Hi");
            assert!(matches!(c.shapes[2].kind, ShapeKind::Image { .. }));
            let ids: Vec<u32> = c.shapes.iter().map(|s| s.id).collect();
            assert_eq!(ids, vec![1, 2, 3]);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&b[4], Block::Image(p) if p.image.format() == office_draw::ImageFormat::Svg));
}

#[test]
fn docx_round_trip() {
    let bytes = save_docx(&sample()).expect("save");
    let pkg = Package::read(&bytes, &Limits::default()).expect("pkg");
    assert!(pkg.entry_names().any(|n| n == "word/media/image1.png"));
    assert!(pkg.entry_names().any(|n| n.starts_with("word/media/") && n.ends_with(".svg")));
    let types = String::from_utf8(pkg.get("[Content_Types].xml").expect("types").to_vec()).expect("utf8");
    assert!(types.contains("Extension=\"svg\""));
    let back = load_docx(&bytes, &Limits::default()).expect("load");
    let b = back.blocks();
    assert_eq!(b.len(), 6, "{b:#?}");
    assert!(matches!(&b[1], Block::Image(p) if p.description == "A red box" && p.align == Align::Right && p.width.round() == 120.0));
    assert!(matches!(&b[2], Block::Diagram(d) if d.source.contains("A[Start]")));
    // Drawings become pictures in Word (SVG preferred over the PNG).
    assert!(matches!(&b[3], Block::Image(p) if p.image.format() == office_draw::ImageFormat::Svg));
    assert!(matches!(&b[4], Block::Image(_)));
}

#[test]
fn html_and_markdown() {
    let doc = sample();
    let html = export::to_html(&doc);
    assert!(html.contains("img-src data:"));
    assert!(html.contains("<img src=\"data:image/png;base64,"));
    assert!(html.contains("alt=\"A red box\""));
    assert!(html.matches("<svg").count() >= 2, "diagram and drawing are inline SVG");
    let md = export::to_markdown(&doc);
    assert!(md.contains("```mermaid\ngraph TD\nA[Start] --> B[End]\n```"));
    assert!(md.contains("![A red box](data:image/png;base64,"));
    assert!(md.contains("![Drawing](data:image/svg+xml;base64,"));
}

#[test]
fn editing_frames_and_canvases() {
    let mut doc = sample();
    doc.set_frame_size(1, 240.0, None).expect("size");
    assert!(matches!(&doc.blocks()[1], Block::Image(p) if p.height.round() == 120.0), "aspect kept");
    doc.set_frame_align(2, Align::Left).expect("align");
    doc.set_diagram_source(2, "pie\n\"a\": 1").expect("source");
    assert!(doc.set_diagram_source(1, "x").is_err());
    let id = doc.edit_canvas(3, false, |shapes, next| ops::add(shapes, vec![Shape::new(ShapeKind::Rect, 0.0, 0.0, 5.0, 5.0)], next)).expect("add")[0];
    assert_eq!(id, 4, "ids continue after the loaded shapes");
    for x in [1.0, 2.0, 3.0] {
        doc.edit_canvas(3, true, |shapes, _| ops::update(shapes, &[(id, ShapePatch { x: Some(x), ..Default::default() })])).expect("move");
    }
    assert!(doc.undo(), "the three moves are one step");
    assert!(matches!(&doc.blocks()[3], Block::Drawing(c) if c.shapes.last().map(|s| s.x) == Some(0.0)));
    assert!(doc.edit_canvas(0, false, |_, _| Ok(())).is_err());
    let png_id = match &doc.blocks()[1] {
        Block::Image(p) => p.image.id().to_owned(),
        _ => unreachable!(),
    };
    assert!(doc.image(&png_id).is_some());
    assert!(doc.image("nope").is_none());
    assert!(doc.plain_text().contains("pie"));
}
