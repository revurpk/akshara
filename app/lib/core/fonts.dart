// Bundled fonts (Google Noto, SIL Open Font License 1.1; see
// assets/fonts/OFL.txt). Bundling them means Indic scripts and maths render
// identically everywhere, with no network access and no system font needed.

/// Fallback chain covering the supported Indic scripts and maths symbols.
const indicFallback = <String>[
  'NotoSansDevanagari', // Hindi, Sanskrit, Marathi, Nepali...
  'NotoSansBengali', // Bengali, Assamese
  'NotoSansOriya', // Odia
  'NotoSansTelugu',
  'NotoSansTamil',
  'NotoSansKannada',
  'NotoSansMalayalam',
  'NotoSansMath',
];

/// User interface font.
const uiFont = 'NotoSans';

/// Document body font (Writer pages, Math formulas).
const docFont = 'NotoSerif';

/// Document fallback: serif Devanagari first (better for Sanskrit and
/// Hindi body text), then the sans families.
const docFallback = <String>['NotoSerifDevanagari', ...indicFallback];

/// Monospaced font for code and formula sources.
const monoFont = 'NotoSansMono';
