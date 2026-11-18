// What Calc prints: every non-empty sheet as a grid of its used cells.
//
// Columns keep their widths; columns that do not fit the page width continue
// on the following pages (top to bottom, then right, like OpenOffice). Each
// page has the sheet name as header and the page number as footer. Cells
// are drawn like on screen: styles, number formats (the displayed text),
// text spilling into empty neighbours, ### for numbers that do not fit.

import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import '../core/native.dart';
import '../core/print_preview.dart';

/// Default column width and row height in points (as on screen at 100 %).
const _defaultWidthPt = 64.0, _rowHeightPt = 16.5, _fontPt = 10.0;

/// Most rows printed per sheet.
const maxPrintRows = 20000;

Color? _color(Object? hex) {
  if (hex is! String || hex.length != 7) return null;
  final v = int.tryParse(hex.substring(1), radix: 16);
  return v == null ? null : Color(0xFF000000 | v);
}

class _Sheet {
  _Sheet(this.name);
  final String name;
  final Map<int, Map<int, Map<String, dynamic>>> rows = {}; // row -> col -> cell
  final Map<String, dynamic> styles = {};
  final Map<int, double> widths = {};
  int nRows = 0, nCols = 0;

  double width(int col) => widths[col] ?? _defaultWidthPt;
}

/// Reads the used part of every sheet from the core.
List<_Sheet> _readSheets(int doc) {
  final out = <_Sheet>[];
  final sheets = core({'op': 'calc.sheets', 'doc': doc}) as List<dynamic>;
  for (var i = 0; i < sheets.length; i++) {
    final sheet = _Sheet((sheets[i] as Map<String, dynamic>)['name'] as String);
    var first = core({'op': 'calc.view', 'doc': doc, 'sheet': i, 'row': 0, 'col': 0, 'rows': 1, 'cols': 1}) as Map<String, dynamic>;
    final used = first['used'] as Map<String, dynamic>?;
    if (used == null) continue;
    sheet.nRows = (used['rows'] as int).clamp(0, maxPrintRows);
    sheet.nCols = used['cols'] as int;
    for (var row = 0; row < sheet.nRows; row += 500) {
      for (var col = 0; col < sheet.nCols; col += 200) {
        first = core({'op': 'calc.view', 'doc': doc, 'sheet': i, 'row': row, 'col': col, 'rows': 500, 'cols': 200})
            as Map<String, dynamic>;
        for (final c in (first['cells'] as List<dynamic>).cast<Map<String, dynamic>>()) {
          (sheet.rows[c['r'] as int] ??= {})[c['c'] as int] = c;
        }
        sheet.styles.addAll(first['styles'] as Map<String, dynamic>);
        (first['widths'] as Map<String, dynamic>).forEach((k, v) => sheet.widths[int.parse(k)] = (v as num).toDouble());
      }
    }
    if (sheet.rows.isNotEmpty) out.add(sheet);
  }
  return out;
}

/// The print job of a workbook (A4, portrait or landscape).
PrintJob calcPrintJob(int doc, {String? title, bool landscape = false}) {
  const margin = 56.69; // 2 cm, OpenOffice's default
  final width = landscape ? a4Height : a4Width, height = landscape ? a4Width : a4Height;
  final body = width - 2 * margin;
  final units = <PrintUnit>[];
  for (final sheet in _readSheets(doc)) {
    // Column bands that fit the page width.
    final bands = <List<int>>[[]];
    var used = 0.0;
    for (var c = 0; c < sheet.nCols; c++) {
      final w = sheet.width(c);
      if (bands.last.isNotEmpty && used + w > body + 0.5) {
        bands.add([]);
        used = 0;
      }
      bands.last.add(c);
      used += w;
    }
    for (final band in bands) {
      // Text in the last used column may spill into the free page width.
      final bandWidth = band.fold<double>(0, (a, c) => a + sheet.width(c));
      final spill = identical(band, bands.last) ? (body - bandWidth).clamp(0.0, body) : 0.0;
      for (var r = 0; r < sheet.nRows; r++) {
        final row = r;
        units.add(PrintUnit(
          (page, pages, first) => _RowView(sheet: sheet, row: row, cols: band, topLine: first || row == 0, spill: spill),
          newSheet: row == 0 && units.isNotEmpty,
          section: sheet.name,
        ));
      }
    }
  }
  const grey = TextStyle(fontFamily: uiFont, fontFamilyFallback: indicFallback, fontSize: 9, color: Color(0xFF555555));
  return PrintJob(
    width: width,
    height: height,
    margin: margin,
    units: units,
    header: (page, pages, section) => Text(section ?? '', textAlign: TextAlign.center, style: grey),
    footer: (page, pages, section) => Text('$page / $pages', textAlign: TextAlign.center, style: grey),
    paper: 'a4',
    title: title,
  );
}

