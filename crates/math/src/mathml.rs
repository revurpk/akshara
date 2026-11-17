//! Conversion between the formula tree and presentation MathML.
//!
//! * [`to_mathml`] writes the MathML that OpenDocument formula files store in
//!   `content.xml` (with the StarMath source as an annotation, exactly like
//!   OpenOffice does).
//! * [`mathml_to_starmath`] converts foreign MathML (without annotation) into
//!   StarMath source so it can be edited.

use crate::ast::{Node, OpKind, Size};
use crate::symbols::{self, Keyword};
use office_odf::xml::{Element, Node as XmlNode, XmlWriter};

/// MathML namespace URI.
pub const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
/// Annotation encoding used for the StarMath source.
pub const STARMATH_ENCODING: &str = "StarMath 5.0";

/// Serialises a formula tree as a complete MathML document.
///
/// When `annotation` is given it is embedded as the `StarMath 5.0` annotation.
pub fn to_mathml(root: &Node, annotation: Option<&str>) -> String {
    let mut w = XmlWriter::new();
    w.start("math").attr("xmlns", MATHML_NS).attr("display", "block");
    w.start("semantics");
    // <semantics> needs exactly one presentation child.
    match root {
        Node::Row { .. } => write_node(&mut w, root),
        other => {
            w.start("mrow");
            write_node(&mut w, other);
            w.end();
        }
    }
    if let Some(source) = annotation {
        w.start("annotation").attr("encoding", STARMATH_ENCODING).text(source).end();
    }
    w.finish()
}

fn token(w: &mut XmlWriter, name: &str, text: &str) {
    w.start(name).text(text).end();
}

fn write_opt(w: &mut XmlWriter, node: &Option<Box<Node>>) {
    match node {
        Some(n) => write_node(w, n),
        None => {
            w.start("none").end();
        }
    }
}

fn fence(w: &mut XmlWriter, text: &str, scalable: bool) {
    if !text.is_empty() {
        w.start("mo")
            .attr("fence", "true")
            .attr("stretchy", if scalable { "true" } else { "false" })
            .text(text)
            .end();
    }
}

