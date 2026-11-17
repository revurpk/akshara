// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Pradyumna Revur
// Modified 2026 by the Akshara Office contributors: added `trans_with` /
// `TransOptions` and made `trans` safe for non-ASCII input (see PATCHES.md).

/// Converts a telugu unicode character to devanagari.
///
/// # Examples
///
/// ```
/// let arg = 'శ';
/// let answer = teltools::tel2hin(arg);
///
/// assert_eq!('श', answer);
/// ```
pub fn tel2hin(telchar: char) -> char {
    let telmin = '\u{0c00}' as u32;
    let telmax = '\u{0c7f}' as u32;
    let hinmin = '\u{0900}' as u32;
    let mut c = telchar as u32;
    let delta = telmin - hinmin;
    if (c >= telmin) && (c <= telmax) {
        c = c - delta;
    }
    match std::char::from_u32(c) {
        Some(c) => c,
        None => telchar,
    }
}

/// Converts a devanagari unicode character to telugu.
///
/// # Examples
///
/// ```
/// let arg = 'श';
/// let answer = teltools::hin2tel(arg);
///
/// assert_eq!('శ', answer);
/// ```
pub fn hin2tel(hinchar: char) -> char {
    let telmin = '\u{0c00}' as u32;
    let hinmin = '\u{0900}' as u32;
    let hinmax = '\u{097f}' as u32;
    let mut c = hinchar as u32;
    let delta = telmin - hinmin;
    if (c >= hinmin)
        && (c <= hinmax)
        && (hinchar != '\u{0950}')
        && (hinchar != '\u{0951}')
        && (hinchar != '\u{0952}')
        && (hinchar != '\u{0953}')
        && (hinchar != '\u{0954}')
        && (hinchar != '\u{0964}')
        && (hinchar != '\u{0965}')
    {
        c = c + delta;
    }
    match std::char::from_u32(c) {
        Some(c) => c,
        None => hinchar,
    }
}

// ---- IAST <-> Telugu ----------------------------------------------------

fn iast_cons2(a: char, b: char) -> Option<&'static str> {
    match (a, b) {
        ('k', 'h') => Some("ఖ"), ('g', 'h') => Some("ఘ"), ('c', 'h') => Some("ఛ"),
        ('j', 'h') => Some("ఝ"), ('ṭ', 'h') => Some("ఠ"), ('ḍ', 'h') => Some("ఢ"),
        ('t', 'h') => Some("థ"), ('d', 'h') => Some("ధ"), ('p', 'h') => Some("ఫ"),
        ('b', 'h') => Some("భ"), _ => None,
    }
}
fn iast_cons1(a: char) -> Option<&'static str> {
    match a {
        'k' => Some("క"), 'g' => Some("గ"), 'ṅ' => Some("ఙ"), 'c' => Some("చ"),
        'j' => Some("జ"), 'ñ' => Some("ఞ"), 'ṭ' => Some("ట"), 'ḍ' => Some("డ"),
        'ṇ' => Some("ణ"), 't' => Some("త"), 'd' => Some("ద"), 'n' => Some("న"),
        'p' => Some("ప"), 'b' => Some("బ"), 'm' => Some("మ"), 'y' => Some("య"),
        'r' => Some("ర"), 'l' => Some("ల"), 'ḻ' => Some("ళ"), 'v' => Some("వ"),
        'ś' => Some("శ"), 'ṣ' => Some("ష"), 's' => Some("స"), 'h' => Some("హ"),
        _ => None,
    }
}
fn iast_matra(a: char) -> Option<&'static str> {
    match a {
        'a' => Some(""), 'ā' => Some("ా"), 'i' => Some("ి"), 'ī' => Some("ీ"),
        'u' => Some("ు"), 'ū' => Some("ూ"), 'ṛ' => Some("ృ"), 'ṝ' => Some("ౄ"),
        'ḷ' => Some("ౢ"), 'ḹ' => Some("ౣ"), 'e' | 'ē' => Some("ే"), 'o' | 'ō' => Some("ో"),
        _ => None,
    }
}
fn iast_vowel(a: char) -> Option<&'static str> {
    match a {
        'a' => Some("అ"), 'ā' => Some("ఆ"), 'i' => Some("ఇ"), 'ī' => Some("ఈ"),
        'u' => Some("ఉ"), 'ū' => Some("ఊ"), 'ṛ' => Some("ఋ"), 'ṝ' => Some("ౠ"),
        'ḷ' => Some("ఌ"), 'ḹ' => Some("ౡ"), 'e' | 'ē' => Some("ఏ"), 'o' | 'ō' => Some("ఓ"),
        _ => None,
    }
}

/// Transliterate an IAST string into Telugu lipi.
pub fn iast2tel(frag: &str) -> String {
    let s: Vec<char> = frag.to_lowercase().chars().collect();
    let n = s.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        // consonant (2-char then 1-char)
        let (cbase, clen) = if i + 1 < n {
            if let Some(x) = iast_cons2(s[i], s[i + 1]) { (Some(x), 2) }
            else if let Some(x) = iast_cons1(s[i]) { (Some(x), 1) } else { (None, 0) }
        } else if let Some(x) = iast_cons1(s[i]) { (Some(x), 1) } else { (None, 0) };
        if let Some(base) = cbase {
            out.push_str(base);
            i += clen;
            // vowel: ai/au first
            if i + 1 < n && s[i] == 'a' && (s[i + 1] == 'i' || s[i + 1] == 'u') {
                out.push_str(if s[i + 1] == 'i' { "ై" } else { "ౌ" });
                i += 2;
            } else if i < n && iast_matra(s[i]).is_some() {
                out.push_str(iast_matra(s[i]).unwrap());
                i += 1;
            } else {
                out.push('\u{0c4d}'); // virama
            }
            continue;
        }
        // independent vowel
        if i + 1 < n && s[i] == 'a' && (s[i + 1] == 'i' || s[i + 1] == 'u') {
            out.push_str(if s[i + 1] == 'i' { "ఐ" } else { "ఔ" });
            i += 2;
            continue;
        }
        if let Some(v) = iast_vowel(s[i]) {
            out.push_str(v);
            i += 1;
            continue;
        }
        match s[i] {
            'ṁ' | 'ṃ' => out.push('ం'),
            'ḥ' => out.push('ః'),
            other => out.push(other),
        }
        i += 1;
    }
    out
}

