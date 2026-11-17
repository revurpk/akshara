//! File format tests: ODS and CSV round trips, hostile input, and property
//! tests of the formula language.

use office_calc::formula::{parse, Expr, Syntax};
use office_calc::{
    load, load_csv, load_ods, save_ods, sheet_to_csv, CellAddr, HAlign, NumberFormat, Range, StylePatch, Value,
    Workbook,
};
use office_odf::{ns, Limits, OdfError, Package, Rgb};
use proptest::prelude::*;

fn a(s: &str) -> CellAddr {
    CellAddr::parse(s).expect("address")
}

fn r(s: &str) -> Range {
    Range::parse(s).expect("range")
}

fn sample() -> Workbook {
    let mut wb = Workbook::new();
    wb.set_fixed_now(Some(45_351.75));
    for (addr, input) in [
        ("A1", "Item"), ("B1", "Qty"), ("C1", "Price"), ("D1", "Total"),
        ("A2", "Widget <&> \"quoted\""), ("B2", "3"), ("C2", "$19.99"), ("D2", "=B2*C2"),
        ("A3", "Gadget\nline two"), ("B3", "10"), ("C3", "$0.5"), ("D3", "=B3*C3"),
        ("A5", "Sum"), ("D5", "=SUM(D2:D3)"), ("E5", "=D5/0"), ("F5", "=IF(D5>60;\"big\";\"small\")"),
        ("A6", "2024-02-29"), ("B6", "18:30"), ("C6", "12.5%"), ("D6", "true"), ("E6", "2024-02-29 06:00"),
        ("A7", "'007"), ("B7", "=1+"), ("C7", "1e-7"), ("D7", "-0.000123456789"), ("E7", "123456789012345"),
        ("AMJ9", "far"), ("A8", "12,34,567.89"), ("B8", "₹1,50,000"), ("C8", "15-08-1947"),
    ] {
        wb.set_input(0, a(addr), input).expect("set");
    }
    wb.set_style(
        0,
        r("A1:D1"),
        &StylePatch {
            bold: Some(true),
            italic: Some(true),
            underline: Some(true),
            align: Some(HAlign::Center),
            color: Some(Some(Rgb(255, 255, 255))),
            background: Some(Some(Rgb(0, 64, 128))),
            ..Default::default()
        },
    )
    .expect("style");
    wb.set_style(0, r("D2:D5"), &StylePatch { format: Some(NumberFormat::Number { decimals: 2, thousands: true }), ..Default::default() })
        .expect("style");
    wb.set_style(0, r("C7"), &StylePatch { format: Some(NumberFormat::Scientific { decimals: 3 }), ..Default::default() })
        .expect("style");
    wb.set_style(0, r("H1"), &StylePatch { format: Some(NumberFormat::Text), ..Default::default() }).expect("style");
    wb.set_col_width(0, 0, Some(144.0)).expect("width");
    wb.set_col_width(0, 3, Some(90.0)).expect("width");
    let second = wb.add_sheet(Some("Other Sheet")).expect("add");
    wb.set_input(second, a("B2"), "=Sheet1.D5*2").expect("set");
    wb.set_input(0, a("G1"), "='Other Sheet'.B2+1").expect("set");
    wb.meta.title = Some("Sample".into());
    wb
}

/// Everything observable about a workbook, for comparing before and after.
fn snapshot(wb: &Workbook) -> Vec<String> {
    let mut out = Vec::new();
    for (index, sheet) in wb.sheets().iter().enumerate() {
        out.push(format!("sheet {} widths {:?}", sheet.name, sheet.col_widths));
        for (addr, cell) in sheet.cells() {
            out.push(format!(
                "{}!{} input={:?} display={:?} value={:?} style={:?}",
                sheet.name,
                addr,
                wb.input_text(index, addr),
                wb.display(index, addr),
                cell.value,
                wb.style(index, addr)
            ));
        }
    }
    out
}