fn write_node(w: &mut XmlWriter, node: &Node) {
    match node {
        Node::Row { items } => {
            w.start("mrow");
            for item in items {
                write_node(w, item);
            }
            w.end();
        }
        Node::Lines { rows } => {
            w.start("mtable").attr("columnalign", "left");
            for row in rows {
                w.start("mtr").start("mtd");
                write_node(w, row);
                w.end().end();
            }
            w.end();
        }
        Node::Ident { text, italic } => {
            let single = text.chars().count() == 1;
            w.start("mi");
            match (italic, single) {
                (true, false) => {
                    w.attr("mathvariant", "italic");
                }
                (false, true) => {
                    w.attr("mathvariant", "normal");
                }
                _ => {}
            }
            w.text(text).end();
        }
        Node::Number { text } => token(w, "mn", text),
        Node::Op { text, kind } => {
            w.start("mo").attr("stretchy", "false");
            if *kind == OpKind::Prefix {
                w.attr("form", "prefix");
            }
            w.text(text).end();
        }
        Node::Text { text } => token(w, "mtext", text),
        Node::Func { name } => {
            w.start("mi").attr("mathvariant", "normal").text(name).end();
        }
        Node::BigOp { text } => {
            w.start("mo").attr("largeop", "true").text(text).end();
        }
        Node::Frac { num, den } => {
            w.start("mfrac");
            write_node(w, num);
            write_node(w, den);
            w.end();
        }
        Node::Script { base, sub, sup, lsub, lsup } => {
            if lsub.is_some() || lsup.is_some() {
                w.start("mmultiscripts");
                write_node(w, base);
                write_opt(w, sub);
                write_opt(w, sup);
                w.start("mprescripts").end();
                write_opt(w, lsub);
                write_opt(w, lsup);
                w.end();
            } else {
                let name = match (sub, sup) {
                    (Some(_), Some(_)) => "msubsup",
                    (Some(_), None) => "msub",
                    _ => "msup",
                };
                w.start(name);
                write_node(w, base);
                for script in [sub, sup].into_iter().flatten() {
                    write_node(w, script);
                }
                w.end();
            }
        }
        Node::UnderOver { base, under, over } => {
            let name = match (under, over) {
                (Some(_), Some(_)) => "munderover",
                (Some(_), None) => "munder",
                _ => "mover",
            };
            w.start(name);
            write_node(w, base);
            for limit in [under, over].into_iter().flatten() {
                write_node(w, limit);
            }
            w.end();
        }
        Node::Sqrt { body } => {
            w.start("msqrt");
            write_node(w, body);
            w.end();
        }
        Node::Root { index, body } => {
            w.start("mroot");
            write_node(w, body);
            write_node(w, index);
            w.end();
        }
        Node::Fenced { open, close, body, scalable } => {
            w.start("mrow");
            fence(w, open, *scalable);
            write_node(w, body);
            fence(w, close, *scalable);
            w.end();
        }
        Node::Matrix { rows } => {
            w.start("mtable");
            for row in rows {
                w.start("mtr");
                for cell in row {
                    w.start("mtd");
                    write_node(w, cell);
                    w.end();
                }
                w.end();
            }
            w.end();
        }
        Node::Accent { body, accent, stretchy, under } => {
            if *under {
                w.start("munder").attr("accentunder", "true");
            } else {
                w.start("mover").attr("accent", "true");
            }
            write_node(w, body);
            w.start("mo").attr("stretchy", if *stretchy { "true" } else { "false" }).text(accent).end();
            w.end();
        }
        Node::Styled { body, bold, italic, color, size, font } => {
            w.start("mstyle");
            let variant = match (bold, italic, font.as_deref()) {
                (Some(true), Some(true), _) => Some("bold-italic"),
                (Some(true), _, Some("sans")) => Some("bold-sans-serif"),
                (Some(true), _, _) => Some("bold"),
                (_, Some(true), Some("sans")) => Some("sans-serif-italic"),
                (_, Some(true), _) => Some("italic"),
                (_, _, Some("sans")) => Some("sans-serif"),
                (_, _, Some("fixed")) => Some("monospace"),
                (Some(false), _, _) | (_, Some(false), _) => Some("normal"),
                _ => None,
            };
            if let Some(variant) = variant {
                w.attr("mathvariant", variant);
            }
            if let Some(color) = color {
                w.attr("mathcolor", color);
            }
            match size {
                Some(Size::Abs(pt)) => {
                    w.attr("mathsize", &format!("{pt}pt"));
                }
                Some(Size::Mul(factor)) => {
                    w.attr("mathsize", &format!("{}%", (factor * 100.0).round()));
                }
                Some(Size::Add(_)) | None => {}
            }
            write_node(w, body);
            w.end();
        }
        Node::Phantom { body } => {
            w.start("mphantom");
            write_node(w, body);
            w.end();
        }
        Node::Brace { body, label, over } => {
            let (outer, brace) = if *over { ("mover", "⏞") } else { ("munder", "⏟") };
            w.start(outer).start(outer);
            write_node(w, body);
            w.start("mo").attr("stretchy", "true").text(brace).end();
            w.end();
            write_node(w, label);
            w.end();
        }
        Node::Space { width } => {
            w.start("mspace").attr("width", &format!("{width}em")).end();
        }
        Node::Placeholder => {
            w.start("mi").attr("mathvariant", "normal").text("□").end();
        }
    }
}

/// Words that can be written verbatim to produce an operator symbol.
const OPERATOR_WORDS: &[&str] = &[
    "neq", "le", "ge", "leslant", "geslant", "ll", "gg", "approx", "sim", "simeq", "equiv", "prop",
    "parallel", "ortho", "divides", "ndivides", "toward", "dlarrow", "dlrarrow", "drarrow", "in",
    "notin", "owns", "subset", "subseteq", "supset", "supseteq", "nsubset", "nsubseteq", "nsupset",
    "nsupseteq", "def", "prec", "succ", "preccurlyeq", "succcurlyeq", "precsim", "succsim",
    "nprec", "nsucc", "plusminus", "minusplus", "or", "union", "oplus", "ominus", "cdot", "times",
    "div", "and", "intersection", "setminus", "circ", "otimes", "odot", "odivide", "infinity",
    "partial", "nabla", "exists", "notexists", "forall", "aleph", "emptyset", "hbar", "leftarrow",
    "uparrow", "downarrow", "dotsaxis", "dotslow", "dotsvert", "dotsup", "dotsdown", "setN",
    "setZ", "setQ", "setR", "setC", "sum", "prod", "coprod", "int", "iint", "iiint", "lint",
    "llint", "lllint",
];

