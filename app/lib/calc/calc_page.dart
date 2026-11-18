// Akshara Calc: the spreadsheet UI.
//
// The grid is a custom-painted viewport over a virtual sheet of
// 1,048,576 x 1024 cells. Only the visible window is fetched from the core.

import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';

import '../core/document.dart';
import '../core/fonts.dart';
import '../core/file_dialog.dart';
import '../core/native.dart';
import '../core/print_preview.dart';
import '../core/translit.dart';
import 'calc_page_setup.dart';
import 'calc_print.dart';

const int maxRows = 1048576;
const int maxCols = 1024;
const double _defaultWidthPt = 64; // ~0.89in, OpenOffice's default
const double _rowHeightPx = 22;
const double _rowHeaderPx = 52;
const double _colHeaderPx = 22;

String colName(int col) {
  var n = col + 1;
  var s = '';
  while (n > 0) {
    final rem = (n - 1) % 26;
    s = String.fromCharCode(65 + rem) + s;
    n = (n - 1) ~/ 26;
  }
  return s;
}

String cellName(int row, int col) => '${colName(col)}${row + 1}';

/// Parses "B7" into (row, col).
(int, int)? parseCell(String s) {
  final m = RegExp(r'^\$?([A-Za-z]{1,3})\$?(\d{1,7})$').firstMatch(s.trim());
  if (m == null) return null;
  var col = 0;
  for (final ch in m.group(1)!.toUpperCase().codeUnits) {
    col = col * 26 + (ch - 64);
  }
  final row = int.parse(m.group(2)!);
  if (col < 1 || col > maxCols || row < 1 || row > maxRows) return null;
  return (row - 1, col - 1);
}

Color? _color(Object? hex) {
  if (hex is! String || hex.length != 7) return null;
  final v = int.tryParse(hex.substring(1), radix: 16);
  return v == null ? null : Color(0xFF000000 | v);
}

String _hex(Color c) => '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';

class _Cell {
  _Cell(this.text, this.type, this.style);

  final String text;
  final String type;
  final int style;
}

class CalcPage extends StatefulWidget {
  const CalcPage({super.key, required this.doc});

  final OpenDoc doc;

  @override
  State<CalcPage> createState() => CalcPageState();
}

class CalcPageState extends State<CalcPage> {
  int _sheet = 0;
  List<Map<String, dynamic>> _sheets = [];
  double _zoom = 1.0;

  // Viewport (in pixels, at the current zoom).
  double _scrollX = 0, _scrollY = 0;
  Size _viewSize = Size.zero;

  // Selection.
  int _anchorRow = 0, _anchorCol = 0, _row = 0, _col = 0;

  // Fetched window.
  final Map<int, _Cell> _cells = {};
  Map<String, dynamic> _styles = {};
  Map<int, double> _widthsPt = {};
  int _usedRows = 0, _usedCols = 0;

  // Current cell details for the formula bar.
  Map<String, dynamic> _cellInfo = {};

  // Editing.
  bool _editing = false;
  final _editor = TextEditingController();
  // Formulas keep their Latin function names: only quoted text inside a
  // formula is transliterated.
  final _editorTranslit = TransliterationFormatter(allowAt: _translitAllowed);
  final _formulaTranslit = TransliterationFormatter(allowAt: _translitAllowed);

  static bool _translitAllowed(String text, int offset) =>
      !text.trimLeft().startsWith('=') || insideQuotes(text, offset);
  final _editorFocus = FocusNode(debugLabel: 'cell editor');
  final _formula = TextEditingController();
  final _formulaFocus = FocusNode(debugLabel: 'formula bar');
  final _nameBox = TextEditingController(text: 'A1');
  final _gridFocus = FocusNode(debugLabel: 'grid');

  // Column resizing.
  int? _resizeCol;
  double _resizeStartX = 0, _resizeStartW = 0;
  bool _selecting = false;

  int get _doc => widget.doc.handle;

  @override
  void initState() {
    super.initState();
    _loadSheets();
    _fetch();
    WidgetsBinding.instance.addPostFrameCallback((_) => _gridFocus.requestFocus());
  }

  @override
  void dispose() {
    _editor.dispose();
    _editorFocus.dispose();
    _formula.dispose();
    _formulaFocus.dispose();
    _nameBox.dispose();
    _gridFocus.dispose();
    super.dispose();
  }

