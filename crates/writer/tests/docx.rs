//! Import of Microsoft Word (.docx) files.

use office_odf::{Limits, Package, Rgb};
use office_writer::*;

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
    xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" mc:Ignorable=\"w14\"";

fn docx(document_body: &str) -> Vec<u8> {
    let mut pkg = Package::new("");
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
        <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
        </Relationships>";
    pkg.insert("_rels/.rels", rels.as_bytes().to_vec()).expect("rels");
    let doc_rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
        <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>\
        <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"https://example.org/\" TargetMode=\"External\"/>\
        <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"javascript:alert(1)\" TargetMode=\"External\"/>\
        <Relationship Id=\"rId5\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>\
        <Relationship Id=\"rId6\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/>\
        </Relationships>";
    pkg.insert("word/_rels/document.xml.rels", doc_rels.as_bytes().to_vec()).expect("doc rels");
    let styles = format!(
        "<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:rPr><w:sz w:val=\"22\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Red\"><w:name w:val=\"Red text\"/><w:basedOn w:val=\"Normal\"/><w:rPr><w:color w:val=\"C00000\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"ListBullet\"><w:name w:val=\"List Bullet\"/><w:pPr><w:numPr><w:numId w:val=\"1\"/></w:numPr></w:pPr></w:style>\
         <w:style w:type=\"character\" w:styleId=\"Strong\"><w:name w:val=\"Strong\"/><w:rPr><w:b/></w:rPr></w:style>\
         </w:styles>"
    );
    pkg.insert("word/styles.xml", styles.into_bytes()).expect("styles");
    let numbering = format!(
        "<w:numbering {W}>\
         <w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl><w:lvl w:ilvl=\"1\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
         <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
         <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num>\
         </w:numbering>"
    );
    pkg.insert("word/numbering.xml", numbering.into_bytes()).expect("numbering");
    let header = format!(
        "<w:hdr {W}><w:p><w:pPr><w:jc w:val=\"right\"/></w:pPr><w:r><w:t xml:space=\"preserve\">Page </w:t></w:r>\
         <w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r>\
         <w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>2</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>\
         <w:r><w:t xml:space=\"preserve\"> of </w:t></w:r><w:fldSimple w:instr=\" NUMPAGES \"><w:r><w:t>5</w:t></w:r></w:fldSimple></w:p></w:hdr>"
    );
    pkg.insert("word/header1.xml", header.into_bytes()).expect("header");
    pkg.insert("word/footer1.xml", format!("<w:ftr {W}><w:p/></w:ftr>").into_bytes()).expect("footer");
    let document = format!("<w:document {W}><w:body>{document_body}</w:body></w:document>");
    pkg.insert("word/document.xml", document.into_bytes()).expect("document");
    let core = "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
        xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Quarterly report</dc:title><dc:creator>A. Writer</dc:creator></cp:coreProperties>";
    pkg.insert("docProps/core.xml", core.as_bytes().to_vec()).expect("core");
    pkg.write().expect("zip")
}

fn paragraphs(doc: &Document) -> Vec<&Paragraph> {
    doc.blocks()
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect()
}