/// Finds the StarMath word that produces the symbol `ch`, if any.
fn word_for_symbol(ch: &str) -> Option<&'static str> {
    OPERATOR_WORDS.iter().copied().find(|word| match symbols::keyword(word) {
        Some(
            Keyword::Rel(s) | Keyword::Sum(s) | Keyword::Prod(s) | Keyword::Symbol(s, _) | Keyword::BigOp(s, _),
        ) => s == ch,
        _ => false,
    })
}

fn local(e: &Element) -> &str {
    e.name.strip_prefix("math:").unwrap_or(&e.name)
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "'"))
}

fn group(s: &str) -> String {
    format!("{{{s}}}")
}

/// Converts the children of `e`, separated by spaces.
fn children(e: &Element) -> Vec<String> {
    e.elements().map(convert).filter(|s| !s.is_empty()).collect()
}

/// The `i`-th child element converted and wrapped in braces (`{}` if absent).
fn arg(e: &Element, i: usize) -> String {
    group(&e.elements().nth(i).map(convert).unwrap_or_default())
}

fn is_big_operator(e: &Element) -> Option<&'static str> {
    if local(e) != "mo" && local(e) != "mi" {
        return None;
    }
    let text = e.text_content();
    let text = text.trim();
    if text == "lim" {
        return Some("lim");
    }
    word_for_symbol(text).filter(|w| matches!(symbols::keyword(w), Some(Keyword::BigOp(..))))
}

fn bracket_word(ch: &str, opening: bool) -> String {
    match ch {
        "" => "none".into(),
        "(" | ")" | "[" | "]" => ch.into(),
        "{" => "lbrace".into(),
        "}" => "rbrace".into(),
        "⟨" | "〈" => "langle".into(),
        "⟩" | "〉" => "rangle".into(),
        "⌈" => "lceil".into(),
        "⌉" => "rceil".into(),
        "⌊" => "lfloor".into(),
        "⌋" => "rfloor".into(),
        "‖" => if opening { "ldline" } else { "rdline" }.into(),
        "|" => if opening { "lline" } else { "rline" }.into(),
        _ => "none".into(),
    }
}

fn convert_operator(text: &str) -> String {
    match text {
        "" | "\u{2061}" | "\u{2062}" | "\u{2063}" | "\u{2064}" => String::new(),
        "+" | "=" | "<" | ">" | "/" | "," | ";" | ":" | "!" | "." | "(" | ")" | "[" | "]" => text.into(),
        "-" | "−" => "-".into(),
        "*" | "∗" => "*".into(),
        "≤" => "<=".into(),
        "≥" => ">=".into(),
        "≠" => "<>".into(),
        "±" => "+-".into(),
        "∓" => "-+".into(),
        "·" => "cdot".into(),
        "→" => "toward".into(),
        "{" => "\\{".into(),
        "}" => "\\}".into(),
        "|" => "\\|".into(),
        "¬" => "neg".into(),
        other => match word_for_symbol(other) {
            Some(word) => word.into(),
            None if other.chars().count() == 1 && !other.is_ascii() => other.into(),
            None => quoted(other),
        },
    }
}

fn convert_identifier(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    if let Some(name) = symbols::special_name(text) {
        return format!("%{name}");
    }
    if let Some(word) = word_for_symbol(text) {
        return word.into();
    }
    let plain_word = text.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && text.chars().all(|c| c.is_ascii_alphanumeric());
    if plain_word {
        return match symbols::keyword(text) {
            None | Some(Keyword::Func(_)) => text.into(),
            Some(Keyword::BigOp(..)) if text.starts_with("lim") => text.into(),
            Some(_) => quoted(text),
        };
    }
    if text.chars().count() == 1 && !text.is_ascii() {
        return text.into();
    }
    quoted(text)
}

