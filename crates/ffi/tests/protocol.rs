//! End-to-end tests of the JSON protocol, through the C ABI functions.

use akshara::{akshara_alloc, akshara_call, akshara_free};
use serde_json::{json, Value};

/// Sends a request through the raw C ABI, exactly as Dart does.
fn call(req: Value) -> Value {
    let bytes = req.to_string().into_bytes();
    let buf = akshara_alloc(bytes.len());
    assert!(!buf.is_null());
    let mut out_len = 0usize;
    #[allow(unsafe_code)]
    let response = unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        let out = akshara_call(buf, bytes.len(), &mut out_len);
        akshara_free(buf, bytes.len());
        let response = std::slice::from_raw_parts(out, out_len).to_vec();
        akshara_free(out, out_len);
        response
    };
    serde_json::from_slice(&response).expect("response is JSON")
}

fn ok(req: Value) -> Value {
    let resp = call(req.clone());
    assert_eq!(resp["ok"], json!(true), "request {req} failed: {resp}");
    resp["result"].clone()
}

fn err_kind(req: Value) -> String {
    let resp = call(req);
    assert_eq!(resp["ok"], json!(false), "expected an error: {resp}");
    resp["error"]["kind"].as_str().expect("kind").to_owned()
}

fn temp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("akshara-ffi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join(name).to_string_lossy().into_owned()
}

#[test]
fn malformed_requests_are_errors_not_crashes() {
    assert_eq!(err_kind(json!("not an object")), "invalid");
    assert_eq!(err_kind(json!({})), "invalid");
    assert_eq!(err_kind(json!({"op": "nope"})), "invalid");
    assert_eq!(err_kind(json!({"op": "info", "doc": 999_999})), "invalid");
    assert_eq!(err_kind(json!({"op": "new", "kind": "keynote"})), "invalid");
    let mut out_len = 0usize;
    #[allow(unsafe_code)]
    unsafe {
        let out = akshara_call(b"{garbage".as_ptr(), 8, &mut out_len);
        let text = String::from_utf8(std::slice::from_raw_parts(out, out_len).to_vec()).expect("utf8");
        assert!(text.contains("\"ok\":false"));
        akshara_free(out, out_len);
        let out = akshara_call(std::ptr::null(), 0, &mut out_len);
        akshara_free(out, out_len);
        assert!(akshara_call(std::ptr::null(), 0, std::ptr::null_mut()).is_null());
        akshara_free(std::ptr::null_mut(), 10);
    }
    assert!(akshara_alloc(0).is_null());
    assert!(akshara_alloc(usize::MAX).is_null());
    assert_eq!(ok(json!({"op": "version"}))["name"], json!("Akshara"));
}