fn dev_cons(ch: char) -> Option<&'static str> {
    match ch {
        'क' => Some("k"), 'ख' => Some("kh"), 'ग' => Some("g"), 'घ' => Some("gh"),
        'ङ' => Some("ṅ"), 'च' => Some("c"), 'छ' => Some("ch"), 'ज' => Some("j"),
        'झ' => Some("jh"), 'ञ' => Some("ñ"), 'ट' => Some("ṭ"), 'ठ' => Some("ṭh"),
        'ड' => Some("ḍ"), 'ढ' => Some("ḍh"), 'ण' => Some("ṇ"), 'त' => Some("t"),
        'थ' => Some("th"), 'द' => Some("d"), 'ध' => Some("dh"), 'न' => Some("n"),
        'प' => Some("p"), 'फ' => Some("ph"), 'ब' => Some("b"), 'भ' => Some("bh"),
        'म' => Some("m"), 'य' => Some("y"), 'र' => Some("r"), 'ल' => Some("l"),
        'व' => Some("v"), 'श' => Some("ś"), 'ष' => Some("ṣ"), 'स' => Some("s"),
        'ह' => Some("h"), 'ळ' => Some("ḷ"), _ => None,
    }
}
fn dev_matra(ch: char) -> Option<&'static str> {
    match ch {
        'ा' => Some("ā"), 'ि' => Some("i"), 'ी' => Some("ī"), 'ु' => Some("u"),
        'ू' => Some("ū"), 'ृ' => Some("ṛ"), 'ॄ' => Some("ṝ"), 'ॢ' => Some("ḷ"),
        'े' => Some("e"), 'ै' => Some("ai"), 'ो' => Some("o"), 'ौ' => Some("au"), _ => None,
    }
}
fn dev_vowel(ch: char) -> Option<&'static str> {
    match ch {
        'अ' => Some("a"), 'आ' => Some("ā"), 'इ' => Some("i"), 'ई' => Some("ī"),
        'उ' => Some("u"), 'ऊ' => Some("ū"), 'ऋ' => Some("ṛ"), 'ॠ' => Some("ṝ"),
        'ऌ' => Some("ḷ"), 'ए' => Some("e"), 'ऐ' => Some("ai"), 'ओ' => Some("o"),
        'औ' => Some("au"), _ => None,
    }
}

/// Transliterate a Devanagari string into IAST.
pub fn dev2iast(s: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for ch in s.chars() {
        if matches!(ch, '\u{0900}' | '\u{093c}' | '\u{0951}'..='\u{0954}' | '\u{200c}' | '\u{200d}') {
            continue;
        }
        if let Some(c) = dev_cons(ch) {
            if pending { out.push('a'); }
            out.push_str(c);
            pending = true;
        } else if ch == '\u{094d}' {
            pending = false;
        } else if let Some(m) = dev_matra(ch) {
            out.push_str(m);
            pending = false;
        } else if let Some(v) = dev_vowel(ch) {
            if pending { out.push('a'); pending = false; }
            out.push_str(v);
        } else if ch == '\u{0902}' {
            if pending { out.push('a'); pending = false; }
            out.push('ṃ');
        } else if ch == '\u{0903}' {
            if pending { out.push('a'); pending = false; }
            out.push('ḥ');
        } else {
            if pending { out.push('a'); pending = false; }
            out.push(ch);
        }
    }
    if pending { out.push('a'); }
    out
}

/// Transliterate a Telugu string into IAST (via Devanagari).
pub fn tel2iast(s: &str) -> String {
    let dev: String = s.chars().map(tel2hin).collect();
    dev2iast(&dev)
}

enum Context {
    Vowel,
    Consonant,
    Mark,
    Other,
}

struct ResultStr {
    pub m_result: String,
    pub m_alt_result: String,
    pub m_context: Context,
}

use std::collections::BTreeMap;

const HALANT: char = '\u{0c4d}';

/// Transliterates an English string into Telugu.
///
/// # Examples
///
/// ```
/// let arg = "om";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("ॐ", answer);
/// ```
///
/// ```
/// let arg = "1";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("౧", answer);
/// ```
///
/// ```
/// let arg = "om gaM gaNapatayE namaH ||";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("ॐ గం గణపతయే నమః ॥", answer);
/// ```
///
/// ```
/// let arg = "om shrI mahA saraswatyai namaH ||\nom shrI gurubhyOH namaH ||";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("ॐ శ్రీ మహా సరస్వత్యై నమః ॥\nॐ శ్రీ గురుభ్యోః నమః ॥", answer);
/// ```
///
/// ```
/// let arg = "prathamO daivyO bhiShak";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("ప్రథమో దైవ్యో భిషక్", answer);
/// ```
///
/// ```
/// let arg = "mAm rakShatu";
/// let answer = teltools::trans(arg);
///
/// assert_eq!("మామ్ రక్షతు", answer);
/// ```
pub fn trans(line: &str) -> String {
    trans_with(line, &TransOptions::BATCH)
}