#[test]
fn ods_round_trip_preserves_everything_we_model() {
    let wb = sample();
    assert_eq!(wb.display(0, a("D5")), "64.97");
    assert_eq!(wb.display(0, a("G1")), "130.94");
    let bytes = save_ods(&wb).expect("save");
    let mut back = load_ods(&bytes, &Limits::default()).expect("load");
    back.set_fixed_now(Some(45_351.75));
    assert_eq!(snapshot(&back), snapshot(&wb));
    assert_eq!(back.meta.title.as_deref(), Some("Sample"));
    assert!(!back.is_modified());
    assert!(!back.can_undo());
    // Saving again is byte-for-byte stable.
    assert_eq!(save_ods(&back).expect("save again"), bytes);
    // The generic loader detects the format.
    assert_eq!(snapshot(&load(&bytes, &Limits::default()).expect("load")), snapshot(&wb));
}

#[test]
fn ods_structure_follows_the_specification() {
    let bytes = save_ods(&sample()).expect("save");
    let package = Package::read(&bytes, &Limits::default()).expect("package");
    assert_eq!(package.mimetype(), ns::MIME_ODS);
    let content = package.require_xml("content.xml", &Limits::default()).expect("content");
    assert_eq!(content.name, "office:document-content");
    assert_eq!(content.attr("office:version"), Some("1.2"));
    let tables: Vec<_> = content
        .descend(&["office:body", "office:spreadsheet"])
        .expect("body")
        .children_named("table:table")
        .collect();
    assert_eq!(tables.len(), 2);
    assert_eq!(tables[1].attr("table:name"), Some("Other Sheet"));
    // Formulas are stored in OpenFormula syntax with cached values.
    let xml = String::from_utf8(package.get("content.xml").expect("content").to_vec()).expect("utf8");
    assert!(xml.contains("table:formula=\"of:=[.B2]*[.C2]\""), "{xml}");
    assert!(xml.contains("table:formula=\"of:=[$&apos;Other Sheet&apos;.B2]+1\"") || xml.contains("of:=[$'Other Sheet'.B2]+1"));
    assert!(xml.contains("office:value-type=\"date\" office:date-value=\"2024-02-29\""));
    assert!(xml.contains("office:value-type=\"time\" office:time-value=\"PT18H30M00S\""));
    assert!(xml.contains("office:value-type=\"percentage\" office:value=\"0.125\""));
    assert!(xml.contains("office:value-type=\"boolean\" office:boolean-value=\"true\""));
    assert!(xml.contains("office:value-type=\"currency\""));
    assert!(package.get("styles.xml").is_some() && package.get("meta.xml").is_some());
}

const NS_DECL: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
    xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
    xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
    xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
    xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
    xmlns:number=\"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0\"";

fn ods_with(body: &str, styles: &str) -> Vec<u8> {
    let content = format!(
        "<?xml version=\"1.0\"?><office:document-content {NS_DECL}><office:automatic-styles>{styles}</office:automatic-styles>\
         <office:body><office:spreadsheet>{body}</office:spreadsheet></office:body></office:document-content>"
    );
    let mut package = Package::new(ns::MIME_ODS);
    package.insert("content.xml", content.into_bytes()).expect("insert");
    package.write().expect("write")
}

