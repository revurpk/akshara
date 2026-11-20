//! Markdown import never panics and always yields a valid document, whatever
//! a chat model or a file throws at it.

use office_writer::markdown::{from_markdown, markdown_blocks};
use office_writer::{save_odt, Block};
use proptest::prelude::*;

/// Characters that matter to the Markdown parser, plus Indic text.
fn markdownish() -> impl Strategy<Value = String> {
    let pieces = prop::sample::select(vec![
        "#", "## ", "* ", "- ", "+ ", "1. ", "2) ", "> ", "```", "```mermaid\n", "~~~", "|", "|---|", ":-:", "**", "*", "_", "__",
        "~~", "`", "[", "]", "(", ")", "![", "<", ">", "http://x.org", "\\", "\n", "\n\n", "  ", "\t", "[ ] ", "[x] ", "---", "===",
        "word", "తెలుగు", "हिन्दी", "graph TD; A-->B", "a_b_c", "2*3*4",
    ]);
    prop::collection::vec(pieces, 0..80).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn never_panics_and_saves(text in markdownish()) {
        let doc = from_markdown(&text, &Default::default()).expect("import");
        save_odt(&doc).expect("save");
    }

    #[test]
    fn arbitrary_unicode(text in "\\PC{0,300}") {
        let blocks = markdown_blocks(&text, 400.0);
        for b in &blocks {
            if let Block::Diagram(d) = b {
                prop_assert!(d.width <= 400.0 && d.width >= 72.0);
            }
        }
    }
}

#[test]
fn deep_nesting_and_long_lines_are_fine() {
    let nested: String = (0..200).map(|i| format!("{}- item {i}\n", " ".repeat(i * 2))).collect();
    let blocks = markdown_blocks(&nested, 400.0);
    assert_eq!(blocks.len(), 200);
    // A line of stars is a horizontal rule; stars after a word are text.
    let stars = "*".repeat(100_000);
    assert!(markdown_blocks(&stars, 400.0).is_empty());
    assert_eq!(markdown_blocks(&format!("a{stars}"), 400.0).len(), 1);
}
