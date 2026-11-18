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
import 'package:akshara/core/print_preview.dart';
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

  testWidgets('writer: header and footer repeat on every page with page numbers', (tester) async {
    final doc = newDoc(DocKind.writer);
    await tester.pumpWidget(host(WriterPage(doc: doc)));
    await tester.pumpAndSettle();
    // Turn the header on from the Page layout menu, type, add a page number.
    await tester.tap(find.byTooltip('Page layout'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Header'));
    await tester.pumpAndSettle();
    var got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect((got['header'] as List).length, 1);
    final header = find.byKey(const ValueKey('para-header:0'));
    expect(header, findsOneWidget);
    await tester.enterText(find.descendant(of: header, matching: find.byType(TextField)), 'Page ');
    await tester.pumpAndSettle();
    final headerController = tester.widget<TextField>(find.descendant(of: header, matching: find.byType(TextField))).controller!;
    headerController.selection = const TextSelection.collapsed(offset: 5);
    await tester.tap(find.byTooltip('Page layout'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Insert page number'));
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect(got['header'][0]['runs'][1]['field'], 'page_number');

    // A page break makes a second page whose header shows "Page 2".
    final body = find.byKey(const ValueKey('para-0'));
    await tester.tap(find.descendant(of: body, matching: find.byType(TextField)));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Insert page break'));
    await tester.pumpAndSettle();
    expect(find.text('Page 2', findRichText: true), findsOneWidget);

    // Superscript by keyboard (OpenOffice's Ctrl+Shift+P).
    final field = find.descendant(of: body, matching: find.byType(TextField));
    await tester.enterText(field, 'x2');
    await tester.pumpAndSettle();
    tester.widget<TextField>(field).controller!.selection = const TextSelection(baseOffset: 1, extentOffset: 2);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyP);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pumpAndSettle();
    got = core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>;
    expect(got['blocks'][0]['runs'][1]['superscript'], true);
  }, skip: skip != null);

  testWidgets('writer: label sheets render and paginate in the print preview', (tester) async {
    final r = core({'op': 'new_labels', 'sheet': 'l7163', 'text': List.generate(20, (i) => 'Name $i\nTown').join('\n\n'), 'same': false})
        as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.writer, null);
    await tester.pumpWidget(host(WriterPage(doc: doc)));
    await tester.pumpAndSettle();
    expect(find.byType(Table), findsNWidgets(2), reason: '14 labels per sheet, 20 labels');
    await tester.tap(find.byTooltip('Print (Ctrl+P)'));
    await tester.pumpAndSettle();
    expect(find.byType(PrintPreview), findsOneWidget);
    expect(find.text('2 page(s)'), findsOneWidget);
    expect(find.descendant(of: find.byType(PrintPreview), matching: find.text('Name 19', findRichText: true)), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
  }, skip: skip != null);

  testWidgets('the + menu offers envelopes, labels and index cards', (tester) async {
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('New document'));
    await tester.pumpAndSettle();
    expect(find.text('Envelope…'), findsOneWidget);
    expect(find.text('Labels…'), findsOneWidget);
    await tester.tap(find.text('Index cards…'));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Create')));
    await tester.pumpAndSettle();
    // The cards open as a new Writer tab with the labelled Page menu.
    expect(find.byType(WriterPage), findsOneWidget);
    expect(find.text('Page'), findsOneWidget);
    expect(find.textContaining('Untitled Text Document'), findsOneWidget);
  }, skip: skip != null);

  testWidgets('writer: envelope wizard opens the envelope as a new document', (tester) async {
    final doc = newDoc(DocKind.writer);
    int? created;
    await tester.pumpWidget(host(WriterPage(doc: doc, onNewDocument: (h) => created = h)));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Page layout'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('New envelope…'));
    await tester.pumpAndSettle();
    await tester.enterText(find.widgetWithText(TextField, 'Recipient address'), 'A. Person\n1 Road');
    await tester.tap(find.text('Create'));
    await tester.pumpAndSettle();
    expect(created, isNotNull);
    final got = core({'op': 'writer.get', 'doc': created}) as Map<String, dynamic>;
    expect(got['paper'], 'env_dl');
    expect(got['blocks'][0]['runs'][0]['text'], 'A. Person');
    core({'op': 'close', 'doc': created});
  }, skip: skip != null);

  testWidgets('calc: print preview splits wide sheets and starts each sheet on a new page', (tester) async {
    final doc = newDoc(DocKind.calc);
    // 10 columns of 64 pt (640 pt) do not fit the A4 portrait text width
    // (482 pt) but fit landscape (729 pt).
    core({'op': 'calc.paste', 'doc': doc.handle, 'sheet': 0, 'cell': 'A1', 'rows': [
      for (var r = 0; r < 3; r++) [for (var c = 0; c < 10; c++) 'r${r}c$c'],
    ]});
    core({'op': 'calc.add_sheet', 'doc': doc.handle, 'name': 'Second'});
    core({'op': 'calc.set_input', 'doc': doc.handle, 'sheet': 1, 'cell': 'B2', 'text': '=6*7'});
    await tester.pumpWidget(host(CalcPage(doc: doc)));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Print (Ctrl+P)'));
    await tester.pumpAndSettle();
    expect(find.byType(PrintPreview), findsOneWidget);
    expect(find.text('3 page(s)'), findsOneWidget);
    expect(find.text('Second'), findsWidgets, reason: 'sheet name in the header');
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    // Page setup: landscape fits all 10 columns, one page per sheet.
    await tester.tap(find.byTooltip('Page setup…'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(SwitchListTile, 'Landscape'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('OK'));
    await tester.pumpAndSettle();
    var page = core({'op': 'calc.page', 'doc': doc.handle}) as Map<String, dynamic>;
    expect((page['width'] as num) > (page['height'] as num), isTrue);
    await tester.tap(find.byTooltip('Print (Ctrl+P)'));
    await tester.pumpAndSettle();
    expect(find.text('2 page(s)'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    // Portrait again, but fit to width and without header and footer.
    page = core({'op': 'calc.page', 'doc': doc.handle}) as Map<String, dynamic>;
    core({'op': 'calc.set_page', 'doc': doc.handle, 'width': page['height'], 'height': page['width'], 'margin': page['margin'],
        'fit_width': true, 'header_footer': false});
    await tester.tap(find.byTooltip('Print (Ctrl+P)'));
    await tester.pumpAndSettle();
    expect(find.text('2 page(s)'), findsOneWidget);
    expect(find.descendant(of: find.byType(PrintPreview), matching: find.text('Second')), findsNothing);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
  }, skip: skip != null);

  testWidgets('math: print preview shows the formula on one page', (tester) async {
    final r = core({'op': 'new', 'kind': 'math', 'source': 'x = {-b +- sqrt{b^2 - 4ac}} over {2a}'}) as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.math, null);
    await tester.pumpWidget(host(MathPage(doc: doc)));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Print (Ctrl+P)'));
    await tester.pumpAndSettle();
    expect(find.text('1 page(s)'), findsOneWidget);
    expect(find.descendant(of: find.byType(PrintPreview), matching: find.byType(MathView)), findsWidgets);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
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
