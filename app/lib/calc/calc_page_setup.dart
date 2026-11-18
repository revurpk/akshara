// Calc's page setup dialog: paper, orientation, margin, scale, grid lines,
// header and footer. The values belong to the workbook (`calc.page`).

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/paper.dart';
import '../l10n/l10n.dart';

const _mm = 72 / 25.4;

/// Shows the dialog for [page] (the JSON of `calc.page`); returns the
/// arguments for `calc.set_page`, or null when cancelled.
Future<Map<String, Object?>?> showCalcPageSetup(BuildContext context, Map<String, dynamic> page) {
  return showDialog<Map<String, Object?>>(context: context, builder: (_) => _CalcPageSetup(page: page));
}

class _CalcPageSetup extends StatefulWidget {
  const _CalcPageSetup({required this.page});
  final Map<String, dynamic> page;

  @override
  State<_CalcPageSetup> createState() => _CalcPageSetupState();
}

class _CalcPageSetupState extends State<_CalcPageSetup> {
  late final List<PaperInfo> _papers = paperCatalog().$1.where((p) => p.kind == 'sheet').toList();
  late double _width = (widget.page['width'] as num).toDouble();
  late double _height = (widget.page['height'] as num).toDouble();
  late final _margin = TextEditingController(text: ((widget.page['margin'] as num) / _mm).toStringAsFixed(0));
  late double _scale = (widget.page['scale'] as num).toDouble();
  late bool _fit = widget.page['fit_width'] == true;
  late bool _grid = widget.page['grid'] == true;
  late bool _headerFooter = widget.page['header_footer'] == true;
  String? _error;

  @override
  void dispose() {
    _margin.dispose();
    super.dispose();
  }

  bool get _landscape => _width > _height;

  /// The paper matching the current size in either orientation, or ''.
  String get _paper {
    for (final p in _papers) {
      final same = (p.width - _width).abs() < 1.5 && (p.height - _height).abs() < 1.5;
      final turned = (p.width - _height).abs() < 1.5 && (p.height - _width).abs() < 1.5;
      if (same || turned) return p.id;
    }
    return '';
  }

  void _pick(String? id) {
    final p = _papers.where((p) => p.id == id).firstOrNull;
    if (p == null) return;
    setState(() {
      // Keep the orientation.
      final short = p.width < p.height ? p.width : p.height, long = p.width < p.height ? p.height : p.width;
      _width = _landscape ? long : short;
      _height = _landscape ? short : long;
    });
  }

  void _ok() {
    final mm = double.tryParse(_margin.text.trim().replaceAll(',', '.'));
    final margin = mm == null ? null : mm * _mm;
    if (margin == null || margin < 0 || margin > (_width < _height ? _width : _height) / 3) {
      setState(() => _error = t('Page size or margins are out of range.'));
      return;
    }
    Navigator.pop(context, <String, Object?>{
      'width': _width,
      'height': _height,
      'margin': margin,
      'scale': _scale.round(),
      'fit_width': _fit,
      'grid': _grid,
      'header_footer': _headerFooter,
    });
  }