/// One row of one column band.
class _RowView extends StatelessWidget {
  const _RowView({required this.sheet, required this.row, required this.cols, required this.topLine, required this.spill});

  final _Sheet sheet;
  final int row;
  final List<int> cols;
  final bool topLine;

  /// Free width right of the band that text may spill into.
  final double spill;

  @override
  Widget build(BuildContext context) {
    final w = cols.fold<double>(0, (a, c) => a + sheet.width(c));
    return Align(
      alignment: Alignment.centerLeft,
      child: SizedBox(
        width: w + spill,
        height: _rowHeightPt,
        child: CustomPaint(painter: _RowPainter(sheet, row, cols, topLine)),
      ),
    );
  }
}

class _RowPainter extends CustomPainter {
  _RowPainter(this.sheet, this.row, this.cols, this.topLine);

  final _Sheet sheet;
  final int row;
  final List<int> cols;
  final bool topLine;

  @override
  void paint(Canvas canvas, Size size) {
    final cells = sheet.rows[row] ?? const {};
    final xs = <double>[0];
    for (final c in cols) {
      xs.add(xs.last + sheet.width(c));
    }
    // Backgrounds first, then text (which may spill right), then the grid.
    for (var i = 0; i < cols.length; i++) {
      final bg = _color((sheet.styles['${cells[cols[i]]?['s']}'] as Map<String, dynamic>?)?['background']);
      if (bg != null) canvas.drawRect(Rect.fromLTRB(xs[i], 0, xs[i + 1], size.height), Paint()..color = bg);
    }
    for (var i = 0; i < cols.length; i++) {
      final cell = cells[cols[i]];
      final text = (cell?['t'] as String? ?? '').replaceAll('\n', ' ');
      if (cell == null || text.isEmpty) continue;
      final style = sheet.styles['${cell['s']}'] as Map<String, dynamic>?;
      final type = cell['v'] as String?;
      final align = switch (style?['align'] as String? ?? 'general') {
        'left' => TextAlign.left,
        'center' => TextAlign.center,
        'right' => TextAlign.right,
        _ => (type == 'n' || type == 'b' || type == 'x') ? TextAlign.right : TextAlign.left,
      };
      final tp = TextPainter(
        text: TextSpan(
          text: text,
          style: TextStyle(
            fontFamily: uiFont,
            fontFamilyFallback: indicFallback,
            fontSize: _fontPt,
            color: _color(style?['color']) ?? const Color(0xFF000000),
            fontWeight: style?['bold'] == true ? FontWeight.bold : null,
            fontStyle: style?['italic'] == true ? FontStyle.italic : null,
            decoration: style?['underline'] == true ? TextDecoration.underline : null,
          ),
        ),
        textDirection: TextDirection.ltr,
        maxLines: 1,
      )..layout();
      final left = xs[i], right = xs[i + 1];
      var clip = right;
      if (align == TextAlign.left && tp.width > right - left - 4) {
        // Spill into empty cells to the right, then into the free width
        // after the last column.
        var j = i + 1;
        for (; j < cols.length && cells[cols[j]] == null && clip - left < tp.width + 4; j++) {
          clip = xs[j + 1];
        }
        if (j == cols.length && clip - left < tp.width + 4) {
          clip = (left + tp.width + 4).clamp(clip, size.width);
        }
      }
      canvas.save();
      canvas.clipRect(Rect.fromLTRB(left, 0, clip, size.height));
      final y = (size.height - tp.height) / 2;
      if (type == 'n' && tp.width > right - left - 3) {
        final hashes = TextPainter(
          text: const TextSpan(text: '###', style: TextStyle(fontSize: _fontPt, color: Color(0xFF000000))),
          textDirection: TextDirection.ltr,
        )..layout();
        hashes.paint(canvas, Offset(right - hashes.width - 2, y));
      } else {
        final dx = switch (align) {
          TextAlign.right => right - tp.width - 2,
          TextAlign.center => left + (right - left - tp.width) / 2,
          _ => left + 2,
        };
        tp.paint(canvas, Offset(dx, y));
      }
      canvas.restore();
    }
    final grid = Paint()
      ..color = const Color(0xFFBBBBBB)
      ..strokeWidth = 0.5;
    final gridRight = xs.last;
    if (topLine) canvas.drawLine(Offset.zero, Offset(gridRight, 0), grid);
    canvas.drawLine(Offset(0, size.height), Offset(gridRight, size.height), grid);
    for (final x in xs) {
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), grid);
    }
  }

  @override
  bool shouldRepaint(_RowPainter old) => old.row != row || old.sheet != sheet || old.cols != cols || old.topLine != topLine;
}