const BODY: &str = "\
    <w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr><w:r><w:t>The Title</w:t></w:r></w:p>\
    <w:p w14:paraId=\"1A2B3C4D\"><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Chapter</w:t></w:r></w:p>\
    <w:p><w:pPr><w:jc w:val=\"both\"/><w:ind w:left=\"720\"/><w:spacing w:before=\"240\"/></w:pPr>\
      <w:r><w:t xml:space=\"preserve\">Plain </w:t></w:r>\
      <w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>bold-italic</w:t></w:r>\
      <w:r><w:rPr><w:rStyle w:val=\"Strong\"/></w:rPr><w:t xml:space=\"preserve\"> strong</w:t></w:r>\
      <w:r><w:rPr><w:u w:val=\"single\"/><w:strike/></w:rPr><w:t xml:space=\"preserve\"> under</w:t></w:r>\
      <w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:t>2</w:t></w:r>\
      <w:r><w:rPr><w:vertAlign w:val=\"subscript\"/></w:rPr><w:t>i</w:t></w:r>\
      <w:r><w:rPr><w:color w:val=\"FF0000\"/><w:highlight w:val=\"yellow\"/><w:sz w:val=\"36\"/></w:rPr><w:t>red</w:t></w:r>\
      <w:r><w:rPr><w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/></w:rPr><w:t>code</w:t></w:r>\
      <w:r><w:tab/><w:t>tab</w:t><w:br/><w:t>line</w:t></w:r>\
      <w:hyperlink r:id=\"rId3\"><w:r><w:t>link</w:t></w:r></w:hyperlink>\
      <w:hyperlink r:id=\"rId4\"><w:r><w:t>evil</w:t></w:r></w:hyperlink>\
      <w:ins w:id=\"1\" w:author=\"x\"><w:r><w:t>+ins</w:t></w:r></w:ins>\
      <w:del w:id=\"2\" w:author=\"x\"><w:r><w:delText>-del</w:delText></w:r></w:del>\
    </w:p>\
    <w:p><w:pPr><w:pStyle w:val=\"Red\"/></w:pPr><w:r><w:t>styled red</w:t></w:r></w:p>\
    <w:p><w:pPr><w:pStyle w:val=\"ListBullet\"/></w:pPr><w:r><w:t>bullet a</w:t></w:r></w:p>\
    <w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>bullet a.1</w:t></w:r></w:p>\
    <w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"2\"/></w:numPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p>\
    <w:p><w:pPr><w:pStyle w:val=\"Quote\"/></w:pPr><w:r><w:t>A quotation</w:t></w:r></w:p>\
    <w:p><w:r><w:t>before</w:t></w:r><w:r><w:br w:type=\"page\"/></w:r><w:r><w:t>after</w:t></w:r></w:p>\
    <w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"2000\"/><w:gridCol w:w=\"4000\"/></w:tblGrid>\
      <w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc>\
            <w:tc><w:tcPr><w:vMerge w:val=\"restart\"/></w:tcPr><w:p><w:r><w:t>tall</w:t></w:r></w:p></w:tc></w:tr>\
      <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc>\
            <w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>\
    </w:tbl>\
    <w:sdt><w:sdtContent><w:p><w:r><w:t>in a content control</w:t></w:r></w:p></w:sdtContent></w:sdt>\
    <w:p><w:r><w:t>anchor</w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wps:txbx xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">\
      <w:txbxContent><w:p><w:r><w:t>boxed text</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></mc:Choice>\
      <mc:Fallback><w:pict><v:shape xmlns:v=\"urn:schemas-microsoft-com:vml\"><v:textbox><w:txbxContent><w:p><w:r><w:t>boxed text</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback>\
      </mc:AlternateContent></w:r></w:p>\
    <w:sectPr><w:headerReference w:type=\"default\" r:id=\"rId5\"/><w:footerReference w:type=\"default\" r:id=\"rId6\"/>\
      <w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr>";

