// Akshara Writer: the word processor UI.
//
// The document model lives in Rust. Each paragraph is shown as its own
// multi-line text field whose controller paints the attributed runs; every
// edit is sent to the core, which owns undo/redo and file I/O.

import 'dart:io' show Platform;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';

import '../core/document.dart';
import '../core/file_dialog.dart';
import '../core/native.dart';
import '../core/translit.dart';
import 'page_dialogs.dart';
import 'page_view.dart';
import 'para_field.dart';
import 'print_preview.dart';

/// Address of a paragraph (top-level block, inside a table cell, or in the
/// page header / footer).
class ParaRef {
  const ParaRef(this.block, [this.cell, this.area = 'body']);

  const ParaRef.header(this.block)
      : cell = null,
        area = 'header';

  const ParaRef.footer(this.block)
      : cell = null,
        area = 'footer';

  final int block;
  final List<int>? cell; // [row, col, paragraph]
  final String area; // body, header, footer

  bool get isBody => area == 'body';

  Map<String, Object?> toJson() => {'block': block, if (cell != null) 'cell': cell, if (!isBody) 'area': area};

  static ParaRef fromJson(Map<String, dynamic> j) =>
      ParaRef(j['block'] as int, (j['cell'] as List?)?.cast<int>(), j['area'] as String? ?? 'body');

  String get key => !isBody ? '$area:$block' : (cell == null ? '$block' : '$block:${cell!.join(':')}');

  @override
  bool operator ==(Object other) => other is ParaRef && other.key == key;

  @override
  int get hashCode => key.hashCode;
}

const _textColors = <Color?>[
  null, Color(0xFF000000), Color(0xFF7F7F7F), Color(0xFFC00000), Color(0xFFE36C09),
  Color(0xFF9C6500), Color(0xFF00843D), Color(0xFF1F5FBF), Color(0xFF6A3FA0),
];
const _highlights = <Color?>[
  null, Color(0xFFFFFF00), Color(0xFF00FF7F), Color(0xFF7FDBFF), Color(0xFFFFB6C1), Color(0xFFFFD27F),
];

String hexOf(Color c) {
  final v = c.toARGB32() & 0xFFFFFF;
  return '#${v.toRadixString(16).padLeft(6, '0')}';
}

class WriterPage extends StatefulWidget {
  const WriterPage({super.key, required this.doc, this.onNewDocument});

  final OpenDoc doc;

  /// Called with the handle of a document created here (envelope, index
  /// cards, labels) so the shell can open it in a new tab.
  final void Function(int handle)? onNewDocument;

  @override
  State<WriterPage> createState() => WriterPageState();
}

class WriterPageState extends State<WriterPage> {
  List<dynamic> _blocks = [];
  List<dynamic> _header = [];
  List<dynamic> _footer = [];
  Map<String, dynamic> _page = {};
  Map<String, dynamic> _stats = {};
  final Map<String, ParaController> _controllers = {};
  final Map<String, FocusNode> _focus = {};
  ParaRef _current = const ParaRef(0);
  double _zoom = 1.0;
  bool _showFind = false;
  final _findText = TextEditingController();
  final _replaceText = TextEditingController();
  bool _matchCase = false;
  List<dynamic> _hits = [];
  int _hitIndex = -1;
  final _scroll = ScrollController();
  final _findTranslit = TransliterationFormatter();
  final _replaceTranslit = TransliterationFormatter();

