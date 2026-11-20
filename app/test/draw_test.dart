// Widget tests of Impress, Draw, the shape editor and pictures, diagrams
// and drawings in Writer, running against the real core.

import 'dart:io';

import 'package:flutter/gestures.dart' show kDoubleTapMinTime;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:akshara/core/document.dart';
import 'package:akshara/core/native.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/draw/draw_page.dart';
import 'package:akshara/draw/mermaid_dialog.dart';
import 'package:akshara/draw/shape_editor.dart';
import 'package:akshara/draw/shape_paint.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/main.dart';
import 'package:akshara/writer/writer_page.dart';

import 'helpers.dart';

OpenDoc newDoc(DocKind kind) {
  final r = core({'op': 'new', 'kind': kind.name}) as Map<String, dynamic>;
  return OpenDoc(r['doc'] as int, kind, null);
}

Widget host(Widget child) => MaterialApp(home: Scaffold(body: child));

Map<String, dynamic> drawGet(OpenDoc d) => core({'op': 'draw.get', 'doc': d.handle}) as Map<String, dynamic>;

List<dynamic> shapesOf(OpenDoc d, int page) => ((drawGet(d)['pages'] as List)[page] as Map)['shapes'] as List<dynamic>;

/// The screen position of a point on the page (in points).
Offset onPage(WidgetTester tester, OpenDoc d, Offset pt) {
  final editor = find.byType(ShapeEditor);
  final box = tester.getRect(editor);
  final w = (drawGet(d)['width'] as num).toDouble();
  return box.topLeft + pt * (box.width / w);
}

