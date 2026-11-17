//! Editing, undo, search and ODT round-trip tests for the Writer core.

use office_odf::{ns, Limits, OdfError, Package, Rgb};
use office_writer::*;
use proptest::prelude::*;

fn bold() -> AttrPatch {
    AttrPatch { bold: Some(true), ..Default::default() }
}

fn runs(doc: &Document, r: ParaRef) -> Vec<(String, bool)> {
    doc.paragraph(r).expect("para").runs.iter().map(|run| (run.text.clone(), run.attrs.bold)).collect()
}

const P0: ParaRef = ParaRef::top(0);

#[test]
fn typing_preserves_attributes_around_the_edit() {
    let mut doc = Document::new();
    doc.set_text(P0, "Hello world").expect("set");
    doc.apply_attrs(P0, 6, 11, &bold()).expect("bold");
    assert_eq!(runs(&doc, P0), [("Hello ".into(), false), ("world".into(), true)]);
    // Typing at the end of the bold run continues bold.
    doc.set_text(P0, "Hello world!").expect("type");
    assert_eq!(runs(&doc, P0), [("Hello ".into(), false), ("world!".into(), true)]);
    // Typing inside the plain part stays plain.
    doc.set_text(P0, "Hello, world!").expect("type");
    assert_eq!(runs(&doc, P0), [("Hello, ".into(), false), ("world!".into(), true)]);
    // Deleting across the boundary merges correctly.
    doc.replace_text(P0, 5, 9, "", None).expect("delete");
    assert_eq!(doc.paragraph(P0).expect("p").text(), "Hellorld!");
    assert_eq!(runs(&doc, P0), [("Hello".into(), false), ("rld!".into(), true)]);
    // Removing bold merges runs.
    doc.apply_attrs(P0, 0, 9, &AttrPatch { bold: Some(false), ..Default::default() }).expect("unbold");
    assert_eq!(runs(&doc, P0), [("Hellorld!".into(), false)]);
}

#[test]
fn offsets_are_utf16_and_validated() {
    let mut doc = Document::new();
    doc.set_text(P0, "a😀b").expect("set");
    doc.apply_attrs(P0, 1, 3, &bold()).expect("bold emoji");
    assert_eq!(runs(&doc, P0), [("a".into(), false), ("😀".into(), true), ("b".into(), false)]);
    assert!(doc.apply_attrs(P0, 2, 3, &bold()).is_err(), "inside a surrogate pair");
    assert!(doc.replace_text(P0, 0, 99, "", None).is_err(), "past the end");
    assert!(doc.replace_text(P0, 3, 1, "", None).is_err(), "reversed");
    assert!(doc.set_text(ParaRef::top(9), "x").is_err(), "no such block");
    assert_eq!(doc.paragraph(P0).expect("p").text(), "a😀b", "failed edits change nothing");
    assert_eq!(doc.utf16_offset(P0, 5).expect("offset"), 3);
    assert!(doc.utf16_offset(P0, 2).is_err());
}

#[test]
fn split_merge_and_kinds() {
    let mut doc = Document::new();
    doc.set_text(P0, "Title here").expect("set");
    doc.set_kind(P0, ParaKind::Heading { level: 9 }).expect("kind");
    assert_eq!(doc.paragraph(P0).expect("p").kind, ParaKind::Heading { level: 6 }, "clamped");
    doc.apply_attrs(P0, 0, 5, &bold()).expect("bold");
    let next = doc.split_paragraph(P0, 5).expect("split");
    assert_eq!(next, ParaRef::top(1));
    assert_eq!(doc.paragraph(P0).expect("p").text(), "Title");
    assert_eq!(doc.paragraph(next).expect("p").text(), " here");
    assert_eq!(doc.paragraph(next).expect("p").kind, ParaKind::Normal, "heading is followed by body text");

    // Splitting at the end of a bold run gives an empty paragraph that types bold.
    let empty = doc.split_paragraph(P0, 5).expect("split at end");
    assert!(doc.paragraph(empty).expect("p").runs[0].attrs.bold);

    // List items continue as list items.
    assert_eq!(empty, ParaRef::top(1));
    let r2 = ParaRef::top(2);
    doc.set_kind(r2, ParaKind::Bullet { level: 0 }).expect("kind");
    doc.indent(r2, 1).expect("indent");
    assert_eq!(doc.paragraph(r2).expect("p").kind, ParaKind::Bullet { level: 1 });
    doc.indent(r2, -5).expect("outdent");
    assert_eq!(doc.paragraph(r2).expect("p").kind, ParaKind::Bullet { level: 0 });
    let r3 = doc.split_paragraph(r2, 2).expect("split");
    assert_eq!(doc.paragraph(r3).expect("p").kind, ParaKind::Bullet { level: 0 });

    let (joined, at) = doc.merge_with_previous(r3).expect("merge").expect("merged");
    assert_eq!((joined, at), (r2, 2));
    assert_eq!(doc.paragraph(r2).expect("p").text(), " here");
    assert_eq!(doc.merge_with_previous(P0).expect("merge"), None);

    doc.set_align(P0, Align::Center).expect("align");
    assert_eq!(doc.paragraph(P0).expect("p").align, Align::Center);
}