#[test]
fn calc_session() {
    let doc = ok(json!({"op": "new", "kind": "calc"}))["doc"].clone();
    ok(json!({"op": "calc.set_input", "doc": doc, "sheet": 0, "cell": "A1", "text": "2"}));
    ok(json!({"op": "calc.paste", "doc": doc, "sheet": 0, "cell": "A2", "rows": "3\t4\n=A1+A2\tx\n"}));
    let cell = ok(json!({"op": "calc.cell", "doc": doc, "sheet": 0, "cell": "A3"}));
    assert_eq!(cell["display"], json!("5"));
    assert_eq!(cell["input"], json!("=A1+A2"));
    ok(json!({"op": "calc.style", "doc": doc, "sheet": 0, "range": "A1:A3",
              "patch": {"bold": true, "background": "#ff0000", "format": {"kind": "number", "decimals": 2}}}));
    let view = ok(json!({"op": "calc.view", "doc": doc, "sheet": 0, "row": 0, "col": 0, "rows": 10, "cols": 5}));
    let cells = view["cells"].as_array().expect("cells");
    assert_eq!(cells.len(), 5);
    assert_eq!(cells[0]["t"], json!("2.00"));
    let style_id = cells[0]["s"].as_u64().expect("style").to_string();
    assert_eq!(view["styles"][&style_id]["bold"], json!(true));
    assert_eq!(view["styles"][&style_id]["background"], json!("#ff0000"));
    assert_eq!(view["used"], json!({"rows": 3, "cols": 2}));

    ok(json!({"op": "calc.insert", "doc": doc, "sheet": 0, "axis": "row", "at": 0, "count": 1}));
    assert_eq!(ok(json!({"op": "calc.cell", "doc": doc, "sheet": 0, "cell": "A4"}))["input"], json!("=A2+A3"));
    assert_eq!(ok(json!({"op": "undo", "doc": doc})), json!(true));
    assert_eq!(ok(json!({"op": "calc.copy_text", "doc": doc, "sheet": 0, "range": "A1:B2"})), json!("2.00\t\n3.00\t4\n"));
    let sheet = ok(json!({"op": "calc.add_sheet", "doc": doc, "name": "Data"}));
    assert_eq!(sheet, json!(1));
    ok(json!({"op": "calc.rename_sheet", "doc": doc, "sheet": 1, "name": "Numbers"}));
    assert_eq!(ok(json!({"op": "calc.sheets", "doc": doc}))[1]["name"], json!("Numbers"));
    assert!(ok(json!({"op": "calc.functions"})).as_array().expect("names").len() > 100);

    assert_eq!(err_kind(json!({"op": "calc.set_input", "doc": doc, "sheet": 0, "cell": "ZZZZ1", "text": "x"})), "invalid");
    assert_eq!(err_kind(json!({"op": "calc.style", "doc": doc, "sheet": 0, "range": "A1:AMJ1048576", "patch": {"bold": true}})), "limit");
    assert_eq!(err_kind(json!({"op": "writer.get", "doc": doc})), "invalid");

    let path = temp("book.ods");
    let info = ok(json!({"op": "save", "doc": doc, "path": path}));
    assert_eq!(info["modified"], json!(false));
    let reopened = ok(json!({"op": "open", "path": path}));
    assert_eq!(reopened["kind"], json!("calc"));
    assert_eq!(ok(json!({"op": "calc.cell", "doc": reopened["doc"], "sheet": 0, "cell": "A3"}))["display"], json!("5.00"));
    let csv = temp("book.csv");
    ok(json!({"op": "save", "doc": doc, "path": csv}));
    assert_eq!(std::fs::read_to_string(&csv).expect("csv"), "2.00\n3.00,4\n5.00,x\n");
    assert_eq!(err_kind(json!({"op": "save", "doc": doc, "path": temp("book.odt")})), "unsupported");
    assert_eq!(ok(json!({"op": "close", "doc": doc})), json!(true));
    assert_eq!(ok(json!({"op": "close", "doc": doc})), json!(false));
}

#[test]
fn writer_session() {
    let doc = ok(json!({"op": "new", "kind": "writer"}))["doc"].clone();
    let p0 = json!({"block": 0});
    ok(json!({"op": "writer.set_text", "doc": doc, "para": p0, "text": "Hello world"}));
    ok(json!({"op": "writer.attrs", "doc": doc, "para": p0, "start": 6, "end": 11, "patch": {"bold": true, "color": "#336699"}}));
    let p1 = ok(json!({"op": "writer.split", "doc": doc, "para": p0, "at": 5}));
    assert_eq!(p1, json!({"block": 1}));
    ok(json!({"op": "writer.kind", "doc": doc, "para": p0, "kind": {"kind": "heading", "level": 2}}));
    ok(json!({"op": "writer.insert_table", "doc": doc, "at": 2, "rows": 2, "cols": 2}));
    ok(json!({"op": "writer.set_text", "doc": doc, "para": {"block": 2, "cell": [1, 1, 0]}, "text": "cell"}));
    let got = ok(json!({"op": "writer.get", "doc": doc}));
    let blocks = got["blocks"].as_array().expect("blocks");
    assert_eq!(blocks[0]["type"], json!("paragraph"));
    assert_eq!(blocks[0]["kind"], json!("heading"));
    assert_eq!(blocks[0]["level"], json!(2));
    assert_eq!(blocks[1]["runs"][1]["bold"], json!(true));
    assert_eq!(blocks[1]["runs"][1]["color"], json!("#336699"));
    assert_eq!(blocks[2]["rows"][1][1][0]["runs"][0]["text"], json!("cell"));
    assert_eq!(got["stats"]["words"], json!(3));

    let hits = ok(json!({"op": "writer.find", "doc": doc, "needle": "world"}));
    assert_eq!(hits, json!([{"para": {"block": 1}, "start": 1, "end": 6}]));
    assert_eq!(ok(json!({"op": "writer.replace_all", "doc": doc, "needle": "o", "replacement": "0"})), json!(2));
    let merged = ok(json!({"op": "writer.merge", "doc": doc, "para": {"block": 1}}));
    assert_eq!(merged, json!({"para": {"block": 0}, "at": 5}));
    assert_eq!(err_kind(json!({"op": "writer.attrs", "doc": doc, "para": p0, "start": 0, "end": 1, "patch": {"link": "javascript:x"}})), "invalid");
    assert_eq!(err_kind(json!({"op": "writer.set_text", "doc": doc, "para": {"block": 99}, "text": "x"})), "invalid");

    let path = temp("doc.odt");
    ok(json!({"op": "save", "doc": doc, "path": path}));
    let reopened = ok(json!({"op": "open", "path": path}))["doc"].clone();
    let again = ok(json!({"op": "writer.get", "doc": reopened}));
    assert_eq!(again["blocks"], ok(json!({"op": "writer.get", "doc": doc}))["blocks"]);
    let html = temp("doc.html");
    ok(json!({"op": "save", "doc": doc, "path": html}));
    let html = std::fs::read_to_string(&html).expect("html");
    assert!(html.contains("<h2>Hell0 <span style=\"color:#336699\"><strong>w0rld</strong></span></h2>"), "{html}");
}

