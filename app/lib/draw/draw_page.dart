// Akshara Impress (presentations) and Akshara Draw (drawings).
//
// Both edit pages of shapes held by the core: a pane of page thumbnails
// on the left (drag to reorder), the page being edited in the middle, and
// for presentations the speaker notes below it and a full-screen slide
// show.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/document.dart';
import '../core/file_dialog.dart';
import '../core/native.dart';
import '../core/print_preview.dart';
import '../l10n/l10n.dart';
import 'mermaid_dialog.dart';
import 'shape_editor.dart';
import 'shape_paint.dart';
import 'slide_show.dart';

/// Slide layouts offered by "New slide".
const slideLayouts = <String, (String, IconData)>{
  'title': ('Title slide', Icons.title),
  'content': ('Title and content', Icons.view_agenda_outlined),
  'two_content': ('Two columns', Icons.view_column_outlined),
  'title_only': ('Title only', Icons.short_text),
  'blank': ('Blank', Icons.crop_landscape),
};

class DrawPage extends StatefulWidget {
  const DrawPage({super.key, required this.doc});

  final OpenDoc doc;

  @override
  State<DrawPage> createState() => DrawPageState();
}

class DrawPageState extends State<DrawPage> {
  Map<String, dynamic> _data = const {'pages': [], 'width': 793.7, 'height': 446.46};
  int _page = 0;
  double _zoom = 1.0;
  late final ShapeEditorController _shapes = ShapeEditorController(
    doc: widget.doc.handle,
    onChanged: _reload,
    onError: _snack,
  )..editDiagram = (source) => showMermaidDialog(context, initial: source);
  final _notes = TextEditingController();
  final _notesFocus = FocusNode();

  bool get _impress => widget.doc.kind == DocKind.impress;
  int get _doc => widget.doc.handle;
  List<dynamic> get _pages => _data['pages'] as List<dynamic>;
  double get _w => (_data['width'] as num).toDouble();
  double get _h => (_data['height'] as num).toDouble();
  Map<String, dynamic>? get _current => _page < _pages.length ? _pages[_page] as Map<String, dynamic> : null;

  @override
  void initState() {
    super.initState();
    _reload();
    _notesFocus.addListener(() {
      if (!_notesFocus.hasFocus) _call('draw.break_undo');
    });
  }

  @override
  void dispose() {
    _shapes.dispose();
    _notes.dispose();
    _notesFocus.dispose();
    CoreImages.instance.forget(_doc);
    super.dispose();
  }

