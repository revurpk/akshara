//! Import of Microsoft Excel (.xlsx) files.

use office_calc::{load_xlsx, save_ods, load_ods, CellAddr, HAlign, NumberFormat, Value, Workbook};
use office_odf::{Limits, Package, Rgb};

const X: &str = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
    xmlns:x14ac=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac\" mc:Ignorable=\"x14ac\"";

fn a(s: &str) -> CellAddr {
    CellAddr::parse(s).expect("address")
}

fn xlsx(workbook_pr: &str, sheet1: &str) -> Vec<u8> {
    let mut pkg = Package::new("");
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
        </Relationships>";
    pkg.insert("_rels/.rels", rels.as_bytes().to_vec()).expect("rels");
    let book_rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
        <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"/xl/worksheets/sheet2.xml\"/>\
        <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings\" Target=\"sharedStrings.xml\"/>\
        <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
        </Relationships>";
    pkg.insert("xl/_rels/workbook.xml.rels", book_rels.as_bytes().to_vec()).expect("book rels");
    let book = format!(
        "<workbook {X}>{workbook_pr}<sheets><sheet name=\"Sales\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Notes &amp; more\" sheetId=\"2\" r:id=\"rId2\"/></sheets>\
         <definedNames><definedName name=\"_xlnm.Print_Area\" localSheetId=\"0\">Sales!$A$1:$E$20</definedName>\
         <definedName name=\"_xlnm.Print_Titles\" localSheetId=\"0\">Sales!$1:$1</definedName>\
         <definedName name=\"TaxRate\">Sales!$F$1</definedName></definedNames></workbook>"
    );
    pkg.insert("xl/workbook.xml", book.into_bytes()).expect("book");
    let sst = format!(
        "<sst {X} count=\"4\" uniqueCount=\"4\"><si><t>Item</t></si><si><t>Qty</t></si>\
         <si><r><rPr><b/></rPr><t>Rich </t></r><r><t>text</t></r><rPh><t>ignored</t></rPh></si><si><t xml:space=\"preserve\">బియ్యం </t></si></sst>"
    );
    pkg.insert("xl/sharedStrings.xml", sst.into_bytes()).expect("sst");
    let styles = format!(
        "<styleSheet {X}><numFmts count=\"2\"><numFmt numFmtId=\"164\" formatCode=\"dd/mm/yyyy\"/><numFmt numFmtId=\"165\" formatCode=\"[$₹-4009]\\ #,##,##0.00\"/></numFmts>\
         <fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><i/><u/><color rgb=\"FFC00000\"/><sz val=\"11\"/></font></fonts>\
         <fills count=\"3\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>\
           <fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFDDEBF7\"/><bgColor indexed=\"64\"/></patternFill></fill></fills>\
         <cellXfs count=\"5\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\"/>\
           <xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" applyFont=\"1\" applyFill=\"1\"><alignment horizontal=\"center\"/></xf>\
           <xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" applyNumberFormat=\"1\"/>\
           <xf numFmtId=\"165\" fontId=\"0\" fillId=\"0\" applyNumberFormat=\"1\"/>\
           <xf numFmtId=\"10\" fontId=\"0\" fillId=\"0\" applyNumberFormat=\"1\"/></cellXfs></styleSheet>"
    );
    pkg.insert("xl/styles.xml", styles.into_bytes()).expect("styles");
    pkg.insert("xl/worksheets/sheet1.xml", format!("<worksheet {X}>{sheet1}</worksheet>").into_bytes()).expect("sheet1");
    let sheet2 = format!("<worksheet {X}><sheetData><row r=\"2\"><c r=\"B2\" t=\"inlineStr\"><is><t>inline</t></is></c><c t=\"b\"><v>1</v></c></row></sheetData></worksheet>");
    pkg.insert("xl/worksheets/sheet2.xml", sheet2.into_bytes()).expect("sheet2");
    let core = "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
        xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Shop</dc:title></cp:coreProperties>";
    pkg.insert("docProps/core.xml", core.as_bytes().to_vec()).expect("core");
    pkg.write().expect("zip")
}

const SHEET1: &str = "\
    <sheetPr><pageSetUpPr fitToPage=\"1\"/></sheetPr>\
    <cols><col min=\"1\" max=\"1\" width=\"20.7109375\" customWidth=\"1\"/><col min=\"2\" max=\"3\" width=\"9.140625\"/></cols>\
    <sheetData>\
      <row r=\"1\"><c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c><c r=\"B1\" s=\"1\" t=\"s\"><v>1</v></c><c r=\"F1\" s=\"4\"><v>0.18</v></c></row>\
      <row r=\"2\"><c r=\"A2\" t=\"s\"><v>3</v></c><c r=\"B2\"><v>2</v></c><c r=\"C2\" s=\"3\"><v>55.5</v></c>\
         <c r=\"D2\" s=\"3\"><f t=\"shared\" ref=\"D2:D3\" si=\"0\">B2*C2</f><v>111</v></c><c r=\"E2\" s=\"2\"><v>46249</v></c></row>\
      <row r=\"3\"><c r=\"A3\" t=\"s\"><v>2</v></c><c r=\"B3\"><v>1</v></c><c r=\"C3\" s=\"3\"><v>120</v></c>\
         <c r=\"D3\" s=\"3\"><f t=\"shared\" si=\"0\"/><v>120</v></c><c r=\"E3\" t=\"d\"><v>2026-08-15T12:00:00</v></c></row>\
      <row r=\"4\"><c r=\"A4\" t=\"str\"><f>\"Total \"&amp;COUNT(B2:B3)</f><v>Total 2</v></c>\
         <c r=\"D4\"><f>_xlfn.SUM(D2:D3)</f><v>231</v></c><c r=\"E4\"><f>SUM(Table1[Qty])</f><v>3</v></c>\
         <c r=\"F4\" t=\"e\"><f>1/0</f><v>#DIV/0!</v></c><c r=\"G4\" t=\"b\"><v>0</v></c></row>\
    </sheetData>\
    <printOptions gridLines=\"1\"/>\
    <pageMargins left=\"0.5\" right=\"0.5\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/>\
    <pageSetup paperSize=\"9\" orientation=\"landscape\" fitToHeight=\"0\"/>\
    <headerFooter><oddFooter>Page &amp;P</oddFooter></headerFooter>";