fn accent_word(ch: &str, stretchy: bool) -> Option<&'static str> {
    Some(match (ch, stretchy) {
        ("^" | "ˆ" | "̂", false) => "hat",
        ("^" | "ˆ" | "̂", true) => "widehat",
        ("~" | "˜" | "̃", false) => "tilde",
        ("~" | "˜" | "̃", true) => "widetilde",
        ("→" | "⃗", false) => "vec",
        ("→" | "⃗", true) => "widevec",
        ("¯" | "‾" | "̄" | "_", true) => "overline",
        ("¯" | "‾" | "̄", false) => "bar",
        ("˙" | "̇", _) => "dot",
        ("¨" | "̈", _) => "ddot",
        ("´", _) => "acute",
        ("`", _) => "grave",
        ("ˇ", _) => "check",
        ("˘", _) => "breve",
        ("˚", _) => "circle",
        _ => return None,
    })
}

/// Converts one MathML element to StarMath source.
///
/// Recursion depth equals the XML depth, which the parser bounds.
fn convert(e: &Element) -> String {
    let text = || e.text_content().trim().to_owned();
    match local(e) {
        "math" | "mstyle" | "mpadded" | "merror" | "mtd" | "mrow" | "semantics" => {
            let parts = children(e);
            match parts.len() {
                1 => parts.into_iter().next().unwrap_or_default(),
                _ if local(e) == "mrow" => group(&parts.join(" ")),
                _ => parts.join(" "),
            }
        }
        "annotation" | "annotation-xml" | "none" | "mprescripts" => String::new(),
        "mi" => convert_identifier(&text()),
        "mn" => {
            let t = text();
            let numeric = !t.is_empty()
                && t.chars().all(|c| c.is_ascii_digit() || c == '.')
                && t.chars().next().is_some_and(|c| c.is_ascii_digit())
                && t.matches('.').count() <= 1
                && !t.ends_with('.');
            if numeric { t } else { quoted(&t) }
        }
        "mo" => convert_operator(&text()),
        "mtext" | "ms" => quoted(&e.text_content()),
        "mspace" => "~".into(),
        "mfrac" => format!("{} over {}", arg(e, 0), arg(e, 1)),
        "msup" => format!("{}^{}", arg(e, 0), arg(e, 1)),
        "msub" => format!("{}_{}", arg(e, 0), arg(e, 1)),
        "msubsup" => format!("{}_{}^{}", arg(e, 0), arg(e, 1), arg(e, 2)),
        "msqrt" => format!("sqrt{}", group(&children(e).join(" "))),
        "mroot" => format!("nroot{}{}", arg(e, 1), arg(e, 0)),
        "mphantom" => format!("phantom{}", group(&children(e).join(" "))),
        "munder" | "mover" | "munderover" => {
            let base = e.elements().next();
            if let Some(word) = base.and_then(is_big_operator) {
                return match local(e) {
                    "munder" => format!("{word} from{}", arg(e, 1)),
                    "mover" => format!("{word} to{}", arg(e, 1)),
                    _ => format!("{word} from{} to{}", arg(e, 1), arg(e, 2)),
                };
            }
            if local(e) != "munderover" {
                if let Some(mark) = e.elements().nth(1).filter(|m| local(m) == "mo") {
                    let stretchy = mark.attr("stretchy") == Some("true");
                    let ch = mark.text_content();
                    if local(e) == "munder" && matches!(ch.trim(), "_" | "̲") {
                        return format!("underline{}", arg(e, 0));
                    }
                    if local(e) == "mover" {
                        if let Some(word) = accent_word(ch.trim(), stretchy) {
                            return format!("{word}{}", arg(e, 0));
                        }
                    }
                }
            }
            match local(e) {
                "munder" => format!("{} csub {}", arg(e, 0), arg(e, 1)),
                "mover" => format!("{} csup {}", arg(e, 0), arg(e, 1)),
                _ => format!("{} csub {} csup {}", arg(e, 0), arg(e, 1), arg(e, 2)),
            }
        }
        "mmultiscripts" => {
            let mut out = arg(e, 0);
            let mut pre = false;
            let mut index = 0;
            for child in e.elements().skip(1) {
                if local(child) == "mprescripts" {
                    pre = true;
                    index = 0;
                    continue;
                }
                if local(child) != "none" {
                    let op = match (pre, index % 2 == 0) {
                        (false, true) => "_",
                        (false, false) => "^",
                        (true, true) => " lsub ",
                        (true, false) => " lsup ",
                    };
                    out.push_str(op);
                    out.push_str(&group(&convert(child)));
                }
                index += 1;
            }
            out
        }
        "mfenced" => {
            let open = e.attr("open").unwrap_or("(");
            let close = e.attr("close").unwrap_or(")");
            format!(
                "left {} {} right {}",
                bracket_word(open, true),
                children(e).join(" , "),
                bracket_word(close, false)
            )
        }
        "mtable" => {
            let rows: Vec<String> = e
                .elements()
                .filter(|r| local(r) == "mtr" || local(r) == "mlabeledtr")
                .map(|r| r.elements().map(convert).collect::<Vec<_>>().join(" # "))
                .collect();
            format!("matrix{{{}}}", rows.join(" ## "))
        }
        _ => children(e).join(" "),
    }
}

