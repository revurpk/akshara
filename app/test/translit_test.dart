// Live transliteration (teltools) through the real Rust core.

import 'dart:io';

import 'package:akshara/calc/calc_page.dart';
import 'package:akshara/core/document.dart';
import 'package:akshara/core/native.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/core/translit.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/math/math_page.dart';
import 'package:akshara/writer/writer_page.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'helpers.dart';

/// Types [keys] one by one through [f], as a keyboard would, and returns
/// the final value.
TextEditingValue typeKeys(TransliterationFormatter f, String keys, [TextEditingValue? start]) {
  var value = start ?? TextEditingValue.empty;
  for (final ch in keys.split('')) {
    final caret = value.selection.isValid ? value.selection.baseOffset : value.text.length;
    final next = TextEditingValue(
      text: value.text.replaceRange(caret, caret, ch),
      selection: TextSelection.collapsed(offset: caret + 1),
    );
    value = f.formatEditUpdate(value, next);
  }
  return value;
}

TextEditingValue backspace(TransliterationFormatter f, TextEditingValue value) {
  final caret = value.selection.baseOffset;
  return f.formatEditUpdate(
    value,
    TextEditingValue(
      text: value.text.replaceRange(caret - 1, caret, ''),
      selection: TextSelection.collapsed(offset: caret - 1),
    ),
  );
}

