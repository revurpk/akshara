//! # office-indic
//!
//! Indic transliteration for Akshara Office, built on
//! [teltools](https://github.com/revurpk/teltools) by Pradyumna Revur
//! (Apache-2.0, vendored in `third_party/teltools`).
//!
//! teltools converts romanized text (the RTS / ITRANS-style scheme) to
//! **Telugu**, and converts between Telugu, Devanagari and IAST. The Unicode
//! blocks of the major Indic scripts share one layout (inherited from ISCII),
//! so this crate maps teltools' Telugu output onto the other scripts at the
//! same position. Where a script lacks a letter, it applies a documented
//! fallback (for example Tamil, which has no aspirated or voiced stops,
//! writes `kha`, `ga` and `gha` all as க).
//!
//! Supported scripts: Devanagari (Hindi, Sanskrit, Marathi…), Telugu, Tamil,
//! Kannada, Malayalam, Odia and Bengali.
//!
//! ```
//! use office_indic::{transliterate, Options, Script};
//!
//! let o = Options::default();
//! assert_eq!(transliterate("namastE", Script::Telugu, &o), "నమస్తే");
//! assert_eq!(transliterate("namastE", Script::Devanagari, &o), "नमस्ते");
//! assert_eq!(transliterate("vaNakkam", Script::Tamil, &o), "வணக்கம்");
//! assert_eq!(transliterate("namaskAra", Script::Kannada, &o), "ನಮಸ್ಕಾರ");
//! ```
//!
//! The crate contains no `unsafe` code and never panics on any input.

use teltools::TransOptions;

/// An Indic script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    /// Devanagari: Hindi, Sanskrit, Marathi, Nepali, Konkani…
    Devanagari,
    /// Bengali–Assamese.
    Bengali,
    /// Odia.
    Oriya,
    /// Tamil.
    Tamil,
    /// Telugu.
    Telugu,
    /// Kannada.
    Kannada,
    /// Malayalam.
    Malayalam,
}

/// Bitmaps of the code points assigned in each script's 128-character block
/// (bit *n* = block start + *n*), from Unicode 16.0.
const ASSIGNED: [(Script, u32, u128); 7] = [
    (Script::Devanagari, 0x0900, 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff),
    (Script::Bengali, 0x0980, 0x7fff_ffcf_b080_799f_f3c5_fdff_fff9_9fef),
    (Script::Oriya, 0x0B00, 0x00ff_ffcf_b0e0_399f_f3ed_fdff_fff9_9fee),
    (Script::Tamil, 0x0B80, 0x07ff_ffc0_0081_3dc7_c3ff_c718_d63d_c7ec),
    (Script::Telugu, 0x0C00, 0xff80_ffcf_2760_3ddf_f3ff_fdff_fffd_dfff),
    (Script::Kannada, 0x0C80, 0x000e_ffcf_6060_3ddf_f3ef_fdff_fffd_dfff),
    (Script::Malayalam, 0x0D00, 0xffff_ffcf_fff0_fddf_ffff_ffff_ffff_dfff),
];

impl Script {
    /// All scripts, in the order shown to users.
    pub const ALL: [Script; 7] = [
        Script::Devanagari,
        Script::Telugu,
        Script::Tamil,
        Script::Kannada,
        Script::Malayalam,
        Script::Oriya,
        Script::Bengali,
    ];