/// Extracts the StarMath annotation from a parsed `<math>` element, if any.
pub fn starmath_annotation(math: &Element) -> Option<String> {
    let semantics = math.child("math:semantics")?;
    semantics
        .children_named("math:annotation")
        .find(|a| a.attr("encoding").is_some_and(|enc| enc.starts_with("StarMath")))
        .map(|a| {
            a.children
                .iter()
                .filter_map(|n| match n {
                    XmlNode::Text(t) => Some(t.as_str()),
                    XmlNode::Element(_) => None,
                })
                .collect::<String>()
        })
}

/// Converts a parsed `<math>` element to StarMath source.
///
/// The embedded `StarMath 5.0` annotation is used when present (lossless);
/// otherwise the presentation markup is translated element by element.
pub fn mathml_to_starmath(math: &Element) -> String {
    if let Some(source) = starmath_annotation(math) {
        return source;
    }
    let out = convert(math);
    // A lone top-level group does not need its braces.
    match out.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(inner) if balanced(inner) => inner.trim().to_owned(),
        _ => out,
    }
}

/// True when braces in `s` are balanced and never close below depth zero.
fn balanced(s: &str) -> bool {
    let mut depth = 0i64;
    let mut in_text = false;
    let mut prev = ' ';
    for c in s.chars() {
        match c {
            '"' => in_text = !in_text,
            '{' if !in_text && prev != '\\' => depth += 1,
            '}' if !in_text && prev != '\\' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
        prev = c;
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use office_odf::{xml, Limits};

    fn mathml(src: &str) -> String {
        to_mathml(&parse(src).root, Some(src))
    }

    fn reparse(xml_text: &str) -> Element {
        xml::parse(xml_text, &Limits::default()).expect("generated MathML must be well formed")
    }

    #[test]
    fn output_is_well_formed_with_annotation() {
        let xml_text = mathml("E = m c^2 & \"<tag>\"");
        let root = reparse(&xml_text);
        assert_eq!(root.name, "math:math");
        assert_eq!(starmath_annotation(&root).as_deref(), Some("E = m c^2 & \"<tag>\""));
        assert_eq!(mathml_to_starmath(&root), "E = m c^2 & \"<tag>\"");
    }

    #[test]
    fn element_mapping() {
        let cases = [
            ("a over b", "<mfrac><mi>a</mi><mi>b</mi></mfrac>"),
            ("x^2", "<msup><mi>x</mi><mn>2</mn></msup>"),
            ("x_1", "<msub><mi>x</mi><mn>1</mn></msub>"),
            ("x_1^2", "<msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup>"),
            ("sqrt x", "<msqrt><mi>x</mi></msqrt>"),
            ("nroot 3 x", "<mroot><mi>x</mi><mn>3</mn></mroot>"),
            ("sin", "<mi mathvariant=\"normal\">sin</mi>"),
            ("%alpha", "<mi mathvariant=\"normal\">α</mi>"),
            ("%ialpha", "<mi>α</mi>"),
            ("abc", "<mi mathvariant=\"italic\">abc</mi>"),
            ("\"t\"", "<mtext>t</mtext>"),
            ("phantom x", "<mphantom><mi>x</mi></mphantom>"),
            ("bold x", "<mstyle mathvariant=\"bold\"><mi>x</mi></mstyle>"),
            ("color red x", "<mstyle mathcolor=\"#ff0000\"><mi>x</mi></mstyle>"),
            ("x lsub a", "<mmultiscripts><mi>x</mi><none/><none/><mprescripts/><mi>a</mi><none/></mmultiscripts>"),
            ("matrix{a # b ## c # d}", "<mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr></mtable>"),
            ("hat a", "<mover accent=\"true\"><mi>a</mi><mo stretchy=\"false\">^</mo></mover>"),
        ];
        for (src, expected) in cases {
            let xml_text = mathml(src);
            assert!(xml_text.contains(expected), "{src}: {xml_text}");
            reparse(&xml_text);
        }
    }

    fn import(body: &str) -> String {
        let doc = format!("<math xmlns=\"{MATHML_NS}\">{body}</math>");
        mathml_to_starmath(&reparse(&doc))
    }

    #[test]
    fn imports_presentation_mathml() {
        assert_eq!(import("<mfrac><mi>a</mi><mi>b</mi></mfrac>"), "{a} over {b}");
        assert_eq!(import("<msup><mi>x</mi><mn>2</mn></msup>"), "{x}^{2}");
        assert_eq!(import("<mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow>"), "a + b");
        assert_eq!(import("<mrow><mi>α</mi><mo>≤</mo><mi>∞</mi></mrow>"), "%alpha <= infinity");
        assert_eq!(import("<msqrt><mi>x</mi><mo>−</mo><mn>1</mn></msqrt>"), "sqrt{x - 1}");
        assert_eq!(import("<mroot><mi>x</mi><mn>3</mn></mroot>"), "nroot{3}{x}");
        assert_eq!(
            import("<munderover><mo>∑</mo><mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow><mi>n</mi></munderover>"),
            "sum from{{i = 1}} to{n}"
        );
        assert_eq!(import("<mfenced><mi>a</mi><mi>b</mi></mfenced>"), "left ( a , b right )");
        assert_eq!(import("<mtext>if \"x\"</mtext>"), "\"if 'x'\"");
        assert_eq!(import("<mi>in</mi>"), "\"in\"");
        assert_eq!(import("<mi>sin</mi>"), "sin");
        assert_eq!(import("<mover accent=\"true\"><mi>a</mi><mo>^</mo></mover>"), "hat{a}");
        assert_eq!(
            import("<mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr><mtr><mtd><mn>3</mn></mtd><mtd><mn>4</mn></mtd></mtr></mtable>"),
            "matrix{1 # 2 ## 3 # 4}"
        );
    }

    #[test]
    fn imported_source_parses_cleanly() {
        for body in [
            "<mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mi>c</mi></mfrac>",
            "<msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup>",
            "<munderover><mo>∫</mo><mn>0</mn><mn>1</mn></munderover>",
            "<mrow><mo>(</mo><mi>a</mi><mo>)</mo></mrow>",
            "<mmultiscripts><mi>X</mi><mn>1</mn><mn>2</mn><mprescripts/><mn>3</mn><none/></mmultiscripts>",
            "<mn>1e5</mn>",
        ] {
            let src = import(body);
            let parsed = parse(&src);
            assert!(parsed.errors.is_empty(), "{body} -> {src:?}: {:?}", parsed.errors);
        }
    }

    #[test]
    fn export_then_import_without_annotation_is_stable() {
        for src in ["a over b", "x^2 + y_1", "sqrt{a + b}", "sum from{i = 1} to{n} i", "matrix{1 # 2 ## 3 # 4}"] {
            let first = to_mathml(&parse(src).root, None);
            let imported = mathml_to_starmath(&reparse(&first));
            let second = to_mathml(&parse(&imported).root, None);
            let imported_again = mathml_to_starmath(&reparse(&second));
            assert_eq!(imported, imported_again, "source {src:?}");
            assert!(parse(&imported).errors.is_empty(), "{imported:?}");
        }
    }
}