#[test]
fn undo_coalesces_typing_per_paragraph() {
    let mut doc = Document::new();
    for text in ["H", "He", "Hel", "Hell", "Hello"] {
        doc.set_text(P0, text).expect("type");
    }
    let p1 = doc.split_paragraph(P0, 5).expect("split");
    doc.set_text(p1, "x").expect("type");
    doc.set_text(p1, "xy").expect("type");
    assert!(doc.undo()); // "xy" typing
    assert_eq!(doc.paragraph(p1).expect("p").text(), "");
    assert!(doc.undo()); // split
    assert_eq!(doc.blocks().len(), 1);
    assert!(doc.undo()); // "Hello" typing
    assert_eq!(doc.paragraph(P0).expect("p").text(), "");
    assert!(!doc.undo());
    assert!(doc.redo());
    assert_eq!(doc.paragraph(P0).expect("p").text(), "Hello");
    // Breaking the group makes the next keystroke its own step.
    doc.break_undo_group();
    doc.set_text(P0, "Hello!").expect("type");
    assert!(doc.undo());
    assert_eq!(doc.paragraph(P0).expect("p").text(), "Hello");
    assert!(doc.is_modified());
    doc.mark_saved();
    assert!(!doc.is_modified());
}

#[test]
fn tables_and_blocks() {
    let mut doc = Document::new();
    doc.insert_block(0, Block::Table(Table::new(2, 3))).expect("table");
    let cell = ParaRef::in_cell(0, 1, 2, 0);
    doc.set_text(cell, "cell text").expect("set");
    let second = doc.split_paragraph(cell, 4).expect("split in cell");
    assert_eq!(second, ParaRef::in_cell(0, 1, 2, 1));
    assert_eq!(doc.paragraph(second).expect("p").text(), " text");
    assert_eq!(doc.merge_with_previous(second).expect("merge"), Some((cell, 4)));
    assert_eq!(doc.merge_with_previous(cell).expect("merge"), None, "cannot merge out of a cell");

    doc.resize_table(0, true, 1, 2).expect("add rows");
    doc.resize_table(0, false, 0, -1).expect("delete column");
    match &doc.blocks()[0] {
        Block::Table(t) => {
            assert_eq!((t.rows.len(), t.cols()), (4, 2));
            assert_eq!(t.rows[3][1][0].text(), "cell text");
        }
        other => panic!("{other:?}"),
    }
    assert!(doc.resize_table(0, true, 9, -1).is_err());
    assert!(doc.resize_table(1, true, 0, 1).is_err(), "not a table");
    doc.resize_table(0, false, 0, -2).expect("delete all columns");
    assert!(matches!(doc.blocks()[0], Block::Paragraph(_)), "empty table disappears");

    doc.insert_block(1, Block::PageBreak).expect("page break");
    assert!(matches!(doc.blocks().last(), Some(Block::Paragraph(_))), "always ends with a paragraph");
    let (_, at) = doc.merge_with_previous(ParaRef::top(2)).expect("merge").expect("removed break");
    assert_eq!(at, 0);
    assert!(!doc.blocks().iter().any(|b| matches!(b, Block::PageBreak)));
    doc.remove_block(0).expect("remove");
    assert!(doc.remove_block(5).is_err());
    assert_eq!(doc.blocks().len(), 1);
}

