// The Mermaid diagram editor: source on one side, a live preview drawn by
// the core on the other, problems listed with line numbers.

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import 'shape_paint.dart';

/// Starting points for each supported diagram type.
const mermaidExamples = <String, String>{
  'Flowchart': 'flowchart TD\n    A[Start] --> B{Is it working?}\n    B -->|Yes| C[Great]\n    B -->|No| D[Fix it]\n    D --> B',
  'Sequence diagram': 'sequenceDiagram\n    participant A as Alice\n    participant B as Bob\n    A->>B: Hello Bob!\n    B-->>A: Hi Alice\n    loop Every day\n        A->>B: How are you?\n    end',
  'Class diagram': 'classDiagram\n    Animal <|-- Dog\n    Animal <|-- Cat\n    class Animal {\n        +String name\n        +speak()\n    }\n    class Dog {\n        +fetch()\n    }',
  'State diagram': 'stateDiagram-v2\n    [*] --> Idle\n    Idle --> Running : start\n    Running --> Idle : stop\n    Running --> [*]',
  'Pie chart': 'pie title Languages\n    "Hindi" : 45\n    "Telugu" : 25\n    "Tamil" : 20\n    "Other" : 10',
};

/// Edits Mermaid source; returns the new source, or null if cancelled.
Future<String?> showMermaidDialog(BuildContext context, {String initial = ''}) {
  return showDialog<String>(
    context: context,
    builder: (_) => _MermaidDialog(initial: initial.isEmpty ? mermaidExamples['Flowchart']! : initial),
  );
}

class _MermaidDialog extends StatefulWidget {
  const _MermaidDialog({required this.initial});

  final String initial;

  @override
  State<_MermaidDialog> createState() => _MermaidDialogState();
}

class _MermaidDialogState extends State<_MermaidDialog> {
  late final TextEditingController _source = TextEditingController(text: widget.initial);
  Map<String, dynamic>? _figure;
  List<dynamic> _errors = const [];
  Timer? _debounce;

  @override
  void initState() {
    super.initState();
    _render();
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _source.dispose();
    super.dispose();
  }

  void _render() {
    try {
      final r = core({'op': 'mermaid.render', 'source': _source.text}) as Map<String, dynamic>;
      setState(() {
        _figure = r['figure'] as Map<String, dynamic>;
        _errors = r['errors'] as List<dynamic>;
      });
    } on CoreError catch (e) {
      setState(() => _errors = [
            {'line': 0, 'message': e.message},
          ]);
    }
  }

  void _changed(String _) {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 150), _render);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final size = MediaQuery.sizeOf(context);
    final wide = size.width > 820;
    final editor = TextField(
      controller: _source,
      maxLines: null,
      expands: true,
      autofocus: true,
      onChanged: _changed,
      style: const TextStyle(fontFamily: monoFont, fontFamilyFallback: indicFallback, fontSize: 13),
      decoration: InputDecoration(
        border: const OutlineInputBorder(),
        hintText: t('Mermaid source, e.g. graph TD; A-->B'),
        alignLabelWithHint: true,
      ),
      textAlignVertical: TextAlignVertical.top,
    );
    final preview = Container(
      decoration: BoxDecoration(color: Colors.white, border: Border.all(color: theme.colorScheme.outlineVariant)),
      padding: const EdgeInsets.all(8),
      child: _figure == null
          ? const SizedBox.shrink()
          : LayoutBuilder(
              builder: (context, box) => InteractiveViewer(
                maxScale: 6,
                child: Center(child: SizedBox(width: box.maxWidth, height: box.maxHeight, child: CustomPaint(painter: _Preview(_figure!)))),
              ),
            ),
    );
    return AlertDialog(
      title: Row(
        children: [
          Expanded(child: Text(t('Diagram (Mermaid)'))),
          PopupMenuButton<String>(
            tooltip: t('Examples'),
            icon: const Icon(Icons.auto_awesome_outlined),
            itemBuilder: (_) => [for (final name in mermaidExamples.keys) PopupMenuItem(value: name, child: Text(t(name)))],
            onSelected: (name) {
              _source.text = mermaidExamples[name]!;
              _render();
            },
          ),
        ],
      ),
      content: SizedBox(
        width: wide ? 900 : size.width,
        height: size.height * 0.7,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Expanded(
              child: wide
                  ? Row(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [Expanded(child: editor), const SizedBox(width: 12), Expanded(child: preview)],
                    )
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [Expanded(child: editor), const SizedBox(height: 8), Expanded(child: preview)],
                    ),
            ),
            const SizedBox(height: 8),
            SizedBox(
              height: 48,
              child: _errors.isEmpty
                  ? Text(
                      t('Flowcharts, sequence, class and state diagrams, and pie charts are supported.'),
                      style: theme.textTheme.bodySmall,
                    )
                  : ListView(
                      children: [
                        for (final e in _errors.cast<Map<String, dynamic>>())
                          Text(
                            e['line'] == 0 ? '${e['message']}' : t('Line {n}: {message}', {'n': e['line'], 'message': e['message']}),
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
                          ),
                      ],
                    ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(context, _source.text), child: Text(t('OK'))),
      ],
    );
  }
}

class _Preview extends CustomPainter {
  _Preview(this.figure);

  final Map<String, dynamic> figure;

  @override
  void paint(Canvas canvas, Size size) => paintFigureInto(canvas, figure, Offset.zero & size);

  @override
  bool shouldRepaint(_Preview old) => old.figure != figure;
}
