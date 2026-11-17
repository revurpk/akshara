//! Tokeniser for the StarMath command language.

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Numeric literal, e.g. `3.14`.
    Number(String),
    /// Keyword or identifier, e.g. `over`, `x`.
    Word(String),
    /// `%name` symbol reference (without the `%`).
    Special(String),
    /// Quoted text (without the quotes).
    Text(String),
    /// Punctuation / operator symbol, e.g. `+`, `<=`, `{`.
    Sym(&'static str),
    /// A character escaped with a backslash, e.g. `\{`.
    Escaped(char),
    /// Any other single character.
    Char(char),
    /// End of input.
    End,
}

/// A token with its byte position in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token.
    pub tok: Tok,
    /// Byte offset of its first character.
    pub pos: usize,
}

/// Multi-character symbols, longest first.
const MULTI: &[&str] = &["<?>", "<<", ">>", "<=", ">=", "<>", "+-", "-+", "##", "->"];

/// Single-character symbols.
const SINGLE: &[&str] = &[
    "+", "-", "*", "/", "^", "_", "=", "<", ">", "(", ")", "[", "]", "{", "}", "#", "~", "`", "&",
    "|", "!", ",", ";", ":", ".", "'", "?", "@", "$",
];

/// Splits `src` into tokens. The result always ends with [`Tok::End`].
pub fn tokenize(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if b == b'%' {
            if bytes.get(i + 1) == Some(&b'%') {
                // Comment until end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            i += 1;
            let name_start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            if i == name_start {
                out.push(Token { tok: Tok::Char('%'), pos: start });
            } else {
                out.push(Token { tok: Tok::Special(src[name_start..i].to_owned()), pos: start });
            }
            continue;
        }
        if b == b'"' {
            i += 1;
            let text_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            out.push(Token { tok: Tok::Text(src[text_start..i].to_owned()), pos: start });
            i = (i + 1).min(bytes.len());
            continue;
        }
        if b.is_ascii_digit() || (b == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push(Token { tok: Tok::Number(src[start..i].to_owned()), pos: start });
            continue;
        }
        if b.is_ascii_alphabetic() {
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            out.push(Token { tok: Tok::Word(src[start..i].to_owned()), pos: start });
            continue;
        }
        if b == b'\\' {
            match src[i + 1..].chars().next() {
                Some(c) if !c.is_whitespace() => {
                    out.push(Token { tok: Tok::Escaped(c), pos: start });
                    i += 1 + c.len_utf8();
                }
                _ => {
                    out.push(Token { tok: Tok::Char('\\'), pos: start });
                    i += 1;
                }
            }
            continue;
        }
        for sym in MULTI.iter().chain(SINGLE) {
            if bytes[i..].starts_with(sym.as_bytes()) {
                out.push(Token { tok: Tok::Sym(sym), pos: start });
                i += sym.len();
                continue 'outer;
            }
        }
        // Anything else (including non-ASCII letters typed directly).
        let c = src[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(Token { tok: Tok::Char(c), pos: start });
        i += c.len_utf8().max(1);
    }
    out.push(Token { tok: Tok::End, pos: src.len() });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        tokenize(src).into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn basic_tokens() {
        assert_eq!(
            toks("a over 2.5 <= %alpha \"hi there\""),
            vec![
                Tok::Word("a".into()),
                Tok::Word("over".into()),
                Tok::Number("2.5".into()),
                Tok::Sym("<="),
                Tok::Special("alpha".into()),
                Tok::Text("hi there".into()),
                Tok::End
            ]
        );
    }

    #[test]
    fn symbols_longest_match() {
        assert_eq!(toks("<?> ## # +- <>"), vec![
            Tok::Sym("<?>"), Tok::Sym("##"), Tok::Sym("#"), Tok::Sym("+-"), Tok::Sym("<>"), Tok::End
        ]);
    }

    #[test]
    fn comments_escapes_and_unicode() {
        assert_eq!(toks("a %% comment\nb"), vec![Tok::Word("a".into()), Tok::Word("b".into()), Tok::End]);
        assert_eq!(toks("\\{ λ"), vec![Tok::Escaped('{'), Tok::Char('λ'), Tok::End]);
        assert_eq!(toks("\"unterminated"), vec![Tok::Text("unterminated".into()), Tok::End]);
        assert_eq!(toks("%"), vec![Tok::Char('%'), Tok::End]);
        assert_eq!(toks("\\"), vec![Tok::Char('\\'), Tok::End]);
        assert_eq!(toks(".5 1."), vec![Tok::Number(".5".into()), Tok::Number("1".into()), Tok::Sym("."), Tok::End]);
    }

    #[test]
    fn positions_are_byte_offsets() {
        let t = tokenize("λ + x");
        assert_eq!(t[1].pos, 3);
        assert_eq!(t[2].pos, 5);
        assert_eq!(t[3].pos, 6);
    }
}