/// Options for [`trans_with`]. (Akshara addition, see PATCHES.md.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransOptions {
    /// `om`, `OM` and `AUM` become ॐ wherever they occur (upstream
    /// behaviour). When false, they are ordinary letters and ॐ is typed as
    /// `\om`, like upstream's Keyman keyboard.
    pub om_anywhere: bool,
    /// Copy `<...>` tags through unconverted (upstream behaviour).
    pub skip_tags: bool,
}

impl TransOptions {
    /// Upstream `trans()` behaviour (batch conversion of marked-up text).
    pub const BATCH: TransOptions = TransOptions { om_anywhere: true, skip_tags: true };
    /// Upstream keyboard behaviour ("sensible mode") for live typing.
    pub const LIVE: TransOptions = TransOptions { om_anywhere: false, skip_tags: false };
}

/// Like [`trans`], with options. Never panics, whatever the input.
///
/// ```
/// use teltools::{trans_with, TransOptions};
/// assert_eq!(trans_with("komala", &TransOptions::LIVE), "కొమల");
/// assert_eq!(trans_with("\\om namaH", &TransOptions::LIVE), "ॐ నమః");
/// ```
pub fn trans_with(line: &str, options: &TransOptions) -> String {
    let mut map = get_telugu_trans_map();
    if !options.om_anywhere {
        map.remove("om");
        map.remove("OM");
        map.remove("AUM");
        map.insert(
            "\\om",
            ResultStr {
                m_result: String::from("\u{0950}"),
                m_alt_result: String::from("\u{0950}"),
                m_context: Context::Mark,
            },
        ); // SIGN_OM
    }
    let mut index = 0;
    let mut out = String::new();

    let mut context = &Context::Other;
    let mut skip_tag = false; //assume open/close tags are on the same line

    while index < line.len() {
        let mut found_match = false;
        // Length of the character at `index` (upstream assumed 1 byte).
        let ch_len = line[index..].chars().next().map_or(1, char::len_utf8);

        if options.skip_tags {
            match &line[index..index + ch_len] {
                "<" => skip_tag = true,
                ">" => skip_tag = false,
                _ => {}
            }
        }

        if skip_tag {
            match context {
                Context::Consonant => out.push(HALANT),
                _ => {}
            }
            out.push_str(&line[index..index + ch_len]);
            index = index + ch_len;
            context = &Context::Other;
            continue;
        }

        for k in map.keys().rev() {
            if line[index..].starts_with(k) {
                found_match = true;
                index = index + k.len();
                if let Some(v) = map.get(k) {
                    let (result, ctx) = append_output(&v, &context);
                    out.push_str(&result);
                    context = ctx;
                } else {
                    out.push_str(k);
                }
                continue;
            }
        }

        if !found_match {
            match context {
                Context::Consonant => out.push(HALANT),
                _ => {}
            }
            out.push_str(&line[index..index + ch_len]);
            index = index + ch_len;
            context = &Context::Other;
        }
    }
    match context {
        Context::Consonant => out.push(HALANT),
        _ => {}
    }

    return out;
}

fn append_output<'a>(
    result_str: &'a ResultStr,
    current_context: &'a Context,
) -> (String, &'a Context) {
    let ctx = &result_str.m_context;
    let res = &result_str.m_result;
    let altres = &result_str.m_alt_result;

    let mut result = String::new();

    match current_context {
        Context::Consonant => match ctx {
            Context::Consonant => result.push_str(&altres),
            Context::Vowel => result.push_str(&altres),
            _ctx1 => result.push_str(&res),
        },
        _ctx2 => {
            result.push_str(&res);
        }
    };

    return (result, ctx);
}

