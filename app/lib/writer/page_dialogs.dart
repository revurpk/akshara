// Writer dialogs for the page: page setup, letterhead, and the wizards that
// create envelopes, index cards and label sheets as new documents.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/native.dart';
import '../core/paper.dart';
import '../l10n/l10n.dart';

const _mm = 72 / 25.4;

/// The page setup chosen in a dialog, in points.
class PageChoice {
  const PageChoice(this.width, this.height, this.margin);
  final double width;
  final double height;
  final double margin;

  Map<String, Object?> toJson() => {'width': width, 'height': height, 'margin': margin};
}

final _number = FilteringTextInputFormatter.allow(RegExp(r'[0-9.,]'));

double? _parseMm(String s) {
  final v = double.tryParse(s.trim().replaceAll(',', '.'));
  return v == null || !v.isFinite ? null : v * _mm;
}

String _fmtMm(double pt) {
  final mm = pt / _mm;
  return mm == mm.roundToDouble() ? mm.toStringAsFixed(0) : mm.toStringAsFixed(1);
}

/// Paper size, orientation and margin. Returns null when cancelled.
Future<PageChoice?> showPageSetupDialog(BuildContext context, Map<String, dynamic> page, {String? kind}) {
  return showDialog<PageChoice>(
    context: context,
    builder: (_) => _PageSetupDialog(page: page),
  );
}

class _PageSetupDialog extends StatefulWidget {
  const _PageSetupDialog({required this.page});
  final Map<String, dynamic> page;

  @override
  State<_PageSetupDialog> createState() => _PageSetupDialogState();
}

class _PageSetupDialogState extends State<_PageSetupDialog> {
  late final List<PaperInfo> _papers = paperCatalog().$1;
  late final _w = TextEditingController(text: _fmtMm((widget.page['width'] as num).toDouble()));
  late final _h = TextEditingController(text: _fmtMm((widget.page['height'] as num).toDouble()));
  late final _m = TextEditingController(text: _fmtMm((widget.page['margin'] as num).toDouble()));
  String? _error;

  @override
  void dispose() {
    _w.dispose();
    _h.dispose();
    _m.dispose();
    super.dispose();
  }

  String? get _matching {
    final w = _parseMm(_w.text), h = _parseMm(_h.text);
    if (w == null || h == null) return null;
    for (final p in _papers) {
      final same = (p.width - w).abs() < 1.5 && (p.height - h).abs() < 1.5;
      final turned = (p.width - h).abs() < 1.5 && (p.height - w).abs() < 1.5;
      if (same || turned) return p.id;
    }
    return null;
  }

  bool get _landscape => (_parseMm(_w.text) ?? 0) > (_parseMm(_h.text) ?? 0);

  void _pick(String? id) {
    final p = _papers.where((p) => p.id == id).firstOrNull;
    if (p == null) return;
    setState(() {
      _w.text = _fmtMm(p.width);
      _h.text = _fmtMm(p.height);
      _error = null;
    });
  }

  void _turn(bool landscape) {
    if (landscape == _landscape) return;
    setState(() {
      final w = _w.text;
      _w.text = _h.text;
      _h.text = w;
    });
  }

  void _ok() {
    final w = _parseMm(_w.text), h = _parseMm(_h.text), m = _parseMm(_m.text);
    if (w == null || h == null || m == null || w < 72 || h < 72 || w > 14400 || h > 14400 || m < 0 || m > (w < h ? w : h) / 3) {
      setState(() => _error = t('Page size or margins are out of range.'));
      return;
    }
    Navigator.pop(context, PageChoice(w, h, m));
  }