void main() {
  final skip = useBuiltCore() == null ? 'native library not built' : null;

  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-test-').path;
  });

  setUp(() {
    setLanguage(AppLanguage.en);
    final view = TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher.views.first;
    view.physicalSize = const Size(1600, 1200);
    view.devicePixelRatio = 1.0;
  });

  testWidgets('home screen offers Impress and Draw', (tester) async {
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    expect(find.text('Impress'), findsOneWidget);
    expect(find.text('Draw'), findsOneWidget);
    await tester.tap(find.text('Impress'));
    await tester.pumpAndSettle();
    expect(find.byType(DrawPage), findsOneWidget);
    expect(find.textContaining('Untitled Presentation'), findsOneWidget);
    expect(find.text('Slide 1 of 1'), findsOneWidget);
    expect(DocKind.impress.saveFormats.keys.first, 'odp');
    expect(DocKind.draw.saveFormats.keys.first, 'odg');
  }, skip: skip != null);

  testWidgets('impress: draw, move, type into and delete shapes; slides', (tester) async {
    final doc = newDoc(DocKind.impress);
    await tester.pumpWidget(host(DrawPage(doc: doc)));
    await tester.pumpAndSettle();
    expect(find.byType(SlideView), findsOneWidget, reason: 'one thumbnail');
    final before = shapesOf(doc, 0).length;

    // Draw a rectangle by dragging with the rectangle tool.
    await tester.tap(find.byTooltip('Rectangle'));
    await tester.pump();
    final a = onPage(tester, doc, const Offset(300, 300)), b = onPage(tester, doc, const Offset(400, 380));
    await tester.dragFrom(a, b - a);
    await tester.pumpAndSettle();
    var shapes = shapesOf(doc, 0);
    expect(shapes.length, before + 1);
    final rect = shapes.last as Map<String, dynamic>;
    expect(rect['kind'], 'rect');
    expect((rect['w'] as num).toDouble(), closeTo(100, 4));

    // It is selected: arrow keys nudge it, as one undo step.
    for (var i = 0; i < 3; i++) {
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    }
    await tester.pumpAndSettle();
    shapes = shapesOf(doc, 0);
    expect(((shapes.last as Map)['x'] as num).toDouble(), closeTo((rect['x'] as num).toDouble() + 3, 0.01));

    // Double click to type into it.
    final mid = onPage(tester, doc, const Offset(353, 340));
    await tester.tapAt(mid);
    await tester.pump(kDoubleTapMinTime);
    await tester.tapAt(mid);
    await tester.pumpAndSettle();
    await tester.enterText(find.descendant(of: find.byType(ShapeEditor), matching: find.byType(TextField)), 'Hello');
    await tester.pumpAndSettle();
    expect((shapesOf(doc, 0).last as Map)['text'], 'Hello');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();

    // Fill colour from the toolbar applies to the selection.
    final id = (shapesOf(doc, 0).last as Map)['id'];
    final state = tester.state<DrawPageState>(find.byType(DrawPage));
    expect(state, isNotNull);
    await tester.tap(find.byTooltip('Bold (Ctrl+B)'));
    await tester.pumpAndSettle();
    final bolded = shapesOf(doc, 0).cast<Map>().firstWhere((s) => s['id'] == id);
    expect((bolded['style'] as Map)['bold'], isTrue);

    // Delete removes it; undo brings it back.
    await tester.tapAt(mid);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.delete);
    await tester.pumpAndSettle();
    expect(shapesOf(doc, 0).length, before);
    await tester.tap(find.byTooltip('Undo (Ctrl+Z)'));
    await tester.pumpAndSettle();
    expect(shapesOf(doc, 0).length, before + 1);

    // A new "Title and content" slide.
    await tester.tap(find.byTooltip('New slide'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Title and content'));
    await tester.pumpAndSettle();
    expect((drawGet(doc)['pages'] as List).length, 2);
    expect(find.text('Slide 2 of 2'), findsOneWidget);
    expect(find.byType(SlideView), findsNWidgets(2));
    expect((shapesOf(doc, 1)[1] as Map)['placeholder'], 'outline');

    // Speaker notes are stored with the slide.
    await tester.enterText(find.widgetWithText(TextField, 'Speaker notes'), 'Remember to smile');
    await tester.pumpAndSettle();
    expect(((drawGet(doc)['pages'] as List)[1] as Map)['notes'], 'Remember to smile');
  }, skip: skip != null);

  testWidgets('impress: the slide show steps through slides and ends', (tester) async {
    final doc = newDoc(DocKind.impress);
    core({'op': 'draw.add_page', 'doc': doc.handle, 'at': 1, 'layout': 'content'});
    await tester.pumpWidget(host(DrawPage(doc: doc)));
    await tester.pumpAndSettle();
    tester.state<DrawPageState>(find.byType(DrawPage)).present(fromStart: true);
    await tester.pumpAndSettle();
    expect(find.byType(SlideView), findsOneWidget, reason: 'the show covers the editor');
    await tester.sendKeyEvent(LogicalKeyboardKey.space);
    await tester.sendKeyEvent(LogicalKeyboardKey.space);
    await tester.pumpAndSettle();
    expect(find.text('End of slide show. Click to exit.'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.text('End of slide show. Click to exit.'), findsNothing);
  }, skip: skip != null);

  testWidgets('draw: pages, page size and diagrams', (tester) async {
    final doc = newDoc(DocKind.draw);
    await tester.pumpWidget(host(DrawPage(doc: doc)));
    await tester.pumpAndSettle();
    expect(find.text('Page 1 of 1'), findsOneWidget);
    await tester.tap(find.byTooltip('New page'));
    await tester.pumpAndSettle();
    expect(find.text('Page 2 of 2'), findsOneWidget);
    await tester.tap(find.byTooltip('Page setup…'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Landscape'));
    await tester.tap(find.text('OK'));
    await tester.pumpAndSettle();
    final data = drawGet(doc);
    expect((data['height'] as num) > (data['width'] as num), isTrue, reason: 'portrait now');

    // A diagram shape is painted from the figure the core laid out.
    final c = ShapeEditorController(doc: doc.handle, onChanged: () {})..target = const {'page': 1};
    c.insertDiagram('graph LR\nA-->B');
    final shape = shapesOf(doc, 1).single as Map<String, dynamic>;
    expect(shape['kind'], 'diagram');
    expect((shape['figure'] as Map)['items'], isNotEmpty);
  }, skip: skip != null);

  testWidgets('mermaid dialog previews and reports problems', (tester) async {
    String? result;
    await tester.pumpWidget(host(Builder(
      builder: (context) => TextButton(onPressed: () async => result = await showMermaidDialog(context), child: const Text('go')),
    )));
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle();
    expect(find.text('Diagram (Mermaid)'), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'graph TD\nA -->');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(find.textContaining('Line 2:'), findsOneWidget);
    await tester.tap(find.text('OK'));
    await tester.pumpAndSettle();
    expect(result, 'graph TD\nA -->');
  }, skip: skip != null);

  testWidgets('writer: pictures, diagrams and drawings', (tester) async {
    final png = '${Directory.systemTemp.createTempSync('akshara-pic-').path}${Platform.pathSeparator}dot.png';
    // A 1x1 PNG.
    File(png).writeAsBytesSync([
      0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01,
      0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41,
      0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
      0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]);
    final doc = newDoc(DocKind.writer);
    core({'op': 'writer.insert_image', 'doc': doc.handle, 'at': 0, 'path': png});
    core({'op': 'writer.insert_diagram', 'doc': doc.handle, 'at': 1, 'source': 'pie\n"a": 1\n"b": 2'});
    core({'op': 'writer.insert_drawing', 'doc': doc.handle, 'at': 2, 'width': 300, 'height': 120});
    await tester.pumpWidget(host(WriterPage(doc: doc)));
    await tester.pumpAndSettle();
    expect(find.byType(CoreImage), findsOneWidget);
    expect(find.byType(FigureView), findsOneWidget);
    expect(find.byType(ShapeEditor), findsOneWidget);

    // Selecting the diagram shows its toolbar; aligning it goes to the core.
    await tester.tap(find.byType(FigureView));
    await tester.pumpAndSettle();
    expect(find.text('Edit diagram…'), findsOneWidget);
    await tester.tap(find.byTooltip('Align right').last);
    await tester.pumpAndSettle();
    final blocks = (core({'op': 'writer.get', 'doc': doc.handle}) as Map)['blocks'] as List;
    expect((blocks[1] as Map)['align'], 'right');

    // Drawing in the canvas.
    await tester.tap(find.byType(ShapeEditor));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Ellipse'));
    await tester.pump();
    final box = tester.getRect(find.byType(ShapeEditor));
    await tester.dragFrom(box.topLeft + const Offset(20, 20), const Offset(80, 50));
    await tester.pumpAndSettle();
    final canvas = ((core({'op': 'writer.get', 'doc': doc.handle}) as Map)['blocks'] as List)[2] as Map;
    expect((canvas['shapes'] as List).single['kind'], 'ellipse');

    // The insert menu offers all three.
    await tester.tap(find.byTooltip('Insert picture, diagram or drawing'));
    await tester.pumpAndSettle();
    expect(find.text('Picture…'), findsOneWidget);
    expect(find.text('Diagram (Mermaid)…'), findsOneWidget);
    expect(find.text('Drawing'), findsOneWidget);
    await tester.tap(find.text('Drawing'));
    await tester.pumpAndSettle();
    expect(find.byType(ShapeEditor), findsNWidgets(2));
  }, skip: skip != null);

  testWidgets('presentations and drawings open from files', (tester) async {
    final dir = Directory.systemTemp.createTempSync('akshara-odp-').path;
    final path = '$dir${Platform.pathSeparator}deck.odp';
    final d = newDoc(DocKind.impress);
    core({'op': 'save', 'doc': d.handle, 'path': path});
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    final shell = tester.state<ShellState>(find.byType(Shell));
    await tester.runAsync(() => shell.openPath(path));
    await tester.pumpAndSettle();
    expect(find.byType(DrawPage), findsOneWidget);
    expect(find.text('deck.odp'), findsOneWidget);
  }, skip: skip != null);
}
