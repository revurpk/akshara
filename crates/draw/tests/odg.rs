//! ODP / ODG round trips and imports of hand-written OpenDocument files.

use office_draw::{odg, Drawing, DrawingKind, HAlign, Image, Layout, Placeholder, Shape, ShapeKind, ShapePatch};
use office_odf::{ns, Limits, Package, Rgb};

fn png() -> Vec<u8> {
    resvg::tiny_skia::Pixmap::new(4, 2).expect("pixmap").encode_png().expect("png")
}

#[test]
fn presentation_round_trip_keeps_everything() {
    let mut d = Drawing::new(DrawingKind::Presentation);
    let title = d.pages()[0].shapes[0].id;
    d.update_shapes(0, &[(title, ShapePatch { text: Some("Hello  world\tagain".into()), bold: Some(true), ..Default::default() })], None).expect("title");
    d.add_page(1, Layout::Content).expect("slide");
    let outline = d.pages()[1].shapes[1].id;
    d.update_shapes(1, &[(outline, ShapePatch { text: Some("One\nTwo".into()), ..Default::default() })], None).expect("outline");
    let mut rect = Shape::new(ShapeKind::RoundRect, 10.0, 20.0, 100.0, 50.0);
    rect.rotation = 45.0;
    rect.dash = true;
    rect.text = "box".into();
    let mut line = Shape::new(ShapeKind::Line, 300.0, 300.0, -100.0, 40.0);
    line.arrow_end = true;
    let picture = Shape::new(ShapeKind::Image { image: Image::from_bytes(png()).expect("img") }, 400.0, 50.0, 40.0, 20.0);
    let diagram = Shape::new(ShapeKind::Diagram { source: "graph LR\nA-->B".into() }, 50.0, 200.0, 200.0, 100.0);
    let tri = Shape::new(ShapeKind::Triangle, 500.0, 300.0, 60.0, 60.0);
    let poly = Shape::new(ShapeKind::Polygon { points: vec![[0.0, 0.0], [1.0, 0.5], [0.0, 1.0]], closed: false }, 600.0, 300.0, 60.0, 60.0);
    d.add_shapes(1, vec![rect, line, picture, diagram, tri, poly]).expect("shapes");
    d.set_page_props(1, Some("Agenda"), Some(Some(Rgb(0xee, 0xee, 0xff))), Some("Say hi")).expect("props");
    d.meta.title = Some("Deck".into());

    let bytes = odg::save(&d).expect("save");
    let pkg = Package::read(&bytes, &Limits::default()).expect("package");
    assert_eq!(pkg.mimetype(), ns::MIME_ODP);
    assert!(pkg.entry_names().any(|n| n.starts_with("Pictures/") && n.ends_with(".png")));
    assert!(pkg.entry_names().any(|n| n.starts_with("Pictures/mermaid-") && n.ends_with(".svg")));

    let back = odg::load(&bytes, &Limits::default()).expect("load");
    assert_eq!(back.kind(), DrawingKind::Presentation);
    assert_eq!(back.pages().len(), 2);
    assert_eq!(back.meta.title.as_deref(), Some("Deck"));
    let p0 = &back.pages()[0].shapes;
    assert_eq!(p0[0].text, "Hello  world\tagain");
    assert!(p0[0].style.bold);
    assert_eq!(p0[0].placeholder, Some(Placeholder::Title));
    assert_eq!(p0[1].placeholder, Some(Placeholder::Subtitle));
    let page = &back.pages()[1];
    assert_eq!(page.name, "Agenda");
    assert_eq!(page.notes, "Say hi");
    assert_eq!(page.background, Some(Rgb(0xee, 0xee, 0xff)));
    let shapes = &page.shapes;
    assert_eq!(shapes.len(), 8);
    assert_eq!(shapes[1].text, "One\nTwo");
    assert!(shapes[1].style.bullets);
    assert_eq!(shapes[1].style.align, HAlign::Left);
    let r = &shapes[2];
    assert!(matches!(r.kind, ShapeKind::RoundRect));
    assert!((r.rotation - 45.0).abs() < 0.01 && (r.x - 10.0).abs() < 0.05 && (r.y - 20.0).abs() < 0.05, "{r:?}");
    assert!(r.dash);
    assert_eq!(r.text, "box");
    let l = &shapes[3];
    assert!(matches!(l.kind, ShapeKind::Line) && l.arrow_end && !l.arrow_start);
    assert!((l.w + 100.0).abs() < 0.05 && (l.h - 40.0).abs() < 0.05);
    assert!(matches!(&shapes[4].kind, ShapeKind::Image { image } if image.pixel_size() == (4, 2)));
    assert!(matches!(&shapes[5].kind, ShapeKind::Diagram { source } if source == "graph LR\nA-->B"));
    assert!(matches!(shapes[6].kind, ShapeKind::Triangle));
    assert!(matches!(&shapes[7].kind, ShapeKind::Polygon { closed: false, points } if points.len() == 3));
    assert_eq!(shapes[7].fill, None);

    // Saving again gives the same bytes (deterministic output).
    assert_eq!(odg::save(&back).expect("save"), odg::save(&odg::load(&odg::save(&back).expect("s"), &Limits::default()).expect("l")).expect("s2"));
}