void main() {
  final skip = useBuiltCore() == null;
  final settings = TranslitSettings.instance;

  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-translit-').path;
    settings.load(Settings.load(), AppLanguage.en);
  });

  setUp(() {
    setLanguage(AppLanguage.en);
    settings.setNativeDigits(false);
    settings.use('telu');
  });

  tearDown(() => settings.setEnabled(false));

  test('typing Roman letters produces Telugu, matching teltools', () {
    final f = TransliterationFormatter();
    expect(typeKeys(f, 'satyamEva jayatE').text, 'సత్యమేవ జయతే');
    expect(typeKeys(TransliterationFormatter(), 'kR^iShNa').text, 'కృష్ణ');
    expect(typeKeys(TransliterationFormatter(), 'komma').text, 'కొమ్మ', reason: 'live mode: om is not ॐ');
    expect(typeKeys(TransliterationFormatter(), '\\om namaH').text, 'ॐ నమః');
  }, skip: skip);

  test('every supported script', () {
    final cases = {
      'deva': ['namastE', 'नमस्ते'],
      'taml': ['vaNakkam', 'வணக்கம்'],
      'knda': ['kannaDa', 'ಕನ್ನಡ'],
      'mlym': ['malayALaM', 'മലയാളം'],
      'orya': ['namaskAra', 'ନମସ୍କାର'],
      'beng': ['bAMlA', 'বাংলা'],
    };
    for (final e in cases.entries) {
      settings.use(e.key);
      expect(typeKeys(TransliterationFormatter(), e.value[0]).text, e.value[1], reason: e.key);
    }
  }, skip: skip);

  test('backspace removes the last Roman letter; punctuation ends the word', () {
    final f = TransliterationFormatter();
    var v = typeKeys(f, 'kha');
    expect(v.text, 'ఖ');
    v = backspace(f, v); // 'kh'
    expect(v.text, 'ఖ్');
    v = backspace(f, v); // 'k'
    expect(v.text, 'క్');
    v = backspace(f, v); // ''
    expect(v.text, '');
    v = typeKeys(f, 'rAma, sItA!');
    expect(v.text, 'రామ, సీతా!');
    expect(v.selection.baseOffset, v.text.length);
  }, skip: skip);

  test('caret moves end the word; text around the word is kept', () {
    final f = TransliterationFormatter();
    var v = typeKeys(f, 'ab', const TextEditingValue(text: 'X Y', selection: TextSelection.collapsed(offset: 2)));
    expect(v.text, 'X అబ్Y');
    // Moving the caret to the start and typing begins a new word there.
    v = typeKeys(f, 'ka', v.copyWith(selection: const TextSelection.collapsed(offset: 0)));
    expect(v.text, 'కX అబ్Y');
  }, skip: skip);

  test('quotes, digits and the off switch', () {
    var v = typeKeys(TransliterationFormatter(), '"rAma" 108');
    expect(v.text, '"రామ" 108', reason: 'quotes are punctuation in live typing; digits stay ASCII');
    settings.setNativeDigits(true);
    v = typeKeys(TransliterationFormatter(), '108');
    expect(v.text, '౧౦౮');
    settings.setEnabled(false);
    v = typeKeys(TransliterationFormatter(), 'rAma');
    expect(v.text, 'rAma');
  }, skip: skip);

  test('quote-only mode for formulas and Math', () {
    final f = TransliterationFormatter(allowAt: insideQuotes);
    expect(typeKeys(f, 'sqrt{x} + "vargamUlaM"').text, 'sqrt{x} + "వర్గమూలం"');
  }, skip: skip);

  testWidgets('Writer paragraph transliterates as you type', (tester) async {
    final r = core({'op': 'new', 'kind': 'writer'}) as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.writer, null);
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: WriterPage(doc: doc))));
    await tester.pumpAndSettle();
    final field = find.byType(TextField).first;
    await tester.tap(field);
    final controller = tester.widget<TextField>(field).controller!;
    for (final ch in 'namaskAraM'.split('')) {
      await tester.enterText(field, controller.text + ch);
      await tester.pump();
    }
    await tester.pumpAndSettle();
    expect(controller.text, 'నమస్కారం');
    final got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect(got['blocks'][0]['runs'][0]['text'], 'నమస్కారం', reason: 'the core receives the converted text');
  }, skip: skip);

  testWidgets('Calc: text is transliterated, formulas are not', (tester) async {
    final view = tester.platformDispatcher.views.first;
    view.physicalSize = const Size(1600, 1200);
    view.devicePixelRatio = 1.0;
    settings.use('deva');
    final r = core({'op': 'new', 'kind': 'calc'}) as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.calc, null);
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: CalcPage(doc: doc))));
    await tester.pumpAndSettle();
    for (final ch in 'bhArata'.split('')) {
      await tester.sendKeyEvent(LogicalKeyboardKey(ch.toLowerCase().codeUnitAt(0)), character: ch);
    }
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A1'})['display'], 'भारत');
    // A formula keeps its function name; only the quoted text converts.
    final formulaBar = find.byType(TextField).at(1);
    await tester.tap(formulaBar);
    final ctl = tester.widget<TextField>(formulaBar).controller!;
    ctl.clear();
    for (final ch in '=LEN("rAma")'.split('')) {
      await tester.enterText(formulaBar, ctl.text + ch);
    }
    expect(ctl.text, '=LEN("राम")');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A2'})['display'], '3');

    // Convert existing cells: Devanagari -> Tamil.
    final n = core({'op': 'calc.transliterate', 'doc': doc.handle, 'sheet': 0, 'range': 'A1:A2', 'mode': 'script', 'script': 'taml'});
    expect(n, 1);
    expect(core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A1'})['display'], 'பாரத');
  }, skip: skip);

  testWidgets('Math converts only quoted text', (tester) async {
    final r = core({'op': 'new', 'kind': 'math'}) as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.math, null);
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: MathPage(doc: doc))));
    await tester.pumpAndSettle();
    final field = find.byType(TextField);
    final ctl = tester.widget<TextField>(field).controller!;
    for (final ch in 'a over "bhAgaM"'.split('')) {
      await tester.enterText(field, ctl.text + ch);
    }
    expect(ctl.text, 'a over "భాగం"');
  }, skip: skip);

  testWidgets('typing guide shows outputs for the chosen script', (tester) async {
    final view = tester.platformDispatcher.views.first;
    view.physicalSize = const Size(1400, 1200);
    view.devicePixelRatio = 1.0;
    await tester.pumpWidget(MaterialApp(home: Builder(builder: (c) => TextButton(onPressed: () => showTranslitGuide(c), child: const Text('open')))));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    expect(find.text('నమస్తే'), findsWidgets);
    expect(find.text('ఖ'), findsOneWidget);
    expect(find.textContaining('teltools'), findsOneWidget);
  }, skip: skip);
}