  void _snack(String message) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(message)));
  }

  dynamic _call(String op, [Map<String, Object?> args = const {}]) {
    try {
      return core({'op': op, 'doc': _doc, ...args});
    } on CoreError catch (e) {
      _snack(e.message);
      return null;
    }
  }

  void _reload() {
    final got = _call('draw.get') as Map<String, dynamic>?;
    if (got == null) return;
    setState(() {
      _data = got;
      if (_page >= _pages.length) _page = _pages.length - 1;
      if (_page < 0) _page = 0;
      _shapes.target = {'page': _page};
      final notes = _current?['notes'] as String? ?? '';
      if (!_notesFocus.hasFocus && _notes.text != notes) _notes.text = notes;
    });
    widget.doc.currentSheet = _page;
    widget.doc.refresh();
  }

  void _goTo(int page) {
    if (page < 0 || page >= _pages.length) return;
    _call('draw.break_undo');
    setState(() {
      _page = page;
      _shapes.target = {'page': page};
      _shapes.selection = {};
      _notes.text = _current?['notes'] as String? ?? '';
    });
    widget.doc.currentSheet = page;
  }

  void _history(String op) {
    if (core({'op': op, 'doc': _doc}) == true) _reload();
  }

  void _addPage(String layout) {
    _call('draw.add_page', {'at': _page + 1, 'layout': layout});
    _page++;
    _reload();
  }

  void _pageAction(String action, int page) {
    switch (action) {
      case 'duplicate':
        _call('draw.duplicate_page', {'page': page});
        _page = page + 1;
      case 'delete':
        if (_pages.length > 1) _call('draw.remove_page', {'page': page});
      case 'up':
        if (page > 0) {
          _call('draw.move_page', {'from': page, 'to': page - 1});
          _page = page - 1;
        }
      case 'down':
        if (page < _pages.length - 1) {
          _call('draw.move_page', {'from': page, 'to': page + 1});
          _page = page + 1;
        }
      case 'rename':
        _rename(page);
        return;
    }
    _reload();
  }

  Future<void> _rename(int page) async {
    final p = _pages[page] as Map<String, dynamic>;
    final name = await askText(context, t('Rename'), initial: p['name'] as String? ?? '', label: t('Name'));
    if (name == null) return;
    _call('draw.page_props', {'page': page, 'name': name});
    _reload();
  }

  void _background(Color? c) {
    _call('draw.page_props', {'page': _page, 'background': c == null ? null : colorHex(c)});
    _reload();
  }

  Future<void> _insertPicture() async {
    final path = await showPictureDialog(context);
    if (path == null) return;
    _shapes.insertPicture(path, x: _w * 0.1, y: _h * 0.1, maxW: _w * 0.8, maxH: _h * 0.8);
  }

  Future<void> _insertDiagram() async {
    final source = await showMermaidDialog(context);
    if (source == null) return;
    _shapes.insertDiagram(source, x: _w * 0.1, y: _h * 0.1, maxW: _w * 0.8, maxH: _h * 0.8);
  }

  Future<void> _pageSetup() async {
    final size = await showPageSizeDialog(context, width: _w, height: _h, presentation: _impress);
    if (size == null) return;
    _call('draw.set_size', {'width': size.$1, 'height': size.$2, 'scale': size.$3});
    _reload();
  }

  void present({bool fromStart = false}) {
    startSlideShow(context, doc: _doc, data: _data, start: fromStart ? 0 : _page);
  }

  Future<void> _print({bool pdfOnly = false}) async {
    for (final p in _pages.cast<Map<String, dynamic>>()) {
      await CoreImages.instance.preload(_doc, shapeImageIds(p['shapes'] as List<dynamic>));
    }
    if (!mounted) return;
    final job = PrintJob(
      width: _w,
      height: _h,
      margin: 0,
      title: widget.doc.title,
      units: [
        for (var i = 0; i < _pages.length; i++)
          PrintUnit(
            (page, pages, first) => SizedBox(
              width: _w,
              height: _h,
              child: CustomPaint(
                painter: PagePainter(
                  doc: _doc,
                  shapes: (_pages[i] as Map<String, dynamic>)['shapes'] as List<dynamic>,
                  width: _w,
                  height: _h,
                  background: hexColor((_pages[i] as Map<String, dynamic>)['background']) ?? Colors.white,
                ),
              ),
            ),
            newSheet: i > 0,
          ),
      ],
    );
    if (mounted) await showPrintPreview(context, job, pdfOnly: pdfOnly);
  }

  // ---- UI ------------------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final meta = Theme.of(context).platform == TargetPlatform.macOS;
    final page = _current;
    return CallbackShortcuts(
      bindings: {
        SingleActivator(LogicalKeyboardKey.keyZ, control: !meta, meta: meta): () => _history('undo'),
        SingleActivator(LogicalKeyboardKey.keyY, control: !meta, meta: meta): () => _history('redo'),
        SingleActivator(LogicalKeyboardKey.keyZ, control: !meta, meta: meta, shift: true): () => _history('redo'),
        SingleActivator(LogicalKeyboardKey.keyP, control: !meta, meta: meta): _print,
        if (_impress) const SingleActivator(LogicalKeyboardKey.f5): () => present(fromStart: true),
        if (_impress) const SingleActivator(LogicalKeyboardKey.f5, shift: true): present,
        const SingleActivator(LogicalKeyboardKey.pageDown): () => _goTo(_page + 1),
        const SingleActivator(LogicalKeyboardKey.pageUp): () => _goTo(_page - 1),
      },
      child: Column(
        children: [
          ShapeToolbar(
            controller: _shapes,
            onInsertPicture: _insertPicture,
            onInsertDiagram: _insertDiagram,
            leading: [
              IconButton(tooltip: t('Undo (Ctrl+Z)'), icon: const Icon(Icons.undo), onPressed: widget.doc.canUndo ? () => _history('undo') : null),
              IconButton(tooltip: t('Redo (Ctrl+Y)'), icon: const Icon(Icons.redo), onPressed: widget.doc.canRedo ? () => _history('redo') : null),
              const SizedBox(height: 28, child: VerticalDivider(width: 12)),
              _impress
                  ? PopupMenuButton<String>(
                      tooltip: t('New slide'),
                      icon: const Icon(Icons.add_box_outlined),
                      itemBuilder: (_) => [
                        for (final e in slideLayouts.entries)
                          PopupMenuItem(value: e.key, child: ListTile(leading: Icon(e.value.$2), title: Text(t(e.value.$1)))),
                      ],
                      onSelected: _addPage,
                    )
                  : IconButton(tooltip: t('New page'), icon: const Icon(Icons.add_box_outlined), onPressed: () => _addPage('blank')),
              if (_impress)
                IconButton(tooltip: t('Start slide show (F5)'), icon: const Icon(Icons.play_circle_outline), onPressed: () => present(fromStart: true)),
              IconButton(tooltip: _impress ? t('Slide size…') : t('Page setup…'), icon: const Icon(Icons.aspect_ratio), onPressed: _pageSetup),
              IconButton(tooltip: t('Print (Ctrl+P)'), icon: const Icon(Icons.print_outlined), onPressed: _print),
              const SizedBox(height: 28, child: VerticalDivider(width: 12)),
            ],
            trailing: [
              const SizedBox(height: 28, child: VerticalDivider(width: 12)),
              colorMenuButton(tooltip: t('Background'), icon: Icons.wallpaper, onSelected: _background, current: hexColor(page?['background'])),
              IconButton(tooltip: t('Export as PDF…'), icon: const Icon(Icons.picture_as_pdf_outlined), onPressed: () => _print(pdfOnly: true)),
            ],
          ),
          const Divider(height: 1),
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (MediaQuery.sizeOf(context).width > 600) SizedBox(width: 190, child: _pagePane(theme)),
                const VerticalDivider(width: 1),
                Expanded(
                  child: Column(
                    children: [
                      Expanded(child: _canvas(theme, page)),
                      if (_impress) _notesField(theme),
                    ],
                  ),
                ),
              ],
            ),
          ),
          _statusBar(theme),
        ],
      ),
    );
  }

  Widget _pagePane(ThemeData theme) {
    return ColoredBox(
      color: theme.colorScheme.surfaceContainerLow,
      child: ReorderableListView.builder(
        padding: const EdgeInsets.symmetric(vertical: 8),
        buildDefaultDragHandles: false,
        itemCount: _pages.length,
        onReorderItem: (from, to) {
          if (from == to) return;
          _call('draw.move_page', {'from': from, 'to': to});
          _page = to;
          _reload();
        },
        itemBuilder: (context, i) {
          final p = _pages[i] as Map<String, dynamic>;
          final selected = i == _page;
          return ReorderableDragStartListener(
            key: ValueKey('page-$i-${p['title']}'),
            index: i,
            child: GestureDetector(
              onTap: () => _goTo(i),
              onSecondaryTapUp: (d) => _pageMenu(d.globalPosition, i),
              onLongPressStart: (d) => _pageMenu(d.globalPosition, i),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(8, 4, 12, 4),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(width: 22, child: Text('${i + 1}', style: theme.textTheme.bodySmall, textAlign: TextAlign.right)),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Tooltip(
                        message: p['title'] as String? ?? '',
                        waitDuration: const Duration(seconds: 1),
                        child: Container(
                          decoration: BoxDecoration(
                            border: Border.all(color: selected ? theme.colorScheme.primary : theme.colorScheme.outlineVariant, width: selected ? 2.5 : 1),
                          ),
                          child: SlideView(
                            doc: _doc,
                            shapes: p['shapes'] as List<dynamic>,
                            width: _w,
                            height: _h,
                            background: hexColor(p['background']),
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          );
        },
      ),
    );
  }

  Future<void> _pageMenu(Offset pos, int page) async {
    final noun = _impress ? t('slide') : t('page');
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(pos.dx, pos.dy, pos.dx, pos.dy),
      items: [
        PopupMenuItem(value: 'duplicate', child: Text(t('Duplicate {what}', {'what': noun}))),
        PopupMenuItem(value: 'rename', child: Text(t('Rename…'))),
        PopupMenuItem(value: 'up', enabled: page > 0, child: Text(t('Move up'))),
        PopupMenuItem(value: 'down', enabled: page < _pages.length - 1, child: Text(t('Move down'))),
        const PopupMenuDivider(),
        PopupMenuItem(value: 'delete', enabled: _pages.length > 1, child: Text(t('Delete {what}', {'what': noun}))),
      ],
    );
    if (action != null) _pageAction(action, page);
  }

  Widget _canvas(ThemeData theme, Map<String, dynamic>? page) {
    if (page == null) return const SizedBox.shrink();
    return ColoredBox(
      color: theme.brightness == Brightness.dark ? const Color(0xFF202124) : const Color(0xFFE8E6E1),
      child: LayoutBuilder(
        builder: (context, box) {
          const pad = 24.0;
          final fit = ((box.maxWidth - 2 * pad) / _w).clamp(0.05, 20.0);
          final fitH = ((box.maxHeight - 2 * pad) / _h).clamp(0.05, 20.0);
          final scale = (fit < fitH ? fit : fitH) * _zoom;
          return Scrollbar(
            child: SingleChildScrollView(
              scrollDirection: Axis.vertical,
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: ConstrainedBox(
                  constraints: BoxConstraints(minWidth: box.maxWidth, minHeight: box.maxHeight),
                  child: Center(
                    child: Padding(
                      padding: const EdgeInsets.all(pad),
                      child: DecoratedBox(
                        decoration: const BoxDecoration(boxShadow: [BoxShadow(blurRadius: 6, color: Color(0x33000000))]),
                        child: ShapeEditor(
                          key: ValueKey('editor-$_page'),
                          controller: _shapes,
                          shapes: page['shapes'] as List<dynamic>,
                          width: _w,
                          height: _h,
                          scale: scale,
                          background: hexColor(page['background']) ?? Colors.white,
                          hints: _impress,
                          autofocus: true,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }

  Widget _notesField(ThemeData theme) {
    return Material(
      color: theme.colorScheme.surfaceContainerLowest,
      child: SizedBox(
        height: 96,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 6, 12, 6),
          child: TextField(
            controller: _notes,
            focusNode: _notesFocus,
            maxLines: null,
            expands: true,
            decoration: InputDecoration(hintText: t('Speaker notes'), border: InputBorder.none),
            onChanged: (v) {
              _call('draw.page_props', {'page': _page, 'notes': v});
              widget.doc.refresh();
            },
          ),
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
            IconButton(iconSize: 18, tooltip: t('Previous'), icon: const Icon(Icons.keyboard_arrow_up), onPressed: _page > 0 ? () => _goTo(_page - 1) : null),
            IconButton(iconSize: 18, tooltip: t('Next'), icon: const Icon(Icons.keyboard_arrow_down), onPressed: _page < _pages.length - 1 ? () => _goTo(_page + 1) : null),
            Expanded(
              child: Text(
                _impress
                    ? t('Slide {n} of {count}', {'n': _page + 1, 'count': _pages.length})
                    : t('Page {n} of {count}', {'n': _page + 1, 'count': _pages.length}),
                style: theme.textTheme.bodySmall,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            IconButton(iconSize: 18, tooltip: t('Zoom out'), icon: const Icon(Icons.zoom_out), onPressed: () => setState(() => _zoom = (_zoom - 0.1).clamp(0.3, 4.0))),
            TextButton(onPressed: () => setState(() => _zoom = 1), child: Text('${(_zoom * 100).round()}%', style: theme.textTheme.bodySmall)),
            IconButton(iconSize: 18, tooltip: t('Zoom in'), icon: const Icon(Icons.zoom_in), onPressed: () => setState(() => _zoom = (_zoom + 0.1).clamp(0.3, 4.0))),
          ],
        ),
      ),
    );
  }
}

/// Asks for a page size: `(width, height, scale shapes)` in points.
Future<(double, double, bool)?> showPageSizeDialog(BuildContext context, {required double width, required double height, required bool presentation}) {
  const cm = 72 / 2.54;
  final sizes = <String, (double, double)>{
    if (presentation) ...{
      'Widescreen 16:9': (28 * cm, 15.75 * cm),
      'Standard 4:3': (25.4 * cm, 19.05 * cm),
      'Widescreen 16:10': (28 * cm, 17.5 * cm),
    },
  };
  try {
    final papers = (core({'op': 'paper_sizes'}) as Map<String, dynamic>)['papers'] as List<dynamic>;
    for (final p in papers.cast<Map<String, dynamic>>().where((p) => p['kind'] == 'sheet')) {
      final w = (p['width'] as num).toDouble(), h = (p['height'] as num).toDouble();
      sizes[p['name'] as String] = (h, w); // landscape first
    }
  } catch (_) {}
  String? current;
  var landscape = width >= height;
  for (final e in sizes.entries) {
    final (w, h) = e.value;
    if (((w - width).abs() < 2 && (h - height).abs() < 2) || ((h - width).abs() < 2 && (w - height).abs() < 2)) current = e.key;
  }
  var choice = current ?? sizes.keys.first;
  var scale = true;
  return showDialog<(double, double, bool)>(
    context: context,
    builder: (context) => StatefulBuilder(
      builder: (context, setState) => AlertDialog(
        title: Text(presentation ? t('Slide size') : t('Page setup')),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            DropdownButton<String>(
              value: choice,
              isExpanded: true,
              items: [for (final k in sizes.keys) DropdownMenuItem(value: k, child: Text(t(k)))],
              onChanged: (v) => setState(() => choice = v ?? choice),
            ),
            SwitchListTile(contentPadding: EdgeInsets.zero, title: Text(t('Landscape')), value: landscape, onChanged: (v) => setState(() => landscape = v)),
            CheckboxListTile(contentPadding: EdgeInsets.zero, title: Text(t('Scale contents to fit')), value: scale, onChanged: (v) => setState(() => scale = v ?? true)),
          ],
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
          FilledButton(
            onPressed: () {
              var (w, h) = sizes[choice]!;
              if ((w > h) != landscape) (w, h) = (h, w);
              Navigator.pop(context, (w, h, scale));
            },
            child: Text(t('OK')),
          ),
        ],
      ),
    ),
  );
}
