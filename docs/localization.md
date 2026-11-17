# Localization

Akshara's interface is available in English, Hindi (हिन्दी), Sanskrit
(संस्कृतम्), Telugu (తెలుగు), Tamil (தமிழ்), Kannada (ಕನ್ನಡ), Malayalam (മലയാളം),
Odia (ଓଡ଼ିଆ) and Bengali (বাংলা).

## How it works

- `app/lib/l10n/l10n.dart` defines `AppLanguage` and `t()`. UI code writes
  `t('Save')` or `t('Saved {name}', {'name': file})`.
- Keys are the **English text itself**, so a missing translation shows
  English instead of a cryptic identifier.
- Each language has one table: `app/lib/l10n/<code>.dart` (`hi`, `sa`, `te`,
  `ta`, `kn`, `ml`, `or`, `bn`).
- Flutter's own widgets (date pickers, text selection menus) use Flutter's
  localizations. Flutter has none for Sanskrit, so those few built-in widgets
  appear in Hindi (same script) when Sanskrit is selected.
- The bundled Noto fonts render every script; see `app/lib/core/fonts.dart`.

## Correcting a translation

Edit the value in the language's table and keep any `{placeholders}`
exactly as they are. Then run:

```bash
cd app && flutter test test/l10n_test.dart
```

The test fails if any key used in the code is missing from a table, if
placeholders differ, or if a table has too many strings left in English.

The current translations were drafted with machine assistance and checked
for terminology consistency (for example, common Indian school terms for
mathematics: भिन्न, वर्गमूल, समाकल). Native-speaker review is welcome,
especially for Sanskrit UI terms, which have no established software
vocabulary.

## Adding a language

1. Create `app/lib/l10n/<code>.dart` with `const Map<String, String>
   <code>Strings = {...}` containing every key (copying `hi.dart` is a good
   start).
2. Add an `AppLanguage` entry (code, native name, English name) and the table
   to `translations` in `l10n.dart`.
3. If the script is not yet covered, add the Noto font for it to
   `app/assets/fonts`, `pubspec.yaml` and `indicFallback` in
   `core/fonts.dart`.
4. Optionally add `Name[<code>]` and `Comment[<code>]` lines to
   `packaging/linux/org.akshara.Office.desktop`.
5. Run the tests.