#[test]
fn drawing_uses_odg_and_page_size() {
    let mut d = Drawing::new(DrawingKind::Drawing);
    d.set_size(300.0, 200.0, false).expect("size");
    let bytes = odg::save(&d).expect("save");
    let pkg = Package::read(&bytes, &Limits::default()).expect("package");
    assert_eq!(pkg.mimetype(), ns::MIME_ODG);
    let back = odg::load(&bytes, &Limits::default()).expect("load");
    assert_eq!(back.kind(), DrawingKind::Drawing);
    let (w, h) = back.size();
    assert!((w - 300.0).abs() < 0.1 && (h - 200.0).abs() < 0.1);
}

const CONTENT: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.2">
 <office:automatic-styles>
  <style:style style:name="gr1" style:family="graphic" style:parent-style-name="objectwithoutfill">
   <style:graphic-properties draw:fill="solid" draw:fill-color="#ff0000" draw:stroke="none"/>
  </style:style>
  <style:style style:name="gr2" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="g1"/></style:style>
  <style:style style:name="pr1" style:family="presentation" style:parent-style-name="Default-title"/>
  <style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style>
  <style:style style:name="T1" style:family="text"><style:text-properties fo:font-size="30pt" fo:font-style="italic" fo:color="#00ff00"/></style:style>
 </office:automatic-styles>
 <office:body><office:presentation>
  <draw:page draw:name="page1" draw:master-page-name="Master">
   <draw:frame presentation:style-name="pr1" presentation:class="title" svg:x="2cm" svg:y="1cm" svg:width="20cm" svg:height="3cm">
    <draw:text-box><text:p text:style-name="P1"><text:span text:style-name="T1">Big</text:span> title</text:p></draw:text-box>
   </draw:frame>
   <draw:g>
    <draw:custom-shape draw:style-name="gr1" svg:x="1in" svg:y="1in" svg:width="1in" svg:height="1in">
     <text:p>Decision</text:p>
     <draw:enhanced-geometry draw:type="flowchart-decision"/>
    </draw:custom-shape>
    <draw:path draw:style-name="gr2" svg:x="3in" svg:y="1in" svg:width="1in" svg:height="1in" svg:viewBox="0 0 100 100" svg:d="M0 0 L100 0 100 100 Z"/>
   </draw:g>
   <draw:connector svg:x1="0cm" svg:y1="0cm" svg:x2="2cm" svg:y2="2cm"/>
   <draw:frame svg:x="0cm" svg:y="0cm" svg:width="1cm" svg:height="1cm"><draw:image xlink:href="http://example.com/x.png"/></draw:frame>
   <unknown:thing xmlns:unknown="urn:x"/>
  </draw:page>
 </office:presentation></office:body>
</office:document-content>"##;

