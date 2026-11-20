//! Whisper's language table. The position of a code is its token offset after
//! the start-of-transcript token, so the order must not change.

/// Every language code Whisper knows, in token order (the last, `yue`, only
/// exists in large-v3 models).
pub const LANGUAGES: [&str; 100] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it", "id", "hi", "fi",
    "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml",
    "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs",
    "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am",
    "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln",
    "ha", "ba", "jw", "su", "yue",
];

/// The languages Akshara offers for voice input, in menu order.
pub const VOICE_LANGUAGES: [&str; 4] = ["en", "te", "hi", "sa"];

/// The English name of a supported voice language, or the code itself.
pub fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "te" => "Telugu",
        "hi" => "Hindi",
        "sa" => "Sanskrit",
        other => other,
    }
}

/// Index of `code` in [`LANGUAGES`].
pub(crate) fn index(code: &str) -> Option<usize> {
    LANGUAGES.iter().position(|&c| c == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_order() {
        assert_eq!(index("en"), Some(0));
        assert_eq!(index("hi"), Some(17));
        assert_eq!(index("te"), Some(40));
        assert_eq!(index("sa"), Some(85));
        assert_eq!(index("yue"), Some(99));
    }
}