#[test]
fn word_documents_are_imported() {
    let doc = load_docx(&docx(BODY), &Limits::default()).expect("load");
    let paras = paragraphs(&doc);
    let texts: Vec<String> = paras.iter().map(|p| p.text()).collect();
    assert_eq!(texts[0], "The Title");
    assert_eq!(paras[0].kind, ParaKind::Title);
    assert_eq!(paras[1].kind, ParaKind::Heading { level: 1 });
    assert!(!paras[1].runs[0].attrs.bold, "heading look comes from the kind, not from runs");

    let rich = paras[2];
    assert_eq!(rich.align, Align::Justify);
    assert_eq!((rich.indent, rich.space_before), (36.0, 12.0));
    assert_eq!(rich.text(), "Plain bold-italic strong under2iredcode\ttab\nlinelinkevil+ins");
    let run = |t: &str| rich.runs.iter().find(|r| r.text == t).unwrap_or_else(|| panic!("run {t:?}: {:?}", rich.runs)).attrs.clone();
    assert!(run("bold-italic").bold && run("bold-italic").italic);
    assert!(run(" strong").bold, "character style");
    assert!(run(" under").underline && run(" under").strike);
    assert!(run("2").superscript && run("i").subscript);
    let red = run("red");
    assert_eq!((red.color, red.highlight, red.size), (Some(Rgb(255, 0, 0)), Some(Rgb(255, 255, 0)), Some(18.0)));
    assert!(run("code").code);
    assert_eq!(run("link").link.as_deref(), Some("https://example.org/"));
    assert!(rich.runs.iter().all(|r| r.attrs.link.as_deref().is_none_or(|l| l.starts_with("https://"))), "unsafe links are dropped");
    assert!(!rich.text().contains("-del"), "tracked deletions are not text");

    assert_eq!(paras[3].runs[0].attrs.color, Some(Rgb(0xC0, 0, 0)), "paragraph style colour");
    assert_eq!(paras[3].runs[0].attrs.size, None, "default size is not repeated on runs");
    assert_eq!(paras[4].kind, ParaKind::Bullet { level: 0 });
    assert_eq!(paras[5].kind, ParaKind::Bullet { level: 1 });
    assert_eq!(paras[6].kind, ParaKind::Numbered { level: 0 });
    assert_eq!(paras[7].kind, ParaKind::Quote);

    // The page break splits the paragraph.
    let before = doc.blocks().iter().position(|b| matches!(b, Block::Paragraph(p) if p.text() == "before")).expect("before");
    assert!(matches!(doc.blocks()[before + 1], Block::PageBreak));
    assert!(matches!(&doc.blocks()[before + 2], Block::Paragraph(p) if p.text() == "after"));

    let table = doc.blocks().iter().find_map(|b| match b {
        Block::Table(t) => Some(t),
        _ => None,
    });
    let table = table.expect("table");
    assert_eq!(table.cols(), 3);
    assert_eq!(table.rows[0][0][0].text(), "wide");
    assert_eq!(table.rows[0][1][0].text(), "", "spanned cell");
    assert_eq!(table.rows[0][2][0].text(), "tall");
    assert_eq!(table.rows[1][2][0].text(), "", "merged continuation");
    assert_eq!(table.col_widths, vec![100.0, 100.0, 200.0]);
    assert!(table.borders);
    assert!(texts.iter().any(|t| t == "in a content control"));
    let anchor = texts.iter().position(|t| t == "anchor").expect("anchor");
    assert_eq!(texts[anchor + 1], "boxed text", "text box after its anchor");
    assert_eq!(texts.iter().filter(|t| *t == "boxed text").count(), 1, "fallback copy skipped");

    // Page, header with fields, empty footer, metadata.
    assert!((doc.page().width - 612.0).abs() < 0.1 && (doc.page().margin - 72.0).abs() < 0.1);
    let header = &doc.header()[0];
    assert_eq!(header.text(), "Page 2 of 5");
    assert_eq!(header.align, Align::Right);
    let fields: Vec<_> = header.runs.iter().filter_map(|r| r.attrs.field).collect();
    assert_eq!(fields, [Field::PageNumber, Field::PageCount]);
    assert!(doc.footer().is_empty());
    assert_eq!(doc.meta.title.as_deref(), Some("Quarterly report"));
    assert!(!doc.is_modified() && !doc.can_undo());

    // And it can be saved as ODT and read back.
    let back = load_odt(&save_odt(&doc).expect("save"), &Limits::default()).expect("reload");
    assert_eq!(back.blocks(), doc.blocks());
}

#[test]
fn broken_and_foreign_files_are_errors() {
    assert!(load_docx(b"not a zip", &Limits::default()).is_err());
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.resize(512, 0);
    let err = load_docx(&ole, &Limits::default()).expect_err("old .doc");
    assert!(err.to_string().contains(".doc"));
    // A zip without a Word document.
    let mut pkg = Package::new("");
    pkg.insert("hello.txt", b"hi".to_vec()).expect("insert");
    assert!(load_docx(&pkg.write().expect("zip"), &Limits::default()).is_err());
    // Too many paragraphs.
    let many: String = (0..50).map(|i| format!("<w:p><w:r><w:t>{i}</w:t></w:r></w:p>")).collect();
    let limits = Limits { max_blocks: 10, ..Limits::default() };
    assert!(load_docx(&docx(&many), &limits).is_err());
}

#[test]
fn empty_body_gives_an_empty_document() {
    let doc = load_docx(&docx(""), &Limits::default()).expect("load");
    assert_eq!(doc.blocks().len(), 1);
}