    /// ISO 15924 code (`deva`, `telu`, `taml`, `knda`, `mlym`, `orya`, `beng`).
    pub fn code(self) -> &'static str {
        match self {
            Script::Devanagari => "deva",
            Script::Bengali => "beng",
            Script::Oriya => "orya",
            Script::Tamil => "taml",
            Script::Telugu => "telu",
            Script::Kannada => "knda",
            Script::Malayalam => "mlym",
        }
    }

    /// Parses an ISO 15924 code.
    pub fn from_code(code: &str) -> Option<Script> {
        Script::ALL.into_iter().find(|s| s.code() == code)
    }

    /// English name.
    pub fn name(self) -> &'static str {
        match self {
            Script::Devanagari => "Devanagari",
            Script::Bengali => "Bengali",
            Script::Oriya => "Odia",
            Script::Tamil => "Tamil",
            Script::Telugu => "Telugu",
            Script::Kannada => "Kannada",
            Script::Malayalam => "Malayalam",
        }
    }

    /// The script's own name, written in it.
    pub fn native_name(self) -> &'static str {
        match self {
            Script::Devanagari => "देवनागरी",
            Script::Bengali => "বাংলা",
            Script::Oriya => "ଓଡ଼ିଆ",
            Script::Tamil => "தமிழ்",
            Script::Telugu => "తెలుగు",
            Script::Kannada => "ಕನ್ನಡ",
            Script::Malayalam => "മലയാളം",
        }
    }

    /// First code point of the script's Unicode block.
    pub fn block_start(self) -> u32 {
        ASSIGNED.iter().find(|(s, _, _)| *s == self).map(|(_, b, _)| *b).unwrap_or(0x0C00)
    }

    fn assigned(self, offset: u32) -> bool {
        offset < 128
            && ASSIGNED.iter().find(|(s, _, _)| *s == self).is_some_and(|(_, _, m)| m >> offset & 1 == 1)
    }

    /// Whether `c` is an assigned character of one of the supported scripts
    /// (Unicode 16.0).
    pub fn is_assigned(c: char) -> bool {
        Script::of_char(c).is_some_and(|s| s.assigned(c as u32 - s.block_start()))
    }

    /// The script a character belongs to, if it is one of the supported ones.
    pub fn of_char(c: char) -> Option<Script> {
        let cp = c as u32;
        ASSIGNED.iter().find(|(_, b, _)| (*b..*b + 128).contains(&cp)).map(|(s, _, _)| *s)
    }
}

/// Options for [`transliterate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Write digits in the script's own numerals (౧౨౩, १२३…). Off by default
    /// so that numbers stay usable in spreadsheets.
    pub native_digits: bool,
    /// teltools behaviour. The default is [`TransOptions::LIVE`], the "sensible
    /// mode" of upstream's keyboard: `om` is ordinary letters (type `\om` for
    /// ॐ) and `<…>` tags are converted like any other text.
    pub teltools: TransOptions,
}

impl Default for Options {
    fn default() -> Self {
        Options { native_digits: false, teltools: TransOptions::LIVE }
    }
}

/// Characters that can be part of a romanized word in the teltools scheme.
/// The UI uses this to decide which keystrokes start or extend a word being
/// transliterated.
pub fn is_roman_input(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '~' | '^' | '.' | '|' | '\\' | '_' | '/' | '\'' | '"')
}

/// Converts romanized text to `script`, like typing with the teltools
/// scheme.
///
/// Only ASCII is converted. Anything else (text already in an Indic script,
/// emoji, accented Latin…) is kept as it is, so the function can be applied
/// to mixed text.
pub fn transliterate(roman: &str, script: Script, options: &Options) -> String {
    let mut out = String::with_capacity(roman.len() * 3);
    for (is_ascii, run) in ascii_runs(roman) {
        if is_ascii {
            let telugu = teltools::trans_with(run, &options.teltools);
            push_from_telugu(&mut out, &telugu, script, Mode::Typing, options.native_digits);
        } else {
            out.push_str(run);
        }
    }
    out
}

/// Splits text into maximal runs of ASCII and non-ASCII characters.
fn ascii_runs(s: &str) -> impl Iterator<Item = (bool, &str)> {
    let mut rest = s;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let ascii = first.is_ascii();
        let end = rest.find(|c: char| c.is_ascii() != ascii).unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        rest = tail;
        Some((ascii, run))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Roman input: teltools' `e`/`o` are Telugu *short* vowels, which Hindi
    /// and Sanskrit do not have, so Devanagari gets ए/ओ.
    Typing,
    /// Converting existing text: keep every distinction the target can show.
    Convert,
}