#[test]
fn math_session() {
    let doc = ok(json!({"op": "new", "kind": "math", "source": "a over b"}))["doc"].clone();
    let got = ok(json!({"op": "math.get", "doc": doc}));
    assert_eq!(got["tree"]["t"], json!("frac"));
    let set = ok(json!({"op": "math.set", "doc": doc, "source": "sqrt{x^2 +"}));
    assert!(!set["errors"].as_array().expect("errors").is_empty());
    let preview = ok(json!({"op": "math.parse", "source": "%alpha"}));
    assert_eq!(preview["tree"], json!({"t": "ident", "text": "α", "italic": false}));
    assert!(ok(json!({"op": "math.mathml", "doc": doc})).as_str().expect("mathml").contains("<msqrt>"));
    assert_eq!(ok(json!({"op": "math.symbols"})).as_array().expect("symbols").len(), 54);
    let path = temp("formula.odf");
    ok(json!({"op": "math.set", "doc": doc, "source": "E = m c^2"}));
    ok(json!({"op": "save", "doc": doc, "path": path}));
    let reopened = ok(json!({"op": "open", "path": path}));
    assert_eq!(reopened["kind"], json!("math"));
    assert_eq!(ok(json!({"op": "math.get", "doc": reopened["doc"]}))["source"], json!("E = m c^2"));
}

#[test]
fn transliteration() {
    let scripts = ok(json!({"op": "indic.scripts"}));
    assert_eq!(scripts.as_array().expect("scripts").len(), 7);
    let t = |mode: &str, text: &str, script: &str| {
        ok(json!({"op": "indic.translit", "mode": mode, "text": text, "script": script}))
    };
    assert_eq!(t("roman", "satyamEva jayatE", "telu"), json!("సత్యమేవ జయతే"));
    assert_eq!(t("roman", "namastE", "deva"), json!("नमस्ते"));
    assert_eq!(t("script", "నమస్తే", "taml"), json!("நமஸ்தே"));
    assert_eq!(t("to_iast", "धर्मः", "deva"), json!("dharmaḥ"));
    assert_eq!(t("from_iast", "kṛṣṇa", "knda"), json!("ಕೃಷ್ಣ"));
    assert_eq!(ok(json!({"op": "indic.translit", "text": "108", "script": "deva", "native_digits": true})), json!("१०८"));
    assert_eq!(err_kind(json!({"op": "indic.translit", "text": "x", "script": "latn"})), "invalid");
    assert_eq!(err_kind(json!({"op": "indic.translit", "text": "x", "script": "deva", "mode": "nope"})), "invalid");
    assert_eq!(ok(json!({"op": "indic.detect", "text": "abc తెలుగు"})), json!("telu"));

    let doc = ok(json!({"op": "new", "kind": "calc"}))["doc"].clone();
    ok(json!({"op": "calc.paste", "doc": doc, "sheet": 0, "cell": "A1", "rows": "rAma\t5\nsItA\t=B1*2\n"}));
    let n = ok(json!({"op": "calc.transliterate", "doc": doc, "sheet": 0, "range": "A1:B2", "mode": "roman", "script": "deva"}));
    assert_eq!(n, json!(2));
    assert_eq!(ok(json!({"op": "calc.cell", "doc": doc, "sheet": 0, "cell": "A2"}))["display"], json!("सीता"));
    assert_eq!(ok(json!({"op": "calc.cell", "doc": doc, "sheet": 0, "cell": "B2"}))["display"], json!("10"));
    assert_eq!(err_kind(json!({"op": "calc.transliterate", "doc": doc, "sheet": 0, "range": "A1", "script": "zzzz"})), "invalid");
}