#[test]
fn reads_foreign_ods_constructs() {
    // Shapes written by OpenOffice / LibreOffice that we never write ourselves:
    // repeated rows and columns, covered cells, grouped rows, annotations,
    // text:s runs, string-value attributes and the legacy formula namespace.
    let body = r#"
      <table:table table:name="Data">
        <table:table-column table:number-columns-repeated="1024"/>
        <table:table-header-rows>
          <table:table-row>
            <table:table-cell office:value-type="string"><text:p>a<text:s text:c="3"/>b<text:tab/>c</text:p><text:p>second</text:p></table:table-cell>
            <table:covered-table-cell/>
            <table:table-cell office:value-type="float" office:value="2.5" table:number-columns-repeated="3"><text:p>2.5</text:p></table:table-cell>
          </table:table-row>
        </table:table-header-rows>
        <table:table-row-group>
          <table:table-row table:number-rows-repeated="2">
            <table:table-cell office:value-type="float" office:value="7"/>
            <table:table-cell table:formula="oooc:=[.A2]*2" office:value-type="float" office:value="14"/>
            <table:table-cell office:value-type="string" office:string-value="attr wins"><text:p>shown</text:p></table:table-cell>
            <table:table-cell office:value-type="string"><office:annotation><text:p>a comment</text:p></office:annotation><text:p>noted</text:p></table:table-cell>
          </table:table-row>
        </table:table-row-group>
        <table:table-row table:number-rows-repeated="1048570">
          <table:table-cell table:number-columns-repeated="1024"/>
        </table:table-row>
        <table:table-row>
          <table:table-cell office:value-type="time" office:time-value="PT06H00M00S"/>
          <table:table-cell office:value-type="date" office:date-value="2024-02-29T12:00:00"/>
          <table:table-cell office:value-type="boolean" office:boolean-value="false"/>
          <table:table-cell office:value-type="percentage" office:value="0.5"/>
        </table:table-row>
      </table:table>
      <table:table table:name="Bad/Name"/>
      <table:table table:name="Data"/>"#;
    let wb = load_ods(&ods_with(body, ""), &Limits::default()).expect("load");
    assert_eq!(wb.sheet_count(), 3);
    assert_eq!(wb.display(0, a("A1")), "a   b\tc\nsecond");
    assert_eq!(wb.display(0, a("B1")), "", "covered cell");
    for cell in ["C1", "D1", "E1"] {
        assert_eq!(wb.display(0, a(cell)), "2.5");
    }
    assert_eq!(wb.display(0, a("A3")), "7");
    assert_eq!(wb.input_text(0, a("B3")), "=A2*2");
    assert_eq!(wb.display(0, a("B2")), "14");
    assert_eq!(wb.display(0, a("C2")), "attr wins");
    assert_eq!(wb.display(0, a("D2")), "noted");
    assert_eq!(wb.display(0, a("A1048574")), "06:00:00");
    assert_eq!(wb.display(0, a("B1048574")), "2024-02-29 12:00:00");
    assert_eq!(wb.display(0, a("C1048574")), "FALSE");
    assert_eq!(wb.display(0, a("D1048574")), "50%");
    // Invalid and duplicate sheet names are repaired.
    assert_eq!(wb.sheets()[1].name, "Bad_Name");
    assert_ne!(wb.sheets()[2].name, "Data");
}