  @override
  Widget build(BuildContext context) {
    final matching = _matching;
    Widget field(TextEditingController c, String label) => Expanded(
          child: TextField(
            controller: c,
            inputFormatters: [_number],
            keyboardType: const TextInputType.numberWithOptions(decimal: true),
            decoration: InputDecoration(labelText: label, isDense: true),
            onChanged: (_) => setState(() => _error = null),
          ),
        );
    return AlertDialog(
      title: Text(t('Page setup')),
      content: SizedBox(
        width: 420,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            DropdownButtonFormField<String>(
              initialValue: matching ?? '',
              isExpanded: true,
              decoration: InputDecoration(labelText: t('Paper size')),
              items: [
                DropdownMenuItem(value: '', child: Text(t('Custom'))),
                for (final p in _papers) DropdownMenuItem(value: p.id, child: Text(p.label, overflow: TextOverflow.ellipsis)),
              ],
              onChanged: _pick,
            ),
            const SizedBox(height: 12),
            Row(children: [field(_w, t('Width (mm)')), const SizedBox(width: 12), field(_h, t('Height (mm)'))]),
            const SizedBox(height: 12),
            Row(children: [
              field(_m, t('Margin (mm)')),
              const SizedBox(width: 12),
              Expanded(
                child: SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  title: Text(t('Landscape')),
                  value: _landscape,
                  onChanged: _turn,
                ),
              ),
            ]),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
        FilledButton(onPressed: _ok, child: Text(t('OK'))),
      ],
    );
  }
}

/// A multi-line text field for addresses.
Widget _multiline(TextEditingController c, String label, {int lines = 4, String? hint}) => TextField(
      controller: c,
      minLines: lines,
      maxLines: lines + 4,
      keyboardType: TextInputType.multiline,
      decoration: InputDecoration(labelText: label, hintText: hint, border: const OutlineInputBorder(), alignLabelWithHint: true),
    );

Widget _paperDropdown(List<PaperInfo> papers, String value, String label, ValueChanged<String> onChanged) =>
    DropdownButtonFormField<String>(
      initialValue: value,
      isExpanded: true,
      decoration: InputDecoration(labelText: label),
      items: [for (final p in papers) DropdownMenuItem(value: p.id, child: Text(p.label, overflow: TextOverflow.ellipsis))],
      onChanged: (v) => v == null ? null : onChanged(v),
    );

/// A dialog that owns [count] text controllers for as long as it is shown
/// (including its closing animation).
class _FormDialog extends StatefulWidget {
  const _FormDialog({required this.count, required this.builder});

  final int count;
  final Widget Function(BuildContext context, List<TextEditingController> text, StateSetter setState) builder;

  @override
  State<_FormDialog> createState() => _FormDialogState();
}

class _FormDialogState extends State<_FormDialog> {
  late final List<TextEditingController> _text = List.generate(widget.count, (_) => TextEditingController());

  @override
  void dispose() {
    for (final c in _text) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.builder(context, _text, setState);
}

/// Asks for envelope size and addresses and creates the envelope document.
/// Returns the new document handle, or null.
Future<int?> showEnvelopeDialog(BuildContext context) async {
  final papers = paperCatalog().$1.where((p) => p.kind == 'envelope').toList();
  var size = papers.first.id;
  final args = await showDialog<Map<String, Object?>>(
    context: context,
    builder: (c) => _FormDialog(
      count: 2,
      builder: (c, text, setState) => AlertDialog(
        title: Text(t('New envelope')),
        content: SizedBox(
          width: 440,
          child: SingleChildScrollView(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              _paperDropdown(papers, size, t('Envelope size'), (v) => setState(() => size = v)),
              const SizedBox(height: 16),
              _multiline(text[0], t('Recipient address')),
              const SizedBox(height: 12),
              _multiline(text[1], t('Sender (return address)'), lines: 3),
            ]),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c), child: Text(t('Cancel'))),
          FilledButton(
            onPressed: () => Navigator.pop(c, {'recipient': text[0].text, 'sender': text[1].text}),
            child: Text(t('Create')),
          ),
        ],
      ),
    ),
  );
  if (args == null) return null;
  final p = papers.firstWhere((p) => p.id == size);
  final r = core({'op': 'new_envelope', 'width': p.width, 'height': p.height, 'margin': 0.4 * 72, ...args}) as Map<String, dynamic>;
  return r['doc'] as int;
}

