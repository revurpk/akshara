// Widget tests of the three applications, running against the real core.

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:akshara/calc/calc_page.dart';
import 'package:akshara/core/document.dart';
import 'package:akshara/core/native.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/main.dart';
import 'package:akshara/math/math_page.dart';
import 'package:akshara/math/math_view.dart';
import 'package:akshara/writer/para_field.dart';
import 'package:akshara/writer/writer_page.dart';

import 'helpers.dart';

OpenDoc newDoc(DocKind kind) {
  final r = core({'op': 'new', 'kind': kind.name}) as Map<String, dynamic>;
  return OpenDoc(r['doc'] as int, kind, null);
}

Widget host(Widget child) => MaterialApp(home: Scaffold(body: child));

void main() {
  final skip = useBuiltCore() == null ? 'native library not built' : null;

  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-test-').path;
  });

  setUp(() {
    setLanguage(AppLanguage.en);
    // Large enough for the toolbars.
    final view = TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher.views.first;
    view.physicalSize = const Size(1600, 1200);
    view.devicePixelRatio = 1.0;
  });

  testWidgets('home screen offers the three applications', (tester) async {
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    expect(find.text('Akshara Office'), findsWidgets);
    expect(find.text('Writer'), findsOneWidget);
    expect(find.text('Calc'), findsOneWidget);
    expect(find.text('Math'), findsOneWidget);
    // Language chips are shown in their own scripts.
    expect(find.text('हिन्दी'), findsWidgets);
    expect(find.text('தமிழ்'), findsWidgets);
    await tester.tap(find.text('Calc'));
    await tester.pumpAndSettle();
    expect(find.byType(CalcPage), findsOneWidget);
    expect(find.textContaining('Untitled Spreadsheet'), findsOneWidget);
  }, skip: skip != null);

  testWidgets('switching to Hindi translates the interface', (tester) async {
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    await tester.tap(find.text('हिन्दी').first);
    await tester.pumpAndSettle();
    expect(find.text('गणक'), findsOneWidget); // "Calc"
    expect(find.text('हाल के दस्तावेज़'), findsNothing); // no recent files in a fresh test
    await tester.tap(find.text('लेखक')); // "Writer"
    await tester.pumpAndSettle();
    expect(find.byTooltip('गाढ़ा (Ctrl+B)'), findsOneWidget);
    // Back to English for the other tests.
    await tester.tap(find.byTooltip('भाषा'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('English').last);
    await tester.pumpAndSettle();
    expect(find.byTooltip('Bold (Ctrl+B)'), findsOneWidget);
  }, skip: skip != null);

  testWidgets('writer: typing, Enter, bold and undo go through the core', (tester) async {
    final doc = newDoc(DocKind.writer);
    await tester.pumpWidget(host(WriterPage(doc: doc)));
    await tester.pumpAndSettle();
    final field = find.byType(TextField).first;
    await tester.tap(field);
    await tester.enterText(field, 'Hello world');
    await tester.pumpAndSettle();
    var got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect(got['blocks'][0]['runs'][0]['text'], 'Hello world');
    expect(find.textContaining('2 words'), findsOneWidget);

    // Select "world" and make it bold via the toolbar.
    final controller = tester.widget<TextField>(field).controller!;
    controller.selection = const TextSelection(baseOffset: 6, extentOffset: 11);
    await tester.tap(find.byTooltip('Bold (Ctrl+B)'));
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    final runs = got['blocks'][0]['runs'] as List<dynamic>;
    expect(runs.length, 2);
    expect(runs[1]['bold'], true);

    // Enter splits the paragraph at the caret.
    controller.selection = const TextSelection.collapsed(offset: 5);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect((got['blocks'] as List).length, 2);
    expect(find.byType(ParaField), findsNWidgets(2));

    // Undo restores one paragraph.
    await tester.tap(find.byTooltip('Undo (Ctrl+Z)'));
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect((got['blocks'] as List).length, 1);

    // Paragraph style menu makes a heading.
    await tester.tap(find.text('Body text'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Heading 1').last);
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect(got['blocks'][0]['kind'], 'heading');
    expect(doc.modified, true);
  }, skip: skip != null);

  testWidgets('calc: typing into the grid enters values and formulas', (tester) async {
    final doc = newDoc(DocKind.calc);
    await tester.pumpWidget(host(CalcPage(doc: doc)));
    await tester.pumpAndSettle();
    // Type "12" Enter, "30" Enter, "=A1+A2" Enter in column A.
    for (final input in ['12', '30']) {
      for (final ch in input.split('')) {
        await tester.sendKeyEvent(LogicalKeyboardKey(ch.codeUnitAt(0)), character: ch);
      }
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
    }
    final formulaBar = find.byType(TextField).at(1);
    await tester.enterText(formulaBar, '=A1+A2');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    final cell = core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A3'}) as Map<String, dynamic>;
    expect(cell['display'], '42');
    expect(core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A1'})['display'], '12');
    expect(doc.modified, true);

    // Bold via toolbar applies to the current cell (A4 after Enter).
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Bold'));
    await tester.pumpAndSettle();
    final styled = core({'op': 'calc.cell', 'doc': doc.handle, 'sheet': 0, 'cell': 'A3'}) as Map<String, dynamic>;
    expect(styled['style']['bold'], true);

    // Add a sheet from the sheet bar.
    await tester.tap(find.byTooltip('Add sheet'));
    await tester.pumpAndSettle();
    expect(find.text('Sheet2'), findsOneWidget);
  }, skip: skip != null);

  testWidgets('math: editing the source updates the rendering', (tester) async {
    final doc = newDoc(DocKind.math);
    await tester.pumpWidget(host(MathPage(doc: doc)));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'a over b');
    await tester.pumpAndSettle();
    final view = tester.widget<MathView>(find.byType(MathView));
    expect(view.tree['t'], 'frac');
    expect(find.descendant(of: find.byType(MathView), matching: find.text('a')), findsOneWidget);
    expect(find.descendant(of: find.byType(MathView), matching: find.text('b')), findsOneWidget);
    // Errors are listed.
    await tester.enterText(find.byType(TextField), 'sqrt{');
    await tester.pumpAndSettle();
    expect(find.textContaining('Position'), findsWidgets);
    expect(doc.modified, true);
  }, skip: skip != null);

  testWidgets('math view renders every node type without throwing', (tester) async {
    const sources = [
      'x = {-b +- sqrt{b^2 - 4 a c}} over {2 a}',
      'sum from {i=1} to n i^2 = {n(n+1)(2n+1)} over 6',
      'int from 0 to infinity e^{-x^2} dx = sqrt{%pi} over 2',
      'left lbrace stack{a # b} right none matrix{1 # 2 ## 3 # 4}',
      'nroot 3 x + abs{y} + lim from {x -> 0} {sin x} over x',
      'bold{color red{size *2 hat a}} + phantom b + vec v + overline{AB} + underline q',
      '{a + b} overbrace "sum" + x lsub 1 lsup 2 csub 3 csup 4',
      'binom n k + font sans "text" newline langle x mline y rangle + lceil z rceil + <?>',
    ];
    for (final src in sources) {
      final parsed = core({'op': 'math.parse', 'source': src}) as Map<String, dynamic>;
      await tester.pumpWidget(host(Center(child: MathView(tree: parsed['tree'] as Map<String, dynamic>))));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull, reason: src);
    }
  }, skip: skip != null);
}
