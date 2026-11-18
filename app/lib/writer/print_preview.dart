// Print preview, printing and PDF export for Writer.
//
// Pages are laid out by Flutter with the same fonts and shaping as the
// editor: blocks are measured off stage, distributed over pages (tables
// split between rows), and each page is rasterised at print resolution.
// Printing uses the operating system's print dialog (system_print.dart):
// on Windows the pages are drawn straight to the printer; on Linux and
// macOS the core wraps the page images into a PDF for GTK's print job or
// the macOS print panel. Where no system dialog is available the PDF goes
// to `lp` with the printer chosen here, or opens in the PDF viewer. No
// plugins, no network.

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:material_ui/material_ui.dart';

import '../core/file_dialog.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import 'page_view.dart';
import 'system_print.dart';

/// Print resolution in dots per inch.
const printDpi = 200.0;

/// Gap between header / footer and the text, in points (as in the ODT).
const areaGap = 7.2;

/// What to print: the JSON of `writer.get`.
class PrintSource {
  PrintSource({required this.blocks, required this.header, required this.footer, required this.page, this.paper, this.title});

  final List<dynamic> blocks;
  final List<Map<String, dynamic>> header;
  final List<Map<String, dynamic>> footer;
  final Map<String, dynamic> page;

  /// Paper size id from the core (`a4`, `env_dl`...), or null if custom.
  final String? paper;
  final String? title;

  double get width => (page['width'] as num).toDouble();
  double get height => (page['height'] as num).toDouble();
  double get margin => (page['margin'] as num).toDouble();
}

/// A piece of body content that is never split across pages: a paragraph or
/// one table row.
class _Unit {
  _Unit(this.build, {this.newSheet = false});

  /// Builds the unit; `first` is true at the top of a page (tables then
  /// repeat their top border).
  final Widget Function(int page, int pages, bool first) build;

  /// A forced page break comes before this unit.
  final bool newSheet;
}

List<_Unit> _units(PrintSource src) {
  final out = <_Unit>[];
  final ink = PageInk.print;
  var breakNext = false;
  // List numbering continues across the document like in the editor.
  final paras = <Map<String, dynamic>>[];
  final labelsAt = <int, int>{};
  for (var b = 0; b < src.blocks.length; b++) {
    final block = src.blocks[b] as Map<String, dynamic>;
    if (block['type'] == 'paragraph') {
      labelsAt[b] = paras.length;
      paras.add(block);
    } else if (block['type'] == 'table') {
      paras.add(const {'kind': 'normal', 'runs': []});
    }
  }
  final labels = listLabels(paras);
  for (var b = 0; b < src.blocks.length; b++) {
    final block = src.blocks[b] as Map<String, dynamic>;
    switch (block['type']) {
      case 'page_break':
        breakNext = true;
      case 'paragraph':
        final label = labels[labelsAt[b]!];
        out.add(_Unit(
          (page, pages, _) => StaticPara(para: block, scale: 1, ink: ink, label: label, page: page, pages: pages),
          newSheet: breakNext,
        ));
        breakNext = false;
      case 'table':
        final rows = (block['rows'] as List<dynamic>).length;
        for (var r = 0; r < rows; r++) {
          out.add(_Unit(
            (page, pages, first) => staticTable(block, 1, ink,
                rows: [r], drawTopBorder: r == 0 || first, page: page, pages: pages),
            newSheet: r == 0 && breakNext,
          ));
        }
        breakNext = false;
    }
  }
  return out;
}

Widget _area(List<Map<String, dynamic>> paras, int page, int pages) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: staticParagraphs(paras, 1, PageInk.print, page: page, pages: pages),
    );

/// Shows the print preview. With [pdfOnly] the action is "Export as PDF".
Future<void> showPrintPreview(BuildContext context, PrintSource src, {bool pdfOnly = false}) {
  return showDialog<void>(
    context: context,
    builder: (_) => PrintPreview(source: src, pdfOnly: pdfOnly),
  );
}

class PrintPreview extends StatefulWidget {
  const PrintPreview({super.key, required this.source, this.pdfOnly = false});

  final PrintSource source;
  final bool pdfOnly;

  @override
  State<PrintPreview> createState() => _PrintPreviewState();
}

