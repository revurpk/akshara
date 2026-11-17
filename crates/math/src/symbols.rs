//! Keyword and symbol tables of the StarMath command language.

/// Which script slot a sub/superscript keyword addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Right subscript (`_`, `sub`, `rsub`).
    Sub,
    /// Right superscript (`^`, `sup`, `rsup`).
    Sup,
    /// Left subscript (`lsub`).
    LSub,
    /// Left superscript (`lsup`).
    LSup,
    /// Centred below (`csub`).
    Under,
    /// Centred above (`csup`).
    Over,
}

/// Font attribute keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontAttr {
    /// `bold`
    Bold,
    /// `nbold`
    NoBold,
    /// `ital` / `italic`
    Italic,
    /// `nitalic`
    NoItalic,
    /// `phantom`
    Phantom,
    /// `size`
    Size,
    /// `color`
    Color,
    /// `font`
    Font,
}

/// Classification of a keyword.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Keyword {
    /// Relation operator with its symbol.
    Rel(&'static str),
    /// Additive operator with its symbol.
    Sum(&'static str),
    /// Multiplicative operator with its symbol.
    Prod(&'static str),
    /// `over`
    Over,
    /// `overbrace` (true) / `underbrace` (false)
    Brace(bool),
    /// Sub/superscript keyword.
    Script(Slot),
    /// Opening bracket keyword with its symbol.
    Open(&'static str),
    /// Closing bracket keyword with its symbol.
    Close(&'static str),
    /// `left`
    Left,
    /// `right`
    Right,
    /// `none` (invisible bracket)
    NoBracket,
    /// `mline`
    MLine,
    /// Function name set upright.
    Func(&'static str),
    /// Large operator: symbol and whether `_`/`^` scripts go below/above.
    BigOp(&'static str, bool),
    /// Accent: symbol, stretchy, placed under.
    Accent(&'static str, bool, bool),
    /// `overstrike`
    Overstrike,
    /// Font attribute.
    Font(FontAttr),
    /// `sqrt`
    Sqrt,
    /// `nroot`
    NRoot,
    /// `abs`
    Abs,
    /// `fact`
    Fact,
    /// `binom`
    Binom,
    /// `stack`
    Stack,
    /// `matrix`
    Matrix,
    /// `newline`
    Newline,
    /// `alignl` / `alignc` / `alignr`
    Align,
    /// `from`
    From,
    /// `to`
    To,
    /// `neg`
    Neg,
    /// `uoper`
    UOper,
    /// `boper`
    BOper,
    /// `oper`
    Oper,
    /// `func`
    FuncKw,
    /// Ordinary symbol: text and whether it is italic.
    Symbol(&'static str, bool),
}

/// Looks up a (case sensitive) keyword.
pub fn keyword(word: &str) -> Option<Keyword> {
    use Keyword::*;
    Some(match word {
        // Relations
        "neq" => Rel("≠"),
        "lt" => Rel("<"),
        "gt" => Rel(">"),
        "le" => Rel("≤"),
        "ge" => Rel("≥"),
        "leslant" => Rel("⩽"),
        "geslant" => Rel("⩾"),
        "ll" => Rel("≪"),
        "gg" => Rel("≫"),
        "approx" => Rel("≈"),
        "sim" => Rel("∼"),
        "simeq" => Rel("≃"),
        "equiv" => Rel("≡"),
        "prop" => Rel("∝"),
        "parallel" => Rel("∥"),
        "ortho" => Rel("⊥"),
        "divides" => Rel("∣"),
        "ndivides" => Rel("∤"),
        "toward" => Rel("→"),
        "dlarrow" => Rel("⇐"),
        "dlrarrow" => Rel("⇔"),
        "drarrow" => Rel("⇒"),
        "in" => Rel("∈"),
        "notin" => Rel("∉"),
        "owns" | "ni" => Rel("∋"),
        "subset" => Rel("⊂"),
        "subseteq" => Rel("⊆"),
        "supset" => Rel("⊃"),
        "supseteq" => Rel("⊇"),
        "nsubset" => Rel("⊄"),
        "nsubseteq" => Rel("⊈"),
        "nsupset" => Rel("⊅"),
        "nsupseteq" => Rel("⊉"),
        "def" => Rel("≝"),
        "transl" => Rel("⊷"),
        "transr" => Rel("⊶"),
        "prec" => Rel("≺"),
        "succ" => Rel("≻"),
        "preccurlyeq" => Rel("≼"),
        "succcurlyeq" => Rel("≽"),
        "precsim" => Rel("≾"),
        "succsim" => Rel("≿"),
        "nprec" => Rel("⊀"),
        "nsucc" => Rel("⊁"),
        // Additive
        "plusminus" => Sum("±"),
        "minusplus" => Sum("∓"),
        "or" => Sum("∨"),
        "union" | "cup" => Sum("∪"),
        "oplus" => Sum("⊕"),
        "ominus" => Sum("⊖"),
        // Multiplicative
        "cdot" => Prod("⋅"),
        "times" => Prod("×"),
        "div" => Prod("÷"),
        "and" => Prod("∧"),
        "intersection" | "cap" => Prod("∩"),
        "setminus" | "bslash" => Prod("∖"),
        "slash" => Prod("/"),
        "circ" => Prod("∘"),
        "otimes" => Prod("⊗"),
        "odot" => Prod("⊙"),
        "odivide" => Prod("⊘"),
        "wideslash" => Prod("∕"),
        "widebslash" => Prod("∖"),
        "over" => Over,
        "overbrace" => Brace(true),
        "underbrace" => Brace(false),
        // Scripts
        "sub" | "rsub" => Script(Slot::Sub),
        "sup" | "rsup" => Script(Slot::Sup),
        "lsub" => Script(Slot::LSub),
        "lsup" => Script(Slot::LSup),
        "csub" => Script(Slot::Under),
        "csup" => Script(Slot::Over),
        // Brackets
        "lbrace" => Open("{"),
        "rbrace" => Close("}"),
        "langle" => Open("⟨"),
        "rangle" => Close("⟩"),
        "lceil" => Open("⌈"),
        "rceil" => Close("⌉"),
        "lfloor" => Open("⌊"),
        "rfloor" => Close("⌋"),
        "lline" => Open("|"),
        "rline" => Close("|"),
        "ldline" => Open("‖"),
        "rdline" => Close("‖"),
        "ldbracket" => Open("⟦"),
        "rdbracket" => Close("⟧"),
        "left" => Left,
        "right" => Right,
        "none" => NoBracket,
        "mline" => MLine,
        // Functions
        "sin" => Func("sin"),
        "cos" => Func("cos"),
        "tan" => Func("tan"),
        "cot" => Func("cot"),
        "sinh" => Func("sinh"),
        "cosh" => Func("cosh"),
        "tanh" => Func("tanh"),
        "coth" => Func("coth"),
        "arcsin" => Func("arcsin"),
        "arccos" => Func("arccos"),
        "arctan" => Func("arctan"),
        "arccot" => Func("arccot"),
        "arsinh" => Func("arsinh"),
        "arcosh" => Func("arcosh"),
        "artanh" => Func("artanh"),
        "arcoth" => Func("arcoth"),
        "ln" => Func("ln"),
        "log" => Func("log"),
        "exp" => Func("exp"),
        // Large operators
        "sum" => BigOp("∑", true),
        "prod" => BigOp("∏", true),
        "coprod" => BigOp("∐", true),
        "int" => BigOp("∫", false),
        "iint" => BigOp("∬", false),
        "iiint" => BigOp("∭", false),
        "lint" => BigOp("∮", false),
        "llint" => BigOp("∯", false),
        "lllint" => BigOp("∰", false),
        "lim" => BigOp("lim", true),
        "liminf" => BigOp("lim inf", true),
        "limsup" => BigOp("lim sup", true),
        // Accents
        "acute" => Accent("´", false, false),
        "grave" => Accent("`", false, false),
        "check" => Accent("ˇ", false, false),
        "breve" => Accent("˘", false, false),
        "circle" => Accent("˚", false, false),
        "dot" => Accent("˙", false, false),
        "ddot" => Accent("¨", false, false),
        "dddot" => Accent("⃛", false, false),
        "bar" => Accent("¯", false, false),
        "vec" => Accent("→", false, false),
        "tilde" => Accent("~", false, false),
        "hat" => Accent("^", false, false),
        "widevec" => Accent("→", true, false),
        "widetilde" => Accent("~", true, false),
        "widehat" => Accent("^", true, false),
        "overline" => Accent("¯", true, false),
        "underline" => Accent("_", true, true),
        "overstrike" => Overstrike,
        // Font attributes
        "bold" => Font(FontAttr::Bold),
        "nbold" => Font(FontAttr::NoBold),
        "ital" | "italic" => Font(FontAttr::Italic),
        "nitalic" => Font(FontAttr::NoItalic),
        "phantom" => Font(FontAttr::Phantom),
        "size" => Font(FontAttr::Size),
        "color" => Font(FontAttr::Color),
        "font" => Font(FontAttr::Font),
        // Structures
        "sqrt" => Sqrt,
        "nroot" => NRoot,
        "abs" => Abs,
        "fact" => Fact,
        "binom" => Binom,
        "stack" => Stack,
        "matrix" => Matrix,
        "newline" => Newline,
        "alignl" | "alignc" | "alignr" | "alignm" | "alignt" | "alignb" => Align,
        "from" => From,
        "to" => To,
        "neg" => Neg,
        "uoper" => UOper,
        "boper" => BOper,
        "oper" => Oper,
        "func" => FuncKw,
        // Symbols
        "infinity" | "infty" => Symbol("∞", false),
        "partial" => Symbol("∂", false),
        "nabla" => Symbol("∇", false),
        "exists" => Symbol("∃", false),
        "notexists" => Symbol("∄", false),
        "forall" => Symbol("∀", false),
        "aleph" => Symbol("ℵ", false),
        "emptyset" => Symbol("∅", false),
        "hbar" => Symbol("ℏ", false),
        "lambdabar" => Symbol("ƛ", false),
        "re" | "Re" => Symbol("ℜ", false),
        "im" | "Im" => Symbol("ℑ", false),
        "wp" => Symbol("℘", false),
        "leftarrow" => Symbol("←", false),
        "rightarrow" => Symbol("→", false),
        "uparrow" => Symbol("↑", false),
        "downarrow" => Symbol("↓", false),
        "dotsaxis" => Symbol("⋯", false),
        "dotslow" => Symbol("…", false),
        "dotsvert" => Symbol("⋮", false),
        "dotsup" | "dotsdiag" => Symbol("⋰", false),
        "dotsdown" => Symbol("⋱", false),
        "backepsilon" => Symbol("϶", false),
        "setN" => Symbol("ℕ", false),
        "setZ" => Symbol("ℤ", false),
        "setQ" => Symbol("ℚ", false),
        "setR" => Symbol("ℝ", false),
        "setC" => Symbol("ℂ", false),
        _ => return None,
    })
}

/// Lower-case Greek letters: `(name, character)`.
const GREEK_LOWER: &[(&str, &str)] = &[
    ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"), ("epsilon", "ϵ"),
    ("zeta", "ζ"), ("eta", "η"), ("theta", "θ"), ("iota", "ι"), ("kappa", "κ"),
    ("lambda", "λ"), ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"), ("omicron", "ο"),
    ("pi", "π"), ("rho", "ρ"), ("sigma", "σ"), ("tau", "τ"), ("upsilon", "υ"),
    ("phi", "ϕ"), ("chi", "χ"), ("psi", "ψ"), ("omega", "ω"),
    ("varepsilon", "ε"), ("vartheta", "ϑ"), ("varpi", "ϖ"), ("varrho", "ϱ"),
    ("varsigma", "ς"), ("varphi", "φ"),
];

/// Upper-case Greek letters: `(name, character)`.
const GREEK_UPPER: &[(&str, &str)] = &[
    ("ALPHA", "Α"), ("BETA", "Β"), ("GAMMA", "Γ"), ("DELTA", "Δ"), ("EPSILON", "Ε"),
    ("ZETA", "Ζ"), ("ETA", "Η"), ("THETA", "Θ"), ("IOTA", "Ι"), ("KAPPA", "Κ"),
    ("LAMBDA", "Λ"), ("MU", "Μ"), ("NU", "Ν"), ("XI", "Ξ"), ("OMICRON", "Ο"),
    ("PI", "Π"), ("RHO", "Ρ"), ("SIGMA", "Σ"), ("TAU", "Τ"), ("UPSILON", "Υ"),
    ("PHI", "Φ"), ("CHI", "Χ"), ("PSI", "Ψ"), ("OMEGA", "Ω"),
];

/// Other `%name` symbols: `(name, character)`.
const SPECIAL: &[(&str, &str)] = &[
    ("element", "∈"), ("noelement", "∉"), ("strictlylessthan", "≪"),
    ("strictlygreaterthan", "≫"), ("notequal", "≠"), ("identical", "≡"),
    ("tendto", "→"), ("infinite", "∞"), ("angle", "∢"), ("perthousand", "‰"),
    ("and", "∧"), ("or", "∨"),
];

/// Resolves a `%name` symbol to `(character, italic)`.
///
/// Greek letters are upright unless the name carries the `i` prefix
/// (`%ialpha`), matching OpenOffice.
pub fn special(name: &str) -> Option<(&'static str, bool)> {
    let find = |n: &str| {
        GREEK_LOWER
            .iter()
            .chain(GREEK_UPPER)
            .chain(SPECIAL)
            .find(|(k, _)| *k == n)
            .map(|(_, v)| *v)
    };
    if let Some(c) = find(name) {
        return Some((c, false));
    }
    let rest = name.strip_prefix('i')?;
    GREEK_LOWER.iter().chain(GREEK_UPPER).find(|(k, _)| *k == rest).map(|(_, v)| (*v, true))
}

/// Reverse lookup used by the MathML importer: character to `%name`.
pub fn special_name(ch: &str) -> Option<&'static str> {
    GREEK_LOWER.iter().chain(GREEK_UPPER).find(|(_, v)| *v == ch).map(|(k, _)| *k)
}

/// All `%name` symbols, for the UI symbol palette: `(name, character)`.
pub fn greek_catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    GREEK_LOWER.iter().chain(GREEK_UPPER).copied()
}

/// Named colours accepted by the `color` attribute.
pub fn color(name: &str) -> Option<&'static str> {
    Some(match name {
        "black" => "#000000",
        "blue" => "#0000ff",
        "green" => "#008000",
        "red" => "#ff0000",
        "cyan" | "aqua" => "#00ffff",
        "magenta" | "fuchsia" => "#ff00ff",
        "yellow" => "#ffff00",
        "gray" | "grey" => "#808080",
        "lime" => "#00ff00",
        "maroon" => "#800000",
        "navy" => "#000080",
        "olive" => "#808000",
        "purple" => "#800080",
        "silver" => "#c0c0c0",
        "teal" => "#008080",
        "white" => "#ffffff",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        assert_eq!(keyword("over"), Some(Keyword::Over));
        assert_eq!(keyword("Over"), None);
        assert_eq!(special("alpha"), Some(("α", false)));
        assert_eq!(special("ialpha"), Some(("α", true)));
        assert_eq!(special("OMEGA"), Some(("Ω", false)));
        assert_eq!(special("iota"), Some(("ι", false)));
        assert_eq!(special("nonsense"), None);
        assert_eq!(special_name("π"), Some("pi"));
        assert_eq!(color("red"), Some("#ff0000"));
        assert_eq!(color("nope"), None);
        assert_eq!(greek_catalog().count(), 54);
    }
}
