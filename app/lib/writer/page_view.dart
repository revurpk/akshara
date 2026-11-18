// Read-only rendering of document content: the header and footer repeated on
// later pages, and every page when printing. Mirrors the layout of the
// editable paragraphs (ParaField) so that print matches the screen.

import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import 'para_field.dart';

/// Colours used by the read-only renderer.
class PageInk {
  const PageInk({required this.text, required this.line, required this.link, required this.shade});

  /// Black on white, for printing.
  static const print = PageInk(
    text: Color(0xFF000000),
    line: Color(0xFF000000),
    link: Color(0xFF1F5FBF),
    shade: Color(0xFFEEEEEE),
  );

  final Color text;
  final Color line;
  final Color link;
  final Color shade;
}

/// List labels ("1.", "•") for a sequence of paragraphs, as the editor
/// numbers them: numbering restarts after any non-list paragraph.
List<String?> listLabels(List<Map<String, dynamic>> paras) {
  final counters = List<int>.filled(10, 0);
  return [
    for (final p in paras) _label(p, counters),
  ];
}

String? _label(Map<String, dynamic> p, List<int> counters) {
  final kind = p['kind'] as String? ?? 'normal';
  final level = (p['level'] as int?) ?? 0;
  if (kind == 'numbered') {
    counters[level]++;
    for (var l = level + 1; l < 10; l++) {
      counters[l] = 0;
    }
    return '${counters[level]}.';
  }
  if (kind == 'bullet') return const ['•', '◦', '▪'][level % 3];
  counters.fillRange(0, 10, 0);
  return null;
}

/// One paragraph, read-only. Fields show their value for [page] of [pages].
class StaticPara extends StatelessWidget {
  const StaticPara({
    super.key,
    required this.para,
    required this.scale,
    required this.ink,
    this.label,
    this.page,
    this.pages,
  });

  final Map<String, dynamic> para;
  final double scale;
  final PageInk ink;
  final String? label;
  final int? page;
  final int? pages;

  @override
  Widget build(BuildContext context) {
    final kind = para['kind'] as String? ?? 'normal';
    final level = (para['level'] as int?) ?? 0;
    final base = paraTextStyle(kind, level, scale);
    final style = base.copyWith(
      color: ink.text,
      fontFamily: base.fontFamily ?? docFont,
      fontFamilyFallback: docFallback,
    );
    final runs = (para['runs'] as List<dynamic>).cast<Map<String, dynamic>>();
    Widget text = Text.rich(
      TextSpan(style: style, children: runSpans(runs, style, ink.link, page: page, pages: pages)),
      textAlign: paraTextAlign(para['align'] as String?),
    );
    // Empty paragraphs keep their line height.
    if (runs.every((r) => runText(r, page: page, pages: pages).isEmpty)) {
      text = Text(' ', style: style);
    }
    final isHeading = kind == 'heading' || kind == 'title';
    if (label != null) {
      text = Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 18.0 * scale * (level + 1),
            child: Padding(
              padding: EdgeInsets.only(right: 6 * scale),
              child: Text(label!, textAlign: TextAlign.right, style: style),
            ),
          ),
          Expanded(child: text),
        ],
      );
    } else if (kind == 'quote') {
      text = Container(
        margin: EdgeInsets.symmetric(horizontal: 18 * scale),
        padding: EdgeInsets.only(left: 10 * scale),
        decoration: BoxDecoration(border: Border(left: BorderSide(color: ink.line, width: 3 * scale / 1.3333))),
        child: text,
      );
    } else if (kind == 'code') {
      text = Container(
        color: ink.shade,
        padding: EdgeInsets.symmetric(horizontal: 6 * scale, vertical: 2),
        child: text,
      );
    }
    final indent = ((para['indent'] as num?) ?? 0).toDouble();
    final spaceBefore = ((para['space_before'] as num?) ?? 0).toDouble();
    return Padding(
      padding: EdgeInsets.only(
        left: indent * scale,
        top: (isHeading ? 10 : 0) * scale + spaceBefore * scale,
        bottom: (kind == 'code' ? 0 : (isHeading ? 4 : 5)) * scale,
      ),
      child: text,
    );
  }
}

