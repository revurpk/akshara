// Every user-visible string passed to t() must be translated into every
// supported language, with the same {placeholders}.

import 'dart:io';

import 'package:akshara/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';

Set<String> keysUsedInCode() {
  final keys = <String>{};
  final pattern = RegExp(r"""\bt\(\s*'((?:[^'\\]|\\.)*)'""");
  for (final f in Directory('lib').listSync(recursive: true).whereType<File>()) {
    if (!f.path.endsWith('.dart') || f.path.contains('${Platform.pathSeparator}l10n${Platform.pathSeparator}')) continue;
    for (final m in pattern.allMatches(f.readAsStringSync())) {
      keys.add(m.group(1)!.replaceAll(r"\'", "'"));
    }
  }
  // Keys used indirectly (enum labels, palette entries, theme names).
  return keys;
}

void main() {
  test('all languages are offered, English first, in native script', () {
    expect(AppLanguage.values.map((l) => l.code).toList(), ['en', 'hi', 'sa', 'te', 'ta', 'kn', 'ml', 'or', 'bn']);
    expect(AppLanguage.sa.materialCode, 'hi');
    expect(AppLanguage.fromCode('xx'), AppLanguage.en);
  });

  test('every key used in the code is translated everywhere', () {
    final used = keysUsedInCode();
    expect(used.length, greaterThan(100));
    for (final entry in translations.entries) {
      final missing = used.where((k) => !entry.value.containsKey(k)).toList();
      expect(missing, isEmpty, reason: 'missing ${entry.key} translations');
    }
  });

  test('translations keep placeholders and are not left in English', () {
    final ph = RegExp(r'\{\w+\}');
    for (final entry in translations.entries) {
      var same = 0;
      for (final kv in entry.value.entries) {
        final a = ph.allMatches(kv.key).map((m) => m.group(0)).toList()..sort();
        final b = ph.allMatches(kv.value).map((m) => m.group(0)).toList()..sort();
        expect(b, a, reason: '${entry.key}: ${kv.key}');
        if (kv.key == kv.value) same++;
      }
      // A few technical terms (MathML, sin, cos...) legitimately stay.
      expect(same, lessThan(12), reason: '${entry.key} has too many untranslated strings');
    }
  });

  test('lookup substitutes arguments and falls back to English', () {
    setLanguage(AppLanguage.hi);
    expect(t('Save'), 'सहेजें');
    expect(t('Saved {name}', {'name': 'a.odt'}), 'a.odt सहेजा गया');
    expect(t('Not a translated key'), 'Not a translated key');
    setLanguage(AppLanguage.ta);
    expect(t('Open document'), 'ஆவணத்தைத் திற');
    setLanguage(AppLanguage.en);
    expect(t('Untitled {kind} {n}', {'kind': 'Formula', 'n': 2}), 'Untitled Formula 2');
  });
}
