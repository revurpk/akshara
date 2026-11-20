// The Writer caret behaves like a word processor's: it is there when a
// document opens, and arrow keys, Page Up/Down, Ctrl+Home/End and clicks in
// the margins move it across paragraphs, not just within one.

import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:akshara/core/document.dart';
import 'package:akshara/core/native.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/writer/writer_page.dart';

import 'helpers.dart';

void main() {
  final skip = useBuiltCore() == null;

  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-test-').path;
  });

  late OpenDoc doc;

  /// 40 paragraphs of about five lines each: "P0 word word …".
  Future<void> open(WidgetTester tester) async {
    setLanguage(AppLanguage.en);
    tester.view.physicalSize = const Size(1400, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final words = List.filled(60, 'word').join(' ');
    final md = List.generate(40, (i) => 'P$i $words').join('\n\n');
    final r = core({'op': 'new_markdown', 'text': md}) as Map<String, dynamic>;
    doc = OpenDoc(r['doc'] as int, DocKind.writer, null);
    addTearDown(() => core({'op': 'close', 'doc': doc.handle}));
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: WriterPage(doc: doc))));
    await tester.pumpAndSettle();
  }

  EditableTextState focused(WidgetTester tester) =>
      tester.stateList<EditableTextState>(find.byType(EditableText)).firstWhere((e) => e.widget.focusNode.hasFocus);

  /// (paragraph number, caret offset, caret x on screen).
  (int, int, double) caret(WidgetTester tester) {
    final e = focused(tester);
    final sel = e.widget.controller.selection;
    final box = e.renderEditable;
    final x = box.localToGlobal(box.getLocalRectForCaret(sel.extent).center).dx;
    final para = int.parse(RegExp(r'^P(\d+)').firstMatch(e.widget.controller.text)!.group(1)!);
    return (para, sel.extentOffset, x);
  }

  Future<void> press(WidgetTester tester, LogicalKeyboardKey key, {bool ctrl = false}) async {
    if (ctrl) await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(key);
    if (ctrl) await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await tester.pumpAndSettle();
  }

  testWidgets('the caret is at the start of a newly opened document', (tester) async {
    await open(tester);
    final (para, offset, _) = caret(tester);
    expect((para, offset), (0, 0));
  }, skip: skip);

  testWidgets('Up and Down cross paragraphs and keep the column', (tester) async {
    await open(tester);
    await tester.tapAt(tester.getCenter(find.byType(EditableText).first));
    await tester.pumpAndSettle();
    final (_, _, x0) = caret(tester);
    // Down through P0 (five lines) into P1, keeping the horizontal position.
    var (para, offset, x) = caret(tester);
    for (var i = 0; i < 8 && para == 0; i++) {
      await press(tester, LogicalKeyboardKey.arrowDown);
      (para, offset, x) = caret(tester);
    }
    expect(para, 1);
    expect((x - x0).abs(), lessThan(20));
    expect(offset, greaterThan(0), reason: 'lands mid-line, not at the start');
    // Up on P1's first line goes back to P0's last line.
    await press(tester, LogicalKeyboardKey.arrowUp);
    (para, offset, x) = caret(tester);
    expect(para, 0);
    expect((x - x0).abs(), lessThan(20));
  }, skip: skip);

  testWidgets('Page Down and Page Up move a screen and back', (tester) async {
    await open(tester);
    await tester.tapAt(tester.getCenter(find.byType(EditableText).first));
    await tester.pumpAndSettle();
    final start = caret(tester);
    await press(tester, LogicalKeyboardKey.pageDown);
    final down = caret(tester);
    expect(down.$1, greaterThan(start.$1 + 1));
    expect((down.$3 - start.$3).abs(), lessThan(20));
    await press(tester, LogicalKeyboardKey.pageUp);
    final back = caret(tester);
    expect(back.$1, start.$1);
    expect((back.$2 - start.$2).abs(), lessThan(10));
  }, skip: skip);

  testWidgets('Ctrl+End, Ctrl+Home and Ctrl+Down', (tester) async {
    await open(tester);
    await press(tester, LogicalKeyboardKey.end, ctrl: true);
    final end = caret(tester);
    expect(end.$1, 39);
    expect(end.$2, focused(tester).widget.controller.text.length);
    await press(tester, LogicalKeyboardKey.home, ctrl: true);
    expect(caret(tester).$1, 0);
    expect(caret(tester).$2, 0);
    await press(tester, LogicalKeyboardKey.arrowDown, ctrl: true);
    expect((caret(tester).$1, caret(tester).$2), (1, 0));
  }, skip: skip);

  testWidgets('a click in the margin puts the caret on that line', (tester) async {
    await open(tester);
    final p2 = tester.getRect(find.byType(EditableText).at(2));
    // Left of the text, level with the paragraph's second line.
    await tester.tapAt(Offset(p2.left - 30, p2.top + p2.height * 0.3));
    await tester.pumpAndSettle();
    final (para, offset, _) = caret(tester);
    expect(para, 2);
    expect(offset, greaterThan(0), reason: 'the start of a later line, not of the paragraph');
  }, skip: skip);

  testWidgets('switching tabs gives the caret back to the document', (tester) async {
    await open(tester);
    final other = core({'op': 'new_markdown', 'text': 'P0 other document'}) as Map<String, dynamic>;
    final docB = OpenDoc(other['doc'] as int, DocKind.writer, null);
    addTearDown(() => core({'op': 'close', 'doc': docB.handle}));
    var tab = 0;
    late StateSetter setTab;
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: StatefulBuilder(builder: (context, set) {
          setTab = set;
          return IndexedStack(index: tab, children: [WriterPage(doc: doc), WriterPage(doc: docB)]);
        }),
      ),
    ));
    await tester.pumpAndSettle();
    final target = find.byWidgetPredicate((w) => w is EditableText && w.controller.text.startsWith('P2 '));
    await tester.tapAt(tester.getCenter(target));
    await tester.pumpAndSettle();
    final before = caret(tester);
    expect(before.$1, 2);
    setTab(() => tab = 1);
    await tester.pumpAndSettle();
    expect(focused(tester).widget.controller.text, 'P0 other document');
    setTab(() => tab = 0);
    await tester.pumpAndSettle();
    final after = caret(tester);
    expect((after.$1, after.$2), (before.$1, before.$2));
  }, skip: skip);
}