const STYLES: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
 <office:styles>
  <draw:gradient draw:name="g1" draw:start-color="#123456" draw:end-color="#ffffff"/>
  <style:style style:name="Default-title" style:family="presentation"><style:text-properties fo:font-size="44pt"/></style:style>
 </office:styles>
 <office:automatic-styles>
  <style:page-layout style:name="PM1"><style:page-layout-properties fo:page-width="25.4cm" fo:page-height="19.05cm"/></style:page-layout>
  <style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties draw:fill="solid" draw:fill-color="#000080"/></style:style>
 </office:automatic-styles>
 <office:master-styles>
  <style:master-page style:name="Master" style:page-layout-name="PM1" draw:style-name="Mdp1">
   <draw:frame presentation:class="title" svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box/></draw:frame>
   <draw:rect svg:x="0cm" svg:y="18cm" svg:width="25.4cm" svg:height="1cm"/>
  </style:master-page>
 </office:master-styles>
</office:document-styles>"##;

fn package(content: &str) -> Vec<u8> {
    let mut p = Package::new(ns::MIME_ODP);
    p.insert("content.xml", content.as_bytes().to_vec()).expect("content");
    p.insert("styles.xml", STYLES.as_bytes().to_vec()).expect("styles");
    p.write().expect("write")
}

#[test]
fn imports_libreoffice_style_presentations() {
    let d = odg::load(&package(CONTENT), &Limits::default()).expect("load");
    let (w, h) = d.size();
    assert!((w - 720.0).abs() < 0.5 && (h - 540.0).abs() < 0.5, "4:3 page {w}x{h}");
    let page = &d.pages()[0];
    assert!(page.name.is_empty(), "default names are dropped");
    assert_eq!(page.background, Some(Rgb(0, 0, 0x80)), "master background");
    let s = &page.shapes;
    // Master decoration first, then the page's shapes; the master's empty
    // title placeholder and the remote picture are skipped.
    assert_eq!(s.len(), 5, "{s:#?}");
    assert!(matches!(s[0].kind, ShapeKind::Rect));
    assert_eq!(s[1].text, "Big title");
    assert_eq!(s[1].placeholder, Some(Placeholder::Title));
    assert_eq!(s[1].style.size, 30.0);
    assert!(s[1].style.italic);
    assert_eq!(s[1].style.color, Rgb(0, 255, 0));
    assert_eq!(s[1].style.align, HAlign::Center);
    assert!(matches!(s[2].kind, ShapeKind::Diamond));
    assert_eq!(s[2].fill, Some(Rgb(255, 0, 0)));
    assert_eq!(s[2].stroke, None);
    assert_eq!(s[2].text, "Decision");
    assert!(matches!(&s[3].kind, ShapeKind::Polygon { closed: true, points } if points.len() == 3));
    assert_eq!(s[3].fill, Some(Rgb(0x12, 0x34, 0x56)), "gradient start colour");
    assert!(matches!(s[4].kind, ShapeKind::Line));
}

#[test]
fn rejects_other_documents_and_garbage() {
    let mut p = Package::new(ns::MIME_ODT);
    p.insert("content.xml", b"<a/>".to_vec()).expect("content");
    assert!(odg::load(&p.write().expect("write"), &Limits::default()).is_err());
    assert!(odg::load(b"PK\x03\x04junk", &Limits::default()).is_err());
    let deep = format!("{}{}", CONTENT.replace("<draw:g>", &"<draw:g>".repeat(40)), "");
    // Unbalanced on purpose: must fail cleanly, not panic.
    assert!(odg::load(&package(&deep), &Limits::default()).is_err());
}

#[test]
fn shape_limit_is_enforced() {
    let many = format!(
        "{}{}{}",
        &CONTENT[..CONTENT.find("<draw:g>").expect("g")],
        "<draw:rect svg:width=\"1cm\" svg:height=\"1cm\"/>".repeat(100),
        &CONTENT[CONTENT.find("<draw:connector").expect("c")..]
    );
    let limits = Limits { max_blocks: 50, ..Limits::default() };
    assert!(odg::load(&package(&many), &limits).is_err());
}