#[test]
fn reads_cell_and_data_styles() {
    let styles = r##"
      <number:number-style style:name="N1"><number:number number:decimal-places="3" number:grouping="true"/></number:number-style>
      <number:currency-style style:name="N2"><number:currency-symbol>€</number:currency-symbol><number:number number:decimal-places="2"/></number:currency-style>
      <number:date-style style:name="N3"><number:day/><number:text>.</number:text><number:month/></number:date-style>
      <style:style style:name="ce1" style:family="table-cell" style:data-style-name="N1">
        <style:text-properties fo:font-weight="bold" fo:color="#FF0000"/>
        <style:table-cell-properties fo:background-color="#00ff00"/>
        <style:paragraph-properties fo:text-align="end"/>
      </style:style>
      <style:style style:name="ce2" style:family="table-cell" style:data-style-name="N2"/>
      <style:style style:name="ce3" style:family="table-cell" style:data-style-name="N3"/>
      <style:style style:name="co1" style:family="table-column"><style:table-column-properties style:column-width="2.54cm"/></style:style>"##;
    let body = r#"<table:table table:name="S">
        <table:table-column table:style-name="co1"/>
        <table:table-row>
          <table:table-cell table:style-name="ce1" office:value-type="float" office:value="1234.5678"/>
          <table:table-cell table:style-name="ce2" office:value-type="currency" office:value="5"/>
          <table:table-cell table:style-name="ce3" office:value-type="date" office:date-value="2024-02-29"/>
          <table:table-cell table:style-name="ce1" table:number-columns-repeated="5"/>
          <table:table-cell table:style-name="ce1" table:number-columns-repeated="1000"/>
        </table:table-row></table:table>"#;
    let wb = load_ods(&ods_with(body, styles), &Limits::default()).expect("load");
    assert_eq!(wb.display(0, a("A1")), "1,234.568");
    let style = wb.style(0, a("A1"));
    assert!(style.bold && !style.italic);
    assert_eq!(style.color, Some(Rgb(255, 0, 0)));
    assert_eq!(style.background, Some(Rgb(0, 255, 0)));
    assert_eq!(style.align, HAlign::Right);
    assert_eq!(wb.display(0, a("B1")), "€5.00");
    assert_eq!(wb.display(0, a("C1")), "29-02-2024", "day-first date styles are kept day-first");
    assert_eq!(wb.sheet(0).expect("sheet").col_widths.get(&0), Some(&72.0));
    // Short style-only runs are kept, huge ones are dropped.
    assert!(wb.style(0, a("D1")).bold);
    assert!(!wb.style(0, a("Z1")).bold);
    assert_eq!(wb.sheet(0).expect("sheet").cell_count(), 8);
}

#[test]
fn hostile_ods_is_rejected_or_bounded() {
    // A tiny file that claims a billion filled cells.
    let body = r#"<table:table table:name="S"><table:table-row table:number-rows-repeated="1048576">
        <table:table-cell office:value-type="float" office:value="1" table:number-columns-repeated="1024"/>
        </table:table-row></table:table>"#;
    let err = load_ods(&ods_with(body, ""), &Limits::default()).expect_err("must be refused");
    assert!(matches!(err, OdfError::Limit(_)), "{err}");

    // Absurd repeat counts on empty cells cost nothing.
    let body = r#"<table:table table:name="S"><table:table-row table:number-rows-repeated="4000000000">
        <table:table-cell table:number-columns-repeated="4000000000"/></table:table-row>
        <table:table-row><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table>"#;
    let wb = load_ods(&ods_with(body, ""), &Limits::default()).expect("load");
    assert_eq!(wb.sheet(0).expect("sheet").cell_count(), 0, "content beyond the last row is dropped");

    // Bad numbers, bad dates and unparseable formulas do not abort loading.
    let body = r#"<table:table table:name="S"><table:table-row>
        <table:table-cell office:value-type="float" office:value="NaN"/>
        <table:table-cell office:value-type="float" office:value="1e999"/>
        <table:table-cell office:value-type="date" office:date-value="yesterday"/>
        <table:table-cell table:formula="of:=((((" office:value-type="float" office:value="3"/>
        <table:table-cell table:formula="of:=UNKNOWNFN([.A1])"/>
        </table:table-row></table:table>"#;
    let wb = load_ods(&ods_with(body, ""), &Limits::default()).expect("load");
    assert_eq!(wb.display(0, a("A1")), "");
    assert_eq!(wb.display(0, a("D1")), "Err:509");
    assert_eq!(wb.input_text(0, a("D1")), "=((((");
    assert_eq!(wb.display(0, a("E1")), "#NAME?");

    // Wrong document types and structural garbage.
    let mut package = Package::new(ns::MIME_ODT);
    package.insert("content.xml", b"<a/>".to_vec()).expect("insert");
    assert!(load_ods(&package.write().expect("write"), &Limits::default()).is_err());
    let mut package = Package::new(ns::MIME_ODS);
    package.insert("content.xml", b"<a/>".to_vec()).expect("insert");
    assert!(load_ods(&package.write().expect("write"), &Limits::default()).is_err());
    assert!(load_ods(&Package::new(ns::MIME_ODS).write().expect("write"), &Limits::default()).is_err());

    // Too many sheets.
    let many = "<table:table table:name=\"x\"/>".repeat(office_calc::MAX_SHEETS + 1);
    assert!(matches!(load_ods(&ods_with(&many, ""), &Limits::default()), Err(OdfError::Limit(_))));
}