#[test]
fn find_and_replace() {
    let mut doc = Document::new();
    doc.set_text(P0, "The cat sat on the CAT mat").expect("set");
    doc.apply_attrs(P0, 4, 7, &bold()).expect("bold");
    doc.insert_block(1, Block::Table(Table::new(1, 1))).expect("table");
    doc.set_text(ParaRef::in_cell(1, 0, 0, 0), "a cat in a cell").expect("set");

    let hits = doc.find("cat", false, 100);
    assert_eq!(hits.len(), 3);
    assert_eq!((hits[0].start, hits[0].end), (4, 7));
    assert_eq!(hits[2].para, ParaRef::in_cell(1, 0, 0, 0));
    assert_eq!(doc.find("cat", true, 100).len(), 2);
    assert_eq!(doc.find("cat", false, 1).len(), 1);
    assert!(doc.find("", false, 100).is_empty());

    assert_eq!(doc.replace_all("cat", "dog", false).expect("replace"), 3);
    assert_eq!(doc.paragraph(P0).expect("p").text(), "The dog sat on the dog mat");
    assert_eq!(runs(&doc, P0)[1], ("dog".into(), true), "replacement keeps formatting");
    assert!(doc.undo());
    assert_eq!(doc.paragraph(P0).expect("p").text(), "The cat sat on the CAT mat");
    assert_eq!(doc.replace_all("zebra", "x", false).expect("replace"), 0);
}

#[test]
fn links_are_sanitised() {
    let mut doc = Document::new();
    doc.set_text(P0, "link").expect("set");
    let patch = |l: &str| AttrPatch { link: Some(Some(l.into())), ..Default::default() };
    assert!(doc.apply_attrs(P0, 0, 4, &patch("javascript:alert(1)")).is_err());
    assert!(doc.apply_attrs(P0, 0, 4, &patch("file:///etc/passwd")).is_err());
    doc.apply_attrs(P0, 0, 4, &patch("https://example.org")).expect("ok link");
    assert_eq!(doc.paragraph(P0).expect("p").runs[0].attrs.link.as_deref(), Some("https://example.org"));
}

#[test]
fn stats_and_plain_text() {
    let mut doc = Document::new();
    doc.set_text(P0, "Two words").expect("set");
    doc.insert_block(1, Block::Table(Table::new(1, 2))).expect("table");
    doc.set_text(ParaRef::in_cell(1, 0, 1, 0), "three more words").expect("set");
    let stats = doc.stats();
    assert_eq!((stats.words, stats.paragraphs, stats.tables), (5, 2, 1));
    assert_eq!(stats.chars, 25);
    assert_eq!(stats.chars_no_spaces, 22);
    assert_eq!(doc.plain_text(), "Two words\n\tthree more words\n");
}

fn rich_document() -> Document {
    let mut doc = Document::new();
    doc.meta.title = Some("Rich".into());
    doc.page = PageSetup { width: 612.0, height: 792.0, margin: 72.0 };
    let mut fancy = Paragraph::plain("");
    fancy.align = Align::Justify;
    fancy.runs = vec![
        Run { text: "  leading spaces,   inner\ttab ".into(), attrs: TextAttrs::default() },
        Run { text: "bold-italic".into(), attrs: TextAttrs { bold: true, italic: true, ..Default::default() } },
        Run { text: " under".into(), attrs: TextAttrs { underline: true, strike: true, ..Default::default() } },
        Run { text: "code".into(), attrs: TextAttrs { code: true, ..Default::default() } },
        Run { text: "2".into(), attrs: TextAttrs { superscript: true, ..Default::default() } },
        Run { text: "i".into(), attrs: TextAttrs { subscript: true, ..Default::default() } },
        Run {
            text: "red big".into(),
            attrs: TextAttrs { color: Some(Rgb(255, 0, 0)), highlight: Some(Rgb(255, 255, 0)), size: Some(18.0), ..Default::default() },
        },
        Run {
            text: "a link & <stuff>".into(),
            attrs: TextAttrs { link: Some("https://example.org/a?b=1&c=2".into()), ..Default::default() },
        },
        Run { text: "line\nbreak ".into(), attrs: TextAttrs::default() },
    ];
    let mut blocks = vec![
        Block::Paragraph(Paragraph::with_kind(ParaKind::Title, "The Title")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Heading { level: 1 }, "Chapter")),
        Block::Paragraph(fancy),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Bullet { level: 0 }, "bullet a")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Bullet { level: 1 }, "bullet a.1")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 2 }, "numbered deep")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Bullet { level: 0 }, "bullet b")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 0 }, "one")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Numbered { level: 0 }, "two")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Quote, "A quotation")),
        Block::Paragraph(Paragraph::with_kind(ParaKind::Code, "fn main() {    }")),
        Block::PageBreak,
        Block::Paragraph(Paragraph::with_kind(ParaKind::Heading { level: 3 }, "After the break")),
    ];
    let mut table = Table::new(2, 2);
    table.rows[0][0] = vec![Paragraph::plain("r0c0")];
    table.rows[1][1] = vec![Paragraph::plain("multi"), Paragraph::with_kind(ParaKind::Bullet { level: 0 }, "item")];
    blocks.push(Block::Table(table));
    blocks.push(Block::Paragraph(Paragraph::plain("")));
    for block in blocks.into_iter().rev() {
        doc.insert_block(0, block).expect("insert");
    }
    doc.remove_block(doc.blocks().len() - 1).expect("drop the initial empty paragraph");
    doc
}