#[test]
fn opening_files_is_defensive() {
    assert_eq!(err_kind(json!({"op": "open", "path": temp("missing.ods")})), "io");
    let exe = temp("evil.exe");
    std::fs::write(&exe, b"MZ").expect("write");
    assert_eq!(err_kind(json!({"op": "open", "path": exe})), "unsupported");
    let fake = temp("fake.ods");
    std::fs::write(&fake, b"PK\x03\x04 not really a zip").expect("write");
    assert_eq!(err_kind(json!({"op": "open", "path": fake})), "zip");
    let txt = temp("notes.txt");
    std::fs::write(&txt, b"line 1\nline 2").expect("write");
    assert_eq!(ok(json!({"op": "open", "path": txt}))["kind"], json!("writer"));
}

#[test]
fn writer_page_setup_header_footer_and_templates() {
    let doc = ok(json!({"op": "new", "kind": "writer"}))["doc"].clone();
    let sizes = ok(json!({"op": "paper_sizes"}));
    let card = sizes["papers"].as_array().expect("papers").iter().find(|p| p["id"] == "card_4x6").expect("card").clone();
    ok(json!({"op": "writer.page", "doc": doc, "width": card["width"], "height": card["height"], "margin": 18}));
    assert_eq!(err_kind(json!({"op": "writer.page", "doc": doc, "width": 5, "height": 5, "margin": 0})), "invalid");
    ok(json!({"op": "writer.header", "doc": doc, "on": true}));
    let h0 = json!({"area": "header", "block": 0});
    ok(json!({"op": "writer.set_text", "doc": doc, "para": h0, "text": "Page "}));
    ok(json!({"op": "writer.field", "doc": doc, "para": h0, "at": 5, "field": "page_number"}));
    assert_eq!(err_kind(json!({"op": "writer.field", "doc": doc, "para": h0, "at": 0, "field": "date"})), "invalid");
    assert_eq!(err_kind(json!({"op": "writer.para", "doc": doc, "para": {"area": "side", "block": 0}})), "invalid");
    let split = ok(json!({"op": "writer.split", "doc": doc, "para": h0, "at": 0}));
    assert_eq!(split, json!({"area": "header", "block": 1}));
    ok(json!({"op": "writer.spacing", "doc": doc, "para": {"block": 0}, "indent": 36, "space_before": 6}));
    let got = ok(json!({"op": "writer.get", "doc": doc}));
    assert_eq!(got["paper"], json!("card_4x6"));
    assert_eq!(got["header"][1]["runs"][1]["field"], json!("page_number"));
    assert_eq!(got["footer"], json!([]));
    assert_eq!(got["blocks"][0]["indent"], json!(36.0));
    ok(json!({"op": "writer.letterhead", "doc": doc, "name": "Name", "details": "Street", "contact": "mail", "align": "center", "color": "#1f5fbf"}));
    let got = ok(json!({"op": "writer.get", "doc": doc}));
    assert_eq!(got["header"][0]["runs"][0]["text"], json!("Name"));
    assert_eq!(got["footer"][0]["align"], json!("center"));

    let env = ok(json!({"op": "new_envelope", "width": 623.6, "height": 311.8, "margin": 28.8, "sender": "Me", "recipient": "You\nTown"}));
    let got = ok(json!({"op": "writer.get", "doc": env["doc"]}));
    assert_eq!(got["paper"], json!("env_dl"));
    assert_eq!(got["blocks"].as_array().map(Vec::len), Some(3));
    let cards = ok(json!({"op": "new_index_cards", "width": 360, "height": 216, "margin": 18, "count": 2}));
    assert_eq!(ok(json!({"op": "writer.get", "doc": cards["doc"]}))["blocks"][2]["type"], json!("page_break"));
    let labels = ok(json!({"op": "new_labels", "sheet": "5160", "text": "A\nB", "same": true}));
    let got = ok(json!({"op": "writer.get", "doc": labels["doc"]}));
    assert_eq!(got["blocks"][0]["borders"], json!(false));
    assert_eq!(err_kind(json!({"op": "new_labels", "sheet": "nope"})), "invalid");
}