class _PrintPreviewState extends State<PrintPreview> {
  late final List<_Unit> _all = _units(widget.source);
  late final List<GlobalKey> _unitKeys = List.generate(_all.length, (_) => GlobalKey());
  final _headerKey = GlobalKey();
  final _footerKey = GlobalKey();

  /// Unit indices per page, once measured.
  List<List<int>>? _pages;
  double _bodyHeight = 0;
  List<GlobalKey> _pageKeys = [];

  List<String> _printers = [];
  String? _printer;
  int _copies = 1;
  bool _busy = false;

  /// The system print dialog: Windows always, Linux when GTK provides it,
  /// macOS when Dart runs on the main thread.
  late final bool _systemDialog = !widget.pdfOnly &&
      (Platform.isWindows || (Platform.isLinux && gtkPrintAvailable) || (Platform.isMacOS && macPrintAvailable));

  /// Fallback: the printer is chosen here and the PDF goes to `lp`.
  bool get _canSpool => !widget.pdfOnly && !_systemDialog && (Platform.isLinux || Platform.isMacOS) && _printers.isNotEmpty;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _paginate());
    if (!widget.pdfOnly && !_systemDialog && (Platform.isLinux || Platform.isMacOS)) _loadPrinters();
  }

  Future<void> _loadPrinters() async {
    try {
      final list = await Process.run('lpstat', ['-e']);
      final def = await Process.run('lpstat', ['-d']);
      final names = (list.stdout as String).split('\n').map((l) => l.trim()).where((l) => l.isNotEmpty).toList();
      final m = RegExp(r':\s*(\S+)\s*$').firstMatch((def.stdout as String).trim());
      if (!mounted) return;
      setState(() {
        _printers = names;
        _printer = m != null && names.contains(m.group(1)) ? m.group(1) : (names.isEmpty ? null : names.first);
      });
    } catch (_) {
      // No CUPS: the PDF opens in the viewer instead.
    }
  }

  double _heightOf(GlobalKey key) => (key.currentContext?.findRenderObject() as RenderBox?)?.size.height ?? 0;

  void _paginate() {
    final src = widget.source;
    final headerH = src.header.isEmpty ? 0.0 : _heightOf(_headerKey) + areaGap;
    final footerH = src.footer.isEmpty ? 0.0 : _heightOf(_footerKey) + areaGap;
    final body = (src.height - 2 * src.margin - headerH - footerH).clamp(36.0, double.infinity);
    final pages = <List<int>>[[]];
    var used = 0.0;
    for (var i = 0; i < _all.length; i++) {
      final h = _heightOf(_unitKeys[i]);
      final full = pages.last.isNotEmpty && used + h > body + 0.5;
      if (_all[i].newSheet || full) {
        pages.add([]);
        used = 0;
      }
      pages.last.add(i);
      used += h;
    }
    setState(() {
      _pages = pages;
      _bodyHeight = body;
      _pageKeys = List.generate(pages.length, (_) => GlobalKey());
    });
  }

  Widget _page(int index) {
    final src = widget.source;
    final pages = _pages!;
    final n = pages.length;
    return RepaintBoundary(
      key: _pageKeys[index],
      child: Container(
        width: src.width,
        height: src.height,
        color: const Color(0xFFFFFFFF),
        padding: EdgeInsets.all(src.margin),
        child: DefaultTextStyle(
          style: const TextStyle(color: Color(0xFF000000)),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (src.header.isNotEmpty) ...[_area(src.header, index + 1, n), const SizedBox(height: areaGap)],
              SizedBox(
                height: _bodyHeight,
                child: ClipRect(
                  child: OverflowBox(
                    alignment: Alignment.topLeft,
                    minHeight: 0,
                    maxHeight: double.infinity,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        for (var k = 0; k < pages[index].length; k++)
                          _all[pages[index][k]].build(index + 1, n, k == 0),
                      ],
                    ),
                  ),
                ),
              ),
              if (src.footer.isNotEmpty) ...[const SizedBox(height: areaGap), _area(src.footer, index + 1, n)],
            ],
          ),
        ),
      ),
    );
  }

  /// Rasterises one page as RGBA for the Windows printer.
  Future<PageRaster> _raster(int index, double pixelRatio) async {
    final boundary = _pageKeys[index].currentContext!.findRenderObject()! as RenderRepaintBoundary;
    final image = await boundary.toImage(pixelRatio: pixelRatio);
    try {
      final bytes = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
      return (rgba: bytes!.buffer.asUint8List(), width: image.width, height: image.height);
    } finally {
      image.dispose();
    }
  }

  /// Rasterises every page to PNG files and has the core build the PDF.
  Future<void> _writePdf(String target) async {
    final dir = Directory.systemTemp.createTempSync('akshara-print-');
    try {
      final pngs = <String>[];
      for (var i = 0; i < _pageKeys.length; i++) {
        final boundary = _pageKeys[i].currentContext!.findRenderObject()! as RenderRepaintBoundary;
        final image = await boundary.toImage(pixelRatio: printDpi / 72);
        final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
        image.dispose();
        final f = File('${dir.path}${Platform.pathSeparator}page-$i.png');
        f.writeAsBytesSync(bytes!.buffer.asUint8List());
        pngs.add(f.path);
      }
      await coreAsync({
        'op': 'pdf_from_pngs',
        'pngs': pngs,
        'width': widget.source.width,
        'height': widget.source.height,
        'path': target,
        if (widget.source.title != null) 'title': widget.source.title,
      });
    } finally {
      try {
        dir.deleteSync(recursive: true);
      } catch (_) {}
    }
  }

  Future<void> _run(Future<void> Function() job) async {
    setState(() => _busy = true);
    try {
      await job();
    } on CoreError catch (e) {
      if (mounted) await showError(context, t('Cannot print'), e);
    } on SystemPrintError catch (e) {
      if (mounted) await showError(context, t('Cannot print'), e.message);
    } on Object catch (e) {
      if (mounted) await showError(context, t('Cannot print'), e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _print() => _run(() async {
        final messenger = ScaffoldMessenger.maybeOf(context);
        final navigator = Navigator.of(context);
        final src = widget.source;
        final title = src.title ?? 'Akshara';
        if (Platform.isWindows) {
          final result = await printWindows(
            title: title,
            pageCount: _pageKeys.length,
            widthPt: src.width,
            heightPt: src.height,
            paper: src.paper,
            render: _raster,
          );
          if (result == SystemPrintResult.printed) {
            messenger?.showSnackBar(SnackBar(content: Text(t('Sent to the printer.'))));
            navigator.pop();
          }
          return;
        }
        final dir = Directory.systemTemp.createTempSync('akshara-pdf-');
        final pdf = '${dir.path}${Platform.pathSeparator}${_safeName(widget.source.title ?? 'document')}.pdf';
        await _writePdf(pdf);
        if (_systemDialog) {
          final result = Platform.isMacOS
              ? printMac(title: title, pdfPath: pdf, widthPt: src.width, heightPt: src.height)
              : await printGtk(title: title, pdfPath: pdf, widthPt: src.width, heightPt: src.height, paper: src.paper);
          if (result == SystemPrintResult.printed) {
            messenger?.showSnackBar(SnackBar(content: Text(t('Sent to the printer.'))));
            navigator.pop();
          }
          return;
        }
        if (_canSpool) {
          final args = [if (_printer != null) ...['-d', _printer!], '-n', '$_copies', '-t', widget.source.title ?? 'Akshara', pdf];
          final r = await Process.run('lp', args);
          if (r.exitCode != 0) throw CoreError('io', (r.stderr as String).trim());
          messenger?.showSnackBar(SnackBar(content: Text(t('Sent {n} page(s) to the printer.', {'n': _pageKeys.length}))));
        } else {
          await _openInViewer(pdf);
          messenger?.showSnackBar(SnackBar(content: Text(t('Opened in your PDF viewer; print from there.'))));
        }
        navigator.pop();
      });

  Future<void> _exportPdf() async {
    final target = await showSaveDialog(
      context,
      suggestedName: '${_safeName(widget.source.title ?? 'Untitled')}.pdf',
      formats: {'pdf': t('PDF document')},
    );
    if (target == null || !mounted) return;
    await _run(() async {
      await _writePdf(target);
      if (!mounted) return;
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text(t('Saved {name}', {'name': baseName(target)}))));
      Navigator.of(context).pop();
    });
  }

  @override
  Widget build(BuildContext context) {
    final src = widget.source;
    final theme = Theme.of(context);
    final bodyWidth = src.width - 2 * src.margin;
    return Dialog(
      insetPadding: const EdgeInsets.all(24),
      child: SizedBox(
        width: 760,
        height: 720,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 12, 8),
              child: Row(children: [
                Expanded(child: Text(widget.pdfOnly ? t('Export as PDF') : t('Print'), style: theme.textTheme.titleLarge)),
                if (_pages != null) Text(t('{n} page(s)', {'n': _pages!.length}), style: theme.textTheme.bodyMedium),
              ]),
            ),
            const Divider(height: 1),
            Expanded(
              child: Stack(
                children: [
                  // Measuring pass: lay out every unit at text width, unseen.
                  if (_pages == null)
                    Offstage(
                      child: OverflowBox(
                        alignment: Alignment.topLeft,
                        minWidth: bodyWidth,
                        maxWidth: bodyWidth,
                        minHeight: 0,
                        maxHeight: double.infinity,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            KeyedSubtree(key: _headerKey, child: _area(src.header, 1, 1)),
                            KeyedSubtree(key: _footerKey, child: _area(src.footer, 1, 1)),
                            for (var i = 0; i < _all.length; i++)
                              KeyedSubtree(key: _unitKeys[i], child: _all[i].build(1, 1, false)),
                          ],
                        ),
                      ),
                    ),
                  if (_pages == null)
                    const Center(child: CircularProgressIndicator())
                  else
                    ColoredBox(
                      color: theme.brightness == Brightness.dark ? const Color(0xFF202124) : const Color(0xFFE8E6E1),
                      child: SingleChildScrollView(
                        padding: const EdgeInsets.all(16),
                        child: Column(
                          children: [
                            for (var i = 0; i < _pages!.length; i++)
                              Padding(
                                padding: const EdgeInsets.only(bottom: 16),
                                child: SizedBox(
                                  // Small sheets (cards, envelopes) are shown at
                                  // their size, big ones fit the preview width.
                                  width: (src.width * 1.0).clamp(0.0, 560.0),
                                  child: FittedBox(
                                    child: DecoratedBox(
                                      decoration: const BoxDecoration(
                                        boxShadow: [BoxShadow(blurRadius: 6, color: Color(0x33000000))],
                                      ),
                                      child: _page(i),
                                    ),
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  if (_busy) const ColoredBox(color: Color(0x55000000), child: Center(child: CircularProgressIndicator())),
                ],
              ),
            ),
            const Divider(height: 1),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
              child: Wrap(
                spacing: 12,
                runSpacing: 8,
                alignment: WrapAlignment.end,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  if (_canSpool) ...[
                    Text(t('Printer')),
                    DropdownButton<String>(
                      value: _printer,
                      items: [for (final p in _printers) DropdownMenuItem(value: p, child: Text(p))],
                      onChanged: (v) => setState(() => _printer = v),
                    ),
                    Text(t('Copies')),
                    IconButton(
                      icon: const Icon(Icons.remove),
                      onPressed: _copies > 1 ? () => setState(() => _copies--) : null,
                    ),
                    Text('$_copies'),
                    IconButton(
                      icon: const Icon(Icons.add),
                      onPressed: _copies < 99 ? () => setState(() => _copies++) : null,
                    ),
                  ],
                  TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
                  if (!widget.pdfOnly)
                    OutlinedButton(
                      onPressed: _pages == null || _busy ? null : _exportPdf,
                      child: Text(t('Export as PDF')),
                    ),
                  FilledButton(
                    onPressed: _pages == null || _busy ? null : (widget.pdfOnly ? _exportPdf : _print),
                    child: Text(widget.pdfOnly ? t('Export as PDF') : (_systemDialog ? t('Print…') : t('Print'))),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

String _safeName(String name) {
  final cleaned = name.replaceAll(RegExp(r'[\\/:*?"<>|\x00-\x1f]'), '_').trim();
  return cleaned.isEmpty ? 'document' : (cleaned.length > 80 ? cleaned.substring(0, 80) : cleaned);
}

/// Opens a file with the system's default application.
Future<void> _openInViewer(String path) async {
  if (Platform.isWindows) {
    await Process.start('explorer.exe', [path], mode: ProcessStartMode.detached);
  } else if (Platform.isMacOS) {
    await Process.start('open', [path], mode: ProcessStartMode.detached);
  } else {
    await Process.start('xdg-open', [path], mode: ProcessStartMode.detached);
  }
}