/// Telugu offset -> offset to try next when the target script lacks a letter.
fn fallback(offset: u32) -> Option<u32> {
    Some(match offset {
        0x01 => 0x02,                 // candrabindu -> anusvara
        0x0D | 0x0E => 0x0F,          // candra / short e -> e
        0x11 | 0x12 => 0x13,          // candra / short o -> o
        0x45 | 0x46 => 0x47,          // vowel signs, same
        0x49 | 0x4A => 0x4B,
        0x16..=0x18 => 0x15,          // kha ga gha -> ka (Tamil)
        0x1B => 0x1A,                 // chha -> cha
        0x1D => 0x1C,                 // jha -> ja
        0x20..=0x22 => 0x1F,          // ttha dda ddha -> tta
        0x25..=0x27 => 0x24,          // tha da dha -> ta
        0x2B..=0x2D => 0x2A,          // pha ba bha -> pa
        0x29 => 0x28,                 // nnna -> na
        0x31 => 0x30,                 // rra -> ra
        0x33 => 0x32,                 // lla -> la
        0x34 => 0x33,                 // llla -> lla
        0x35 => 0x2C,                 // va -> ba (Bengali)
        0x36 => 0x37,                 // sha -> ssa
        0x37 => 0x38,                 // ssa -> sa
        0x60 => 0x0B,                 // vocalic rr -> r
        0x61 => 0x0C,                 // vocalic ll -> l
        _ => return None,
    })
}

/// Appends the equivalent of Telugu text in `script`.
fn push_from_telugu(out: &mut String, telugu: &str, script: Script, mode: Mode, native_digits: bool) {
    for c in telugu.chars() {
        push_char_from_telugu(out, c, script, mode, native_digits);
    }
}

fn push_char_from_telugu(out: &mut String, c: char, script: Script, mode: Mode, native_digits: bool) {
    let cp = c as u32;
    if !(0x0C00..0x0C80).contains(&cp) {
        // Shared signs teltools writes in Devanagari (ॐ, accents, dandas).
        if c == '\u{0950}' && script == Script::Tamil {
            out.push('ௐ');
        } else {
            out.push(c);
        }
        return;
    }
    let base = script.block_start();
    let mut o = cp - 0x0C00;

    if (0x66..=0x6F).contains(&o) {
        let digit = if native_digits { char::from_u32(base + o) } else { char::from_u32(u32::from(b'0') + o - 0x66) };
        out.push(digit.unwrap_or(c));
        return;
    }

    // teltools writes the grave/acute accents (`.r`, `.e`) at unassigned
    // Telugu positions; the real characters are the shared Devanagari ones.
    if o == 0x53 || o == 0x54 {
        out.push(char::from_u32(0x0900 + o).unwrap_or(c));
        return;
    }

    // Script-specific rules.
    match (script, o) {
        // Hindi and Sanskrit have no short e/o: typed e/o mean ए/ओ.
        (Script::Devanagari, 0x0E | 0x12 | 0x46 | 0x4A) if mode == Mode::Typing => o += 1,
        // Tamil writes the nasal as ம் and spells out vocalic r/l.
        (Script::Tamil, 0x01 | 0x02) => return out.push_str("ம்"),
        (Script::Tamil, 0x0B) => return out.push_str("ரு"),
        (Script::Tamil, 0x60) => return out.push_str("ரூ"),
        (Script::Tamil, 0x0C) => return out.push_str("லு"),
        (Script::Tamil, 0x61) => return out.push_str("லூ"),
        (Script::Tamil, 0x43) => return out.push_str("்ரு"),
        (Script::Tamil, 0x44) => return out.push_str("்ரூ"),
        (Script::Tamil, 0x62) => return out.push_str("்லு"),
        (Script::Tamil, 0x63) => return out.push_str("்லூ"),
        // Kannada's LLLA is encoded outside the parallel layout.
        (Script::Kannada, 0x34) => return out.push('ೞ'),
        _ => {}
    }
    if script != Script::Telugu {
        match o {
            // Telugu-only letters tsa/dza: nearest equivalents.
            0x58 => o = 0x1A,
            0x59 => o = 0x1C,
            // Length marks exist (with this meaning) only in Kannada.
            0x55 | 0x56 if script != Script::Kannada => return,
            // Telugu fractions and tuumu have no counterpart: keep them.
            0x78..=0x7F => return out.push(c),
            _ => {}
        }
    }

    // Use the letter at the same position, or follow the fallback chain.
    for _ in 0..4 {
        if script.assigned(o) {
            if let Some(ch) = char::from_u32(base + o) {
                out.push(ch);
            }
            return;
        }
        match fallback(o) {
            Some(next) => o = next,
            None => break,
        }
    }
    match o {
        0x3C | 0x04 | 0x00 => {} // nukta and rare signs: drop
        0x3D => out.push('ऽ'),   // avagraha: the shared Devanagari sign
        // Keep a real Telugu character rather than lose it; never emit an
        // unassigned code point.
        _ if Script::Telugu.assigned(cp - 0x0C00) => out.push(c),
        _ => {}
    }
}