fn get_telugu_trans_map<'a>() -> BTreeMap<&'a str, ResultStr> {
    let mut map = BTreeMap::new();

    map.insert(
        ".N",
        ResultStr {
            m_result: String::from("\u{0c01}"),
            m_alt_result: String::from("\u{0c01}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_CANDRABINDU
    map.insert(
        ".n",
        ResultStr {
            m_result: String::from("\u{0c02}"),
            m_alt_result: String::from("\u{0c02}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_ANUSVARA
    map.insert(
        "M",
        ResultStr {
            m_result: String::from("\u{0c02}"),
            m_alt_result: String::from("\u{0c02}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_ANUSVARA
    map.insert(
        "H",
        ResultStr {
            m_result: String::from("\u{0c03}"),
            m_alt_result: String::from("\u{0c03}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_VISARGA
    map.insert(
        "^a",
        ResultStr {
            m_result: String::from("\u{0c04}"),
            m_alt_result: String::from("\u{0c04}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_SHORT_A
    map.insert(
        "a",
        ResultStr {
            m_result: String::from("\u{0c05}"),
            m_alt_result: String::from(""),
            m_context: Context::Vowel,
        },
    ); // LETTER_A
    map.insert(
        "A",
        ResultStr {
            m_result: String::from("\u{0c06}"),
            m_alt_result: String::from("\u{0c3e}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_AA
    map.insert(
        "aa",
        ResultStr {
            m_result: String::from("\u{0c06}"),
            m_alt_result: String::from("\u{0c3e}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_AA
    map.insert(
        "i",
        ResultStr {
            m_result: String::from("\u{0c07}"),
            m_alt_result: String::from("\u{0c3f}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_I
    map.insert(
        "I",
        ResultStr {
            m_result: String::from("\u{0c08}"),
            m_alt_result: String::from("\u{0c40}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_II
    map.insert(
        "ii",
        ResultStr {
            m_result: String::from("\u{0c08}"),
            m_alt_result: String::from("\u{0c40}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_II
    map.insert(
        "u",
        ResultStr {
            m_result: String::from("\u{0c09}"),
            m_alt_result: String::from("\u{0c41}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_U
    map.insert(
        "U",
        ResultStr {
            m_result: String::from("\u{0c0a}"),
            m_alt_result: String::from("\u{0c42}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_UU
    map.insert(
        "uu",
        ResultStr {
            m_result: String::from("\u{0c0a}"),
            m_alt_result: String::from("\u{0c42}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_UU
    map.insert(
        "R^i",
        ResultStr {
            m_result: String::from("\u{0c0b}"),
            m_alt_result: String::from("\u{0c43}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_VOCALIC_R
    map.insert(
        "L^i",
        ResultStr {
            m_result: String::from("\u{0c0c}"),
            m_alt_result: String::from("\u{0c44}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_VOCALIC_L
    map.insert(
        "^e",
        ResultStr {
            m_result: String::from("\u{0c0d}"),
            m_alt_result: String::from("\u{0c45}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_CANDRA_E
    map.insert(
        "e",
        ResultStr {
            m_result: String::from("\u{0c0e}"),
            m_alt_result: String::from("\u{0c46}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_E
    map.insert(
        "E",
        ResultStr {
            m_result: String::from("\u{0c0f}"),
            m_alt_result: String::from("\u{0c47}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_EE
    map.insert(
        "ee",
        ResultStr {
            m_result: String::from("\u{0c0f}"),
            m_alt_result: String::from("\u{0c47}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_EE
    map.insert(
        "ai",
        ResultStr {
            m_result: String::from("\u{0c10}"),
            m_alt_result: String::from("\u{0c48}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_AI
    map.insert(
        "^o",
        ResultStr {
            m_result: String::from("\u{0c11}"),
            m_alt_result: String::from("\u{0c49}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_CANDRA_O
    map.insert(
        "o",
        ResultStr {
            m_result: String::from("\u{0c12}"),
            m_alt_result: String::from("\u{0c4a}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_O
    map.insert(
        "O",
        ResultStr {
            m_result: String::from("\u{0c13}"),
            m_alt_result: String::from("\u{0c4b}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_OO
    map.insert(
        "oo",
        ResultStr {
            m_result: String::from("\u{0c13}"),
            m_alt_result: String::from("\u{0c4b}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_OO
    map.insert(
        "au",
        ResultStr {
            m_result: String::from("\u{0c14}"),
            m_alt_result: String::from("\u{0c4c}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_AU
    map.insert(
        "k",
        ResultStr {
            m_result: String::from("\u{0c15}"),
            m_alt_result: String::from("\u{0c4d}\u{0c15}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_KA
    map.insert(
        "kSh",
        ResultStr {
            m_result: String::from("\u{0c15}\u{0c4d}\u{0c37}"),
            m_alt_result: String::from("\u{0c4d}\u{0c15}\u{0c4d}\u{0c37}"),
            m_context: Context::Consonant,
        },
    ); // CONJUNCT_KSHA
    map.insert(
        "x",
        ResultStr {
            m_result: String::from("\u{0c15}\u{0c4d}\u{0c37}"),
            m_alt_result: String::from("\u{0c4d}\u{0c15}\u{0c4d}\u{0c37}"),
            m_context: Context::Consonant,
        },
    ); // CONJUNCT_KSHA
    map.insert(
        "kh",
        ResultStr {
            m_result: String::from("\u{0c16}"),
            m_alt_result: String::from("\u{0c4d}\u{0c16}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_KHA
    map.insert(
        "g",
        ResultStr {
            m_result: String::from("\u{0c17}"),
            m_alt_result: String::from("\u{0c4d}\u{0c17}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_GA
    map.insert(
        "ga.N",
        ResultStr {
            m_result: String::from("\u{0c17}\u{0c4d}\u{0c02}"),
            m_alt_result: String::from("\u{0c4d}\u{0c17}\u{0c4d}\u{0c02}"),
            m_context: Context::Consonant,
        },
    ); // CONJUNCT_GUM
    map.insert(
        "gh",
        ResultStr {
            m_result: String::from("\u{0c18}"),
            m_alt_result: String::from("\u{0c4d}\u{0c18}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_GHA
    map.insert(
        "~N",
        ResultStr {
            m_result: String::from("\u{0c19}"),
            m_alt_result: String::from("\u{0c4d}\u{0c19}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NGA
    map.insert(
        "N^",
        ResultStr {
            m_result: String::from("\u{0c19}"),
            m_alt_result: String::from("\u{0c4d}\u{0c19}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NGA
    map.insert(
        "ch",
        ResultStr {
            m_result: String::from("\u{0c1a}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1a}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_CA
    map.insert(
        "chh",
        ResultStr {
            m_result: String::from("\u{0c1b}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1b}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_CHA
    map.insert(
        "j",
        ResultStr {
            m_result: String::from("\u{0c1c}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1c}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_JA
    map.insert(
        "jh",
        ResultStr {
            m_result: String::from("\u{0c1d}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1d}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_JHA
    map.insert(
        "~n",
        ResultStr {
            m_result: String::from("\u{0c1e}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1e}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NYA
    map.insert(
        "JN",
        ResultStr {
            m_result: String::from("\u{0c1e}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1e}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NYA
    map.insert(
        "T",
        ResultStr {
            m_result: String::from("\u{0c1f}"),
            m_alt_result: String::from("\u{0c4d}\u{0c1f}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_TTA
    map.insert(
        "Th",
        ResultStr {
            m_result: String::from("\u{0c20}"),
            m_alt_result: String::from("\u{0c4d}\u{0c20}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_TTHA
    map.insert(
        "D",
        ResultStr {
            m_result: String::from("\u{0c21}"),
            m_alt_result: String::from("\u{0c4d}\u{0c21}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_DDA
    map.insert(
        "Dh",
        ResultStr {
            m_result: String::from("\u{0c22}"),
            m_alt_result: String::from("\u{0c4d}\u{0c22}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_DDHA
    map.insert(
        "N",
        ResultStr {
            m_result: String::from("\u{0c23}"),
            m_alt_result: String::from("\u{0c4d}\u{0c23}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NNA
    map.insert(
        "t",
        ResultStr {
            m_result: String::from("\u{0c24}"),
            m_alt_result: String::from("\u{0c4d}\u{0c24}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_TA
    map.insert(
        "th",
        ResultStr {
            m_result: String::from("\u{0c25}"),
            m_alt_result: String::from("\u{0c4d}\u{0c25}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_THA
    map.insert(
        "d",
        ResultStr {
            m_result: String::from("\u{0c26}"),
            m_alt_result: String::from("\u{0c4d}\u{0c26}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_DA
    map.insert(
        "dh",
        ResultStr {
            m_result: String::from("\u{0c27}"),
            m_alt_result: String::from("\u{0c4d}\u{0c27}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_DHA
    map.insert(
        "n",
        ResultStr {
            m_result: String::from("\u{0c28}"),
            m_alt_result: String::from("\u{0c4d}\u{0c28}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NA
    map.insert(
        "N.",
        ResultStr {
            m_result: String::from("\u{0c29}"),
            m_alt_result: String::from("\u{0c4d}\u{0c29}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_NNNA
    map.insert(
        "p",
        ResultStr {
            m_result: String::from("\u{0c2a}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2a}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_PA
    map.insert(
        "ph",
        ResultStr {
            m_result: String::from("\u{0c2b}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2b}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_PHA
    map.insert(
        "b",
        ResultStr {
            m_result: String::from("\u{0c2c}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2c}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_BA
    map.insert(
        "bh",
        ResultStr {
            m_result: String::from("\u{0c2d}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2d}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_BHA
    map.insert(
        "m",
        ResultStr {
            m_result: String::from("\u{0c2e}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2e}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_MA
    map.insert(
        "y",
        ResultStr {
            m_result: String::from("\u{0c2f}"),
            m_alt_result: String::from("\u{0c4d}\u{0c2f}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_YA
    map.insert(
        "r",
        ResultStr {
            m_result: String::from("\u{0c30}"),
            m_alt_result: String::from("\u{0c4d}\u{0c30}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_RA
    map.insert(
        "R",
        ResultStr {
            m_result: String::from("\u{0c31}"),
            m_alt_result: String::from("\u{0c4d}\u{0c31}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_RRA
    map.insert(
        "l",
        ResultStr {
            m_result: String::from("\u{0c32}"),
            m_alt_result: String::from("\u{0c4d}\u{0c32}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_LA
    map.insert(
        "L",
        ResultStr {
            m_result: String::from("\u{0c33}"),
            m_alt_result: String::from("\u{0c4d}\u{0c33}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_LLA
    map.insert(
        ".L",
        ResultStr {
            m_result: String::from("\u{0c34}"),
            m_alt_result: String::from("\u{0c4d}\u{0c34}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_LLLA
    map.insert(
        "v",
        ResultStr {
            m_result: String::from("\u{0c35}"),
            m_alt_result: String::from("\u{0c4d}\u{0c35}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_VA
    map.insert(
        "w",
        ResultStr {
            m_result: String::from("\u{0c35}"),
            m_alt_result: String::from("\u{0c4d}\u{0c35}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_VA
    map.insert(
        "sh",
        ResultStr {
            m_result: String::from("\u{0c36}"),
            m_alt_result: String::from("\u{0c4d}\u{0c36}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_SHA
    map.insert(
        "S",
        ResultStr {
            m_result: String::from("\u{0c36}"),
            m_alt_result: String::from("\u{0c4d}\u{0c36}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_SHA (ITRANS alternate)
    map.insert(
        "Sh",
        ResultStr {
            m_result: String::from("\u{0c37}"),
            m_alt_result: String::from("\u{0c4d}\u{0c37}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_SSA
    map.insert(
        "shh",
        ResultStr {
            m_result: String::from("\u{0c37}"),
            m_alt_result: String::from("\u{0c4d}\u{0c37}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_SSA
    map.insert(
        "s",
        ResultStr {
            m_result: String::from("\u{0c38}"),
            m_alt_result: String::from("\u{0c4d}\u{0c38}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_SA
    map.insert(
        "h",
        ResultStr {
            m_result: String::from("\u{0c39}"),
            m_alt_result: String::from("\u{0c4d}\u{0c39}"),
            m_context: Context::Consonant,
        },
    ); // LETTER_HA
    map.insert(
        "^n",
        ResultStr {
            m_result: String::from("\u{0c3c}"),
            m_alt_result: String::from("\u{0c3c}"),
            m_context: Context::Mark,
        },
    ); // SIGN_NUKTA
    map.insert(
        ".a",
        ResultStr {
            m_result: String::from("\u{0c3d}"),
            m_alt_result: String::from("\u{0c3d}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_AVAGRAHA
    map.insert(
        ".h",
        ResultStr {
            m_result: String::from("\u{0c4d}"),
            m_alt_result: String::from("\u{0c4d}"),
            m_context: Context::Mark,
        },
    ); // SIGN_VIRAMA
    map.insert(
        ".r",
        ResultStr {
            m_result: String::from("\u{0c53}"),
            m_alt_result: String::from("\u{0c53}"),
            m_context: Context::Mark,
        },
    ); // SIGN_GRAVE_ACCENT
    map.insert(
        ".e",
        ResultStr {
            m_result: String::from("\u{0c54}"),
            m_alt_result: String::from("\u{0c54}"),
            m_context: Context::Mark,
        },
    ); // SIGN_ACUTE_ACCENT
    map.insert(
        "XAA",
        ResultStr {
            m_result: String::from("\u{0c55}"),
            m_alt_result: String::from("\u{0c55}"),
            m_context: Context::Vowel,
        },
    ); // LENGTH_MARK
    map.insert(
        "XAI",
        ResultStr {
            m_result: String::from("\u{0c56}"),
            m_alt_result: String::from("\u{0c56}"),
            m_context: Context::Vowel,
        },
    ); // AI_LENGTH_MARK
    map.insert(
        "XAU",
        ResultStr {
            m_result: String::from("\u{0c57}"),
            m_alt_result: String::from("\u{0c57}"),
            m_context: Context::Vowel,
        },
    ); // AU_LENGTH_MARK
    map.insert(
        "R^I",
        ResultStr {
            m_result: String::from("\u{0c60}"),
            m_alt_result: String::from("\u{0c60}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_VOCALIC_RR
    map.insert(
        "L^I",
        ResultStr {
            m_result: String::from("\u{0c61}"),
            m_alt_result: String::from("\u{0c61}"),
            m_context: Context::Vowel,
        },
    ); // LETTER_VOCALIC_LL
    map.insert(
        "XL^i",
        ResultStr {
            m_result: String::from("\u{0c62}"),
            m_alt_result: String::from("\u{0c62}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_VOCALIC_L
    map.insert(
        "XL^I",
        ResultStr {
            m_result: String::from("\u{0c63}"),
            m_alt_result: String::from("\u{0c63}"),
            m_context: Context::Vowel,
        },
    ); // SIGN_VOCALIC_LL
    map.insert(
        "0",
        ResultStr {
            m_result: String::from("\u{0c66}"),
            m_alt_result: String::from("\u{0c66}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_ZERO
    map.insert(
        "1",
        ResultStr {
            m_result: String::from("\u{0c67}"),
            m_alt_result: String::from("\u{0c67}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_ONE
    map.insert(
        "2",
        ResultStr {
            m_result: String::from("\u{0c68}"),
            m_alt_result: String::from("\u{0c68}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_TWO
    map.insert(
        "3",
        ResultStr {
            m_result: String::from("\u{0c69}"),
            m_alt_result: String::from("\u{0c69}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_THREE
    map.insert(
        "4",
        ResultStr {
            m_result: String::from("\u{0c6a}"),
            m_alt_result: String::from("\u{0c6a}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_FOUR
    map.insert(
        "5",
        ResultStr {
            m_result: String::from("\u{0c6b}"),
            m_alt_result: String::from("\u{0c6b}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_FIVE
    map.insert(
        "6",
        ResultStr {
            m_result: String::from("\u{0c6c}"),
            m_alt_result: String::from("\u{0c6c}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_SIX
    map.insert(
        "7",
        ResultStr {
            m_result: String::from("\u{0c6d}"),
            m_alt_result: String::from("\u{0c6d}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_SEVEN
    map.insert(
        "8",
        ResultStr {
            m_result: String::from("\u{0c6e}"),
            m_alt_result: String::from("\u{0c6e}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_EIGHT
    map.insert(
        "9",
        ResultStr {
            m_result: String::from("\u{0c6f}"),
            m_alt_result: String::from("\u{0c6f}"),
            m_context: Context::Mark,
        },
    ); // DIGIT_NINE
    map.insert(
        "^.",
        ResultStr {
            m_result: String::from("\u{0307}"),
            m_alt_result: String::from("\u{0307}"),
            m_context: Context::Mark,
        },
    ); // DOT_ABOVE
    map.insert(
        "_.",
        ResultStr {
            m_result: String::from("\u{0323}"),
            m_alt_result: String::from("\u{0323}"),
            m_context: Context::Mark,
        },
    ); // DOT_BELOW
    map.insert(
        "om",
        ResultStr {
            m_result: String::from("\u{0950}"),
            m_alt_result: String::from("\u{0950}"),
            m_context: Context::Mark,
        },
    ); // SIGN_OM
    map.insert(
        "AUM",
        ResultStr {
            m_result: String::from("\u{0950}"),
            m_alt_result: String::from("\u{0950}"),
            m_context: Context::Mark,
        },
    ); // SIGN_OM
    map.insert(
        "OM",
        ResultStr {
            m_result: String::from("\u{0950}"),
            m_alt_result: String::from("\u{0950}"),
            m_context: Context::Mark,
        },
    ); // SIGN_OM
    map.insert(
        "\'",
        ResultStr {
            m_result: String::from("\u{0951}"),
            m_alt_result: String::from("\u{0951}"),
            m_context: Context::Mark,
        },
    ); // SIGN_SWARITA
    map.insert(
        "\\_",
        ResultStr {
            m_result: String::from("\u{0952}"),
            m_alt_result: String::from("\u{0952}"),
            m_context: Context::Mark,
        },
    ); // SIGN_ANUDATTA
    map.insert(
        "|",
        ResultStr {
            m_result: String::from("\u{0964}"),
            m_alt_result: String::from("\u{0964}"),
            m_context: Context::Mark,
        },
    ); // SIGN_DANDA
    map.insert(
        "||",
        ResultStr {
            m_result: String::from("\u{0965}"),
            m_alt_result: String::from("\u{0965}"),
            m_context: Context::Mark,
        },
    ); // SIGN_DOUBLE_DANDA
    map.insert(
        "cs",
        ResultStr {
            m_result: String::from("\u{0C58}"),
            m_alt_result: String::from("\u{0C58}"),
            m_context: Context::Consonant,
        },
    ); // TELUGU_LETTER_TSA
    map.insert(
        "dz",
        ResultStr {
            m_result: String::from("\u{0C59}"),
            m_alt_result: String::from("\u{0C59}"),
            m_context: Context::Consonant,
        },
    ); // TELUGU_LETTER_DZA
    map.insert(
        ".H",
        ResultStr {
            m_result: String::from("\u{0C78}"),
            m_alt_result: String::from("\u{0C78}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_ZERO_FOR_ODD_POWERS_OF_FOUR
    map.insert(
        "1/4o",
        ResultStr {
            m_result: String::from("\u{0C79}"),
            m_alt_result: String::from("\u{0C79}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_ONE_FOR_ODD_POWERS_OF_FOUR
    map.insert(
        "2/4o",
        ResultStr {
            m_result: String::from("\u{0C7A}"),
            m_alt_result: String::from("\u{0C7A}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_TWO_FOR_ODD_POWERS_OF_FOUR
    map.insert(
        "3/4o",
        ResultStr {
            m_result: String::from("\u{0C7B}"),
            m_alt_result: String::from("\u{0C7B}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_THREE_FOR_ODD_POWERS_OF_FOUR
    map.insert(
        "1/4e",
        ResultStr {
            m_result: String::from("\u{0C7C}"),
            m_alt_result: String::from("\u{0C7C}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_ONE_FOR_EVEN_POWERS_OF_FOUR
    map.insert(
        "2/4e",
        ResultStr {
            m_result: String::from("\u{0C7D}"),
            m_alt_result: String::from("\u{0C7D}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_TWO_FOR_EVEN_POWERS_OF_FOUR
    map.insert(
        "3/4e",
        ResultStr {
            m_result: String::from("\u{0C7E}"),
            m_alt_result: String::from("\u{0C7E}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_FRACTION_DIGIT_THREE_FOR_EVEN_POWERS_OF_FOUR
    map.insert(
        ".t",
        ResultStr {
            m_result: String::from("\u{0C7F}"),
            m_alt_result: String::from("\u{0C7F}"),
            m_context: Context::Mark,
        },
    ); // TELUGU_SIGN_TUUMU
    map.insert(
        "\\kar",
        ResultStr {
            m_result: String::from("\u{1CD0}"),
            m_alt_result: String::from("\u{1CD0}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_KARSHANA
    map.insert(
        "\\shr",
        ResultStr {
            m_result: String::from("\u{1CD1}"),
            m_alt_result: String::from("\u{1CD1}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_SHARA
    map.insert(
        "\\prn",
        ResultStr {
            m_result: String::from("\u{1CD2}"),
            m_alt_result: String::from("\u{1CD2}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_PRENKHA
    map.insert(
        "\\nih",
        ResultStr {
            m_result: String::from("\u{1CD3}"),
            m_alt_result: String::from("\u{1CD3}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_NIHSHVASA
    map.insert(
        "\\yms",
        ResultStr {
            m_result: String::from("\u{1CD4}"),
            m_alt_result: String::from("\u{1CD4}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_YAJURVEDIC_MIDLINE_SVARITA
    map.insert(
        "\\yais",
        ResultStr {
            m_result: String::from("\u{1CD5}"),
            m_alt_result: String::from("\u{1CD5}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_YAJURVEDIC_AGGRAVATED_INDEPENDENT_SVARITA
    map.insert(
        "\\yis",
        ResultStr {
            m_result: String::from("\u{1CD6}"),
            m_alt_result: String::from("\u{1CD6}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_YAJURVEDIC_INDEPENDENT_SVARITA
    map.insert(
        "\\ykis",
        ResultStr {
            m_result: String::from("\u{1CD7}"),
            m_alt_result: String::from("\u{1CD7}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_YAJURVEDIC_KATHAKA_INDEPENDENT_SVARITA
    map.insert(
        "\\cb",
        ResultStr {
            m_result: String::from("\u{1CD8}"),
            m_alt_result: String::from("\u{1CD8}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_CANDRA_BELOW
    map.insert(
        "\\ykiss",
        ResultStr {
            m_result: String::from("\u{1CD9}"),
            m_alt_result: String::from("\u{1CD9}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_YAJURVEDIC_KATHAKA_INDEPENDENT_SVARITA_SCHROEDER
    map.insert(
        "\"",
        ResultStr {
            m_result: String::from("\u{1CDA}"),
            m_alt_result: String::from("\u{1CDA}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_DOUBLE_SVARITA
    map.insert(
        "\\3s",
        ResultStr {
            m_result: String::from("\u{1CDB}"),
            m_alt_result: String::from("\u{1CDB}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_TRIPLE_SVARITA
    map.insert(
        "\\ka",
        ResultStr {
            m_result: String::from("\u{1CDC}"),
            m_alt_result: String::from("\u{1CDC}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_KATHAKA_ANUDATTA
    map.insert(
        "\\.b",
        ResultStr {
            m_result: String::from("\u{1CDD}"),
            m_alt_result: String::from("\u{1CDD}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_DOT_BELOW
    map.insert(
        "\\..b",
        ResultStr {
            m_result: String::from("\u{1CDE}"),
            m_alt_result: String::from("\u{1CDE}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_TWO_DOTS_BELOW
    map.insert(
        "\\2.b",
        ResultStr {
            m_result: String::from("\u{1CDE}"),
            m_alt_result: String::from("\u{1CDE}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_TWO_DOTS_BELOW
    map.insert(
        "\\3.b",
        ResultStr {
            m_result: String::from("\u{1CDF}"),
            m_alt_result: String::from("\u{1CDF}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_THREE_DOTS_BELOW
    map.insert(
        "\rkis",
        ResultStr {
            m_result: String::from("\u{1CE0}"),
            m_alt_result: String::from("\u{1CE0}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_RIGVEDIC_KASHMIRI_INDEPENDENT_SVARITA
    map.insert(
        "\\ais",
        ResultStr {
            m_result: String::from("\u{1CE1}"),
            m_alt_result: String::from("\u{1CE1}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_TONE_ATHARVAVEDIC_INDEPENDENT_SVARITA
    map.insert(
        "\\vs",
        ResultStr {
            m_result: String::from("\u{1CE2}"),
            m_alt_result: String::from("\u{1CE2}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_VISARGA_SVARITA
    map.insert(
        "\\vu",
        ResultStr {
            m_result: String::from("\u{1CE3}"),
            m_alt_result: String::from("\u{1CE3}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_VISARGA_UDATTA
    map.insert(
        "\rvu",
        ResultStr {
            m_result: String::from("\u{1CE4}"),
            m_alt_result: String::from("\u{1CE4}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_REVERSED_VISARGA_UDATTA
    map.insert(
        "\\van",
        ResultStr {
            m_result: String::from("\u{1CE5}"),
            m_alt_result: String::from("\u{1CE5}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_VISARGA_ANUDATTA
    map.insert(
        "\rvan",
        ResultStr {
            m_result: String::from("\u{1CE6}"),
            m_alt_result: String::from("\u{1CE6}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_REVERSED_VISARGA_ANUDATTA
    map.insert(
        "\\vut",
        ResultStr {
            m_result: String::from("\u{1CE7}"),
            m_alt_result: String::from("\u{1CE7}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_VISARGA_UDATTA_WITH_TAIL
    map.insert(
        "\\vat",
        ResultStr {
            m_result: String::from("\u{1CE8}"),
            m_alt_result: String::from("\u{1CE8}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_VISARGA_ANUDATTA_WITH_TAIL
    map.insert(
        "\\aag",
        ResultStr {
            m_result: String::from("\u{1CE9}"),
            m_alt_result: String::from("\u{1CE9}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ANUSVARA_ANTARGOMUKHA
    map.insert(
        "\\abg",
        ResultStr {
            m_result: String::from("\u{1CEA}"),
            m_alt_result: String::from("\u{1CEA}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ANUSVARA_BAHIRGOMUKHA
    map.insert(
        "\\avg",
        ResultStr {
            m_result: String::from("\u{1CEB}"),
            m_alt_result: String::from("\u{1CEB}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ANUSVARA_VAMAGOMUKHA
    map.insert(
        "\\avgt",
        ResultStr {
            m_result: String::from("\u{1CEC}"),
            m_alt_result: String::from("\u{1CEC}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ANUSVARA_VAMAGOMUKHA_WITH_TAIL
    map.insert(
        "\\3k",
        ResultStr {
            m_result: String::from("\u{1CED}"),
            m_alt_result: String::from("\u{1CED}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_TIRYAK
    map.insert(
        "\\6n",
        ResultStr {
            m_result: String::from("\u{1CEE}"),
            m_alt_result: String::from("\u{1CEE}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_HEXIFORM_LONG_ANUSVARA
    map.insert(
        "\\ln",
        ResultStr {
            m_result: String::from("\u{1CEF}"),
            m_alt_result: String::from("\u{1CEF}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_LONG_ANUSVARA
    map.insert(
        "\rln",
        ResultStr {
            m_result: String::from("\u{1CF0}"),
            m_alt_result: String::from("\u{1CF0}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_RTHANG_LONG_ANUSVARA
    map.insert(
        "\num",
        ResultStr {
            m_result: String::from("\u{1CF1}"),
            m_alt_result: String::from("\u{1CF1}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ANUSVARA_UBHAYATO_MUKHA
    map.insert(
        "\\avs",
        ResultStr {
            m_result: String::from("\u{1CF2}"),
            m_alt_result: String::from("\u{1CF2}"),
            m_context: Context::Mark,
        },
    ); // VEDIC_SIGN_ARDHAVISARGA

    return map;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii() -> Result<(), String> {
        assert_eq!(tel2hin('a'), 'a');
        Ok(())
    }
    #[test]
    fn test_tel() -> Result<(), String> {
        assert_eq!(tel2hin('\u{0c02}'), '\u{0902}');
        Ok(())
    }

    #[test]
    fn test_hin() -> Result<(), String> {
        assert_eq!(tel2hin('\u{0902}'), '\u{0902}');
        Ok(())
    }
}
