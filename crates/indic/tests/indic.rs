//! Behaviour of the Indic transliteration layer built on teltools.

use office_indic::*;
use proptest::prelude::*;
use teltools::TransOptions;

fn tr(s: &str, script: Script) -> String {
    transliterate(s, script, &Options::default())
}

#[test]
fn telugu_output_is_exactly_teltools() {
    // For Telugu the layer is teltools itself (live mode, ASCII digits off).
    let native = Options { native_digits: true, ..Options::default() };
    for s in ["satyamEva jayatE", "aMdamaina telugu bhASha", "kR^iShNa", "dharmaH", "rAmuDu 108", "\\om namaH shivAya"] {
        assert_eq!(transliterate(s, Script::Telugu, &native), teltools::trans_with(s, &TransOptions::LIVE), "{s}");
    }
    assert_eq!(tr("satyamEva jayatE", Script::Telugu), "సత్యమేవ జయతే");
}

#[test]
fn batch_mode_reproduces_upstream_corpus_for_telugu() {
    let corpus = include_str!("../../../third_party/teltools/tests/data/corpus_trans.txt");
    let expected = include_str!("../../../third_party/teltools/tests/data/expected_trans.txt");
    let batch = Options { native_digits: true, teltools: TransOptions::BATCH };
    // e.g. candra e/o (U+0C0D, U+0C11), NNNA (U+0C29), accents (U+0C53/54).
    let unassigned = |c: char| Script::of_char(c) == Some(Script::Telugu) && !Script::is_assigned(c);
    for (src, exp) in corpus.lines().zip(expected.lines()) {
        // teltools emits a few *unassigned* Telugu code points; the layer
        // replaces those with real letters, which is the only allowed change.
        let got = transliterate(src, Script::Telugu, &batch);
        assert!(!got.chars().any(unassigned), "{src}: {got:?}");
        if exp.chars().any(unassigned) {
            // Every real character upstream emits appears, in order.
            let mut rest = got.chars();
            for e in exp.chars().filter(|c| !unassigned(*c)) {
                assert!(rest.any(|g| g == e), "{src}: {got:?} lacks {e:?} from {exp:?}");
            }
        } else {
            assert_eq!(got, exp, "{src}");
        }
    }
}

#[test]
fn hindi_and_sanskrit() {
    assert_eq!(tr("namastE", Script::Devanagari), "नमस्ते");
    assert_eq!(tr("namaste", Script::Devanagari), "नमस्ते", "typed e is ए in Hindi");
    assert_eq!(tr("bhArata", Script::Devanagari), "भारत");
    assert_eq!(tr("hindI", Script::Devanagari), "हिन्दी");
    assert_eq!(tr("saMskRutam", Script::Devanagari).chars().next(), Some('स'));
    assert_eq!(tr("saMskR^itam", Script::Devanagari), "संस्कृतम्");
    assert_eq!(tr("sarvE bhavaMtu sukhinaH", Script::Devanagari), "सर्वे भवंतु सुखिनः");
    assert_eq!(tr("kShatriya", Script::Devanagari), "क्षत्रिय");
    assert_eq!(tr("D^okTara", Script::Devanagari), "डॉक्टर", "candra o for English loanwords");
    assert_eq!(tr("\\om", Script::Devanagari), "ॐ");
    assert_eq!(tr("rAma ||", Script::Devanagari), "राम ॥");
}

#[test]
fn tamil_fallbacks() {
    assert_eq!(tr("vaNakkam", Script::Tamil), "வணக்கம்");
    assert_eq!(tr("tamiL", Script::Tamil), "தமிள்");
    assert_eq!(tr("tami.L", Script::Tamil), "தமிழ்");
    assert_eq!(tr("gaMgA", Script::Tamil), "கம்கா", "voiced g -> க, anusvara -> ம்");
    assert_eq!(tr("bhArata", Script::Tamil), "பாரத");
    assert_eq!(tr("kR^iShNa", Script::Tamil), "க்ருஷ்ண");
    assert_eq!(tr("N.aN.Ru", Script::Tamil), "னன்று", "N. is ன, n is ந");
    assert_eq!(tr("naN.Ri", Script::Tamil), "நன்றி");
    assert_eq!(tr("\\om", Script::Tamil), "ௐ");
    assert_eq!(tr("dharmaH", Script::Tamil), "தர்மஃ");
}

