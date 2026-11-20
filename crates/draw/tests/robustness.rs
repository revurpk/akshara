//! Mermaid rendering and drawing import never panic, whatever the input.

use proptest::prelude::*;

const HEADERS: &[&str] = &["graph TD", "flowchart LR", "sequenceDiagram", "classDiagram", "stateDiagram-v2", "pie", "graph BT", ""];

fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("-->".to_owned()),
        Just("-.->".to_owned()),
        Just("==>".to_owned()),
        Just("--o".to_owned()),
        Just("|lbl|".to_owned()),
        Just("[".to_owned()),
        Just("]".to_owned()),
        Just("((".to_owned()),
        Just("{".to_owned()),
        Just("}".to_owned()),
        Just("\"".to_owned()),
        Just("subgraph".to_owned()),
        Just("end".to_owned()),
        Just("[*]".to_owned()),
        Just("<|--".to_owned()),
        Just("->>".to_owned()),
        Just("loop".to_owned()),
        Just("alt".to_owned()),
        Just("else".to_owned()),
        Just("Note over".to_owned()),
        Just(":".to_owned()),
        Just(";".to_owned()),
        Just("&".to_owned()),
        Just("\n".to_owned()),
        Just("<br>".to_owned()),
        Just("#quot;".to_owned()),
        "[a-zA-Z0-9 ]{0,6}",
        "\\PC{0,3}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    #[test]
    fn mermaid_never_panics(header in prop::sample::select(HEADERS), parts in prop::collection::vec(fragment(), 0..60)) {
        let source = format!("{header}\n{}", parts.concat());
        let r = office_draw::mermaid::render(&source);
        prop_assert!(r.figure.width.is_finite() && r.figure.height.is_finite());
        let svg = r.figure.to_svg(None);
        prop_assert!(svg.starts_with("<svg"));
    }

    #[test]
    fn odg_import_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        let _ = office_draw::odg::load(&bytes, &office_odf::Limits::tiny());
    }
}

#[test]
fn pathological_diagrams_finish() {
    // A long chain, a dense graph and deep subgraph nesting.
    let chain: String = (0..2000).map(|i| format!("n{i} --> n{}\n", i + 1)).collect();
    let r = office_draw::mermaid::render(&format!("graph TD\n{chain}"));
    assert!(r.errors.is_empty());
    let dense: String = (0..300).map(|i| format!("a{} --> a{}\n", i % 40, (i * 7) % 40)).collect();
    office_draw::mermaid::render(&format!("graph LR\n{dense}"));
    let nested = format!("graph TD\n{}x-->y\n{}", "subgraph s\n".repeat(200), "end\n".repeat(200));
    office_draw::mermaid::render(&nested);
}
