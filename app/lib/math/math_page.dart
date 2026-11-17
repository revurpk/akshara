// Akshara Math: formula editor with live preview.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';

import '../core/document.dart';
import '../core/fonts.dart';
import '../core/native.dart';
import '../core/translit.dart';
import 'math_view.dart';

/// Insertable templates: (label shown, StarMath inserted, tooltip).
const _palette = <String, List<(String, String, String)>>{
  'Operators': [
    ('a+b', '<?> + <?> ', 'Addition'),
    ('a−b', '<?> - <?> ', 'Subtraction'),
    ('a·b', '<?> cdot <?> ', 'Multiplication (dot)'),
    ('a×b', '<?> times <?> ', 'Multiplication (cross)'),
    ('a/b', '<?> over <?> ', 'Fraction'),
    ('a÷b', '<?> div <?> ', 'Division'),
    ('±a', '+-<?> ', 'Plus/minus'),
    ('¬a', 'neg <?> ', 'Boolean NOT'),
    ('a∧b', '<?> and <?> ', 'Boolean AND'),
    ('a∨b', '<?> or <?> ', 'Boolean OR'),
  ],
  'Relations': [
    ('a=b', '<?> = <?> ', 'Equals'),
    ('a≠b', '<?> <> <?> ', 'Not equal'),
    ('a≈b', '<?> approx <?> ', 'Approximately'),
    ('a≤b', '<?> <= <?> ', 'Less or equal'),
    ('a≥b', '<?> >= <?> ', 'Greater or equal'),
    ('a≡b', '<?> equiv <?> ', 'Identical'),
    ('a∝b', '<?> prop <?> ', 'Proportional'),
    ('a→b', '<?> toward <?> ', 'Toward'),
    ('a⇒b', '<?> drarrow <?> ', 'Implies'),
    ('a⇔b', '<?> dlrarrow <?> ', 'If and only if'),
  ],
  'Sets': [
    ('a∈A', '<?> in <?> ', 'Element of'),
    ('a∉A', '<?> notin <?> ', 'Not element of'),
    ('A∪B', '<?> union <?> ', 'Union'),
    ('A∩B', '<?> intersection <?> ', 'Intersection'),
    ('A⊂B', '<?> subset <?> ', 'Subset'),
    ('A⊆B', '<?> subseteq <?> ', 'Subset or equal'),
    ('∅', 'emptyset ', 'Empty set'),
    ('ℕ', 'setN ', 'Natural numbers'),
    ('ℤ', 'setZ ', 'Integers'),
    ('ℝ', 'setR ', 'Real numbers'),
    ('ℂ', 'setC ', 'Complex numbers'),
  ],
  'Functions': [
    ('xⁿ', '<?>^{<?>} ', 'Power'),
    ('xₙ', '<?>_{<?>} ', 'Subscript'),
    ('√x', 'sqrt{<?>} ', 'Square root'),
    ('ⁿ√x', 'nroot{<?>}{<?>} ', 'N-th root'),
    ('|x|', 'abs{<?>} ', 'Absolute value'),
    ('x!', 'fact{<?>} ', 'Factorial'),
    ('eˣ', 'func e^{<?>} ', 'Exponential'),
    ('ln', 'ln(<?>) ', 'Natural logarithm'),
    ('log', 'log(<?>) ', 'Logarithm'),
    ('sin', 'sin(<?>) ', 'Sine'),
    ('cos', 'cos(<?>) ', 'Cosine'),
    ('tan', 'tan(<?>) ', 'Tangent'),
  ],
  'Calculus': [
    ('∑', 'sum from{<?>} to{<?>} <?> ', 'Sum'),
    ('∏', 'prod from{<?>} to{<?>} <?> ', 'Product'),
    ('∫', 'int from{<?>} to{<?>} <?> ', 'Integral'),
    ('∬', 'iint <?> ', 'Double integral'),
    ('∮', 'lint <?> ', 'Curve integral'),
    ('lim', 'lim from{<?>} <?> ', 'Limit'),
    ('∂', 'partial ', 'Partial'),
    ('∇', 'nabla ', 'Nabla'),
    ('∞', 'infinity ', 'Infinity'),
  ],
  'Brackets': [
    ('(a)', '(<?>) ', 'Round brackets'),
    ('[a]', '[<?>] ', 'Square brackets'),
    ('{a}', 'lbrace <?> rbrace ', 'Braces'),
    ('⟨a⟩', 'langle <?> rangle ', 'Angle brackets'),
    ('⌈a⌉', 'lceil <?> rceil ', 'Ceiling'),
    ('⌊a⌋', 'lfloor <?> rfloor ', 'Floor'),
    ('⎛a⎞', 'left( <?> right) ', 'Scalable brackets'),
    ('(ⁿₖ)', 'left( binom{<?>}{<?>} right) ', 'Binomial'),
  ],
  'Layout': [
    ('a/b', '{<?>} over {<?>} ', 'Fraction'),
    ('stack', 'stack{<?> # <?> # <?>} ', 'Vertical stack'),
    ('matrix', 'matrix{<?> # <?> ## <?> # <?>} ', '2×2 matrix'),
    ('â', 'hat{<?>} ', 'Hat'),
    ('a⃗', 'vec{<?>} ', 'Vector arrow'),
    ('ā', 'overline{<?>} ', 'Overline'),
    ('a̲', 'underline{<?>} ', 'Underline'),
    ('⏞', '{<?>} overbrace {<?>} ', 'Overbrace'),
    ('⏟', '{<?>} underbrace {<?>} ', 'Underbrace'),
    ('↵', 'newline ', 'New line'),
  ],
  'Format': [
    ('B', 'bold{<?>} ', 'Bold'),
    ('I', 'ital{<?>} ', 'Italic'),
    ('A+', 'size *1.5{<?>} ', 'Larger'),
    ('A−', 'size /1.5{<?>} ', 'Smaller'),
    ('red', 'color red{<?>} ', 'Red'),
    ('blue', 'color blue{<?>} ', 'Blue'),
    ('"t"', '"<?>" ', 'Text'),
  ],
};