/// Asks for card size and count and creates the index cards document.
Future<int?> showIndexCardsDialog(BuildContext context) async {
  final papers = paperCatalog().$1.where((p) => p.kind == 'card').toList();
  var size = papers.first.id;
  var count = 10.0;
  final ok = await showDialog<bool>(
    context: context,
    builder: (c) => StatefulBuilder(
      builder: (c, setState) => AlertDialog(
        title: Text(t('New index cards')),
        content: SizedBox(
          width: 400,
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            _paperDropdown(papers, size, t('Card size'), (v) => setState(() => size = v)),
            const SizedBox(height: 16),
            Text('${t('Number of cards')}: ${count.round()}'),
            Slider(value: count, min: 1, max: 100, divisions: 99, onChanged: (v) => setState(() => count = v)),
          ]),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c, false), child: Text(t('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(c, true), child: Text(t('Create'))),
        ],
      ),
    ),
  );
  if (ok != true) return null;
  final p = papers.firstWhere((p) => p.id == size);
  final r = core({'op': 'new_index_cards', 'width': p.width, 'height': p.height, 'margin': 0.25 * 72, 'count': count.round()})
      as Map<String, dynamic>;
  return r['doc'] as int;
}

/// Asks for a label sheet and the label text and creates the labels.
Future<int?> showLabelsDialog(BuildContext context) async {
  final sheets = paperCatalog().$2;
  var sheet = sheets.first.id;
  var same = true;
  final value = await showDialog<String>(
    context: context,
    builder: (c) => _FormDialog(
      count: 1,
      builder: (c, text, setState) => AlertDialog(
        title: Text(t('New labels')),
        content: SizedBox(
          width: 480,
          child: SingleChildScrollView(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              DropdownButtonFormField<String>(
                initialValue: sheet,
                isExpanded: true,
                decoration: InputDecoration(labelText: t('Label sheet')),
                items: [for (final s in sheets) DropdownMenuItem(value: s.id, child: Text(s.label, overflow: TextOverflow.ellipsis))],
                onChanged: (v) => setState(() => sheet = v ?? sheet),
              ),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Same text on every label')),
                value: same,
                onChanged: (v) => setState(() => same = v),
              ),
              _multiline(text[0], t('Label text'), lines: 6, hint: same ? null : t('Separate labels with a blank line')),
            ]),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c), child: Text(t('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(c, text[0].text), child: Text(t('Create'))),
        ],
      ),
    ),
  );
  if (value == null) return null;
  final r = core({'op': 'new_labels', 'sheet': sheet, 'text': value, 'same': same}) as Map<String, dynamic>;
  return r['doc'] as int;
}

/// The letterhead entered in the dialog (core `writer.letterhead` arguments).
Future<Map<String, Object?>?> showLetterheadDialog(BuildContext context) {
  var centred = true;
  var coloured = true;
  return showDialog<Map<String, Object?>>(
    context: context,
    builder: (c) => _FormDialog(
      count: 3,
      builder: (c, text, setState) => AlertDialog(
        title: Text(t('Letterhead')),
        content: SizedBox(
          width: 440,
          child: SingleChildScrollView(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              TextField(
                controller: text[0],
                autofocus: true,
                decoration: InputDecoration(labelText: t('Name or organisation'), border: const OutlineInputBorder()),
              ),
              const SizedBox(height: 12),
              _multiline(text[1], t('Address or tagline'), lines: 2),
              const SizedBox(height: 12),
              _multiline(text[2], t('Contact details (footer)'), lines: 2),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Centre')),
                value: centred,
                onChanged: (v) => setState(() => centred = v),
              ),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Coloured name')),
                value: coloured,
                onChanged: (v) => setState(() => coloured = v),
              ),
            ]),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c), child: Text(t('Cancel'))),
          FilledButton(
            onPressed: () => Navigator.pop(c, <String, Object?>{
              'name': text[0].text,
              'details': text[1].text,
              'contact': text[2].text,
              'align': centred ? 'center' : 'left',
              'color': coloured ? '#1f5fbf' : null,
            }),
            child: Text(t('OK')),
          ),
        ],
      ),
    ),
  );
}