#[test]
fn kannada_malayalam_odia_bengali() {
    assert_eq!(tr("kannaDa", Script::Kannada), "ಕನ್ನಡ");
    assert_eq!(tr("namaskAra", Script::Kannada), "ನಮಸ್ಕಾರ");
    assert_eq!(tr("ma.Le", Script::Kannada), "ಮೞೆ");
    assert_eq!(tr("malayALaM", Script::Malayalam), "മലയാളം");
    assert_eq!(tr("malayALam", Script::Malayalam), "മലയാളമ്", "final m is a bare consonant");
    assert_eq!(tr("namaskAraM", Script::Malayalam), "നമസ്കാരം");
    assert_eq!(tr("oDiA", Script::Oriya), "ଓଡିଆ");
    assert_eq!(tr("namaskAra", Script::Oriya), "ନମସ୍କାର");
    assert_eq!(tr("bAMlA", Script::Bengali), "বাংলা");
    assert_eq!(tr("namaskAra", Script::Bengali), "নমস্কার");
    assert_eq!(tr("vEda", Script::Bengali), "বেদ", "Bengali writes va as ব");
    assert_eq!(tr("kela", Script::Bengali), "কেল", "no short e: falls back to ে");
}

#[test]
fn digits_are_ascii_unless_asked() {
    assert_eq!(tr("rAma 108", Script::Devanagari), "राम 108");
    let native = Options { native_digits: true, ..Options::default() };
    assert_eq!(transliterate("108", Script::Devanagari, &native), "१०८");
    assert_eq!(transliterate("108", Script::Telugu, &native), "౧౦౮");
    assert_eq!(transliterate("2026", Script::Bengali, &native), "২০২৬");
    assert_eq!(transliterate("42", Script::Tamil, &native), "௪௨");
}

#[test]
fn live_mode_om_and_tags() {
    assert_eq!(tr("komma", Script::Telugu), "కొమ్మ");
    assert_eq!(tr("sOma", Script::Devanagari), "सोम");
    assert_eq!(tr("a<b>", Script::Telugu), "అ<బ్>");
}

#[test]
fn mixed_text_keeps_non_ascii() {
    assert_eq!(tr("రామ rAma 😀", Script::Devanagari), "రామ राम 😀");
    assert_eq!(tr("", Script::Tamil), "");
}

#[test]
fn script_conversion() {
    let gita = "ధర్మక్షేత్రే కురుక్షేత్రే సమవేతా యుయుత్సవః";
    assert_eq!(convert_script(gita, Script::Devanagari), "धर्मक्षेत्रे कुरुक्षेत्रे समवेता युयुत्सवः");
    assert_eq!(convert_script(gita, Script::Kannada), "ಧರ್ಮಕ್ಷೇತ್ರೇ ಕುರುಕ್ಷೇತ್ರೇ ಸಮವೇತಾ ಯುಯುತ್ಸವಃ");
    assert_eq!(convert_script(gita, Script::Malayalam), "ധര്മക്ഷേത്രേ കുരുക്ഷേത്രേ സമവേതാ യുയുത്സവഃ");
    // Short e survives Telugu -> Devanagari conversion (ISCII-exact).
    assert_eq!(convert_script("ఎల్ల", Script::Devanagari), "ऎल्ल");
    // ...but not into Bengali, which has no short e.
    assert_eq!(convert_script("ఎల్ల", Script::Bengali), "এল্ল");
    // Devanagari nukta letters and Malayalam chillus.
    assert_eq!(convert_script("\u{095B}रा", Script::Telugu), "జ\u{0C3C}రా");
    assert_eq!(convert_script("ज\u{093C}रा", Script::Telugu), "జ\u{0C3C}రా", "decomposed form too");
    assert_eq!(convert_script("അവൻ", Script::Telugu), "అవన్");
    assert_eq!(convert_script("ৎ", Script::Devanagari), "त्");
    // Tamil OM and Latin text.
    assert_eq!(convert_script("ௐ ok", Script::Devanagari), "ॐ ok");
    // Digits convert between scripts.
    assert_eq!(convert_script("౧౨౩", Script::Devanagari), "१२३");
}

