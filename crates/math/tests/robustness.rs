#![allow(clippy::unwrap_used)] // proptest bodies are tests too
//! Property tests: the parser and converters must be total (never panic) and
//! their output must always be well formed, whatever the input.

use office_math::{mathml_to_starmath, parse, to_mathml, Formula};
use office_odf::{xml, Limits};
use proptest::prelude::*;

/// Fragments that exercise every corner of the grammar when shuffled together.
const FRAGMENTS: &[&str] = &[
    "a", "b", "x", "12", "3.5", "+", "-", "*", "/", "^", "_", "=", "<", ">", "<=", ">=", "<>", "{", "}",
    "(", ")", "[", "]", "#", "##", "over", "sqrt", "nroot", "sum", "int", "lim", "from", "to", "left",
    "right", "none", "lbrace", "rbrace", "langle", "rangle", "mline", "matrix", "stack", "binom",
    "bold", "ital", "size", "color", "red", "font", "sans", "hat", "vec", "overline", "underline",
    "%alpha", "%OMEGA", "%bogus", "\"text\"", "newline", "~", "`", "<?>", "sin", "func", "abs",
    "fact", "neg", "csub", "csup", "lsub", "lsup", "overbrace", "underbrace", "boper", "uoper",
    "oper", "phantom", "alignl", "infinity", "times", "cdot", "in", "\\{", "\\", "%", "\"", "é", "∑",
    ",", ";", "!", "|", "&", "+-", "->",
];

fn fragment_soup() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(FRAGMENTS), 0..40).prop_map(|parts| parts.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(600))]

    #[test]
    fn parser_is_total_on_arbitrary_text(src in "\\PC{0,200}") {
        let parsed = parse(&src);
        prop_assert!(parsed.errors.len() <= 100);
    }

    #[test]
    fn grammar_soup_produces_well_formed_mathml(src in fragment_soup()) {
        let parsed = parse(&src);
        let mathml = to_mathml(&parsed.root, Some(&src));
        let root = xml::parse(&mathml, &Limits::default());
        prop_assert!(root.is_ok(), "malformed MathML for {:?}: {:?}", src, root.err());
        // The annotation must round-trip the source exactly, modulo XML's
        // dropping of characters that are illegal in XML.
        let root = root.unwrap();
        prop_assert_eq!(mathml_to_starmath(&root), src);
    }

    #[test]
    fn import_of_exported_mathml_is_total(src in fragment_soup()) {
        let parsed = parse(&src);
        let mathml = to_mathml(&parsed.root, None);
        let root = xml::parse(&mathml, &Limits::default()).unwrap();
        let imported = mathml_to_starmath(&root);
        // The imported source must itself be parseable without panicking.
        let _ = parse(&imported);
    }

    #[test]
    fn odf_round_trip(src in fragment_soup()) {
        let formula = Formula::new(src);
        let bytes = formula.to_odf_bytes().unwrap();
        let back = Formula::from_bytes(&bytes, &Limits::default()).unwrap();
        prop_assert_eq!(back.source, formula.source);
    }

    #[test]
    fn loader_is_total_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = Formula::from_bytes(&bytes, &Limits::default());
    }

    #[test]
    fn tree_serialises_to_json(src in fragment_soup()) {
        let parsed = parse(&src);
        let json = serde_json::to_string(&parsed);
        prop_assert!(json.is_ok());
    }
}