#[test]
fn excel_workbooks_are_imported() {
    let wb = load_xlsx(&xlsx("", SHEET1), &Limits::default()).expect("load");
    assert_eq!(wb.sheet_count(), 2);
    assert_eq!(wb.sheet(0).expect("sheet").name, "Sales");
    assert_eq!(wb.sheet(1).expect("sheet").name, "Notes & more");
    let d = |s: &str| wb.display(0, a(s));
    assert_eq!((d("A1"), d("B1"), d("A2"), d("A3")), ("Item".into(), "Qty".into(), "బియ్యం ".into(), "Rich text".into()));
    // Formulas are live: the shared formula is translated to row 3.
    assert_eq!(wb.value(0, a("D2")), &Value::Number(111.0));
    assert_eq!(wb.value(0, a("D3")), &Value::Number(120.0));
    assert_eq!(wb.input_text(0, a("D3")), "=B3*C3");
    assert_eq!(wb.value(0, a("D4")), &Value::Number(231.0), "_xlfn. prefix dropped");
    assert_eq!(d("A4"), "Total 2");
    // A formula Akshara cannot parse keeps its last value as a constant.
    assert_eq!(wb.value(0, a("E4")), &Value::Number(3.0));
    assert!(!wb.input_text(0, a("E4")).starts_with('='));
    assert_eq!(d("F4"), "#DIV/0!");
    assert_eq!(wb.value(0, a("G4")), &Value::Bool(false));
    // Styles and number formats.
    let header = wb.style(0, a("A1"));
    assert!(header.bold && header.italic && header.underline);
    assert_eq!((header.color, header.background, header.align), (Some(Rgb(0xC0, 0, 0)), Some(Rgb(0xDD, 0xEB, 0xF7)), HAlign::Center));
    assert_eq!(wb.style(0, a("E2")).format, NumberFormat::DateDmy);
    assert_eq!(d("E2"), "15-08-2026");
    assert_eq!(wb.style(0, a("C2")).format, NumberFormat::Currency { decimals: 2, symbol: "₹".into() });
    assert_eq!(d("F1"), "18.00%");
    assert!(matches!(wb.value(0, a("E3")), Value::Number(n) if (n.fract() - 0.5).abs() < 1e-9), "ISO date with time");
    // Column widths: only custom ones.
    let widths = &wb.sheet(0).expect("sheet").col_widths;
    assert!((widths[&0] - 112.5).abs() < 1.0, "{widths:?}");
    assert!(!widths.contains_key(&1));
    // Print settings and page setup.
    let area = *wb.sheet(0).expect("sheet").print_area();
    assert_eq!(area.range.map(|r| r.to_a1()), Some("A1:E20".into()));
    assert_eq!(area.repeat_rows, Some((0, 0)));
    let page = wb.page();
    assert!(page.width > page.height && page.fit_width && page.grid && page.header_footer);
    assert!((page.margin - 36.0).abs() < 0.1);
    // Second sheet: inline string, cell without an address.
    assert_eq!(wb.display(1, a("B2")), "inline");
    assert_eq!(wb.value(1, a("C2")), &Value::Bool(true));
    assert_eq!(wb.meta.title.as_deref(), Some("Shop"));
    assert!(!wb.is_modified() && !wb.can_undo());
    // Saved as ODS and read back, the data stays.
    let back = load_ods(&save_ods(&wb).expect("save"), &Limits::default()).expect("reload");
    assert_eq!(back.display(0, a("D3")), wb.display(0, a("D3")));
    assert_eq!(back.input_text(0, a("D4")), wb.input_text(0, a("D4")));
}

#[test]
fn the_1904_date_system_is_converted() {
    let sheet = "<sheetData><row r=\"1\"><c r=\"A1\" s=\"2\"><v>0</v></c></row></sheetData>";
    let wb: Workbook = load_xlsx(&xlsx("<workbookPr date1904=\"1\"/>", sheet), &Limits::default()).expect("load");
    assert_eq!(wb.display(0, a("A1")), "01-01-1904");
}

#[test]
fn broken_and_foreign_files_are_errors() {
    assert!(load_xlsx(b"garbage", &Limits::default()).is_err());
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.resize(512, 0);
    assert!(load_xlsx(&ole, &Limits::default()).expect_err("xls").to_string().contains(".xls"));
    let many: String = (1..=50).map(|r| format!("<row r=\"{r}\"><c r=\"A{r}\"><v>{r}</v></c></row>")).collect();
    let limits = Limits { max_cells: 10, ..Limits::default() };
    assert!(load_xlsx(&xlsx("", &format!("<sheetData>{many}</sheetData>")), &limits).is_err());
}