  int get _doc => widget.doc.handle;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    for (final c in _controllers.values) {
      c.dispose();
    }
    for (final f in _focus.values) {
      f.dispose();
    }
    _findText.dispose();
    _replaceText.dispose();
    _scroll.dispose();
    super.dispose();
  }

  // ---- core access ------------------------------------------------------------

  dynamic _call(String op, [Map<String, Object?> args = const {}]) {
    try {
      return core({'op': op, 'doc': _doc, ...args});
    } on CoreError catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
      }
      return null;
    }
  }

  /// Reloads the whole document from the core (after structural changes).
  void _reload({ParaRef? focus, int? offset}) {
    final got = _call('writer.get') as Map<String, dynamic>?;
    if (got == null) return;
    setState(() {
      _blocks = got['blocks'] as List<dynamic>;
      _header = got['header'] as List<dynamic>? ?? [];
      _footer = got['footer'] as List<dynamic>? ?? [];
      _page = got['page'] as Map<String, dynamic>;
      _stats = got['stats'] as Map<String, dynamic>;
      final live = <String>{};
      void sync(ParaRef ref, Map<String, dynamic> para) {
        live.add(ref.key);
        final c = _controllers.putIfAbsent(ref.key, () => ParaController());
        c.setParagraph(para);
        _focus.putIfAbsent(ref.key, () => FocusNode(debugLabel: 'para ${ref.key}'));
      }

      for (var i = 0; i < _header.length; i++) {
        sync(ParaRef.header(i), _header[i] as Map<String, dynamic>);
      }
      for (var i = 0; i < _footer.length; i++) {
        sync(ParaRef.footer(i), _footer[i] as Map<String, dynamic>);
      }
      for (var b = 0; b < _blocks.length; b++) {
        final block = _blocks[b] as Map<String, dynamic>;
        if (block['type'] == 'paragraph') {
          sync(ParaRef(b), block);
        } else if (block['type'] == 'table') {
          final rows = block['rows'] as List<dynamic>;
          for (var r = 0; r < rows.length; r++) {
            final cells = rows[r] as List<dynamic>;
            for (var c = 0; c < cells.length; c++) {
              final paras = cells[c] as List<dynamic>;
              for (var p = 0; p < paras.length; p++) {
                sync(ParaRef(b, [r, c, p]), paras[p] as Map<String, dynamic>);
              }
            }
          }
        }
      }
      for (final key in _controllers.keys.toList()) {
        if (!live.contains(key)) {
          final c = _controllers.remove(key);
          final f = _focus.remove(key);
          // Dispose after this frame: widgets may still reference them.
          WidgetsBinding.instance.addPostFrameCallback((_) {
            c?.dispose();
            f?.dispose();
          });
        }
      }
    });
    widget.doc.refresh();
    if (focus != null) _focusPara(focus, offset ?? 0);
  }

  /// Refreshes one paragraph's runs after typing (cheap).
  void _refreshPara(ParaRef ref) {
    final para = _call('writer.para', {'para': ref.toJson()}) as Map<String, dynamic>?;
    if (para != null) {
      _controllers[ref.key]?.setRuns(para);
      _updateBlockJson(ref, para);
    }
    final stats = _call('writer.stats') as Map<String, dynamic>?;
    if (stats != null) setState(() => _stats = stats);
    widget.doc.refresh();
  }

  void _updateBlockJson(ParaRef ref, Map<String, dynamic> para) {
    if (!ref.isBody) {
      final area = ref.area == 'header' ? _header : _footer;
      if (ref.block < area.length) area[ref.block] = para;
      return;
    }
    if (ref.block >= _blocks.length) return;
    if (ref.cell == null) {
      _blocks[ref.block] = {...para, 'type': 'paragraph'};
    } else {
      final rows = (_blocks[ref.block] as Map<String, dynamic>)['rows'] as List<dynamic>;
      final cell = (rows[ref.cell![0]] as List<dynamic>)[ref.cell![1]] as List<dynamic>;
      cell[ref.cell![2]] = para;
    }
  }

  Map<String, dynamic>? _paraJson(ParaRef ref) {
    if (!ref.isBody) {
      final area = ref.area == 'header' ? _header : _footer;
      return ref.block < area.length ? area[ref.block] as Map<String, dynamic> : null;
    }
    if (ref.block >= _blocks.length) return null;
    final block = _blocks[ref.block] as Map<String, dynamic>;
    if (ref.cell == null) return block['type'] == 'paragraph' ? block : null;
    try {
      final rows = block['rows'] as List<dynamic>;
      return ((rows[ref.cell![0]] as List<dynamic>)[ref.cell![1]] as List<dynamic>)[ref.cell![2]]
          as Map<String, dynamic>;
    } catch (_) {
      return null;
    }
  }

  void _focusPara(ParaRef ref, int offset) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final c = _controllers[ref.key];
      final f = _focus[ref.key];
      if (c == null || f == null) return;
      final o = offset.clamp(0, c.text.length);
      c.selection = TextSelection.collapsed(offset: o);
      f.requestFocus();
      setState(() => _current = ref);
    });
  }

  // ---- editing callbacks (from ParaField) --------------------------------------------

  void onTextChanged(ParaRef ref, String text) {
    _call('writer.set_text', {'para': ref.toJson(), 'text': text});
    _refreshPara(ref);
  }

  void onSplit(ParaRef ref, int start, int end) {
    if (end > start) {
      _call('writer.replace', {'para': ref.toJson(), 'start': start, 'end': end, 'text': ''});
    }
    final next = _call('writer.split', {'para': ref.toJson(), 'at': start});
    if (next is Map<String, dynamic>) {
      _reload(focus: ParaRef.fromJson(next), offset: 0);
    }
  }

  void onBackspaceAtStart(ParaRef ref) {
    final para = _paraJson(ref);
    final kind = para?['kind'] as String? ?? 'normal';
    if (kind == 'bullet' || kind == 'numbered' || kind == 'quote' || kind == 'code') {
      // First backspace turns a list item / quote back into body text.
      _call('writer.kind', {'para': ref.toJson(), 'kind': {'kind': 'normal'}});
      _reload(focus: ref, offset: 0);
      return;
    }
    final merged = _call('writer.merge', {'para': ref.toJson()});
    if (merged is Map<String, dynamic>) {
      _reload(focus: ParaRef.fromJson(merged['para'] as Map<String, dynamic>), offset: merged['at'] as int);
    }
  }

  void onIndent(ParaRef ref, int delta) {
    final kind = _paraJson(ref)?['kind'] as String? ?? 'normal';
    if (kind == 'bullet' || kind == 'numbered') {
      final sel = _controllers[ref.key]?.selection.baseOffset ?? 0;
      _call('writer.indent', {'para': ref.toJson(), 'delta': delta});
      _reload(focus: ref, offset: sel);
    } else if (ref.cell != null) {
      _moveToCell(ref, delta);
    } else if (delta > 0) {
      final c = _controllers[ref.key]!;
      final sel = c.selection;
      final start = sel.start < 0 ? c.text.length : sel.start, end = sel.end < 0 ? c.text.length : sel.end;
      _call('writer.replace', {'para': ref.toJson(), 'start': start, 'end': end, 'text': '\t'});
      _reload(focus: ref, offset: start + 1);
    }
  }

  /// Toolbar indent buttons: list level for list items, left indent for
  /// other paragraphs (Tab in body text still types a tab).
  void _indentButton(int delta) {
    final ref = _current;
    final sel = _selection().$1;
    _call('writer.indent', {'para': ref.toJson(), 'delta': delta});
    _reload(focus: ref, offset: sel);
  }

  void _moveToCell(ParaRef ref, int delta) {
    final block = _blocks[ref.block] as Map<String, dynamic>;
    final rows = block['rows'] as List<dynamic>;
    final cols = (rows.first as List<dynamic>).length;
    var index = ref.cell![0] * cols + ref.cell![1] + delta;
    if (index < 0) return;
    if (index >= rows.length * cols) {
      // Tab in the last cell appends a row, like OpenOffice.
      _call('writer.table', {'block': ref.block, 'rows': true, 'at': rows.length, 'count': 1});
      _reload(focus: ParaRef(ref.block, [rows.length, 0, 0]));
      return;
    }
    _focusPara(ParaRef(ref.block, [index ~/ cols, index % cols, 0]), 0);
  }

  /// Moves the caret to the neighbouring paragraph (arrow keys at an edge).
  void onNavigate(ParaRef ref, bool forward) {
    final order = _paragraphOrder();
    final i = order.indexOf(ref);
    final j = forward ? i + 1 : i - 1;
    if (i < 0 || j < 0 || j >= order.length) return;
    final target = order[j];
    _focusPara(target, forward ? 0 : (_controllers[target.key]?.text.length ?? 0));
  }

  List<ParaRef> _paragraphOrder() {
    final out = <ParaRef>[for (var i = 0; i < _header.length; i++) ParaRef.header(i)];
    for (var b = 0; b < _blocks.length; b++) {
      final block = _blocks[b] as Map<String, dynamic>;
      if (block['type'] == 'paragraph') {
        out.add(ParaRef(b));
      } else if (block['type'] == 'table') {
        final rows = block['rows'] as List<dynamic>;
        for (var r = 0; r < rows.length; r++) {
          final cells = rows[r] as List<dynamic>;
          for (var c = 0; c < cells.length; c++) {
            for (var p = 0; p < (cells[c] as List<dynamic>).length; p++) {
              out.add(ParaRef(b, [r, c, p]));
            }
          }
        }
      }
    }
    out.addAll([for (var i = 0; i < _footer.length; i++) ParaRef.footer(i)]);
    return out;
  }

  void onFocused(ParaRef ref) {
    if (_current != ref) {
      _call('writer.break_undo');
      setState(() => _current = ref);
    }
  }

  void undo() => _history('undo');

  void redo() => _history('redo');

  void _history(String op) {
    final sel = _controllers[_current.key]?.selection.baseOffset ?? 0;
    if (core({'op': op, 'doc': _doc}) == true) {
      final order = _paragraphOrder();
      _reload(focus: order.contains(_current) ? _current : (order.isEmpty ? null : order.first), offset: sel);
    }
  }

  // ---- formatting ------------------------------------------------------------------

  (int, int) _selection() {
    final c = _controllers[_current.key];
    if (c == null) return (0, 0);
    final s = c.selection;
    if (!s.isValid) return (c.text.length, c.text.length);
    return (s.start, s.end);
  }

  Map<String, dynamic> _attrsAtSelection() {
    final c = _controllers[_current.key];
    if (c == null) return const {};
    final (start, end) = _selection();
    return c.attrsAt(start, end);
  }

  void toggleAttr(String name) {
    final (start, end) = _selection();
    final on = _attrsAtSelection()[name] == true;
    final patch = <String, Object?>{name: !on};
    if (name == 'superscript' && !on) patch['subscript'] = false;
    if (name == 'subscript' && !on) patch['superscript'] = false;
    _applyPatch(patch, start, end);
  }

  void _applyPatch(Map<String, Object?> patch, int start, int end) {
    final ref = _current;
    if (_call('writer.attrs', {'para': ref.toJson(), 'start': start, 'end': end, 'patch': patch}) == null &&
        _controllers[ref.key] == null) {
      return;
    }
    _reload();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final c = _controllers[ref.key];
      if (c != null) {
        c.selection = TextSelection(baseOffset: start, extentOffset: end);
        _focus[ref.key]?.requestFocus();
      }
    });
  }

  void _setKind(String kind, [int level = 0]) {
    final sel = _selection().$1;
    _call('writer.kind', {'para': _current.toJson(), 'kind': {'kind': kind, 'level': level}});
    _reload(focus: _current, offset: sel);
  }

  void _toggleList(String kind) {
    final current = _paraJson(_current)?['kind'];
    _setKind(current == kind ? 'normal' : kind);
  }

  void _setAlign(String align) {
    final sel = _selection().$1;
    _call('writer.align', {'para': _current.toJson(), 'align': align});
    _reload(focus: _current, offset: sel);
  }

  Future<void> _editLink() async {
    final (start, end) = _selection();
    if (start == end) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t('Select the text to link first.'))));
      return;
    }
    final current = _attrsAtSelection()['link'] as String? ?? 'https://';
    final url = await askText(context, 'Hyperlink', initial: current, label: t('http(s):// or mailto: address'));
    if (url == null) return;
    _applyPatch({'link': url.trim().isEmpty ? null : url.trim()}, start, end);
  }

  void _insertTable() async {
    final spec = await askText(context, 'Insert table', initial: '3 x 3', label: t('Rows x columns'));
    if (spec == null) return;
    final m = RegExp(r'(\d+)\s*[x×*, ]\s*(\d+)').firstMatch(spec);
    if (m == null) return;
    final rows = int.parse(m.group(1)!).clamp(1, 200), cols = int.parse(m.group(2)!).clamp(1, 20);
    final at = _current.block + 1;
    _call('writer.insert_table', {'at': at, 'rows': rows, 'cols': cols});
    _reload(focus: ParaRef(at, const [0, 0, 0]));
  }

  /// Converts the selection (or the whole paragraph) with teltools.
  void _convertText(String spec) {
    final c = _controllers[_current.key];
    if (c == null) return;
    var (start, end) = _selection();
    if (start == end) {
      start = 0;
      end = c.text.length;
    }
    if (start == end) return;
    final parts = spec.split(':');
    final String converted;
    try {
      converted = transliterate(c.text.substring(start, end), script: parts[1], mode: parts[0]);
    } on CoreError catch (e) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
      return;
    }
    final ref = _current;
    _call('writer.replace', {'para': ref.toJson(), 'start': start, 'end': end, 'text': converted});
    _reload();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final ctl = _controllers[ref.key];
      if (ctl == null) return;
      ctl.selection = TextSelection(baseOffset: start, extentOffset: (start + converted.length).clamp(0, ctl.text.length));
      _focus[ref.key]?.requestFocus();
    });
  }

  void _insertPageBreak() {
    final at = _current.block + 1;
    _call('writer.insert_page_break', {'at': at});
    _reload(focus: ParaRef(at + 1));
  }

  // ---- page: setup, header and footer, templates, printing ----------------------------

  Future<void> _pageSetup() async {
    final choice = await showPageSetupDialog(context, _page);
    if (choice == null) return;
    _call('writer.page', choice.toJson());
    _reload();
  }

  void _toggleArea(String area) {
    final on = (area == 'header' ? _header : _footer).isEmpty;
    _call('writer.$area', {'on': on});
    _reload(focus: on ? (area == 'header' ? const ParaRef.header(0) : const ParaRef.footer(0)) : null);
  }

  void _insertField(String field) {
    final ref = _current;
    final (start, end) = _selection();
    if (end > start) _call('writer.replace', {'para': ref.toJson(), 'start': start, 'end': end, 'text': ''});
    _call('writer.field', {'para': ref.toJson(), 'at': start, 'field': field});
    _reload(focus: ref, offset: start + 1);
  }

  Future<void> _letterhead() async {
    final args = await showLetterheadDialog(context);
    if (args == null) return;
    _call('writer.letterhead', args);
    _reload();
  }

  Future<void> _newFromWizard(Future<int?> Function(BuildContext) wizard) async {
    try {
      final handle = await wizard(context);
      if (handle == null) return;
      final open = widget.onNewDocument;
      if (open != null) {
        open(handle);
      } else {
        core({'op': 'close', 'doc': handle});
      }
    } on CoreError catch (e) {
      if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    }
  }

  void _print({bool pdfOnly = false}) {
    final got = _call('writer.get') as Map<String, dynamic>?;
    if (got == null) return;
    List<Map<String, dynamic>> paras(Object? v) => (v as List<dynamic>? ?? []).cast<Map<String, dynamic>>();
    showPrintPreview(
      context,
      PrintSource(
        blocks: got['blocks'] as List<dynamic>,
        header: paras(got['header']),
        footer: paras(got['footer']),
        page: got['page'] as Map<String, dynamic>,
        title: widget.doc.title,
      ),
      // Phones and tablets have no print spooler to talk to: export only.
      pdfOnly: pdfOnly || Platform.isAndroid || Platform.isIOS,
    );
  }

  void _tableAction(int block, int row, int col, String action) {
    switch (action) {
      case 'row_above':
        _call('writer.table', {'block': block, 'rows': true, 'at': row, 'count': 1});
      case 'row_below':
        _call('writer.table', {'block': block, 'rows': true, 'at': row + 1, 'count': 1});
      case 'col_left':
        _call('writer.table', {'block': block, 'rows': false, 'at': col, 'count': 1});
      case 'col_right':
        _call('writer.table', {'block': block, 'rows': false, 'at': col + 1, 'count': 1});
      case 'del_row':
        _call('writer.table', {'block': block, 'rows': true, 'at': row, 'count': -1});
      case 'del_col':
        _call('writer.table', {'block': block, 'rows': false, 'at': col, 'count': -1});
      case 'del_table':
        _call('writer.remove_block', {'at': block});
    }
    _reload();
  }

  // ---- find / replace -----------------------------------------------------------------

  void _find({bool forward = true}) {
    final needle = _findText.text;
    if (needle.isEmpty) return;
    final hits = _call('writer.find', {'needle': needle, 'match_case': _matchCase}) as List<dynamic>? ?? [];
    setState(() {
      _hits = hits;
      if (hits.isEmpty) {
        _hitIndex = -1;
      } else {
        _hitIndex = forward ? (_hitIndex + 1) % hits.length : (_hitIndex - 1 + hits.length) % hits.length;
      }
    });
    if (_hitIndex >= 0) {
      final hit = hits[_hitIndex] as Map<String, dynamic>;
      final ref = ParaRef.fromJson(hit['para'] as Map<String, dynamic>);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        final c = _controllers[ref.key];
        if (c == null) return;
        c.selection = TextSelection(baseOffset: hit['start'] as int, extentOffset: hit['end'] as int);
        _focus[ref.key]?.requestFocus();
        final ctx = _focus[ref.key]?.context;
        if (ctx != null) Scrollable.ensureVisible(ctx, alignment: 0.3, duration: const Duration(milliseconds: 150));
      });
    }
  }

  void _replaceAll() {
    final count = _call('writer.replace_all', {
      'needle': _findText.text,
      'replacement': _replaceText.text,
      'match_case': _matchCase,
    });
    _reload();
    if (count is int && mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(t('Replaced {n} occurrence(s).', {'n': count}))));
    }
  }

  void openFind() => setState(() => _showFind = true);

  // ---- UI ----------------------------------------------------------------------------

  double get _scale => 1.3333 * _zoom; // points to logical pixels

  double _pt(String key, double fallback) => ((_page[key] as num?)?.toDouble() ?? fallback);

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final pageWidth = _pt('width', 595) * _scale;
    final pageHeight = _pt('height', 842) * _scale;
    final margin = _pt('margin', 56) * _scale;
    final narrow = MediaQuery.sizeOf(context).width < pageWidth + 32;
    final effMargin = narrow ? 16.0 : margin;
    final sheets = _buildSheets(theme);
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyF, control: true): openFind,
        const SingleActivator(LogicalKeyboardKey.keyF, meta: true): openFind,
        const SingleActivator(LogicalKeyboardKey.keyP, control: true): _print,
        const SingleActivator(LogicalKeyboardKey.keyP, meta: true): _print,
      },
      child: Column(
        children: [
          _toolbar(theme),
          if (_showFind) _findBar(theme),
          const Divider(height: 1),
          Expanded(
            child: ColoredBox(
              color: theme.brightness == Brightness.dark ? const Color(0xFF202124) : const Color(0xFFE8E6E1),
              child: Scrollbar(
                controller: _scroll,
                child: SingleChildScrollView(
                  controller: _scroll,
                  padding: EdgeInsets.symmetric(vertical: narrow ? 0 : 24, horizontal: narrow ? 0 : 16),
                  child: Center(
                    child: Column(
                      children: [
                        for (var i = 0; i < sheets.length; i++) ...[
                          if (i > 0) SizedBox(height: narrow ? 8 : 24),
                          _sheet(theme, i, sheets.length, sheets[i],
                              width: narrow ? double.infinity : pageWidth,
                              minHeight: narrow ? 0 : pageHeight,
                              margin: effMargin,
                              shadow: !narrow),
                        ],
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
          _statusBar(theme),
        ],
      ),
    );
  }

  /// One page: header, body blocks, footer (pushed to the bottom).
  Widget _sheet(ThemeData theme, int index, int count, List<Widget> body,
      {required double width, required double minHeight, required double margin, required bool shadow}) {
    final header = _area(theme, 'header', index, count);
    final footer = _area(theme, 'footer', index, count);
    final gap = areaGap * _scale;
    final estimate = _areaEstimate(_header) + _areaEstimate(_footer);
    final bodyMin = (minHeight - 2 * margin - estimate).clamp(0.0, double.infinity);
    return Container(
      width: width,
      decoration: BoxDecoration(
        color: theme.brightness == Brightness.dark ? const Color(0xFF2B2B2B) : Colors.white,
        boxShadow: shadow ? const [BoxShadow(blurRadius: 6, color: Color(0x33000000))] : null,
      ),
      padding: EdgeInsets.all(margin),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (header != null) ...[header, SizedBox(height: gap)],
          ConstrainedBox(
            constraints: BoxConstraints(minHeight: bodyMin),
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: body),
          ),
          if (footer != null) ...[SizedBox(height: gap), footer],
        ],
      ),
    );
  }

  /// Rough height of a header or footer in logical pixels, to size the body
  /// so the footer sits near the bottom of the page.
  double _areaEstimate(List<dynamic> paras) {
    if (paras.isEmpty) return 0;
    var h = areaGap;
    for (final p in paras.cast<Map<String, dynamic>>()) {
      var size = 12.0;
      for (final r in (p['runs'] as List<dynamic>).cast<Map<String, dynamic>>()) {
        final s = (r['size'] as num?)?.toDouble() ?? 12.0;
        if (s > size) size = s;
      }
      h += size * 1.4 + 5 + ((p['space_before'] as num?) ?? 0).toDouble();
    }
    return h * _scale;
  }

  /// The header or footer of a page: editable on the first page, a
  /// read-only copy with the page's field values on the others (tap to edit).
  Widget? _area(ThemeData theme, String area, int index, int count) {
    final paras = (area == 'header' ? _header : _footer).cast<Map<String, dynamic>>();
    if (paras.isEmpty) return null;
    final guide = BorderSide(color: theme.colorScheme.outlineVariant, width: 1);
    final Widget content;
    if (index == 0) {
      final labels = listLabels(paras);
      content = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var i = 0; i < paras.length; i++)
            _para(area == 'header' ? ParaRef.header(i) : ParaRef.footer(i), paras[i], labels[i], theme),
        ],
      );
    } else {
      final ink = PageInk(
        text: theme.colorScheme.onSurface,
        line: theme.colorScheme.outline,
        link: theme.colorScheme.primary,
        shade: theme.colorScheme.surfaceContainerHighest,
      );
      content = GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => _focusPara(area == 'header' ? const ParaRef.header(0) : const ParaRef.footer(0), 0),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: staticParagraphs(paras, _scale, ink, page: index + 1, pages: count),
        ),
      );
    }
    return Tooltip(
      message: t(area == 'header' ? 'Header' : 'Footer'),
      waitDuration: const Duration(seconds: 1),
      child: Container(
        decoration: BoxDecoration(border: area == 'header' ? Border(bottom: guide) : Border(top: guide)),
        child: content,
      ),
    );
  }

  /// Body blocks grouped into pages at forced page breaks.
  List<List<Widget>> _buildSheets(ThemeData theme) {
    final sheets = <List<Widget>>[[]];
    final counters = List<int>.filled(10, 0);
    for (var b = 0; b < _blocks.length; b++) {
      final block = _blocks[b] as Map<String, dynamic>;
      switch (block['type']) {
        case 'paragraph':
          String? label;
          final kind = block['kind'] as String;
          final level = (block['level'] as int?) ?? 0;
          if (kind == 'numbered') {
            counters[level]++;
            for (var l = level + 1; l < 10; l++) {
              counters[l] = 0;
            }
            label = '${counters[level]}.';
          } else if (kind == 'bullet') {
            label = const ['•', '◦', '▪'][level % 3];
          } else {
            counters.fillRange(0, 10, 0);
          }
          sheets.last.add(_para(ParaRef(b), block, label, theme));
        case 'table':
          counters.fillRange(0, 10, 0);
          sheets.last.add(_table(b, block, theme));
        case 'page_break':
          sheets.add([]);
      }
    }
    return sheets;
  }

  Widget _para(ParaRef ref, Map<String, dynamic> block, String? label, ThemeData theme) {
    final c = _controllers[ref.key];
    final f = _focus[ref.key];
    if (c == null || f == null) return const SizedBox.shrink();
    return ParaField(
      key: ValueKey('para-${ref.key}'),
      paraRef: ref,
      controller: c,
      focusNode: f,
      kind: block['kind'] as String,
      level: (block['level'] as int?) ?? 0,
      align: block['align'] as String? ?? 'left',
      label: label,
      scale: _scale,
      page: this,
      indent: ((block['indent'] as num?) ?? 0).toDouble(),
      spaceBefore: ((block['space_before'] as num?) ?? 0).toDouble(),
    );
  }

  Widget _table(int b, Map<String, dynamic> block, ThemeData theme) {
    final rows = block['rows'] as List<dynamic>;
    final borderless = block['borders'] == false;
    return Padding(
      padding: EdgeInsets.symmetric(vertical: borderless ? 0 : 6),
      child: tableLayout(
        table: block,
        scale: _scale,
        // Borderless tables (label sheets) show faint guides on screen only.
        borderColor: borderless ? theme.colorScheme.outlineVariant.withValues(alpha: 0.6) : theme.colorScheme.outline,
        cell: (r, c) {
          final paras = (rows[r] as List<dynamic>)[c] as List<dynamic>;
          return GestureDetector(
            onSecondaryTapDown: (d) => _cellMenu(d.globalPosition, b, r, c),
            onLongPressStart: (d) => _cellMenu(d.globalPosition, b, r, c),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                for (var p = 0; p < paras.length; p++)
                  _para(ParaRef(b, [r, c, p]), paras[p] as Map<String, dynamic>, null, theme),
              ],
            ),
          );
        },
      ),
    );
  }

  Future<void> _cellMenu(Offset pos, int block, int row, int col) async {
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(pos.dx, pos.dy, pos.dx, pos.dy),
      items: [
        PopupMenuItem(value: 'row_above', child: Text(t('Insert row above'))),
        PopupMenuItem(value: 'row_below', child: Text(t('Insert row below'))),
        PopupMenuItem(value: 'col_left', child: Text(t('Insert column left'))),
        PopupMenuItem(value: 'col_right', child: Text(t('Insert column right'))),
        PopupMenuDivider(),
        PopupMenuItem(value: 'del_row', child: Text(t('Delete row'))),
        PopupMenuItem(value: 'del_col', child: Text(t('Delete column'))),
        PopupMenuItem(value: 'del_table', child: Text(t('Delete table'))),
      ],
    );
    if (action != null) _tableAction(block, row, col, action);
  }

  Widget _toolbar(ThemeData theme) {
    final attrs = _attrsAtSelection();
    final para = _paraJson(_current);
    final kind = para?['kind'] as String? ?? 'normal';
    final level = para?['level'] as int? ?? 0;
    final styleValue = kind == 'heading' ? 'heading$level' : kind;
    Widget toggle(String name, IconData icon, String tip) => IconButton(
          tooltip: t(tip),
          isSelected: attrs[name] == true,
          icon: Icon(icon),
          onPressed: () => toggleAttr(name),
        );
    return Material(
      color: theme.colorScheme.surfaceContainer,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        child: Row(
          children: [
            IconButton(tooltip: t('Undo (Ctrl+Z)'), icon: Icon(Icons.undo), onPressed: widget.doc.canUndo ? undo : null),
            IconButton(tooltip: t('Redo (Ctrl+Y)'), icon: Icon(Icons.redo), onPressed: widget.doc.canRedo ? redo : null),
            _Sep(),
            DropdownButton<String>(
              value: ['normal', 'title', 'heading1', 'heading2', 'heading3', 'heading4', 'heading5', 'heading6', 'quote', 'code']
                      .contains(styleValue)
                  ? styleValue
                  : 'normal',
              underline: SizedBox.shrink(),
              items: [
                DropdownMenuItem(value: 'normal', child: Text(t('Body text'))),
                DropdownMenuItem(value: 'title', child: Text(t('Title'))),
                DropdownMenuItem(value: 'heading1', child: Text(t('Heading 1'))),
                DropdownMenuItem(value: 'heading2', child: Text(t('Heading 2'))),
                DropdownMenuItem(value: 'heading3', child: Text(t('Heading 3'))),
                DropdownMenuItem(value: 'heading4', child: Text(t('Heading 4'))),
                DropdownMenuItem(value: 'heading5', child: Text(t('Heading 5'))),
                DropdownMenuItem(value: 'heading6', child: Text(t('Heading 6'))),
                DropdownMenuItem(value: 'quote', child: Text(t('Quotation'))),
                DropdownMenuItem(value: 'code', child: Text(t('Preformatted'))),
              ],
              onChanged: (v) {
                if (v == null) return;
                if (v.startsWith('heading')) {
                  _setKind('heading', int.parse(v.substring(7)));
                } else {
                  _setKind(v);
                }
              },
            ),
            const _Sep(),
            toggle('bold', Icons.format_bold, 'Bold (Ctrl+B)'),
            toggle('italic', Icons.format_italic, 'Italic (Ctrl+I)'),
            toggle('underline', Icons.format_underlined, 'Underline (Ctrl+U)'),
            toggle('strike', Icons.format_strikethrough, 'Strike-through'),
            toggle('code', Icons.code, 'Monospace'),
            toggle('superscript', Icons.superscript, 'Superscript (Ctrl+Shift+P)'),
            toggle('subscript', Icons.subscript, 'Subscript (Ctrl+Shift+B)'),
            _colorMenu('Text colour', Icons.format_color_text, _textColors, 'color'),
            _colorMenu('Highlight', Icons.border_color, _highlights, 'highlight'),
            PopupMenuButton<double?>(
              tooltip: t('Font size'),
              icon: const Icon(Icons.format_size),
              itemBuilder: (_) => [
                PopupMenuItem(value: -1, child: Text(t('Default'))),
                for (final s in const [8.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 24.0, 32.0, 48.0])
                  PopupMenuItem(value: s, child: Text('${s.toInt()} pt')),
              ],
              onSelected: (s) {
                final (start, end) = _selection();
                _applyPatch({'size': s == -1 ? null : s}, start, end);
              },
            ),
            IconButton(tooltip: t('Hyperlink'), icon: const Icon(Icons.link), onPressed: _editLink),
            const _Sep(),
            IconButton(tooltip: t('Align left'), isSelected: para?['align'] == 'left', icon: const Icon(Icons.format_align_left), onPressed: () => _setAlign('left')),
            IconButton(tooltip: t('Centre'), isSelected: para?['align'] == 'center', icon: const Icon(Icons.format_align_center), onPressed: () => _setAlign('center')),
            IconButton(tooltip: t('Align right'), isSelected: para?['align'] == 'right', icon: const Icon(Icons.format_align_right), onPressed: () => _setAlign('right')),
            IconButton(tooltip: t('Justify'), isSelected: para?['align'] == 'justify', icon: const Icon(Icons.format_align_justify), onPressed: () => _setAlign('justify')),
            const _Sep(),
            IconButton(tooltip: t('Bulleted list'), isSelected: kind == 'bullet', icon: const Icon(Icons.format_list_bulleted), onPressed: () => _toggleList('bullet')),
            IconButton(tooltip: t('Numbered list'), isSelected: kind == 'numbered', icon: const Icon(Icons.format_list_numbered), onPressed: () => _toggleList('numbered')),
            IconButton(tooltip: t('Decrease indent (Shift+Tab)'), icon: const Icon(Icons.format_indent_decrease), onPressed: () => _indentButton(-1)),
            IconButton(tooltip: t('Increase indent (Tab)'), icon: const Icon(Icons.format_indent_increase), onPressed: () => _indentButton(1)),
            const _Sep(),
            IconButton(tooltip: t('Insert table'), icon: const Icon(Icons.table_chart_outlined), onPressed: _current.isBody ? _insertTable : null),
            IconButton(tooltip: t('Insert page break'), icon: const Icon(Icons.insert_page_break_outlined), onPressed: _current.isBody ? _insertPageBreak : null),
            _pageMenu(),
            IconButton(tooltip: t('Print (Ctrl+P)'), icon: const Icon(Icons.print_outlined), onPressed: _print),
            PopupMenuButton<String>(
              tooltip: t('Convert text'),
              icon: const Icon(Icons.sync_alt),
              itemBuilder: (_) => convertMenuItems(),
              onSelected: _convertText,
            ),
            IconButton(tooltip: t('Find & replace (Ctrl+F)'), icon: const Icon(Icons.find_replace), onPressed: () => setState(() => _showFind = !_showFind)),
          ],
        ),
      ),
    );
  }

  Widget _pageMenu() {
    PopupMenuItem<String> item(String value, IconData icon, String label, {bool? checked, bool enabled = true}) =>
        PopupMenuItem(
          value: value,
          enabled: enabled,
          child: Row(children: [
            Icon(checked == true ? Icons.check_box_outlined : (checked == false ? Icons.check_box_outline_blank : icon), size: 20),
            const SizedBox(width: 12),
            Expanded(child: Text(label)),
          ]),
        );
    return PopupMenuButton<String>(
      tooltip: t('Page layout'),
      icon: const Icon(Icons.article_outlined),
      itemBuilder: (_) => [
        item('setup', Icons.crop_portrait, t('Page setup…')),
        item('header', Icons.vertical_align_top, t('Header'), checked: _header.isNotEmpty),
        item('footer', Icons.vertical_align_bottom, t('Footer'), checked: _footer.isNotEmpty),
        item('page_number', Icons.tag, t('Insert page number')),
        item('page_count', Icons.numbers, t('Insert page count')),
        item('letterhead', Icons.badge_outlined, t('Letterhead…')),
        const PopupMenuDivider(),
        item('envelope', Icons.mail_outline, t('New envelope…')),
        item('labels', Icons.label_outline, t('New labels…')),
        item('cards', Icons.style_outlined, t('New index cards…')),
        const PopupMenuDivider(),
        item('pdf', Icons.picture_as_pdf_outlined, t('Export as PDF…')),
      ],
      onSelected: (v) => switch (v) {
        'setup' => _pageSetup(),
        'header' || 'footer' => _toggleArea(v),
        'page_number' || 'page_count' => _insertField(v),
        'letterhead' => _letterhead(),
        'envelope' => _newFromWizard(showEnvelopeDialog),
        'labels' => _newFromWizard(showLabelsDialog),
        'cards' => _newFromWizard(showIndexCardsDialog),
        'pdf' => _print(pdfOnly: true),
        _ => null,
      },
    );
  }

  Widget _colorMenu(String tip, IconData icon, List<Color?> colors, String attr) {
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
                decoration: BoxDecoration(
                  color: colors[i],
                  border: Border.all(color: Colors.grey),
                ),
                child: colors[i] == null ? const Icon(Icons.close, size: 16) : null,
              ),
              const SizedBox(width: 12),
              Text(colors[i] == null ? 'Automatic / none' : hexOf(colors[i]!)),
            ]),
          ),
      ],
      onSelected: (i) {
        final (start, end) = _selection();
        _applyPatch({attr: colors[i] == null ? null : hexOf(colors[i]!)}, start, end);
      },
    );
  }

  Widget _findBar(ThemeData theme) {
    return Material(
      color: theme.colorScheme.surfaceContainerHighest,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        child: Wrap(
          spacing: 8,
          runSpacing: 6,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            SizedBox(
              width: 220,
              child: TextField(
                controller: _findText,
                inputFormatters: [_findTranslit],
                autofocus: true,
                decoration: InputDecoration(isDense: true, hintText: t('Find'), prefixIcon: Icon(Icons.search)),
                onSubmitted: (_) => _find(),
              ),
            ),
            IconButton(tooltip: t('Previous'), icon: const Icon(Icons.keyboard_arrow_up), onPressed: () => _find(forward: false)),
            IconButton(tooltip: t('Next'), icon: const Icon(Icons.keyboard_arrow_down), onPressed: _find),
            Text(_hits.isEmpty ? '' : '${_hitIndex + 1} / ${_hits.length}'),
            SizedBox(
              width: 220,
              child: TextField(
                controller: _replaceText,
                inputFormatters: [_replaceTranslit],
                decoration: InputDecoration(isDense: true, hintText: t('Replace with')),
              ),
            ),
            OutlinedButton(onPressed: _replaceAll, child: Text(t('Replace all'))),
            FilterChip(label: Text(t('Match case')), selected: _matchCase, onSelected: (v) => setState(() => _matchCase = v)),
            IconButton(tooltip: t('Close'), icon: const Icon(Icons.close), onPressed: () => setState(() => _showFind = false)),
          ],
        ),
      ),
    );
  }

  Widget _statusBar(ThemeData theme) {
    return Material(
      color: theme.colorScheme.surfaceContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        child: Row(
          children: [
            Expanded(
              child: Text(
                t('{w} words · {c} characters · {p} paragraphs', {'w': _stats['words'] ?? 0, 'c': _stats['chars'] ?? 0, 'p': _stats['paragraphs'] ?? 0}),
                style: theme.textTheme.bodySmall,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            IconButton(iconSize: 18, tooltip: t('Zoom out'), icon: const Icon(Icons.zoom_out), onPressed: () => setState(() => _zoom = (_zoom - 0.1).clamp(0.5, 3.0))),
            Text('${(_zoom * 100).round()}%', style: theme.textTheme.bodySmall),
            IconButton(iconSize: 18, tooltip: t('Zoom in'), icon: const Icon(Icons.zoom_in), onPressed: () => setState(() => _zoom = (_zoom + 0.1).clamp(0.5, 3.0))),
          ],
        ),
      ),
    );
  }
}

class _Sep extends StatelessWidget {
  const _Sep();

  @override
  Widget build(BuildContext context) =>
      const SizedBox(height: 28, child: VerticalDivider(width: 12));
}
