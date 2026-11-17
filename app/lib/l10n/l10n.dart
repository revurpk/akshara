// User interface languages and string lookup.
//
// Strings are keyed by their English text, so untranslated keys fall back to
// English automatically. Add a language by adding a table in this folder and
// an entry to [AppLanguage].

import 'dart:ui' show PlatformDispatcher;

import 'bn.dart';
import 'hi.dart';
import 'kn.dart';
import 'ml.dart';
import 'or.dart';
import 'sa.dart';
import 'ta.dart';
import 'te.dart';

/// Supported interface languages, in the order shown to users.
enum AppLanguage {
  en('en', 'English', 'English'),
  hi('hi', 'हिन्दी', 'Hindi'),
  sa('sa', 'संस्कृतम्', 'Sanskrit'),
  te('te', 'తెలుగు', 'Telugu'),
  ta('ta', 'தமிழ்', 'Tamil'),
  kn('kn', 'ಕನ್ನಡ', 'Kannada'),
  ml('ml', 'മലയാളം', 'Malayalam'),
  or('or', 'ଓଡ଼ିଆ', 'Odia'),
  bn('bn', 'বাংলা', 'Bengali');

  AppLanguage(this.code, this.nativeName, this.englishName);

  final String code;
  final String nativeName;
  final String englishName;

  /// Locale used for Flutter's built-in widgets. Flutter has no Sanskrit
  /// localisation, so built-in widgets use Hindi (same script) instead.
  String get materialCode => this == AppLanguage.sa ? 'hi' : code;

  static AppLanguage fromCode(String? code) =>
      AppLanguage.values.firstWhere((l) => l.code == code, orElse: () => AppLanguage.en);

  /// The operating system's language if supported, else English.
  static AppLanguage system() {
    for (final locale in PlatformDispatcher.instance.locales) {
      for (final l in AppLanguage.values) {
        if (l.code == locale.languageCode) return l;
      }
    }
    return AppLanguage.en;
  }
}

/// All translation tables, by language code.
const Map<String, Map<String, String>> translations = {
  'hi': hiStrings,
  'sa': saStrings,
  'te': teStrings,
  'ta': taStrings,
  'kn': knStrings,
  'ml': mlStrings,
  'or': orStrings,
  'bn': bnStrings,
};

AppLanguage _current = AppLanguage.en;

AppLanguage get currentLanguage => _current;

void setLanguage(AppLanguage language) => _current = language;

/// Translates [key] (English text) into the current language. `{name}`
/// placeholders are replaced from [args].
String t(String key, [Map<String, Object?> args = const {}]) {
  var s = translations[_current.code]?[key] ?? key;
  for (final e in args.entries) {
    s = s.replaceAll('{${e.key}}', '${e.value}');
  }
  return s;
}