class MathPage extends StatefulWidget {
  const MathPage({super.key, required this.doc});

  final OpenDoc doc;

  @override
  State<MathPage> createState() => MathPageState();
}

class MathPageState extends State<MathPage> {
  final _source = TextEditingController();
  final _focus = FocusNode(debugLabel: 'formula source');
  // StarMath keywords stay Latin; only "quoted text" is transliterated.
  final _translit = TransliterationFormatter(allowAt: insideQuotes);
  Map<String, dynamic> _tree = const {'t': 'row', 'items': []};
  List<dynamic> _errors = const [];
  double _previewSize = 32;
  List<Map<String, dynamic>> _greek = const [];

  @override
  void initState() {
    super.initState();
    try {
      final got = core({'op': 'math.get', 'doc': widget.doc.handle}) as Map<String, dynamic>;
      _source.text = got['source'] as String;
      _tree = got['tree'] as Map<String, dynamic>;
      _errors = got['errors'] as List<dynamic>;
      _greek = (core({'op': 'math.symbols'}) as List<dynamic>).cast<Map<String, dynamic>>();
    } on CoreError catch (e) {
      _errors = [
        {'pos': 0, 'message': e.message},
      ];
    }
  }

  @override
  void dispose() {
    _source.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _changed(String text) {
    try {
      final r = core({'op': 'math.set', 'doc': widget.doc.handle, 'source': text}) as Map<String, dynamic>;
      setState(() {
        _tree = r['tree'] as Map<String, dynamic>;
        _errors = r['errors'] as List<dynamic>;
      });
      widget.doc.markModified();
    } on CoreError catch (e) {
      setState(() => _errors = [
            {'pos': 0, 'message': e.message},
          ]);
    }
  }

  /// Inserts a template at the cursor and selects its first placeholder.
  void insert(String template) {
    final sel = _source.selection;
    final text = _source.text;
    final start = sel.isValid ? sel.start : text.length;
    final end = sel.isValid ? sel.end : text.length;
    var insertion = template;
    // If text is selected, it replaces the first placeholder.
    final selected = text.substring(start, end);
    if (selected.isNotEmpty && insertion.contains('<?>')) {
      insertion = insertion.replaceFirst('<?>', selected);
    }
    final newText = text.replaceRange(start, end, insertion);
    final ph = newText.indexOf('<?>', start);
    _source.value = TextEditingValue(
      text: newText,
      selection: ph >= 0 && ph < start + insertion.length
          ? TextSelection(baseOffset: ph, extentOffset: ph + 3)
          : TextSelection.collapsed(offset: start + insertion.length),
    );
    _changed(newText);
    _focus.requestFocus();
  }

  /// F4 / Tab jumps to the next placeholder, like OpenOffice Math.
  void _nextPlaceholder() {
    final text = _source.text;
    final from = _source.selection.isValid ? _source.selection.end : 0;
    var i = text.indexOf('<?>', from);
    if (i < 0) i = text.indexOf('<?>');
    if (i >= 0) _source.selection = TextSelection(baseOffset: i, extentOffset: i + 3);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final wide = MediaQuery.sizeOf(context).width > 900;
    final preview = Container(
      color: theme.brightness == Brightness.dark ? const Color(0xFF1E1E1E) : Colors.white,
      child: Column(
        children: [
          Expanded(
            child: InteractiveViewer(
              constrained: false,
              boundaryMargin: const EdgeInsets.all(400),
              minScale: 0.3,
              maxScale: 6,
              child: Padding(
                padding: const EdgeInsets.all(32),
                child: MathView(tree: _tree, fontSize: _previewSize),
              ),
            ),
          ),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              IconButton(tooltip: t('Smaller'), icon: const Icon(Icons.zoom_out), onPressed: () => setState(() => _previewSize = (_previewSize - 4).clamp(12, 120))),
              Text('${_previewSize.round()} px'),
              IconButton(tooltip: t('Larger'), icon: const Icon(Icons.zoom_in), onPressed: () => setState(() => _previewSize = (_previewSize + 4).clamp(12, 120))),
              const SizedBox(width: 8),
            ],
          ),
        ],
      ),
    );
    final editor = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          child: Padding(
            padding: const EdgeInsets.all(8),
            child: CallbackShortcuts(
              bindings: {
                const SingleActivator(LogicalKeyboardKey.f4): _nextPlaceholder,
              },
              child: TextField(
                controller: _source,
                inputFormatters: [_translit],
                autocorrect: false,
                enableSuggestions: false,
                focusNode: _focus,
                maxLines: null,
                expands: true,
                textAlignVertical: TextAlignVertical.top,
                style: const TextStyle(fontFamily: monoFont, fontFamilyFallback: indicFallback, fontSize: 15),
                decoration: InputDecoration(
                  border: OutlineInputBorder(),
                  hintText: t('Type a formula, e.g.  x = {-b +- sqrt{b^2 - 4ac}} over {2a}'),
                ),
                onChanged: _changed,
              ),
            ),
          ),
        ),
        if (_errors.isNotEmpty)
          Container(
            color: theme.colorScheme.errorContainer,
            constraints: const BoxConstraints(maxHeight: 96),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
            child: ListView(
              shrinkWrap: true,
              children: [
                for (final e in _errors.take(5))
                  InkWell(
                    onTap: () {
                      final pos = (e['pos'] as int).clamp(0, _source.text.length);
                      _source.selection = TextSelection.collapsed(offset: pos);
                      _focus.requestFocus();
                    },
                    child: Text(
                      t('Position {n}: {message}', {'n': e['pos'], 'message': e['message']}),
                      style: TextStyle(color: theme.colorScheme.onErrorContainer),
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
    return Column(
      children: [
        _paletteBar(theme),
        const Divider(height: 1),
        Expanded(
          child: wide
              ? Row(children: [
                  Expanded(flex: 2, child: editor),
                  const VerticalDivider(width: 1),
                  Expanded(flex: 3, child: preview),
                ])
              : Column(children: [
                  Expanded(child: preview),
                  const Divider(height: 1),
                  SizedBox(height: 220, child: editor),
                ]),
        ),
      ],
    );
  }

  Widget _paletteBar(ThemeData theme) {
    return Material(
      color: theme.colorScheme.surfaceContainer,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        child: Row(children: [
          for (final group in _palette.entries)
            Padding(
              padding: const EdgeInsets.only(right: 4),
              child: MenuAnchor(
                builder: (context, controller, _) => TextButton.icon(
                  icon: const Icon(Icons.arrow_drop_down),
                  label: Text(t(group.key)),
                  onPressed: () => controller.isOpen ? controller.close() : controller.open(),
                ),
                menuChildren: [
                  for (final item in group.value)
                    MenuItemButton(
                      leadingIcon: SizedBox(width: 48, child: Text(item.$1, style: const TextStyle(fontSize: 16))),
                      onPressed: () => insert(item.$2),
                      child: Text(t(item.$3)),
                    ),
                ],
              ),
            ),
          MenuAnchor(
            builder: (context, controller, _) => TextButton.icon(
              icon: const Icon(Icons.arrow_drop_down),
              label: Text(t('Greek')),
              onPressed: () => controller.isOpen ? controller.close() : controller.open(),
            ),
            menuChildren: [
              SizedBox(
                width: 360,
                child: Wrap(children: [
                  for (final g in _greek)
                    Tooltip(
                      message: '%${g['name']}',
                      child: InkWell(
                        onTap: () => insert('%${g['name']} '),
                        child: SizedBox(width: 36, height: 36, child: Center(child: Text(g['char'] as String, style: const TextStyle(fontSize: 18)))),
                      ),
                    ),
                ]),
              ),
            ],
          ),
          const SizedBox(width: 8),
          OutlinedButton.icon(
            icon: const Icon(Icons.skip_next),
            label: Text(t('Next placeholder (F4)')),
            onPressed: _nextPlaceholder,
          ),
        ]),
      ),
    );
  }
}
