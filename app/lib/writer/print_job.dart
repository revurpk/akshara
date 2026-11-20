// What Writer prints: the document's pages as a PrintJob.

import 'package:material_ui/material_ui.dart';

import '../core/print_preview.dart';
import '../draw/shape_paint.dart';
import 'page_view.dart';

/// The print job of a document, from the JSON of `writer.get`. Paragraphs
/// and table rows are the units; page breaks start new pages.
PrintJob writerPrintJob(Map<String, dynamic> got, {String? title, int doc = 0}) {
  final blocks = got['blocks'] as List<dynamic>;
  final page = got['page'] as Map<String, dynamic>;
  List<Map<String, dynamic>> paras(Object? v) => (v as List<dynamic>? ?? []).cast<Map<String, dynamic>>();
  final header = paras(got['header']), footer = paras(got['footer']);
  Widget area(List<Map<String, dynamic>> p, int n, int count) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: staticParagraphs(p, 1, PageInk.print, page: n, pages: count),
      );
  return PrintJob(
    width: (page['width'] as num).toDouble(),
    height: (page['height'] as num).toDouble(),
    margin: (page['margin'] as num).toDouble(),
    units: _units(blocks, doc),
    header: header.isEmpty ? null : (n, count, _) => area(header, n, count),
    footer: footer.isEmpty ? null : (n, count, _) => area(footer, n, count),
    paper: got['paper'] as String?,
    title: title,
  );
}

/// A picture, diagram or drawing at print size (1 pixel per point).
Widget _frame(Map<String, dynamic> block, int doc) {
  final w = ((block['width'] as num?) ?? 100).toDouble(), h = ((block['height'] as num?) ?? 100).toDouble();
  final Widget content = switch (block['type']) {
    'image' => CoreImage(doc: doc, id: block['image'] as String, width: w, height: h),
    'diagram' => FigureView(figure: block['figure'] as Map<String, dynamic>, width: w, height: h),
    _ => SizedBox(
        width: w,
        height: h,
        child: CustomPaint(painter: PagePainter(doc: doc, shapes: block['shapes'] as List<dynamic>? ?? const [], width: w, height: h)),
      ),
  };
  return Padding(
    padding: const EdgeInsets.symmetric(vertical: 6),
    child: Align(
      alignment: switch (block['align']) { 'left' || 'justify' => Alignment.centerLeft, 'right' => Alignment.centerRight, _ => Alignment.center },
      child: content,
    ),
  );
}

List<PrintUnit> _units(List<dynamic> blocks, int doc) {
  final out = <PrintUnit>[];
  const ink = PageInk.print;
  var breakNext = false;
  // List numbering continues across the document like in the editor.
  final paras = <Map<String, dynamic>>[];
  final labelsAt = <int, int>{};
  for (var b = 0; b < blocks.length; b++) {
    final block = blocks[b] as Map<String, dynamic>;
    if (block['type'] == 'paragraph') {
      labelsAt[b] = paras.length;
      paras.add(block);
    } else if (block['type'] == 'table') {
      paras.add(const {'kind': 'normal', 'runs': []});
    }
  }
  final labels = listLabels(paras);
  for (var b = 0; b < blocks.length; b++) {
    final block = blocks[b] as Map<String, dynamic>;
    switch (block['type']) {
      case 'page_break':
        breakNext = true;
      case 'paragraph':
        final label = labels[labelsAt[b]!];
        out.add(PrintUnit(
          (page, pages, _) => StaticPara(para: block, scale: 1, ink: ink, label: label, page: page, pages: pages),
          newSheet: breakNext,
        ));
        breakNext = false;
      case 'image' || 'diagram' || 'drawing':
        out.add(PrintUnit((page, pages, _) => _frame(block, doc), newSheet: breakNext));
        breakNext = false;
      case 'table':
        final rows = (block['rows'] as List<dynamic>).length;
        for (var r = 0; r < rows; r++) {
          out.add(PrintUnit(
            (page, pages, first) => staticTable(block, 1, ink, rows: [r], drawTopBorder: r == 0 || first, page: page, pages: pages),
            newSheet: r == 0 && breakNext,
          ));
        }
        breakNext = false;
    }
  }
  return out;
}