#[test]
fn csv_import_and_export() {
    let csv = "name;qty;price\n\"Widget; large\";3;19.99\nGadget;10;0.5\n;;=B2*C2+B3*C3\n";
    let wb = load_csv(csv.as_bytes(), None, &Limits::default()).expect("load");
    assert_eq!(wb.display(0, a("A2")), "Widget; large");
    assert_eq!(wb.value(0, a("B2")), &Value::Number(3.0));
    assert_eq!(wb.display(0, a("C4")), "64.97");
    assert!(!wb.can_undo() && !wb.is_modified());
    assert_eq!(
        sheet_to_csv(&wb, 0, ',', false).expect("export"),
        "name,qty,price\nWidget; large,3,19.99\nGadget,10,0.5\n,,64.97\n"
    );
    assert_eq!(sheet_to_csv(&wb, 0, ';', false).expect("export").lines().nth(1), Some("\"Widget; large\";3;19.99"));
    // The generic loader treats non-ZIP input as CSV.
    assert_eq!(load(b"1,2\n3,4\n", &Limits::default()).expect("load").display(0, a("B2")), "4");
    assert!(sheet_to_csv(&wb, 5, ',', false).is_err());
    assert_eq!(sheet_to_csv(&Workbook::new(), 0, ',', false).expect("export"), "");
}

#[test]
fn csv_export_neutralises_formula_injection() {
    let mut wb = Workbook::new();
    wb.set_input(0, a("A1"), "'=HYPERLINK(\"http://evil\")").expect("set");
    wb.set_input(0, a("A2"), "'@SUM(1)").expect("set");
    wb.set_input(0, a("A3"), "'+1").expect("set");
    wb.set_input(0, a("A4"), "-5").expect("set");
    wb.set_input(0, a("A5"), "say \"hi\", ok").expect("set");
    let safe = sheet_to_csv(&wb, 0, ',', true).expect("export");
    assert_eq!(safe, "\"'=HYPERLINK(\"\"http://evil\"\")\"\n'@SUM(1)\n'+1\n-5\n\"say \"\"hi\"\", ok\"\n");
    let raw = sheet_to_csv(&wb, 0, ',', false).expect("export");
    assert!(raw.starts_with("\"=HYPERLINK"));
}