/// Converts one character of any supported script to its Telugu equivalent
/// (the hub representation teltools works with).
fn push_to_telugu(out: &mut String, c: char) {
    let cp = c as u32;
    match Script::of_char(c) {
        None | Some(Script::Telugu) => out.push(c),
        Some(Script::Devanagari) => match c {
            // Precomposed nukta letters (U+0958..U+095F): base + nukta.
            '\u{0958}'..='\u{095F}' => {
                const BASES: [char; 8] = ['క', 'ఖ', 'గ', 'జ', 'డ', 'ఢ', 'ఫ', 'య'];
                out.push(BASES[(cp - 0x0958) as usize]);
                out.push('\u{0C3C}');
            }
            _ => out.push(teltools::hin2tel(c)),
        },
        Some(script) => {
            let special: Option<&str> = match c {
                'ௐ' => Some("ॐ"),
                'ৎ' => Some("త్"),
                '\u{09DC}' | '\u{0B5C}' => Some("డ\u{0C3C}"), // ড় ଡ଼
                '\u{09DD}' | '\u{0B5D}' => Some("ఢ\u{0C3C}"), // ঢ় ଢ଼
                '\u{09DF}' | '\u{0B5F}' => Some("య"),        // য় ୟ
                'ୱ' => Some("వ"),
                'ൺ' => Some("ణ్"),
                'ൻ' => Some("న్"),
                'ർ' => Some("ర్"),
                'ൽ' => Some("ల్"),
                'ൾ' => Some("ళ్"),
                'ൿ' => Some("క్"),
                'ൔ' => Some("మ్"),
                'ൕ' => Some("య్"),
                'ൖ' => Some("ఴ్"),
                'ೞ' => Some("ఴ"),
                _ => None,
            };
            if let Some(s) = special {
                out.push_str(s);
                return;
            }
            let o = cp - script.block_start();
            // Only the parallel part of the layout (letters, signs, digits).
            if o < 0x58 || (0x60..0x70).contains(&o) {
                out.push(char::from_u32(0x0C00 + o).unwrap_or(c));
            } else {
                out.push(c);
            }
        }
    }
}

/// Converts text written in any supported Indic script to `to`. Other text
/// (Latin, digits, punctuation) is unchanged.
///
/// ```
/// use office_indic::{convert_script, Script};
/// assert_eq!(convert_script("సత్యమేవ జయతే", Script::Devanagari), "सत्यमेव जयते");
/// assert_eq!(convert_script("नमस्ते", Script::Telugu), "నమస్తే");
/// assert_eq!(convert_script("নমস্কার", Script::Oriya), "ନମସ୍କାର");
/// ```
pub fn convert_script(text: &str, to: Script) -> String {
    let mut out = String::with_capacity(text.len());
    let mut hub = String::new();
    for c in text.chars() {
        if Script::of_char(c).is_some_and(|s| s != to) {
            hub.clear();
            push_to_telugu(&mut hub, c);
            push_from_telugu(&mut out, &hub, to, Mode::Convert, true);
        } else {
            out.push(c);
        }
    }
    out
}