#[test]
fn odt_round_trip_preserves_the_model() {
    let doc = rich_document();
    let bytes = save_odt(&doc).expect("save");
    let back = load_odt(&bytes, &Limits::default()).expect("load");
    for (i, (a, b)) in doc.blocks().iter().zip(back.blocks()).enumerate() {
        assert_eq!(a, b, "block {i}");
    }
    assert_eq!(back.blocks().len(), doc.blocks().len());
    assert_eq!(back.meta.title.as_deref(), Some("Rich"));
    assert!((back.page.width - 612.0).abs() < 0.1 && (back.page.margin - 72.0).abs() < 0.1);
    assert_eq!(save_odt(&back).expect("save again"), bytes, "stable output");
    assert!(!back.is_modified() && !back.can_undo());
    assert_eq!(load(&bytes, &Limits::default()).expect("generic").blocks(), doc.blocks());
}

const NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
    xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
    xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
    xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
    xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
    xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
    xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
    xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\"";

fn odt_with(auto_styles: &str, body: &str, styles_xml: Option<&str>) -> Vec<u8> {
    let content = format!(
        "<office:document-content {NS}><office:automatic-styles>{auto_styles}</office:automatic-styles>\
         <office:body><office:text>{body}</office:text></office:body></office:document-content>"
    );
    let mut package = Package::new(ns::MIME_ODT);
    package.insert("content.xml", content.into_bytes()).expect("insert");
    if let Some(styles) = styles_xml {
        package.insert("styles.xml", format!("<office:document-styles {NS}>{styles}</office:document-styles>").into_bytes()).expect("insert");
    }
    package.write().expect("write")
}

