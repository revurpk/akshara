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
    assert_eq!(err_kind(json!({"op": "new", "kind": "impress"})), "invalid");
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