#[test]
fn pdf_is_built_from_page_images() {
    // A 1×1 white PNG, written by hand: signature, IHDR, IDAT, IEND.
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
        0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xFF, 0xFF, 0x3F, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xDC,
        0xCC, 0x59, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let page = temp("page1.png");
    std::fs::write(&page, png).expect("write png");
    let out = temp("out.pdf");
    let n = ok(json!({"op": "pdf_from_pngs", "pngs": [page, page], "width": 595.28, "height": 841.89, "path": out, "title": "T"}));
    assert_eq!(n, json!(2));
    let pdf = std::fs::read(&out).expect("pdf");
    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert_eq!(err_kind(json!({"op": "pdf_from_pngs", "pngs": [out], "width": 100, "height": 100, "path": temp("x.pdf")})), "invalid");
    assert_eq!(err_kind(json!({"op": "pdf_from_pngs", "pngs": [], "width": 100, "height": 100, "path": temp("y.pdf")})), "invalid");
}

#[test]
fn calc_page_setup() {
    let doc = ok(json!({"op": "new", "kind": "calc"}))["doc"].clone();
    let page = ok(json!({"op": "calc.page", "doc": doc}));
    assert_eq!(page["paper"], json!("a4"));
    assert_eq!((page["scale"].clone(), page["grid"].clone()), (json!(100), json!(true)));
    ok(json!({"op": "calc.set_page", "doc": doc, "width": 841.89, "height": 595.28, "margin": 36, "fit_width": true, "grid": false}));
    let page = ok(json!({"op": "calc.page", "doc": doc}));
    assert_eq!((page["fit_width"].clone(), page["grid"].clone(), page["paper"].clone()), (json!(true), json!(false), json!("a4")));
    assert_eq!(err_kind(json!({"op": "calc.set_page", "doc": doc, "width": 841.89, "height": 595.28, "margin": 36, "scale": 1000})), "invalid");
    assert_eq!(ok(json!({"op": "info", "doc": doc}))["can_undo"], json!(true));
}

#[test]
fn calc_print_area() {
    let doc = ok(json!({"op": "new", "kind": "calc"}))["doc"].clone();
    assert_eq!(ok(json!({"op": "calc.print_area", "doc": doc, "sheet": 0})), json!({"range": null, "repeat_rows": null}));
    ok(json!({"op": "calc.set_print_area", "doc": doc, "sheet": 0, "range": "$A$1:$D$20", "repeat_rows": "1:2"}));
    assert_eq!(ok(json!({"op": "calc.print_area", "doc": doc, "sheet": 0})), json!({"range": "A1:D20", "repeat_rows": "1:2"}));
    ok(json!({"op": "calc.set_print_area", "doc": doc, "sheet": 0, "range": "", "repeat_rows": "3"}));
    assert_eq!(ok(json!({"op": "calc.print_area", "doc": doc, "sheet": 0})), json!({"range": null, "repeat_rows": "3"}));
    assert_eq!(err_kind(json!({"op": "calc.set_print_area", "doc": doc, "sheet": 0, "range": "nonsense"})), "invalid");
    assert_eq!(err_kind(json!({"op": "calc.set_print_area", "doc": doc, "sheet": 0, "repeat_rows": "0"})), "invalid");
    assert_eq!(err_kind(json!({"op": "calc.set_print_area", "doc": doc, "sheet": 0, "repeat_rows": "5:2"})), "invalid");
}

/// A minimal OOXML package: `_rels/.rels` pointing at `main`, plus parts.
fn ooxml(main: &str, parts: &[(&str, String)]) -> Vec<u8> {
    let mut pkg = office_odf::Package::new("");
    let rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{main}\"/></Relationships>"
    );
    pkg.insert("_rels/.rels", rels.into_bytes()).expect("rels");
    for (name, xml) in parts {
        pkg.insert(*name, xml.clone().into_bytes()).expect("part");
    }
    pkg.write().expect("zip")
}