  @override
  Widget build(BuildContext context) {
    final paper = _paper;
    return AlertDialog(
      title: Text(t('Page setup')),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              DropdownButtonFormField<String>(
                initialValue: paper,
                isExpanded: true,
                decoration: InputDecoration(labelText: t('Paper size')),
                items: [
                  if (paper.isEmpty) DropdownMenuItem(value: '', child: Text(t('Custom'))),
                  for (final p in _papers) DropdownMenuItem(value: p.id, child: Text(p.label, overflow: TextOverflow.ellipsis)),
                ],
                onChanged: _pick,
              ),
              Row(children: [
                Expanded(
                  child: TextField(
                    controller: _margin,
                    inputFormatters: [FilteringTextInputFormatter.allow(RegExp(r'[0-9.,]'))],
                    keyboardType: const TextInputType.numberWithOptions(decimal: true),
                    decoration: InputDecoration(labelText: t('Margin (mm)'), isDense: true),
                    onChanged: (_) => setState(() => _error = null),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(t('Landscape')),
                    value: _landscape,
                    onChanged: (v) => setState(() {
                      if (v == _landscape) return;
                      final w = _width;
                      _width = _height;
                      _height = w;
                    }),
                  ),
                ),
              ]),
              const SizedBox(height: 8),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Fit columns to page width')),
                value: _fit,
                onChanged: (v) => setState(() => _fit = v),
              ),
              Row(children: [
                Text(t('Scale')),
                Expanded(
                  child: Slider(
                    value: _scale,
                    min: 10,
                    max: 400,
                    divisions: 78,
                    label: '${_scale.round()} %',
                    onChanged: _fit ? null : (v) => setState(() => _scale = v),
                  ),
                ),
                SizedBox(width: 52, child: Text('${_scale.round()} %', textAlign: TextAlign.right)),
              ]),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Print grid lines')),
                value: _grid,
                onChanged: (v) => setState(() => _grid = v),
              ),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Header and footer')),
                subtitle: Text(t('Sheet name at the top, page number at the bottom')),
                value: _headerFooter,
                onChanged: (v) => setState(() => _headerFooter = v),
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
        FilledButton(onPressed: _ok, child: Text(t('OK'))),
      ],
    );
  }
}

/// Asks for the print range and the rows to repeat of a sheet. [area] is
/// the JSON of `calc.print_area`, [selection] the selected range (`B2:D9`).
/// Returns the arguments for `calc.set_print_area`, or null when cancelled.
Future<Map<String, Object?>?> showPrintAreaDialog(BuildContext context, Map<String, dynamic> area, String selection) {
  return showDialog<Map<String, Object?>>(
    context: context,
    builder: (_) => _PrintAreaDialog(area: area, selection: selection),
  );
}

class _PrintAreaDialog extends StatefulWidget {
  const _PrintAreaDialog({required this.area, required this.selection});
  final Map<String, dynamic> area;
  final String selection;

  @override
  State<_PrintAreaDialog> createState() => _PrintAreaDialogState();
}

class _PrintAreaDialogState extends State<_PrintAreaDialog> {
  late final _range = TextEditingController(text: widget.area['range'] as String? ?? '');
  late final _rows = TextEditingController(text: widget.area['repeat_rows'] as String? ?? '');

  @override
  void dispose() {
    _range.dispose();
    _rows.dispose();
    super.dispose();
  }

  /// The rows of the selection, as `1` or `1:2`.
  String get _selectionRows {
    final rows = widget.selection.split(':').map((c) => int.tryParse(c.replaceAll(RegExp('[^0-9]'), '')) ?? 1).toList();
    final a = rows.first, b = rows.last;
    return a == b ? '$a' : '${a < b ? a : b}:${a < b ? b : a}';
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(t('Print range')),
      content: SizedBox(
        width: 400,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(children: [
              Expanded(
                child: TextField(
                  controller: _range,
                  decoration: InputDecoration(labelText: t('Print range'), hintText: t('Whole used area'), helperText: 'A1:F40'),
                ),
              ),
              TextButton(onPressed: () => setState(() => _range.text = widget.selection), child: Text(t('Use selection'))),
            ]),
            const SizedBox(height: 12),
            Row(children: [
              Expanded(
                child: TextField(
                  controller: _rows,
                  decoration: InputDecoration(labelText: t('Rows to repeat'), hintText: t('None'), helperText: '1 · 1:2'),
                ),
              ),
              TextButton(onPressed: () => setState(() => _rows.text = _selectionRows), child: Text(t('Use selection'))),
            ]),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => setState(() {
            _range.clear();
            _rows.clear();
          }),
          child: Text(t('Clear')),
        ),
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
        FilledButton(
          onPressed: () => Navigator.pop(context, <String, Object?>{'range': _range.text.trim(), 'repeat_rows': _rows.text.trim()}),
          child: Text(t('OK')),
        ),
      ],
    );
  }
}