/// A list of paragraphs (header, footer, table cell), read-only.
List<Widget> staticParagraphs(
  List<Map<String, dynamic>> paras,
  double scale,
  PageInk ink, {
  int? page,
  int? pages,
}) {
  final labels = listLabels(paras);
  return [
    for (var i = 0; i < paras.length; i++)
      StaticPara(para: paras[i], scale: scale, ink: ink, label: labels[i], page: page, pages: pages),
  ];
}

/// Cell padding in points: bordered tables match the ODT export (0.05 in),
/// borderless ones (label sheets) 0.08 in.
double cellPadding(Map<String, dynamic> table) => table['borders'] == false ? 5.76 : 3.6;

/// Lays out a table (or some of its rows) with the table's fixed widths and
/// exact row heights. [cell] builds the content of one cell.
Widget tableLayout({
  required Map<String, dynamic> table,
  required double scale,
  required Color borderColor,
  required Widget Function(int row, int col) cell,
  Iterable<int>? rows,
  bool drawTopBorder = true,
}) {
  final allRows = table['rows'] as List<dynamic>;
  final widths = (table['col_widths'] as List<dynamic>?)?.cast<num>() ?? const [];
  final heights = (table['row_heights'] as List<dynamic>?)?.cast<num>() ?? const [];
  final cols = allRows.isEmpty ? 0 : (allRows.first as List<dynamic>).length;
  final pad = cellPadding(table) * scale;
  final side = BorderSide(color: borderColor, width: 1);
  final border = table['borders'] == false && borderColor.a == 0
      ? null
      : TableBorder(
          top: drawTopBorder ? side : BorderSide.none,
          bottom: side,
          left: side,
          right: side,
          horizontalInside: side,
          verticalInside: side,
        );
  return Table(
    border: border,
    defaultVerticalAlignment: TableCellVerticalAlignment.top,
    columnWidths: widths.length == cols
        ? {for (var c = 0; c < cols; c++) c: FixedColumnWidth(widths[c].toDouble() * scale)}
        : null,
    defaultColumnWidth: const FlexColumnWidth(),
    children: [
      for (final r in rows ?? List<int>.generate(allRows.length, (i) => i))
        TableRow(children: [
          for (var c = 0; c < cols; c++)
            _sizedCell(
              heights.length == allRows.length ? heights[r].toDouble() * scale : null,
              Padding(padding: EdgeInsets.all(pad), child: cell(r, c)),
            ),
        ]),
    ],
  );
}

Widget _sizedCell(double? height, Widget child) {
  if (height == null) return child;
  // Exact height: content that does not fit is cut off, as in OpenOffice.
  return SizedBox(
    height: height,
    child: ClipRect(
      child: OverflowBox(
        alignment: Alignment.topLeft,
        minHeight: 0,
        maxHeight: double.infinity,
        child: child,
      ),
    ),
  );
}

/// A table block (or some of its rows), read-only.
Widget staticTable(
  Map<String, dynamic> table,
  double scale,
  PageInk ink, {
  Iterable<int>? rows,
  bool drawTopBorder = true,
  int? page,
  int? pages,
}) {
  final allRows = table['rows'] as List<dynamic>;
  return tableLayout(
    table: table,
    scale: scale,
    borderColor: table['borders'] == false ? const Color(0x00000000) : ink.line,
    rows: rows,
    drawTopBorder: drawTopBorder,
    cell: (r, c) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: staticParagraphs(
        ((allRows[r] as List<dynamic>)[c] as List<dynamic>).cast<Map<String, dynamic>>(),
        scale,
        ink,
        page: page,
        pages: pages,
      ),
    ),
  );
}