#[test]
fn word_and_excel_files_open_read_only() {
    let docx = ooxml("word/document.xml", &[(
        "word/document.xml",
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>\
         <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hello from Word</w:t></w:r></w:p></w:body></w:document>".to_owned(),
    )]);
    let path = temp("letter.docx");
    std::fs::write(&path, docx).expect("write docx");
    let opened = ok(json!({"op": "open", "path": path}));
    assert_eq!(opened["kind"], json!("writer"));
    let got = ok(json!({"op": "writer.get", "doc": opened["doc"]}));
    assert_eq!(got["blocks"][0]["runs"][0]["text"], json!("Hello from Word"));
    assert_eq!(got["blocks"][0]["runs"][0]["bold"], json!(true));
    // Saving back to Word works and clears "modified"; the copy reopens.
    ok(json!({"op": "writer.set_text", "doc": opened["doc"], "para": {"block": 0}, "text": "Hello again"}));
    let saved = ok(json!({"op": "save", "doc": opened["doc"], "path": temp("copy.docx")}));
    assert_eq!(saved["modified"], json!(false));
    let again = ok(json!({"op": "open", "path": temp("copy.docx")}));
    assert_eq!(ok(json!({"op": "writer.get", "doc": again["doc"]}))["blocks"][0]["runs"][0]["text"], json!("Hello again"));

    let main = "xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
    let xlsx = ooxml("xl/workbook.xml", &[
        ("xl/workbook.xml", format!("<workbook {main}><sheets><sheet name=\"Data\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>")),
        ("xl/_rels/workbook.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
          <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>".to_owned()),
        ("xl/worksheets/sheet1.xml", format!("<worksheet {main}><sheetData><row r=\"1\"><c r=\"A1\"><v>2</v></c><c r=\"B1\"><f>A1*21</f><v>42</v></c></row></sheetData></worksheet>")),
    ]);
    let path = temp("book.xlsx");
    std::fs::write(&path, xlsx).expect("write xlsx");
    let opened = ok(json!({"op": "open", "path": path}));
    assert_eq!(opened["kind"], json!("calc"));
    let cell = ok(json!({"op": "calc.cell", "doc": opened["doc"], "sheet": 0, "cell": "B1"}));
    assert_eq!((cell["input"].clone(), cell["display"].clone()), (json!("=A1*21"), json!("42")));
    assert_eq!(ok(json!({"op": "calc.sheets", "doc": opened["doc"]}))[0]["name"], json!("Data"));
    ok(json!({"op": "save", "doc": opened["doc"], "path": temp("copy.xlsx")}));
    let again = ok(json!({"op": "open", "path": temp("copy.xlsx")}));
    let cell = ok(json!({"op": "calc.cell", "doc": again["doc"], "sheet": 0, "cell": "B1"}));
    assert_eq!((cell["input"].clone(), cell["display"].clone()), (json!("=A1*21"), json!("42")));

    // Old binary Office files get a clear message.
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.resize(512, 0);
    let path = temp("old.doc");
    std::fs::write(&path, ole).expect("write doc");
    assert_eq!(err_kind(json!({"op": "open", "path": path})), "unsupported");
}

#[test]
fn impress_and_draw_documents() {
    let doc = ok(json!({"op": "new", "kind": "impress"}));
    assert_eq!(doc["kind"], "impress");
    let d = doc["doc"].clone();
    let got = ok(json!({"op": "draw.get", "doc": d}));
    assert_eq!(got["kind"], "impress");
    assert_eq!(got["pages"].as_array().map(Vec::len), Some(1));
    assert_eq!(got["pages"][0]["title"], "Slide 1");
    assert_eq!(got["pages"][0]["shapes"][0]["placeholder"], "title");
    ok(json!({"op": "draw.add_page", "doc": d, "at": 1, "layout": "content"}));
    let ids = ok(json!({"op": "shapes.add", "doc": d, "page": 1, "shapes": [
        {"kind": "ellipse", "x": 10, "y": 10, "w": 100, "h": 50, "text": "Hi", "fill": "#ff0000", "bold": true},
        {"kind": "diagram", "x": 10, "y": 100, "w": 200, "h": 100, "source": "graph LR\nA-->B"},
        {"kind": "polygon", "x": 0, "y": 0, "w": 50, "h": 50, "points": [[0, 0], [1, 0], [0.5, 1]]},
    ]}));
    let ids: Vec<u64> = ids.as_array().expect("ids").iter().filter_map(Value::as_u64).collect();
    assert_eq!(ids.len(), 3);
    let page = ok(json!({"op": "draw.page", "doc": d, "page": 1}));
    let shapes = page["shapes"].as_array().expect("shapes");
    assert_eq!(shapes[2]["fill"], "#ff0000");
    assert_eq!(shapes[2]["style"]["bold"], true);
    assert!(shapes[3]["figure"]["items"].as_array().is_some_and(|i| !i.is_empty()), "diagrams carry their figure");
    // Typing in a shape coalesces into one undo step.
    for text in ["a", "ab", "abc"] {
        ok(json!({"op": "shapes.update", "doc": d, "page": 1, "key": "text", "updates": [{"id": ids[0], "patch": {"text": text}}]}));
    }
    ok(json!({"op": "undo", "doc": d}));
    assert_eq!(ok(json!({"op": "draw.page", "doc": d, "page": 1}))["shapes"][2]["text"], "Hi");
    ok(json!({"op": "shapes.order", "doc": d, "page": 1, "id": ids[0], "to": "back"}));
    assert_eq!(ok(json!({"op": "shapes.copy", "doc": d, "page": 1, "ids": [ids[0]]})), json!(1));
    let pasted = ok(json!({"op": "shapes.paste", "doc": d, "page": 0}));
    assert_eq!(pasted.as_array().map(Vec::len), Some(1));
    ok(json!({"op": "shapes.remove", "doc": d, "page": 1, "ids": [ids[1]]}));
    ok(json!({"op": "draw.page_props", "doc": d, "page": 1, "name": "Two", "background": "#eeeeee", "notes": "say it"}));
    ok(json!({"op": "draw.move_page", "doc": d, "from": 1, "to": 0}));
    ok(json!({"op": "draw.duplicate_page", "doc": d, "page": 0}));
    assert_eq!(err_kind(json!({"op": "shapes.update", "doc": d, "page": 0, "updates": [{"id": 9999, "patch": {}}]})), "invalid");
    assert_eq!(err_kind(json!({"op": "shapes.add", "doc": d, "page": 0, "shapes": [{"kind": "blob", "x": 0, "y": 0, "w": 1, "h": 1}]})), "invalid");

    let path = temp("deck.odp");
    ok(json!({"op": "save", "doc": d, "path": path}));
    assert_eq!(ok(json!({"op": "info", "doc": d}))["modified"], false);
    assert_eq!(err_kind(json!({"op": "save", "doc": d, "path": temp("deck.odg")})), "unsupported");
    let svg = temp("slide.svg");
    ok(json!({"op": "save", "doc": d, "path": svg, "page": 0}));
    assert!(std::fs::read_to_string(&svg).expect("svg").contains("<svg"));
    let back = ok(json!({"op": "open", "path": path}));
    assert_eq!(back["kind"], "impress");
    let got = ok(json!({"op": "draw.get", "doc": back["doc"]}));
    assert_eq!(got["pages"].as_array().map(Vec::len), Some(3));
    assert_eq!(got["pages"][0]["name"], "Two");
    assert_eq!(got["pages"][0]["notes"], "say it");

    let draw = ok(json!({"op": "new", "kind": "draw"}));
    assert_eq!(draw["kind"], "draw");
    ok(json!({"op": "draw.set_size", "doc": draw["doc"], "width": 400, "height": 300}));
    assert_eq!(err_kind(json!({"op": "draw.set_size", "doc": draw["doc"], "width": 1, "height": 300})), "invalid");
    let odg = temp("pic.odg");
    ok(json!({"op": "save", "doc": draw["doc"], "path": odg}));
    assert_eq!(ok(json!({"op": "open", "path": odg}))["kind"], "draw");
    std::fs::write(temp("x.pptx"), b"PK").expect("write");
    assert_eq!(err_kind(json!({"op": "open", "path": temp("x.pptx")})), "unsupported");
}

#[test]
fn writer_pictures_diagrams_and_drawings() {
    let png = temp("dot.png");
    std::fs::write(&png, resvg::tiny_skia::Pixmap::new(30, 20).expect("pixmap").encode_png().expect("png")).expect("write");
    let d = ok(json!({"op": "new", "kind": "writer"}))["doc"].clone();
    ok(json!({"op": "writer.insert_image", "doc": d, "at": 0, "path": png}));
    ok(json!({"op": "writer.insert_diagram", "doc": d, "at": 1, "source": "sequenceDiagram\nA->>B: hi"}));
    ok(json!({"op": "writer.insert_drawing", "doc": d, "at": 2, "width": 200, "height": 100}));
    let blocks = ok(json!({"op": "writer.get", "doc": d}))["blocks"].clone();
    assert_eq!(blocks[0]["type"], "image");
    assert_eq!(blocks[0]["width"], 22.5, "30 px at 96 dpi");
    assert_eq!(blocks[1]["type"], "diagram");
    assert!(blocks[1]["height"].as_f64().is_some_and(|h| h > 10.0));
    assert!(blocks[1]["figure"]["items"].is_array());
    assert_eq!(blocks[2]["type"], "drawing");
    let id = blocks[0]["image"].as_str().expect("id").to_owned();
    let data = ok(json!({"op": "image.data", "doc": d, "id": id}));
    assert_eq!(data["format"], "png");
    assert_eq!((data["width"].as_u64(), data["height"].as_u64()), (Some(30), Some(20)));
    assert!(data["data"].as_str().is_some_and(|s| s.starts_with("iVBOR")));
    // Shapes in the canvas.
    let ids = ok(json!({"op": "shapes.add", "doc": d, "block": 2, "shapes": [{"kind": "rect", "x": 0, "y": 0, "w": 50, "h": 20}]}));
    assert_eq!(ids, json!([1]));
    ok(json!({"op": "shapes.insert_image", "doc": d, "block": 2, "path": png}));
    assert_eq!(err_kind(json!({"op": "shapes.add", "doc": d, "block": 0, "shapes": []})), "invalid");
    ok(json!({"op": "writer.frame", "doc": d, "block": 0, "width": 45, "align": "right", "description": "dot"}));
    ok(json!({"op": "writer.frame", "doc": d, "block": 1, "source": "pie\n\"a\": 1"}));
    let blocks = ok(json!({"op": "writer.get", "doc": d}))["blocks"].clone();
    assert_eq!((blocks[0]["width"].as_f64(), blocks[0]["height"].as_f64()), (Some(45.0), Some(30.0)));
    assert_eq!(blocks[0]["align"], "right");
    assert_eq!(blocks[2]["shapes"].as_array().map(Vec::len), Some(2));
    for ext in ["odt", "docx", "html", "md"] {
        let path = temp(&format!("pics.{ext}"));
        ok(json!({"op": "save", "doc": d, "path": path}));
    }
    let back = ok(json!({"op": "open", "path": temp("pics.odt")}));
    let blocks = ok(json!({"op": "writer.get", "doc": back["doc"]}))["blocks"].clone();
    assert_eq!(blocks[0]["description"], "dot");
    assert_eq!(blocks[1]["source"], "pie\n\"a\": 1");
    assert_eq!(blocks[2]["shapes"].as_array().map(Vec::len), Some(2));
    let bad = temp("bad.png");
    std::fs::write(&bad, b"not a picture").expect("write");
    assert_eq!(err_kind(json!({"op": "writer.insert_image", "doc": d, "at": 0, "path": bad})), "unsupported");
}

#[test]
fn mermaid_preview_is_stateless_and_total() {
    let r = ok(json!({"op": "mermaid.render", "source": "graph TD\nA-->"}));
    assert_eq!(r["kind"], "flowchart");
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(1));
    assert!(r["figure"]["width"].as_f64().is_some_and(|w| w > 0.0));
    let svg = ok(json!({"op": "mermaid.svg", "source": "pie\n\"x\": 2"}));
    assert!(svg.as_str().is_some_and(|s| s.starts_with("<svg")));
    let huge = "graph TD\n".to_owned() + &"A-->B\n".repeat(20_000);
    assert_eq!(err_kind(json!({"op": "mermaid.render", "source": huge})), "limit");
}