  dynamic _call(String op, [Map<String, Object?> args = const {}]) {
    try {
      return core({'op': op, 'doc': _doc, ...args});
    } on CoreError catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
      return null;
    }
  }

  void _loadSheets() {
    final sheets = _call('calc.sheets') as List<dynamic>?;
    if (sheets == null) return;
    setState(() {
      _sheets = sheets.cast<Map<String, dynamic>>();
      if (_sheet >= _sheets.length) _sheet = _sheets.length - 1;
      widget.doc.currentSheet = _sheet;
    });
  }

  // ---- geometry -------------------------------------------------------------------

  double get _pt => 1.3333 * _zoom;
  double get _rowH => _rowHeightPx * _zoom;

  double _colWidth(int col) => (_widthsPt[col] ?? _defaultWidthPt) * _pt;

  double _colX(int col) {
    var x = col * _defaultWidthPt * _pt;
    for (final e in _widthsPt.entries) {
      if (e.key < col) x += (e.value - _defaultWidthPt) * _pt;
    }
    return x;
  }

  int _colAt(double x) {
    if (_widthsPt.isEmpty) return (x / (_defaultWidthPt * _pt)).floor().clamp(0, maxCols - 1);
    var pos = 0.0;
    for (var c = 0; c < maxCols; c++) {
      final w = _colWidth(c);
      if (x < pos + w) return c;
      pos += w;
    }
    return maxCols - 1;
  }

  int _rowAt(double y) => (y / _rowH).floor().clamp(0, maxRows - 1);

  double get _maxScrollX => math.max(0, _colX(maxCols) - (_viewSize.width - _rowHeaderPx));
  double get _maxScrollY => math.max(0, maxRows * _rowH - (_viewSize.height - _colHeaderPx));

  // ---- data ------------------------------------------------------------------------

  void _fetch() {
    final firstRow = _rowAt(_scrollY);
    final firstCol = _colAt(_scrollX);
    final rows = ((_viewSize.height / _rowH).ceil() + 2).clamp(10, 500);
    var cols = 1;
    var w = 0.0;
    while (w < _viewSize.width + 200 && firstCol + cols < maxCols && cols < 200) {
      w += _colWidth(firstCol + cols);
      cols++;
    }
    final view = _call('calc.view', {
      'sheet': _sheet,
      'row': firstRow,
      'col': firstCol,
      'rows': rows,
      'cols': math.max(cols, 10),
    }) as Map<String, dynamic>?;
    if (view == null) return;
    final info = _call('calc.cell', {'sheet': _sheet, 'cell': cellName(_row, _col)}) as Map<String, dynamic>?;
    setState(() {
      _cells.clear();
      for (final c in (view['cells'] as List<dynamic>).cast<Map<String, dynamic>>()) {
        _cells[(c['r'] as int) * maxCols + (c['c'] as int)] = _Cell(c['t'] as String, c['v'] as String, c['s'] as int);
      }
      _styles = view['styles'] as Map<String, dynamic>;
      _widthsPt = {
        for (final e in (view['widths'] as Map<String, dynamic>).entries) int.parse(e.key): (e.value as num).toDouble(),
      };
      final used = view['used'] as Map<String, dynamic>?;
      _usedRows = used?['rows'] as int? ?? 0;
      _usedCols = used?['cols'] as int? ?? 0;
      _cellInfo = info ?? {};
      if (!_formulaFocus.hasFocus) _formula.text = _cellInfo['input'] as String? ?? '';
      _nameBox.text = _anchorRow == _row && _anchorCol == _col
          ? cellName(_row, _col)
          : '${cellName(math.min(_anchorRow, _row), math.min(_anchorCol, _col))}:${cellName(math.max(_anchorRow, _row), math.max(_anchorCol, _col))}';
    });
    widget.doc.refresh();
  }

  String get _selectionRange {
    final r0 = math.min(_anchorRow, _row), r1 = math.max(_anchorRow, _row);
    final c0 = math.min(_anchorCol, _col), c1 = math.max(_anchorCol, _col);
    return r0 == r1 && c0 == c1 ? cellName(r0, c0) : '${cellName(r0, c0)}:${cellName(r1, c1)}';
  }

  // ---- selection & navigation -----------------------------------------------------------

  void select(int row, int col, {bool extend = false}) {
    row = row.clamp(0, maxRows - 1);
    col = col.clamp(0, maxCols - 1);
    if (_editing) _commitEdit();
    _row = row;
    _col = col;
    if (!extend) {
      _anchorRow = row;
      _anchorCol = col;
    }
    _ensureVisible(row, col);
    _fetch();
  }

  void _ensureVisible(int row, int col) {
    final viewW = _viewSize.width - _rowHeaderPx, viewH = _viewSize.height - _colHeaderPx;
    if (viewW <= 0 || viewH <= 0) return;
    final x = _colX(col), w = _colWidth(col);
    final y = row * _rowH;
    if (x < _scrollX) _scrollX = x;
    if (x + w > _scrollX + viewW) _scrollX = x + w - viewW;
    if (y < _scrollY) _scrollY = y;
    if (y + _rowH > _scrollY + viewH) _scrollY = y + _rowH - viewH;
    _scrollX = _scrollX.clamp(0, _maxScrollX);
    _scrollY = _scrollY.clamp(0, _maxScrollY);
  }

  void _scrollBy(double dx, double dy) {
    setState(() {
      _scrollX = (_scrollX + dx).clamp(0, _maxScrollX);
      _scrollY = (_scrollY + dy).clamp(0, _maxScrollY);
    });
    _fetch();
  }

  // ---- editing ------------------------------------------------------------------------

  void _startEdit({String? initial}) {
    setState(() {
      _editing = true;
      _editorTranslit.reset();
      if (initial != null && initial.length == 1) {
        // A key typed on the grid starts the edit: let it go through the
        // transliteration like every following key.
        _editor.value = _editorTranslit.formatEditUpdate(
          TextEditingValue.empty,
          TextEditingValue(text: initial, selection: const TextSelection.collapsed(offset: 1)),
        );
      } else {
        _editor.text = initial ?? (_cellInfo['input'] as String? ?? '');
        _editor.selection = TextSelection.collapsed(offset: _editor.text.length);
      }
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => _editorFocus.requestFocus());
  }

  void _commitEdit({int dRow = 0, int dCol = 0}) {
    if (!_editing) return;
    _editing = false;
    _call('calc.set_input', {'sheet': _sheet, 'cell': cellName(_row, _col), 'text': _editor.text});
    _gridFocus.requestFocus();
    if (dRow != 0 || dCol != 0) {
      select(_row + dRow, _col + dCol);
    } else {
      _fetch();
    }
  }

  void _cancelEdit() {
    setState(() => _editing = false);
    _gridFocus.requestFocus();
  }

  void _commitFormulaBar() {
    _call('calc.set_input', {'sheet': _sheet, 'cell': cellName(_row, _col), 'text': _formula.text});
    _gridFocus.requestFocus();
    select(_row + 1, _col);
  }

  void undo() {
    if (_call('undo') == true) {
      _loadSheets();
      _fetch();
    }
  }

  void redo() {
    if (_call('redo') == true) {
      _loadSheets();
      _fetch();
    }
  }

  Future<void> _copy({bool cut = false}) async {
    final text = _call('calc.copy_text', {'sheet': _sheet, 'range': _selectionRange});
    if (text is String) {
      await Clipboard.setData(ClipboardData(text: text));
      if (cut) {
        _call('calc.clear', {'sheet': _sheet, 'range': _selectionRange});
        _fetch();
      }
    }
  }

  Future<void> _paste() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text == null || text.isEmpty) return;
    final r0 = math.min(_anchorRow, _row), c0 = math.min(_anchorCol, _col);
    _call('calc.paste', {'sheet': _sheet, 'cell': cellName(r0, c0), 'rows': text});
    _fetch();
  }

  void _style(Map<String, Object?> patch) {
    _call('calc.style', {'sheet': _sheet, 'range': _selectionRange, 'patch': patch});
    _fetch();
  }

  void _toggleStyle(String name) {
    final style = _cellInfo['style'] as Map<String, dynamic>? ?? {};
    _style({name: style[name] != true});
  }

  KeyEventResult _onGridKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) return KeyEventResult.ignored;
    if (_editing) {
      // Keys typed before the cell editor has taken focus must not be lost.
      final ch = event.character;
      if (ch != null && ch.isNotEmpty && ch.codeUnitAt(0) >= 32 && ch.codeUnitAt(0) != 127) {
        final old = _editor.value;
        final typed = TextEditingValue(
          text: old.text + ch,
          selection: TextSelection.collapsed(offset: old.text.length + ch.length),
        );
        _editor.value = _editorTranslit.formatEditUpdate(
          old.copyWith(selection: TextSelection.collapsed(offset: old.text.length)),
          typed,
        );
        return KeyEventResult.handled;
      }
      return _onEditorKey(node, event);
    }
    final keys = HardwareKeyboard.instance;
    final ctrl = keys.isControlPressed || keys.isMetaPressed;
    final shift = keys.isShiftPressed;
    final key = event.logicalKey;
    final page = math.max(1, ((_viewSize.height - _colHeaderPx) / _rowH).floor() - 1);
    void move(int dr, int dc) => select(_row + dr, _col + dc, extend: shift);
    if (ctrl) {
      if (key == LogicalKeyboardKey.keyP) {
        _print();
      } else if (key == LogicalKeyboardKey.keyC) {
        _copy();
      } else if (key == LogicalKeyboardKey.keyX) {
        _copy(cut: true);
      } else if (key == LogicalKeyboardKey.keyV) {
        _paste();
      } else if (key == LogicalKeyboardKey.keyZ && !shift) {
        undo();
      } else if (key == LogicalKeyboardKey.keyY || (key == LogicalKeyboardKey.keyZ && shift)) {
        redo();
      } else if (key == LogicalKeyboardKey.keyB) {
        _toggleStyle('bold');
      } else if (key == LogicalKeyboardKey.keyI) {
        _toggleStyle('italic');
      } else if (key == LogicalKeyboardKey.keyU) {
        _toggleStyle('underline');
      } else if (key == LogicalKeyboardKey.home) {
        select(0, 0, extend: shift);
      } else if (key == LogicalKeyboardKey.end) {
        select(math.max(0, _usedRows - 1), math.max(0, _usedCols - 1), extend: shift);
      } else if (key == LogicalKeyboardKey.keyA) {
        _anchorRow = 0;
        _anchorCol = 0;
        select(math.max(0, _usedRows - 1), math.max(0, _usedCols - 1), extend: true);
      } else if (key == LogicalKeyboardKey.arrowDown) {
        move(math.max(1, _usedRows - 1 - _row), 0);
      } else if (key == LogicalKeyboardKey.arrowRight) {
        move(0, math.max(1, _usedCols - 1 - _col));
      } else if (key == LogicalKeyboardKey.arrowUp) {
        select(0, _col, extend: shift);
      } else if (key == LogicalKeyboardKey.arrowLeft) {
        select(_row, 0, extend: shift);
      } else {
        return KeyEventResult.ignored;
      }
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowUp) {
      move(-1, 0);
    } else if (key == LogicalKeyboardKey.arrowDown) {
      move(1, 0);
    } else if (key == LogicalKeyboardKey.arrowLeft) {
      move(0, -1);
    } else if (key == LogicalKeyboardKey.arrowRight) {
      move(0, 1);
    } else if (key == LogicalKeyboardKey.pageDown) {
      move(page, 0);
    } else if (key == LogicalKeyboardKey.pageUp) {
      move(-page, 0);
    } else if (key == LogicalKeyboardKey.home) {
      select(_row, 0, extend: shift);
    } else if (key == LogicalKeyboardKey.tab) {
      select(_row, _col + (shift ? -1 : 1));
    } else if (key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.numpadEnter) {
      select(_row + (shift ? -1 : 1), _col);
    } else if (key == LogicalKeyboardKey.f2) {
      _startEdit();
    } else if (key == LogicalKeyboardKey.delete || key == LogicalKeyboardKey.backspace) {
      _call('calc.clear', {'sheet': _sheet, 'range': _selectionRange});
      _fetch();
    } else if (event.character != null &&
        event.character!.isNotEmpty &&
        event.character!.codeUnitAt(0) >= 32 &&
        event.character!.codeUnitAt(0) != 127) {
      _startEdit(initial: event.character);
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  KeyEventResult _onEditorKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final shift = HardwareKeyboard.instance.isShiftPressed;
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.numpadEnter) {
      if (HardwareKeyboard.instance.isAltPressed) return KeyEventResult.ignored;
      _commitEdit(dRow: shift ? -1 : 1);
    } else if (key == LogicalKeyboardKey.tab) {
      _commitEdit(dCol: shift ? -1 : 1);
    } else if (key == LogicalKeyboardKey.escape) {
      _cancelEdit();
    } else {
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  // ---- pointer ------------------------------------------------------------------------

  (int, int)? _hit(Offset p) {
    if (p.dx < _rowHeaderPx || p.dy < _colHeaderPx) return null;
    return (_rowAt(p.dy - _colHeaderPx + _scrollY), _colAt(p.dx - _rowHeaderPx + _scrollX));
  }

  int? _resizeHandleAt(Offset p) {
    if (p.dy > _colHeaderPx || p.dx < _rowHeaderPx) return null;
    final x = p.dx - _rowHeaderPx + _scrollX;
    final col = _colAt(x);
    final right = _colX(col) + _colWidth(col);
    if ((x - right).abs() < 5) return col;
    final left = _colX(col);
    if (col > 0 && (x - left).abs() < 5) return col - 1;
    return null;
  }

  void _pointerDown(PointerDownEvent e) {
    _gridFocus.requestFocus();
    final p = e.localPosition;
    final handle = _resizeHandleAt(p);
    if (handle != null && e.kind == PointerDeviceKind.mouse) {
      _resizeCol = handle;
      _resizeStartX = p.dx;
      _resizeStartW = _widthsPt[handle] ?? _defaultWidthPt;
      return;
    }
    final shift = HardwareKeyboard.instance.isShiftPressed;
    if (p.dy < _colHeaderPx && p.dx >= _rowHeaderPx) {
      // Column header: select the whole used column.
      final col = _colAt(p.dx - _rowHeaderPx + _scrollX);
      if (!shift) {
        _anchorCol = col;
        _anchorRow = 0;
      }
      _row = math.max(0, _usedRows - 1);
      _col = col;
      _fetch();
      return;
    }
    if (p.dx < _rowHeaderPx && p.dy >= _colHeaderPx) {
      final row = _rowAt(p.dy - _colHeaderPx + _scrollY);
      if (!shift) {
        _anchorRow = row;
        _anchorCol = 0;
      }
      _row = row;
      _col = math.max(0, _usedCols - 1);
      _fetch();
      return;
    }
    final hit = _hit(p);
    if (hit == null) return;
    if (e.kind == PointerDeviceKind.mouse && (e.buttons & kSecondaryMouseButton) != 0) {
      final inside = _inSelection(hit.$1, hit.$2);
      if (!inside) select(hit.$1, hit.$2);
      _contextMenu(e.position);
      return;
    }
    select(hit.$1, hit.$2, extend: shift);
    _selecting = e.kind == PointerDeviceKind.mouse;
  }

  bool _inSelection(int r, int c) =>
      r >= math.min(_anchorRow, _row) &&
      r <= math.max(_anchorRow, _row) &&
      c >= math.min(_anchorCol, _col) &&
      c <= math.max(_anchorCol, _col);

  void _pointerMove(PointerMoveEvent e) {
    if (_resizeCol != null) {
      final w = (_resizeStartW + (e.localPosition.dx - _resizeStartX) / _pt).clamp(8.0, 1500.0);
      setState(() => _widthsPt[_resizeCol!] = w);
      return;
    }
    if (_selecting) {
      final hit = _hit(e.localPosition);
      if (hit != null && (hit.$1 != _row || hit.$2 != _col)) {
        setState(() {
          _row = hit.$1;
          _col = hit.$2;
        });
      }
    } else if (e.kind != PointerDeviceKind.mouse) {
      _scrollBy(-e.delta.dx, -e.delta.dy);
    }
  }

  void _pointerUp(PointerUpEvent e) {
    if (_resizeCol != null) {
      _call('calc.col_width', {'sheet': _sheet, 'col': _resizeCol, 'width': _widthsPt[_resizeCol!]});
      _resizeCol = null;
      _fetch();
    }
    if (_selecting) {
      _selecting = false;
      _fetch();
    }
  }

  Future<void> _contextMenu(Offset pos) async {
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(pos.dx, pos.dy, pos.dx, pos.dy),
      items: [
        PopupMenuItem(value: 'cut', child: Text(t('Cut'))),
        PopupMenuItem(value: 'copy', child: Text(t('Copy'))),
        PopupMenuItem(value: 'paste', child: Text(t('Paste'))),
        PopupMenuItem(value: 'clear', child: Text(t('Clear contents'))),
        PopupMenuDivider(),
        PopupMenuItem(value: 'row_above', child: Text(t('Insert row above'))),
        PopupMenuItem(value: 'col_left', child: Text(t('Insert column left'))),
        PopupMenuItem(value: 'del_rows', child: Text(t('Delete rows'))),
        PopupMenuItem(value: 'del_cols', child: Text(t('Delete columns'))),
      ],
    );
    switch (action) {
      case 'cut':
        _copy(cut: true);
      case 'copy':
        _copy();
      case 'paste':
        _paste();
      case 'clear':
        _call('calc.clear', {'sheet': _sheet, 'range': _selectionRange});
        _fetch();
      case 'row_above':
        _structure('calc.insert', 'row');
      case 'col_left':
        _structure('calc.insert', 'col');
      case 'del_rows':
        _structure('calc.delete', 'row');
      case 'del_cols':
        _structure('calc.delete', 'col');
    }
  }

  void _structure(String op, String axis) {
    final at = axis == 'row' ? math.min(_anchorRow, _row) : math.min(_anchorCol, _col);
    final count = axis == 'row' ? (_anchorRow - _row).abs() + 1 : (_anchorCol - _col).abs() + 1;
    _call(op, {'sheet': _sheet, 'axis': axis, 'at': at, 'count': count});
    _fetch();
  }

  /// Converts the text cells of the selection with teltools.
  void _convertCells(String spec) {
    final parts = spec.split(':');
    final count = _call('calc.transliterate', {
      'sheet': _sheet,
      'range': _selectionRange,
      'mode': parts[0],
      'script': parts[1],
      'native_digits': TranslitSettings.instance.nativeDigits,
    });
    _fetch();
    if (count is int && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t('Converted {n} cell(s).', {'n': count}))));
    }
  }

  void _sort(bool ascending) {
    var range = _selectionRange;
    if (!range.contains(':')) {
      if (_usedRows == 0) return;
      range = '${cellName(0, 0)}:${cellName(_usedRows - 1, math.max(0, _usedCols - 1))}';
    }
    _call('calc.sort', {'sheet': _sheet, 'range': range, 'col': _col, 'ascending': ascending});
    _fetch();
  }

  void _fill(bool down) {
    final r0 = math.min(_anchorRow, _row), r1 = math.max(_anchorRow, _row);
    final c0 = math.min(_anchorCol, _col), c1 = math.max(_anchorCol, _col);
    final src = down ? '${cellName(r0, c0)}:${cellName(r0, c1)}' : '${cellName(r0, c0)}:${cellName(r1, c0)}';
    _call('calc.fill', {'sheet': _sheet, 'src': src, 'dst': _selectionRange});
    _fetch();
  }

  void _autoSum() {
    // Sum the run of numbers above the current cell, like OpenOffice.
    var top = _row - 1;
    while (top >= 0 && _cells[top * maxCols + _col]?.type == 'n') {
      top--;
    }
    final formula = top + 1 <= _row - 1 ? '=SUM(${cellName(top + 1, _col)}:${cellName(_row - 1, _col)})' : '=SUM()';
    _startEdit(initial: formula);
  }

  Future<void> _insertFunction() async {
    final names = (_call('calc.functions') as List<dynamic>? ?? []).cast<String>();
    final picked = await showDialog<String>(
      context: context,
      builder: (c) => _FunctionPicker(names: names),
    );
    if (picked != null) _startEdit(initial: '=$picked(');
  }

  // ---- sheets ------------------------------------------------------------------------------

  void _switchSheet(int index) {
    if (_editing) _commitEdit();
    setState(() {
      _sheet = index;
      widget.doc.currentSheet = index;
      _scrollX = 0;
      _scrollY = 0;
      _row = _col = _anchorRow = _anchorCol = 0;
    });
    _fetch();
  }

  Future<void> _sheetMenu(int index, Offset pos) async {
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(pos.dx, pos.dy, pos.dx, pos.dy),
      items: [
        PopupMenuItem(value: 'rename', child: Text(t('Rename sheet…'))),
        PopupMenuItem(value: 'delete', child: Text(t('Delete sheet'))),
      ],
    );
    if (!mounted) return;
    if (action == 'rename') {
      final name = await askText(context, 'Rename sheet', initial: _sheets[index]['name'] as String);
      if (name != null && name.trim().isNotEmpty) {
        _call('calc.rename_sheet', {'sheet': index, 'name': name.trim()});
      }
    } else if (action == 'delete') {
      _call('calc.remove_sheet', {'sheet': index});
      if (_sheet >= index && _sheet > 0) _sheet--;
    }
    _loadSheets();
    _fetch();
  }

  // ---- UI ---------------------------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Column(
      children: [
        _toolbar(theme),
        _formulaBar(theme),
        const Divider(height: 1),
        Expanded(
          child: LayoutBuilder(builder: (context, constraints) {
            final size = constraints.biggest;
            if (size != _viewSize) {
              _viewSize = size;
              WidgetsBinding.instance.addPostFrameCallback((_) => _fetch());
            }
            return Focus(
              focusNode: _gridFocus,
              onKeyEvent: _onGridKey,
              child: Stack(
                children: [
                  Positioned.fill(
                    child: Listener(
                      onPointerDown: _pointerDown,
                      onPointerMove: _pointerMove,
                      onPointerUp: _pointerUp,
                      onPointerSignal: (e) {
                        if (e is PointerScrollEvent) {
                          final shift = HardwareKeyboard.instance.isShiftPressed;
                          _scrollBy(shift ? e.scrollDelta.dy : e.scrollDelta.dx, shift ? 0 : e.scrollDelta.dy);
                        }
                      },
                      child: GestureDetector(
                        onDoubleTap: _startEdit,
                        child: MouseRegion(
                          cursor: SystemMouseCursors.basic,
                          child: CustomPaint(
                            painter: _GridPainter(this, theme),
                            size: Size.infinite,
                          ),
                        ),
                      ),
                    ),
                  ),
                  if (_editing) _editorOverlay(theme),
                ],
              ),
            );
          }),
        ),
        _sheetBar(theme),
      ],
    );
  }

  Widget _editorOverlay(ThemeData theme) {
    final x = _rowHeaderPx + _colX(_col) - _scrollX;
    final y = _colHeaderPx + _row * _rowH - _scrollY;
    return Positioned(
      left: x,
      top: y,
      child: Container(
        constraints: BoxConstraints(minWidth: math.max(_colWidth(_col), 120), minHeight: _rowH, maxWidth: 480),
        decoration: BoxDecoration(
          color: theme.colorScheme.surface,
          border: Border.all(color: theme.colorScheme.primary, width: 2),
        ),
        child: IntrinsicWidth(
          child: Focus(
            onKeyEvent: _onEditorKey,
            child: TextField(
              controller: _editor,
              inputFormatters: [_editorTranslit],
              autocorrect: !TranslitSettings.instance.enabled,
              enableSuggestions: !TranslitSettings.instance.enabled,
              focusNode: _editorFocus,
              maxLines: null,
              style: TextStyle(fontSize: 13 * _zoom),
              decoration: const InputDecoration.collapsed(hintText: null).copyWith(
                contentPadding: const EdgeInsets.symmetric(horizontal: 3, vertical: 2),
              ),
              onTapOutside: (_) {},
            ),
          ),
        ),
      ),
    );
  }

  Widget _toolbar(ThemeData theme) {
    final style = _cellInfo['style'] as Map<String, dynamic>? ?? {};
    return Material(
      color: theme.colorScheme.surfaceContainer,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        child: Row(children: [
          IconButton(tooltip: t('Undo (Ctrl+Z)'), icon: const Icon(Icons.undo), onPressed: widget.doc.canUndo ? undo : null),
          IconButton(tooltip: t('Redo (Ctrl+Y)'), icon: const Icon(Icons.redo), onPressed: widget.doc.canRedo ? redo : null),
          const _Sep(),
          IconButton(tooltip: t('Bold'), isSelected: style['bold'] == true, icon: const Icon(Icons.format_bold), onPressed: () => _toggleStyle('bold')),
          IconButton(tooltip: t('Italic'), isSelected: style['italic'] == true, icon: const Icon(Icons.format_italic), onPressed: () => _toggleStyle('italic')),
          IconButton(tooltip: t('Underline'), isSelected: style['underline'] == true, icon: const Icon(Icons.format_underlined), onPressed: () => _toggleStyle('underline')),
          _colorMenu('Text colour', Icons.format_color_text, 'color'),
          _colorMenu('Background', Icons.format_color_fill, 'background'),
          const _Sep(),
          IconButton(tooltip: t('Align left'), isSelected: style['align'] == 'left', icon: const Icon(Icons.format_align_left), onPressed: () => _style({'align': style['align'] == 'left' ? 'general' : 'left'})),
          IconButton(tooltip: t('Centre'), isSelected: style['align'] == 'center', icon: const Icon(Icons.format_align_center), onPressed: () => _style({'align': style['align'] == 'center' ? 'general' : 'center'})),
          IconButton(tooltip: t('Align right'), isSelected: style['align'] == 'right', icon: const Icon(Icons.format_align_right), onPressed: () => _style({'align': style['align'] == 'right' ? 'general' : 'right'})),
          _Sep(),
          PopupMenuButton<Map<String, Object?>>(
            tooltip: t('Number format'),
            icon: Icon(Icons.onetwothree),
            itemBuilder: (_) => [
              PopupMenuItem(value: const {'kind': 'general'}, child: Text(t('General'))),
              PopupMenuItem(value: const {'kind': 'number', 'decimals': 2, 'thousands': false}, child: Text('${t('Number')}  0.00')),
              PopupMenuItem(value: const {'kind': 'number', 'decimals': 2, 'thousands': true}, child: Text('${t('Number')}  1,234.00')),
              PopupMenuItem(value: const {'kind': 'indian', 'decimals': 2}, child: Text('${t('Indian grouping')}  12,34,567.00')),
              PopupMenuItem(value: const {'kind': 'indian', 'decimals': 0}, child: Text('${t('Indian grouping')}  12,34,567')),
              PopupMenuItem(value: const {'kind': 'number', 'decimals': 0, 'thousands': true}, child: Text('${t('Integer')}  1,234')),
              PopupMenuItem(value: const {'kind': 'percent', 'decimals': 0}, child: Text('${t('Percent')}  12%')),
              PopupMenuItem(value: const {'kind': 'percent', 'decimals': 2}, child: Text('${t('Percent')}  12.34%')),
              PopupMenuItem(value: const {'kind': 'currency', 'decimals': 2, 'symbol': '₹'}, child: Text('${t('Currency')}  ₹12,34,567.00')),
              PopupMenuItem(value: const {'kind': 'currency', 'decimals': 2, 'symbol': r'$'}, child: Text('${t('Currency')}  \$1,234.00')),
              PopupMenuItem(value: const {'kind': 'currency', 'decimals': 2, 'symbol': '€'}, child: Text('${t('Currency')}  €1,234.00')),
              PopupMenuItem(value: const {'kind': 'currency', 'decimals': 2, 'symbol': '£'}, child: Text('${t('Currency')}  £1,234.00')),
              PopupMenuItem(value: const {'kind': 'scientific', 'decimals': 2}, child: Text('${t('Scientific')}  1.23E+04')),
              PopupMenuItem(value: const {'kind': 'date_dmy'}, child: Text('${t('Date')}  31-12-2026')),
              PopupMenuItem(value: const {'kind': 'date'}, child: Text('${t('Date')}  2026-12-31')),
              PopupMenuItem(value: const {'kind': 'time'}, child: Text('${t('Time')}  13:45:00')),
              PopupMenuItem(value: const {'kind': 'datetime'}, child: Text(t('Date & time'))),
              PopupMenuItem(value: const {'kind': 'text'}, child: Text(t('Text (no formulas)'))),
            ],
            onSelected: (f) => _style({'format': f}),
          ),
          IconButton(tooltip: t('AutoSum'), icon: const Icon(Icons.functions), onPressed: _autoSum),
          IconButton(tooltip: t('Insert function…'), icon: const Icon(Icons.calculate_outlined), onPressed: _insertFunction),
          const _Sep(),
          PopupMenuButton<String>(
            tooltip: t('Convert text'),
            icon: const Icon(Icons.sync_alt),
            itemBuilder: (_) => convertMenuItems(),
            onSelected: _convertCells,
          ),
          IconButton(tooltip: t('Sort ascending'), icon: const Icon(Icons.arrow_upward), onPressed: () => _sort(true)),
          IconButton(tooltip: t('Sort descending'), icon: const Icon(Icons.arrow_downward), onPressed: () => _sort(false)),
          IconButton(tooltip: t('Fill down'), icon: const Icon(Icons.vertical_align_bottom), onPressed: () => _fill(true)),
          IconButton(tooltip: t('Fill right'), icon: const Icon(Icons.keyboard_tab), onPressed: () => _fill(false)),
          const _Sep(),
          IconButton(tooltip: t('Insert rows'), icon: const Icon(Icons.table_rows_outlined), onPressed: () => _structure('calc.insert', 'row')),
          IconButton(tooltip: t('Insert columns'), icon: const Icon(Icons.view_column_outlined), onPressed: () => _structure('calc.insert', 'col')),
          IconButton(tooltip: t('Delete rows'), icon: const Icon(Icons.playlist_remove), onPressed: () => _structure('calc.delete', 'row')),
          IconButton(tooltip: t('Delete columns'), icon: const Icon(Icons.delete_sweep_outlined), onPressed: () => _structure('calc.delete', 'col')),
          const _Sep(),
          IconButton(tooltip: t('Page setup…'), icon: const Icon(Icons.crop_portrait), onPressed: _pageSetup),
          IconButton(tooltip: t('Print (Ctrl+P)'), icon: const Icon(Icons.print_outlined), onPressed: _print),
        ]),
      ),
    );
  }

  /// Print preview of every non-empty sheet, with the system print dialog.
  void _print() {
    try {
      showPrintPreview(context, calcPrintJob(widget.doc.handle, title: widget.doc.title));
    } on CoreError catch (e) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    }
  }

  /// Paper, margin, scale, grid lines, header and footer for printing.
  Future<void> _pageSetup() async {
    final page = _call('calc.page') as Map<String, dynamic>?;
    if (page == null) return;
    final args = await showCalcPageSetup(context, page);
    if (args == null) return;
    _call('calc.set_page', args);
    widget.doc.refresh();
  }

  Widget _colorMenu(String tip, IconData icon, String attr) {
    const colors = <Color?>[
      null, Color(0xFF000000), Color(0xFFFFFFFF), Color(0xFFC00000), Color(0xFFFF9900), Color(0xFFFFF2CC),
      Color(0xFFE2EFDA), Color(0xFF00843D), Color(0xFFDDEBF7), Color(0xFF1F5FBF), Color(0xFF7F7F7F), Color(0xFFD9D9D9),
    ];
    return PopupMenuButton<int>(
      tooltip: t(tip),
      icon: Icon(icon),
      itemBuilder: (_) => [
        for (var i = 0; i < colors.length; i++)
          PopupMenuItem(
            value: i,
            child: Row(children: [
              Container(
                width: 20,
                height: 20,
                decoration: BoxDecoration(color: colors[i], border: Border.all(color: Colors.grey)),
                child: colors[i] == null ? const Icon(Icons.close, size: 16) : null,
              ),
              const SizedBox(width: 12),
              Text(colors[i] == null ? 'None / automatic' : _hex(colors[i]!)),
            ]),
          ),
      ],
      onSelected: (i) => _style({attr: colors[i] == null ? null : _hex(colors[i]!)}),
    );
  }

  Widget _formulaBar(ThemeData theme) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      child: Row(children: [
        SizedBox(
          width: 110,
          child: TextField(
            controller: _nameBox,
            style: const TextStyle(fontSize: 13),
            decoration: const InputDecoration(isDense: true, border: OutlineInputBorder()),
            onSubmitted: (v) {
              final parts = v.split(':');
              final a = parseCell(parts.first);
              if (a == null) return;
              if (parts.length == 2) {
                final b = parseCell(parts[1]);
                if (b != null) {
                  select(a.$1, a.$2);
                  select(b.$1, b.$2, extend: true);
                }
              } else {
                select(a.$1, a.$2);
              }
              _gridFocus.requestFocus();
            },
          ),
        ),
        const SizedBox(width: 8),
        Text('ƒx', style: theme.textTheme.titleMedium?.copyWith(fontStyle: FontStyle.italic)),
        const SizedBox(width: 8),
        Expanded(
          child: Focus(
            onKeyEvent: (node, event) {
              if (event is KeyDownEvent && event.logicalKey == LogicalKeyboardKey.escape) {
                _formula.text = _cellInfo['input'] as String? ?? '';
                _gridFocus.requestFocus();
                return KeyEventResult.handled;
              }
              return KeyEventResult.ignored;
            },
            child: TextField(
              controller: _formula,
              inputFormatters: [_formulaTranslit],
              autocorrect: !TranslitSettings.instance.enabled,
              enableSuggestions: !TranslitSettings.instance.enabled,
              focusNode: _formulaFocus,
              style: const TextStyle(fontSize: 13),
              decoration: const InputDecoration(isDense: true, border: OutlineInputBorder()),
              onSubmitted: (_) => _commitFormulaBar(),
            ),
          ),
        ),
      ]),
    );
  }

  Widget _sheetBar(ThemeData theme) {
    return Material(
      color: theme.colorScheme.surfaceContainer,
      child: SizedBox(
        height: 36,
        child: Row(children: [
          IconButton(
            tooltip: t('Add sheet'),
            icon: const Icon(Icons.add, size: 18),
            onPressed: () {
              final index = _call('calc.add_sheet');
              _loadSheets();
              if (index is int) _switchSheet(index);
            },
          ),
          Expanded(
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                for (var i = 0; i < _sheets.length; i++)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 4),
                    child: GestureDetector(
                      onSecondaryTapDown: (d) => _sheetMenu(i, d.globalPosition),
                      onLongPressStart: (d) => _sheetMenu(i, d.globalPosition),
                      child: ChoiceChip(
                        label: Text(_sheets[i]['name'] as String),
                        selected: i == _sheet,
                        onSelected: (_) => _switchSheet(i),
                      ),
                    ),
                  ),
              ],
            ),
          ),
          Text(_statusText(), style: theme.textTheme.bodySmall),
          IconButton(iconSize: 18, tooltip: t('Zoom out'), icon: const Icon(Icons.zoom_out), onPressed: () => setState(() {
                _zoom = (_zoom - 0.1).clamp(0.5, 3.0);
                _fetch();
              })),
          Text('${(_zoom * 100).round()}%', style: theme.textTheme.bodySmall),
          IconButton(iconSize: 18, tooltip: t('Zoom in'), icon: const Icon(Icons.zoom_in), onPressed: () => setState(() {
                _zoom = (_zoom + 0.1).clamp(0.5, 3.0);
                _fetch();
              })),
        ]),
      ),
    );
  }

  String _statusText() {
    // Sum / count of the visible part of the selection (like a status bar).
    if (_anchorRow == _row && _anchorCol == _col) return '';
    double sum = 0;
    var count = 0;
    for (final e in _cells.entries) {
      final r = e.key ~/ maxCols, c = e.key % maxCols;
      if (_inSelection(r, c) && e.value.type == 'n') {
        final v = double.tryParse(e.value.text.replaceAll(RegExp(r'[^0-9eE+\-.]'), ''));
        if (v != null) {
          sum += v;
          count++;
        }
      }
    }
    if (count == 0) return '';
    final s = sum == sum.roundToDouble() ? sum.toStringAsFixed(0) : sum.toStringAsFixed(4);
    return 'Sum=$s  Count=$count   ';
  }
}