fn expr_strategy() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        (0u32..1000).prop_map(|n| n.to_string()),
        (0u32..100, 1u32..100).prop_map(|(x, y)| format!("{x}.{y}")),
        "[A-D][1-9]".prop_map(|s| s),
        "\\$[A-D]\\$[1-9]".prop_map(|s| s),
        "[A-C][1-3]:[D-F][4-6]".prop_map(|s| format!("SUM({s})")),
        "[a-z ]{0,6}".prop_map(|s| format!("\"{s}\"")),
        Just("TRUE".to_owned()),
        Just("PI()".to_owned()),
        Just("Sheet1.A1".to_owned()),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            (inner.clone(), prop::sample::select(vec!["+", "-", "*", "/", "^", "&", "=", "<>", "<", ">="]), inner.clone())
                .prop_map(|(l, op, r)| format!("{l}{op}{r}")),
            inner.clone().prop_map(|e| format!("({e})")),
            inner.clone().prop_map(|e| format!("-{e}")),
            inner.clone().prop_map(|e| format!("{e}%")),
            (prop::sample::select(vec!["SUM", "MAX", "IF", "ABS", "CONCATENATE", "ROUND", "NOSUCH"]), prop::collection::vec(inner, 0..4))
                .prop_map(|(name, args)| format!("{name}({})", args.join(";"))),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn formula_printing_round_trips(src in expr_strategy()) {
        let expr = parse(&src, Syntax::Ui);
        prop_assert!(!matches!(expr, Expr::Invalid(_)), "generated formula must parse: {}", src);
        // UI syntax: print -> parse is the identity on trees.
        let printed = expr.to_source(Syntax::Ui);
        prop_assert_eq!(&parse(&printed, Syntax::Ui), &expr, "printed as {}", printed);
        // OpenFormula syntax round-trips to the same tree as well.
        let of = expr.to_open_formula();
        prop_assert_eq!(&office_calc::formula::parse_open_formula(&of), &expr, "OpenFormula {}", of);
    }

    #[test]
    fn evaluation_is_total_and_survives_a_file_round_trip(src in expr_strategy()) {
        let mut wb = Workbook::new();
        for (i, v) in ["1", "2.5", "text", "", "TRUE", "=1/0", "-3", "100", "0"].iter().enumerate() {
            wb.set_input(0, CellAddr::new(i as u32, 0), v).unwrap();
            wb.set_input(0, CellAddr::new(i as u32, 1), v).unwrap();
        }
        wb.set_input(0, a("H1"), &format!("={src}")).unwrap();
        let before = wb.display(0, a("H1"));
        let back = load_ods(&save_ods(&wb).unwrap(), &Limits::default()).unwrap();
        prop_assert_eq!(back.display(0, a("H1")), before);
        prop_assert_eq!(back.input_text(0, a("H1")), wb.input_text(0, a("H1")));
    }

    #[test]
    fn parser_is_total_on_arbitrary_text(src in "\\PC{0,120}") {
        let _ = parse(&src, Syntax::Ui);
        let _ = parse(&src, Syntax::OpenFormula);
        let mut wb = Workbook::new();
        wb.set_input(0, a("A1"), &format!("={src}")).unwrap();
        wb.set_input(0, a("A2"), &src).unwrap();
        let _ = wb.display(0, a("A1"));
    }

    #[test]
    fn loaders_are_total_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..600)) {
        let _ = load(&bytes, &Limits::default());
        let mut zipish = b"PK\x03\x04".to_vec();
        zipish.extend_from_slice(&bytes);
        let _ = load(&zipish, &Limits::default());
    }

    #[test]
    fn csv_round_trips_text(rows in prop::collection::vec(prop::collection::vec("[a-z ,;\"\n]{0,8}", 1..5), 1..6)) {
        let mut wb = Workbook::new();
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                wb.set_content(0, CellAddr::new(r as u32, c as u32), office_calc::Content::Text(text.clone())).unwrap();
            }
        }
        let csv = sheet_to_csv(&wb, 0, ',', false).unwrap();
        let back = office_calc::csv::parse_csv(&csv, ',', &Limits::default()).unwrap();
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                let got = back.get(r).and_then(|x| x.get(c)).map(String::as_str).unwrap_or("");
                prop_assert_eq!(got, text.as_str());
            }
        }
    }

    #[test]
    fn incremental_recalculation_matches_full_recalculation(
        edits in prop::collection::vec((0u32..6, 0u32..4, prop::sample::select(vec![
            "1", "2", "", "=A1+1", "=SUM(A1:B3)", "=B2*2", "=C1&\"x\"", "=IF(A1>1;B1;C1)", "=A1", "=D4", "=SUM(A1:D6)",
        ])), 1..40)
    ) {
        let mut wb = Workbook::new();
        for (row, col, input) in &edits {
            wb.set_input(0, CellAddr::new(*row, *col), input).unwrap();
        }
        let incremental = snapshot(&wb);
        wb.recalculate_all();
        prop_assert_eq!(incremental, snapshot(&wb));
    }
}