#[test]
fn iast() {
    assert_eq!(to_iast("धर्मः"), "dharmaḥ");
    assert_eq!(to_iast("సత్యమేవ జయతే"), "satyameva jayate");
    assert_eq!(to_iast("ఎల్ల"), "ella", "short e romanized as e");
    assert_eq!(to_iast("தமிழ்"), "tamiḷ", "ழ romanized via ळ");
    assert_eq!(to_iast("Hello, ভারত!"), "Hello, bhārata!");
    assert_eq!(from_iast("kṛṣṇaṁ vande jagadgurum", Script::Telugu), "కృష్ణం వన్దే జగద్గురుమ్");
    assert_eq!(from_iast("śrī", Script::Devanagari), "श्री");
    assert_eq!(from_iast("dharma", Script::Tamil), "தர்ம");
}

#[test]
fn detection() {
    assert_eq!(detect_script("hello"), None);
    assert_eq!(detect_script("नमस्ते and నమస్తే అందరికీ"), Some(Script::Telugu));
    assert_eq!(detect_script("தமிழ்"), Some(Script::Tamil));
}

#[test]
fn roman_input_characters() {
    for c in ['a', 'Z', '0', '~', '^', '.', '|', '\\', '_', '/', '\'', '"'] {
        assert!(is_roman_input(c), "{c}");
    }
    for c in [' ', ',', '!', '?', '(', '<', 'é', 'క', '\n'] {
        assert!(!is_roman_input(c), "{c:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn never_panics_on_any_text(s in "\\PC{0,60}", i in 0usize..7) {
        let script = Script::ALL[i];
        let _ = transliterate(&s, script, &Options::default());
        let _ = transliterate(&s, script, &Options { native_digits: true, teltools: TransOptions::BATCH });
        let _ = convert_script(&s, script);
        let _ = to_iast(&s);
        let _ = from_iast(&s, script);
    }

    #[test]
    fn output_stays_in_the_target_script(s in "[a-zA-Z~^.| ]{0,30}", i in 0usize..7) {
        let script = Script::ALL[i];
        let out = transliterate(&s, script, &Options::default());
        for c in out.chars() {
            let in_block = Script::of_char(c) == Some(script);
            let ok = c.is_ascii() || in_block || matches!(c, 'ॐ' | 'ௐ' | '।' | '॥' | 'ऽ' | '\u{0951}'..='\u{0954}')
                || (script == Script::Kannada && c == 'ೞ')
                // Telugu fractions/tuumu have no counterpart and are kept.
                || ('\u{0C78}'..='\u{0C7F}').contains(&c)
                // Generic combining marks and Vedic signs from the scheme.
                || ('\u{0300}'..='\u{036F}').contains(&c)
                || ('\u{1CD0}'..='\u{1CFF}').contains(&c);
            prop_assert!(ok, "{:?} -> {:?}: stray {:?}", s, out, c);
            // Never an unassigned code point of the target block.
            if in_block {
                prop_assert!(Script::is_assigned(c), "unassigned {:?} in {:?}", c, out);
            }
        }
    }

    #[test]
    fn telugu_devanagari_round_trip(s in "[abdeghijklmnoprstuvwxyADEIKLMNORSTUH ]{0,30}") {
        let telugu = transliterate(&s, Script::Telugu, &Options::default());
        let deva = convert_script(&telugu, Script::Devanagari);
        prop_assert_eq!(convert_script(&deva, Script::Telugu), telugu);
    }
}