class _GridPainter extends CustomPainter {
  _GridPainter(this.s, this.theme);

  final CalcPageState s;
  final ThemeData theme;

  @override
  void paint(Canvas canvas, Size size) {
    final dark = theme.brightness == Brightness.dark;
    final bg = dark ? const Color(0xFF1E1E1E) : Colors.white;
    final grid = dark ? const Color(0xFF3A3A3A) : const Color(0xFFDADCE0);
    final headerBg = dark ? const Color(0xFF2A2A2A) : const Color(0xFFF1F3F4);
    final headerSel = theme.colorScheme.primaryContainer;
    final textColor = theme.colorScheme.onSurface;
    final rowH = s._rowH;

    canvas.drawRect(Offset.zero & size, Paint()..color = bg);
    final firstRow = s._rowAt(s._scrollY);
    final lastRow = math.min(maxRows - 1, s._rowAt(s._scrollY + size.height));
    final firstCol = s._colAt(s._scrollX);
    final lastCol = math.min(maxCols - 1, s._colAt(s._scrollX + size.width));

    final r0 = math.min(s._anchorRow, s._row), r1 = math.max(s._anchorRow, s._row);
    final c0 = math.min(s._anchorCol, s._col), c1 = math.max(s._anchorCol, s._col);

    canvas.save();
    canvas.clipRect(Rect.fromLTRB(_rowHeaderPx, _colHeaderPx, size.width, size.height));
    final gridPaint = Paint()
      ..color = grid
      ..strokeWidth = 1;

    double xOf(int c) => _rowHeaderPx + s._colX(c) - s._scrollX;
    double yOf(int r) => _colHeaderPx + r * rowH - s._scrollY;

    // Cell backgrounds and text.
    final fontSize = 13 * s._zoom;
    for (var r = firstRow; r <= lastRow; r++) {
      for (var c = firstCol; c <= lastCol; c++) {
        final cell = s._cells[r * maxCols + c];
        if (cell == null) continue;
        final style = s._styles['${cell.style}'] as Map<String, dynamic>?;
        final rect = Rect.fromLTWH(xOf(c), yOf(r), s._colWidth(c), rowH);
        final background = _color(style?['background']);
        if (background != null) canvas.drawRect(rect, Paint()..color = background);
        if (cell.text.isEmpty) continue;
        final align = style?['align'] as String? ?? 'general';
        final isError = cell.type == 'x';
        final textAlign = switch (align) {
          'left' => TextAlign.left,
          'center' => TextAlign.center,
          'right' => TextAlign.right,
          _ => (cell.type == 'n' || cell.type == 'b' || isError) ? TextAlign.right : TextAlign.left,
        };
        final tp = TextPainter(
          text: TextSpan(
            text: cell.text.replaceAll('\n', ' '),
            style: TextStyle(
              fontFamily: uiFont,
              fontFamilyFallback: indicFallback,
              fontSize: fontSize,
              color: isError ? theme.colorScheme.error : (_color(style?['color']) ?? textColor),
              fontWeight: style?['bold'] == true ? FontWeight.bold : null,
              fontStyle: style?['italic'] == true ? FontStyle.italic : null,
              decoration: style?['underline'] == true ? TextDecoration.underline : null,
            ),
          ),
          textAlign: textAlign,
          textDirection: TextDirection.ltr,
          maxLines: 1,
        )..layout();
        // Text overflows into empty neighbours to the right (left-aligned
        // text) like in every spreadsheet; numbers that do not fit show ###.
        var clipRight = rect.right;
        if (textAlign == TextAlign.left && tp.width > rect.width - 6) {
          var c2 = c + 1;
          while (c2 <= lastCol + 1 && s._cells[r * maxCols + c2] == null && clipRight - rect.left < tp.width + 6) {
            clipRight += s._colWidth(c2);
            c2++;
          }
        }
        canvas.save();
        canvas.clipRect(Rect.fromLTRB(rect.left, rect.top, clipRight, rect.bottom));
        if (cell.type == 'n' && tp.width > rect.width - 4) {
          final hashes = TextPainter(
            text: TextSpan(text: '###', style: TextStyle(fontSize: fontSize, color: textColor)),
            textDirection: TextDirection.ltr,
          )..layout();
          hashes.paint(canvas, Offset(rect.right - hashes.width - 3, rect.top + (rowH - hashes.height) / 2));
        } else {
          final dx = switch (textAlign) {
            TextAlign.right => rect.right - tp.width - 3,
            TextAlign.center => rect.left + (rect.width - tp.width) / 2,
            _ => rect.left + 3,
          };
          tp.paint(canvas, Offset(dx, rect.top + (rowH - tp.height) / 2));
        }
        canvas.restore();
      }
    }

    // Grid lines.
    for (var r = firstRow; r <= lastRow + 1; r++) {
      final y = yOf(r);
      canvas.drawLine(Offset(_rowHeaderPx, y), Offset(size.width, y), gridPaint);
    }
    for (var c = firstCol; c <= lastCol + 1; c++) {
      final x = xOf(c);
      canvas.drawLine(Offset(x, _colHeaderPx), Offset(x, size.height), gridPaint);
    }

    // Selection.
    final selRect = Rect.fromLTRB(xOf(c0), yOf(r0), xOf(c1) + s._colWidth(c1), yOf(r1) + rowH);
    canvas.drawRect(selRect, Paint()..color = theme.colorScheme.primary.withValues(alpha: 0.12));
    canvas.drawRect(
      selRect,
      Paint()
        ..color = theme.colorScheme.primary
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
    final cur = Rect.fromLTWH(xOf(s._col), yOf(s._row), s._colWidth(s._col), rowH);
    canvas.drawRect(
      cur,
      Paint()
        ..color = theme.colorScheme.primary
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2.5,
    );
    canvas.restore();

    // Headers.
    final headerText = TextStyle(fontSize: 12 * math.min(1.2, s._zoom), color: textColor.withValues(alpha: 0.8));
    canvas.drawRect(Rect.fromLTWH(0, 0, size.width, _colHeaderPx), Paint()..color = headerBg);
    canvas.drawRect(Rect.fromLTWH(0, 0, _rowHeaderPx, size.height), Paint()..color = headerBg);
    canvas.save();
    canvas.clipRect(Rect.fromLTRB(_rowHeaderPx, 0, size.width, _colHeaderPx));
    for (var c = firstCol; c <= lastCol; c++) {
      final x = xOf(c), w = s._colWidth(c);
      if (c >= c0 && c <= c1) canvas.drawRect(Rect.fromLTWH(x, 0, w, _colHeaderPx), Paint()..color = headerSel);
      final tp = TextPainter(text: TextSpan(text: colName(c), style: headerText), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset(x + (w - tp.width) / 2, (_colHeaderPx - tp.height) / 2));
      canvas.drawLine(Offset(x, 0), Offset(x, _colHeaderPx), gridPaint);
    }
    canvas.restore();
    canvas.save();
    canvas.clipRect(Rect.fromLTRB(0, _colHeaderPx, _rowHeaderPx, size.height));
    for (var r = firstRow; r <= lastRow; r++) {
      final y = yOf(r);
      if (r >= r0 && r <= r1) canvas.drawRect(Rect.fromLTWH(0, y, _rowHeaderPx, rowH), Paint()..color = headerSel);
      final tp = TextPainter(text: TextSpan(text: '${r + 1}', style: headerText), textDirection: TextDirection.ltr)..layout();
      tp.paint(canvas, Offset((_rowHeaderPx - tp.width) / 2, y + (rowH - tp.height) / 2));
      canvas.drawLine(Offset(0, y), Offset(_rowHeaderPx, y), gridPaint);
    }
    canvas.restore();
    canvas.drawLine(const Offset(0, _colHeaderPx), Offset(size.width, _colHeaderPx), gridPaint);
    canvas.drawLine(const Offset(_rowHeaderPx, 0), Offset(_rowHeaderPx, size.height), gridPaint);
  }

  @override
  bool shouldRepaint(covariant _GridPainter old) => true;
}

class _FunctionPicker extends StatefulWidget {
  const _FunctionPicker({required this.names});

  final List<String> names;

  @override
  State<_FunctionPicker> createState() => _FunctionPickerState();
}

class _FunctionPickerState extends State<_FunctionPicker> {
  String _filter = '';

  @override
  Widget build(BuildContext context) {
    final shown = widget.names.where((n) => n.contains(_filter.toUpperCase())).toList();
    return AlertDialog(
      title: Text(t('Insert function')),
      content: SizedBox(
        width: 320,
        height: 400,
        child: Column(children: [
          TextField(
            autofocus: true,
            decoration: InputDecoration(prefixIcon: Icon(Icons.search), hintText: t('Search')),
            onChanged: (v) => setState(() => _filter = v),
            onSubmitted: (_) {
              if (shown.isNotEmpty) Navigator.pop(context, shown.first);
            },
          ),
          Expanded(
            child: ListView(children: [
              for (final n in shown) ListTile(dense: true, title: Text(n), onTap: () => Navigator.pop(context, n)),
            ]),
          ),
        ]),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel')))],
    );
  }
}

class _Sep extends StatelessWidget {
  const _Sep();

  @override
  Widget build(BuildContext context) => const SizedBox(height: 28, child: VerticalDivider(width: 12));
}