#[test]
fn reads_foreign_odt_constructs() {
    let styles = r#"<office:font-face-decls><style:font-face style:name="Mono1" svg:font-family="'DejaVu Sans Mono'"/></office:font-face-decls>
      <office:styles>
        <style:style style:name="Heading_20_2" style:family="paragraph"/>
        <style:style style:name="MyHeading" style:family="paragraph" style:parent-style-name="Heading_20_2"/>
        <style:style style:name="Emphasis" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
        <text:list-style style:name="Numbering_20_123"><text:list-level-style-number text:level="1"/><text:list-level-style-bullet text:level="2"/></text:list-style>
      </office:styles>"#;
    let auto = r#"<style:style style:name="P1" style:family="paragraph" style:parent-style-name="MyHeading"><style:paragraph-properties fo:text-align="center"/></style:style>
      <style:style style:name="P2" style:family="paragraph"><style:paragraph-properties fo:break-before="page"/></style:style>
      <style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="700" style:text-position="33% 58%"/></style:style>
      <style:style style:name="T2" style:family="text"><style:text-properties style:font-name="Mono1"/></style:style>
      <style:style style:name="Loop1" style:family="text" style:parent-style-name="Loop2"/>
      <style:style style:name="Loop2" style:family="text" style:parent-style-name="Loop1"/>"#;
    let body = r#"
      <text:sequence-decls/>
      <text:p text:style-name="P1">Centered heading</text:p>
      <text:section><text:p>In a <text:span text:style-name="Emphasis">section</text:span><text:span text:style-name="Loop1">!</text:span></text:p></text:section>
      <text:p>raised<text:span text:style-name="T1">x</text:span> mono<text:span text:style-name="T2">code</text:span><text:note><text:note-body><text:p>footnote</text:p></text:note-body></text:note><office:annotation><text:p>comment</text:p></office:annotation></text:p>
      <text:p>bad <text:a xlink:href="javascript:evil()">link</text:a> and <text:a xlink:href="mailto:a@b.c">mail</text:a><text:s text:c="999999"/>.</text:p>
      <text:list text:style-name="Numbering_20_123">
        <text:list-header><text:p>header</text:p></text:list-header>
        <text:list-item><text:p>first</text:p><text:list><text:list-item><text:p>nested bullet</text:p></text:list-item></text:list></text:list-item>
      </text:list>
      <text:p text:style-name="P2">new page<draw:frame><draw:text-box><text:p>in frame</text:p></draw:text-box></draw:frame></text:p>
      <table:table><table:table-column table:number-columns-repeated="2"/>
        <table:table-header-rows><table:table-row><table:table-cell table:number-columns-repeated="2"><text:p>h</text:p></table:table-cell></table:table-row></table:table-header-rows>
        <table:table-row><table:table-cell><table:table><table:table-row><table:table-cell><text:p>nested</text:p></table:table-cell></table:table-row></table:table></table:table-cell><table:covered-table-cell/></table:table-row>
      </table:table>
      <text:h text:outline-level="12">deep heading</text:h>"#;
    let doc = load_odt(&odt_with(auto, body, Some(styles)), &Limits::default()).expect("load");
    let b = doc.blocks();
    let para = |i: usize| match &b[i] {
        Block::Paragraph(p) => p.clone(),
        other => panic!("block {i}: {other:?}"),
    };
    assert_eq!(para(0).kind, ParaKind::Heading { level: 2 }, "inherited heading style");
    assert_eq!(para(0).align, Align::Center);
    assert_eq!(para(1).text(), "In a section!");
    assert!(para(1).runs[1].attrs.italic);
    let p2 = para(2);
    assert_eq!(p2.text(), "raisedx monocode", "notes and annotations are not inline text");
    assert!(p2.runs[1].attrs.bold && p2.runs[1].attrs.superscript);
    assert!(p2.runs.iter().any(|r| r.text == "code" && r.attrs.code));
    let p3 = para(3);
    assert!(p3.runs.iter().all(|r| r.attrs.link.as_deref() != Some("javascript:evil()")));
    assert!(p3.runs.iter().any(|r| r.attrs.link.as_deref() == Some("mailto:a@b.c")));
    assert!(p3.text().len() < 1100, "space runs are capped");
    assert_eq!(para(4).kind, ParaKind::Normal, "list header is not an item");
    assert_eq!(para(5).kind, ParaKind::Numbered { level: 0 });
    assert_eq!(para(6).kind, ParaKind::Bullet { level: 1 });
    assert!(matches!(b[7], Block::PageBreak));
    assert_eq!(para(8).text(), "new page", "frame content is skipped");
    match &b[9] {
        Block::Table(t) => {
            assert_eq!(t.rows.len(), 2);
            assert_eq!(t.cols(), 2);
            assert_eq!(t.rows[0][1][0].text(), "h");
            assert_eq!(t.rows[1][0][0].text(), "nested", "nested tables are flattened");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(para(10).kind, ParaKind::Heading { level: 6 });
}

#[test]
fn hostile_odt_is_rejected_or_bounded() {
    let many = "<text:p>x</text:p>".repeat(100);
    let limits = Limits { max_blocks: 50, ..Limits::default() };
    assert!(matches!(load_odt(&odt_with("", &many, None), &limits), Err(OdfError::Limit(_))));
    let rows = "<table:table-row table:number-rows-repeated=\"1000000\"><table:table-cell table:number-columns-repeated=\"100000\"/></table:table-row>";
    let doc = load_odt(&odt_with("", &format!("<table:table>{rows}</table:table>"), None), &Limits::default()).expect("load");
    match &doc.blocks()[0] {
        Block::Table(t) => assert!(t.rows.len() <= 100 && t.cols() <= 64),
        other => panic!("{other:?}"),
    }
    let deep = format!("{}x{}", "<text:span>".repeat(200), "</text:span>".repeat(200));
    assert!(load_odt(&odt_with("", &format!("<text:p>{deep}</text:p>"), None), &Limits::default()).is_ok());
    let mut ods = Package::new(ns::MIME_ODS);
    ods.insert("content.xml", b"<a/>".to_vec()).expect("insert");
    assert!(load_odt(&ods.write().expect("write"), &Limits::default()).is_err());
    let mut empty = Package::new(ns::MIME_ODT);
    empty.insert("content.xml", b"<a/>".to_vec()).expect("insert");
    assert!(load_odt(&empty.write().expect("write"), &Limits::default()).is_err());
}

fn para_strategy() -> impl Strategy<Value = Paragraph> {
    let run = (
        "[a-zA-Z0-9 ,.!?<>&\"'\t\n😀é]{0,12}",
        any::<(bool, bool, bool, bool, bool)>(),
        prop::option::of((any::<u8>(), any::<u8>(), any::<u8>())),
        prop::option::of(prop::sample::select(vec![8.0f32, 12.0, 18.5])),
        prop::option::of(prop::sample::select(vec!["https://a.example/x?y=1&z", "mailto:x@y.z"])),
    )
        .prop_map(|(text, (b, i, u, s, c), color, size, link)| Run {
            text,
            attrs: TextAttrs {
                bold: b,
                italic: i,
                underline: u,
                strike: s,
                code: c,
                color: color.map(|(r, g, b)| Rgb(r, g, b)),
                size,
                link: link.map(str::to_owned),
                ..Default::default()
            },
        });
    let kind = prop_oneof![
        Just(ParaKind::Normal),
        (1u8..=6).prop_map(|level| ParaKind::Heading { level }),
        (0u8..3).prop_map(|level| ParaKind::Bullet { level }),
        (0u8..3).prop_map(|level| ParaKind::Numbered { level }),
        Just(ParaKind::Quote),
        Just(ParaKind::Title),
    ];
    let align = prop::sample::select(vec![Align::Left, Align::Center, Align::Right, Align::Justify]);
    (kind, align, prop::collection::vec(run, 0..5)).prop_map(|(kind, align, runs)| {
        let mut p = Paragraph { kind, align, runs };
        p.normalize();
        p
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn odt_round_trip_is_lossless_for_the_model(paras in prop::collection::vec(para_strategy(), 1..12)) {
        // A list must start at level 0 to be representable in ODF.
        let mut blocks = Vec::new();
        let mut prev_list_level: Option<u8> = None;
        for mut p in paras {
            let level = match p.kind { ParaKind::Bullet { level } | ParaKind::Numbered { level } => Some(level), _ => None };
            if let Some(level) = level {
                let max = prev_list_level.map(|l| l + 1).unwrap_or(0);
                if level > max {
                    p.kind = match p.kind { ParaKind::Bullet { .. } => ParaKind::Bullet { level: max }, _ => ParaKind::Numbered { level: max } };
                }
                prev_list_level = Some(level.min(max));
            } else {
                prev_list_level = None;
            }
            blocks.push(Block::Paragraph(p));
        }
        // The formatting of an empty paragraph (what new text would get) is
        // an editing convenience that ODF cannot store.
        for block in &mut blocks {
            if let Block::Paragraph(p) = block {
                if p.text().is_empty() {
                    p.runs = vec![Run::default()];
                }
            }
        }
        let doc = Document::from_blocks(blocks);
        let back = load_odt(&save_odt(&doc).unwrap(), &Limits::default()).unwrap();
        prop_assert_eq!(back.blocks(), doc.blocks());
    }

    #[test]
    fn set_text_keeps_the_text_and_formatting_consistent(
        initial in "[a-z😀 ]{0,20}", edits in prop::collection::vec("[a-z😀 ]{0,20}", 1..8)
    ) {
        let mut doc = Document::new();
        doc.set_text(P0, &initial).unwrap();
        let len = initial.encode_utf16().count();
        if len > 0 {
            let half = initial.chars().take(initial.chars().count() / 2).map(char::len_utf16).sum::<usize>();
            doc.apply_attrs(P0, 0, half, &bold()).unwrap();
        }
        for text in &edits {
            doc.set_text(P0, text).unwrap();
            let p = doc.paragraph(P0).unwrap();
            prop_assert_eq!(&p.text(), text);
            prop_assert!(p.runs.len() == 1 || p.runs.iter().all(|r| !r.text.is_empty()));
        }
        while doc.undo() {}
        prop_assert_eq!(doc.paragraph(P0).unwrap().text(), "");
    }

    #[test]
    fn loaders_are_total(bytes in prop::collection::vec(any::<u8>(), 0..500)) {
        let _ = load(&bytes, &Limits::default());
        let mut zipish = b"PK\x03\x04".to_vec();
        zipish.extend_from_slice(&bytes);
        let _ = load(&zipish, &Limits::default());
    }

    #[test]
    fn html_and_markdown_are_total(paras in prop::collection::vec(para_strategy(), 0..8)) {
        let doc = Document::from_blocks(paras.into_iter().map(Block::Paragraph).collect());
        let html = export::to_html(&doc);
        prop_assert!(!html.contains("<script"));
        let _ = export::to_markdown(&doc);
    }
}