/// Romanizes Indic text as IAST (`धर्मः` → `dharmaḥ`), using teltools'
/// Devanagari → IAST converter. Short e/o (ఎ, ఒ…), which Sanskrit IAST does
/// not distinguish, become `e`/`o`. Non-Indic text is unchanged.
///
/// ```
/// use office_indic::to_iast;
/// assert_eq!(to_iast("ధర్మక్షేత్రే"), "dharmakṣetre");
/// assert_eq!(to_iast("ಕನ್ನಡ"), "kannaḍa");
/// ```
pub fn to_iast(text: &str) -> String {
    // Convert every Indic run to Devanagari, then let teltools romanize it.
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if !run.is_empty() {
            out.push_str(&teltools::dev2iast(run));
            run.clear();
        }
    };
    for c in text.chars() {
        if Script::of_char(c).is_some() {
            let mut deva = String::new();
            if Script::of_char(c) == Some(Script::Devanagari) {
                deva.push(c);
            } else {
                let mut hub = String::new();
                push_to_telugu(&mut hub, c);
                for h in hub.chars() {
                    deva.push(if (0x0C00..0x0C80).contains(&(h as u32)) { teltools::tel2hin(h) } else { h });
                }
            }
            for d in deva.chars() {
                run.push(match d {
                    'ऎ' | 'ऍ' => 'ए',
                    'ऒ' | 'ऑ' => 'ओ',
                    'ॆ' | 'ॅ' => 'े',
                    'ॊ' | 'ॉ' => 'ो',
                    // Letters teltools' dev2iast does not know.
                    'ऴ' => 'ळ',
                    'ऱ' => 'र',
                    'ऩ' => 'न',
                    other => other,
                });
            }
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Converts IAST (`kṛṣṇaḥ`) to `script`, using teltools' IAST → Telugu
/// converter. Note that teltools lower-cases its input.
///
/// ```
/// use office_indic::{from_iast, Script};
/// assert_eq!(from_iast("kṛṣṇaṁ vande", Script::Devanagari), "कृष्णं वन्दे");
/// ```
pub fn from_iast(text: &str, to: Script) -> String {
    let telugu = teltools::iast2tel(text);
    let mut out = String::with_capacity(telugu.len());
    push_from_telugu(&mut out, &telugu, to, Mode::Convert, true);
    out
}

/// The supported script used most in `text`, if any.
pub fn detect_script(text: &str) -> Option<Script> {
    let mut counts = [0usize; 7];
    for c in text.chars() {
        if let Some(s) = Script::of_char(c) {
            if let Some(i) = Script::ALL.iter().position(|x| *x == s) {
                counts[i] += 1;
            }
        }
    }
    let (i, n) = counts.iter().enumerate().max_by_key(|(_, n)| **n)?;
    (*n > 0).then(|| Script::ALL[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigned_tables_agree_with_known_letters() {
        assert!(Script::Tamil.assigned(0x15)); // க
        assert!(!Script::Tamil.assigned(0x16)); // no kha
        assert!(Script::Tamil.assigned(0x29)); // ன
        assert!(!Script::Bengali.assigned(0x35)); // no separate va
        assert!(Script::Kannada.assigned(0x31)); // ಱ
        assert!(Script::Devanagari.assigned(0x7F));
        assert!(!Script::Telugu.assigned(0x0D));
        assert!(!Script::Telugu.assigned(200));
        for s in Script::ALL {
            assert_eq!(Script::from_code(s.code()), Some(s));
            assert_eq!(Script::of_char(char::from_u32(s.block_start() + 5).unwrap_or('a')), Some(s));
        }
        assert_eq!(Script::of_char('a'), None);
    }

    #[test]
    fn runs_split_ascii() {
        let runs: Vec<_> = ascii_runs("ab😀cd").collect();
        assert_eq!(runs, vec![(true, "ab"), (false, "😀"), (true, "cd")]);
        assert_eq!(ascii_runs("").count(), 0);
    }
}
