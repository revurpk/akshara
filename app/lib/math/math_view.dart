// Renders a formula tree (JSON from the Rust parser) with Flutter widgets.
//
// Layout is box based: fractions stack, scripts shrink and shift, roots and
// stretchy brackets are painted to the height of their content. It is not a
// TeX-grade typesetter, but it is fast, needs no fonts beyond the system
// ones and handles every node type the parser produces.

import 'dart:math' as math;

import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';

class MathView extends StatelessWidget {
  const MathView({super.key, required this.tree, this.fontSize = 28, this.color});

  final Map<String, dynamic> tree;
  final double fontSize;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = color ?? Theme.of(context).colorScheme.onSurface;
    return DefaultTextStyle(
      style: TextStyle(
        fontSize: fontSize,
        color: c,
        fontFamily: docFont,
        fontFamilyFallback: docFallback,
      ),
      child: _Builder(fontSize, c, 0).build(tree),
    );
  }
}

class _Builder {
  _Builder(this.size, this.color, this.depth);

  final double size;
  final Color color;
  final int depth;

  _Builder smaller([double f = 0.7]) => _Builder(math.max(size * f, 8), color, depth + 1);
  _Builder withSize(double s) => _Builder(s.clamp(6.0, 400.0), color, depth + 1);
  _Builder withColor(Color c) => _Builder(size, c, depth + 1);

  TextStyle get _serif =>
      TextStyle(fontSize: size, color: color, height: 1.15, fontFamily: docFont, fontFamilyFallback: docFallback);

  Widget _text(String s, {bool italic = false, bool bold = false, double? scale, String? family}) => Text(
        s,
        softWrap: false,
        style: _serif.copyWith(
          fontSize: size * (scale ?? 1),
          fontStyle: italic ? FontStyle.italic : FontStyle.normal,
          fontWeight: bold ? FontWeight.bold : null,
          fontFamily: family ?? docFont,
        ),
      );

  Widget build(Map<String, dynamic>? n) {
    if (n == null) return const SizedBox.shrink();
    if (depth > 200) return _text('…');
    Map<String, dynamic>? child(String key) => n[key] as Map<String, dynamic>?;
    switch (n['t']) {
      case 'row':
        final items = (n['items'] as List<dynamic>).cast<Map<String, dynamic>>();
        if (items.isEmpty) return SizedBox(width: size * 0.2, height: size);
        return Row(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [for (final i in items) build(i)],
        );
      case 'lines':
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final r in (n['rows'] as List<dynamic>).cast<Map<String, dynamic>>())
              Padding(padding: EdgeInsets.symmetric(vertical: size * 0.15), child: build(r)),
          ],
        );
      case 'ident':
        return _text(n['text'] as String, italic: n['italic'] == true);
      case 'number':
        return _text(n['text'] as String);
      case 'text':
        return _text(n['text'] as String, family: uiFont);
      case 'func':
        return Padding(padding: EdgeInsets.only(right: size * 0.12), child: _text(n['name'] as String));
      case 'op':
        final kind = n['kind'] as String;
        final pad = switch (kind) {
          'relation' => size * 0.28,
          'binary' => size * 0.22,
          _ => size * 0.04,
        };
        return Padding(
          padding: EdgeInsets.symmetric(horizontal: kind == 'prefix' ? 0 : pad),
          child: _text(n['text'] as String),
        );
      case 'big_op':
        return _text(n['text'] as String, scale: 1.6);
      case 'frac':
        return Padding(
          padding: EdgeInsets.symmetric(horizontal: size * 0.08),
          child: IntrinsicWidth(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Center(child: smaller(0.85).build(child('num'))),
                Container(
                  height: math.max(1, size * 0.05),
                  margin: EdgeInsets.symmetric(vertical: size * 0.08),
                  color: color,
                ),
                Center(child: smaller(0.85).build(child('den'))),
              ],
            ),
          ),
        );
      case 'script':
        final small = smaller();
        Widget stack(Map<String, dynamic>? sup, Map<String, dynamic>? sub, CrossAxisAlignment align) {
          if (sup == null && sub == null) return const SizedBox.shrink();
          return Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: align,
            children: [
              if (sup != null) small.build(sup) else SizedBox(height: size * 0.5),
              SizedBox(height: size * 0.35),
              if (sub != null) small.build(sub) else SizedBox(height: size * 0.5),
            ],
          );
        }
        return Row(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.center,
          children: [
            stack(child('lsup'), child('lsub'), CrossAxisAlignment.end),
            build(child('base')),
            stack(child('sup'), child('sub'), CrossAxisAlignment.start),
          ],
        );
      case 'under_over':
        final small = smaller();
        return Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (child('over') != null) small.build(child('over')),
            build(child('base')),
            if (child('under') != null) small.build(child('under')),
          ],
        );
      case 'sqrt':
      case 'root':
        final body = build(child('body'));
        final index = n['t'] == 'root' ? smaller(0.55).build(child('index')) : null;
        return Row(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (index != null) Padding(padding: EdgeInsets.only(top: size * 0.05), child: index),
            IntrinsicHeight(
              child: Row(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  CustomPaint(size: Size(size * 0.55, size), painter: _RadicalPainter(color, size)),
                  Container(
                    padding: EdgeInsets.only(top: size * 0.12, left: size * 0.05, right: size * 0.05),
                    decoration: BoxDecoration(border: Border(top: BorderSide(color: color, width: math.max(1, size * 0.05)))),
                    child: body,
                  ),
                ],
              ),
            ),
          ],
        );
      case 'fenced':
        final open = n['open'] as String, close = n['close'] as String;
        return IntrinsicHeight(
          child: Row(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (open.isNotEmpty) CustomPaint(size: Size(_fenceWidth(open), size), painter: _FencePainter(open, color, size, true)),
              Padding(padding: EdgeInsets.symmetric(horizontal: size * 0.06, vertical: size * 0.04), child: Center(child: build(child('body')))),
              if (close.isNotEmpty) CustomPaint(size: Size(_fenceWidth(close), size), painter: _FencePainter(close, color, size, false)),
            ],
          ),
        );
      case 'matrix':
        final rows = (n['rows'] as List<dynamic>).map((r) => (r as List<dynamic>).cast<Map<String, dynamic>>()).toList();
        if (rows.isEmpty) return const SizedBox.shrink();
        return Padding(
          padding: EdgeInsets.symmetric(horizontal: size * 0.1),
          child: Table(
            defaultColumnWidth: const IntrinsicColumnWidth(),
            defaultVerticalAlignment: TableCellVerticalAlignment.middle,
            children: [
              for (final row in rows)
                TableRow(children: [
                  for (final cell in row)
                    Padding(
                      padding: EdgeInsets.symmetric(horizontal: size * 0.25, vertical: size * 0.08),
                      child: Center(child: build(cell)),
                    ),
                ]),
            ],
          ),
        );
      case 'accent':
        final under = n['under'] == true, stretchy = n['stretchy'] == true;
        final accent = n['accent'] as String;
        final mark = stretchy
            ? SizedBox(
                height: size * 0.25,
                child: CustomPaint(painter: _AccentPainter(accent, color, size)),
              )
            : Center(child: _text(accent, scale: 0.8));
        return IntrinsicWidth(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (!under) SizedBox(height: size * 0.35, child: OverflowBox(maxHeight: size, child: mark)),
              Center(child: build(child('body'))),
              if (under) SizedBox(height: size * 0.3, child: mark),
            ],
          ),
        );
      case 'styled':
        var b = this;
        final sizeSpec = n['size'] as Map<String, dynamic>?;
        if (sizeSpec != null) {
          final v = (sizeSpec['value'] as num).toDouble();
          b = switch (sizeSpec['mode']) {
            'abs' => b.withSize(v * 1.3333),
            'add' => b.withSize(size + v * 1.3333),
            _ => b.withSize(size * v),
          };
        }
        final c = n['color'] as String?;
        if (c != null) {
          final v = int.tryParse(c.substring(1), radix: 16);
          if (v != null) b = b.withColor(Color(0xFF000000 | v));
        }
        Widget w = b.build(child('body'));
        final family = switch (n['font']) {
          'sans' => uiFont,
          'fixed' => monoFont,
          _ => null,
        };
        w = DefaultTextStyle.merge(
          style: TextStyle(
            fontWeight: n['bold'] == true ? FontWeight.bold : null,
            fontStyle: n['italic'] == true ? FontStyle.italic : (n['italic'] == false ? FontStyle.normal : null),
            fontFamily: family,
          ),
          child: w,
        );
        // Text widgets set explicit styles, so bold/italic are applied by
        // rebuilding leaves through an inherited override.
        return _StyleOverride(bold: n['bold'] as bool?, italic: n['italic'] as bool?, family: family, child: w);
      case 'phantom':
        return Visibility(visible: false, maintainSize: true, maintainAnimation: true, maintainState: true, child: build(child('body')));
      case 'brace':
        final over = n['over'] == true;
        final brace = SizedBox(height: size * 0.35, child: CustomPaint(painter: _BracePainter(color, over)));
        return IntrinsicWidth(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (over) Center(child: smaller().build(child('label'))),
              if (over) brace,
              Center(child: build(child('body'))),
              if (!over) brace,
              if (!over) Center(child: smaller().build(child('label'))),
            ],
          ),
        );
      case 'space':
        return SizedBox(width: ((n['width'] as num?)?.toDouble() ?? 0.5) * size);
      case 'placeholder':
        return Container(
          width: size * 0.6,
          height: size * 0.8,
          margin: EdgeInsets.symmetric(horizontal: size * 0.05),
          decoration: BoxDecoration(border: Border.all(color: color.withValues(alpha: 0.6), width: 1.2)),
        );
      default:
        return _text('?');
    }
  }

  double _fenceWidth(String f) => switch (f) {
        '|' => size * 0.25,
        '‖' => size * 0.35,
        _ => size * 0.38,
      };
}

/// Applies bold/italic/family overrides to all [Text] below by rebuilding
/// them with a merged style.
class _StyleOverride extends StatelessWidget {
  const _StyleOverride({required this.child, this.bold, this.italic, this.family});

  final Widget child;
  final bool? bold;
  final bool? italic;
  final String? family;

  @override
  Widget build(BuildContext context) {
    if (bold == null && italic == null && family == null) return child;
    return _apply(child);
  }

  Widget _apply(Widget w) {
    if (w is Text) {
      return Text(
        w.data ?? '',
        softWrap: false,
        style: (w.style ?? const TextStyle()).copyWith(
          fontWeight: bold == null ? null : (bold! ? FontWeight.bold : FontWeight.normal),
          fontStyle: italic == null ? null : (italic! ? FontStyle.italic : FontStyle.normal),
          fontFamily: family,
        ),
      );
    }
    if (w is Row) {
      return Row(mainAxisSize: w.mainAxisSize, crossAxisAlignment: w.crossAxisAlignment, children: w.children.map(_apply).toList());
    }
    if (w is Padding && w.child != null) return Padding(padding: w.padding, child: _apply(w.child!));
    if (w is Center && w.child != null) return Center(child: _apply(w.child!));
    if (w is DefaultTextStyle) return DefaultTextStyle(style: w.style, child: _apply(w.child));
    return w;
  }
}

class _RadicalPainter extends CustomPainter {
  _RadicalPainter(this.color, this.fontSize);

  final Color color;
  final double fontSize;

  @override
  void paint(Canvas canvas, Size size) {
    final p = Paint()
      ..color = color
      ..strokeWidth = math.max(1, fontSize * 0.05)
      ..style = PaintingStyle.stroke
      ..strokeJoin = StrokeJoin.round;
    final h = size.height, w = size.width;
    final path = Path()
      ..moveTo(0, h * 0.62)
      ..lineTo(w * 0.25, h * 0.52)
      ..lineTo(w * 0.55, h * 0.98)
      ..lineTo(w, p.strokeWidth / 2);
    canvas.drawPath(path, p);
  }

  @override
  bool shouldRepaint(covariant _RadicalPainter old) => old.color != color || old.fontSize != fontSize;
}

class _FencePainter extends CustomPainter {
  _FencePainter(this.fence, this.color, this.fontSize, this.opening);

  final String fence;
  final Color color;
  final double fontSize;
  final bool opening;

  @override
  void paint(Canvas canvas, Size size) {
    final p = Paint()
      ..color = color
      ..strokeWidth = math.max(1.2, fontSize * 0.06)
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round;
    final h = size.height, w = size.width;
    final inset = p.strokeWidth;
    // Draw the opening shape; mirror it for closing brackets.
    canvas.save();
    final shape = _canonical(fence);
    // Closing forms ( ) ] } ... ) are drawn as the mirrored opening shape.
    final mirror = shape.$2;
    if (mirror) {
      canvas.translate(w, 0);
      canvas.scale(-1, 1);
    }
    final path = Path();
    switch (shape.$1) {
      case '(':
        path
          ..moveTo(w * 0.8, inset)
          ..quadraticBezierTo(w * 0.05, h / 2, w * 0.8, h - inset);
      case '[':
        path
          ..moveTo(w * 0.8, inset)
          ..lineTo(w * 0.3, inset)
          ..lineTo(w * 0.3, h - inset)
          ..lineTo(w * 0.8, h - inset);
      case '{':
        path
          ..moveTo(w * 0.85, inset)
          ..quadraticBezierTo(w * 0.45, inset, w * 0.45, h * 0.2)
          ..lineTo(w * 0.45, h * 0.4)
          ..quadraticBezierTo(w * 0.45, h * 0.5, w * 0.1, h * 0.5)
          ..quadraticBezierTo(w * 0.45, h * 0.5, w * 0.45, h * 0.6)
          ..lineTo(w * 0.45, h * 0.8)
          ..quadraticBezierTo(w * 0.45, h - inset, w * 0.85, h - inset);
      case '<':
        path
          ..moveTo(w * 0.8, inset)
          ..lineTo(w * 0.2, h / 2)
          ..lineTo(w * 0.8, h - inset);
      case 'ceil':
        path
          ..moveTo(w * 0.8, inset)
          ..lineTo(w * 0.3, inset)
          ..lineTo(w * 0.3, h - inset);
      case 'floor':
        path
          ..moveTo(w * 0.3, inset)
          ..lineTo(w * 0.3, h - inset)
          ..lineTo(w * 0.8, h - inset);
      case '||':
        path
          ..moveTo(w * 0.3, inset)
          ..lineTo(w * 0.3, h - inset)
          ..moveTo(w * 0.7, inset)
          ..lineTo(w * 0.7, h - inset);
      case '[[':
        path
          ..moveTo(w * 0.85, inset)
          ..lineTo(w * 0.2, inset)
          ..lineTo(w * 0.2, h - inset)
          ..lineTo(w * 0.85, h - inset)
          ..moveTo(w * 0.45, inset)
          ..lineTo(w * 0.45, h - inset);
      default: // '|'
        path
          ..moveTo(w / 2, inset)
          ..lineTo(w / 2, h - inset);
    }
    canvas.drawPath(path, p);
    canvas.restore();
  }

  /// Maps any bracket to (opening shape, is-closing-form).
  (String, bool) _canonical(String f) => switch (f) {
        '(' => ('(', false),
        ')' => ('(', true),
        '[' => ('[', false),
        ']' => ('[', true),
        '{' => ('{', false),
        '}' => ('{', true),
        '⟨' || '〈' => ('<', false),
        '⟩' || '〉' => ('<', true),
        '⌈' => ('ceil', false),
        '⌉' => ('ceil', true),
        '⌊' => ('floor', false),
        '⌋' => ('floor', true),
        '‖' => ('||', false),
        '⟦' => ('[[', false),
        '⟧' => ('[[', true),
        _ => ('|', false),
      };

  @override
  bool shouldRepaint(covariant _FencePainter old) => old.fence != fence || old.color != color;
}

class _AccentPainter extends CustomPainter {
  _AccentPainter(this.accent, this.color, this.fontSize);

  final String accent;
  final Color color;
  final double fontSize;

  @override
  void paint(Canvas canvas, Size size) {
    final p = Paint()
      ..color = color
      ..strokeWidth = math.max(1, fontSize * 0.05)
      ..style = PaintingStyle.stroke;
    final w = size.width, h = size.height;
    final path = Path();
    switch (accent) {
      case '^':
        path
          ..moveTo(0, h * 0.9)
          ..lineTo(w / 2, h * 0.15)
          ..lineTo(w, h * 0.9);
      case '~':
        path
          ..moveTo(0, h * 0.7)
          ..cubicTo(w * 0.25, 0, w * 0.5, h, w, h * 0.3);
      case '→':
        path
          ..moveTo(0, h / 2)
          ..lineTo(w, h / 2)
          ..moveTo(w - h * 0.5, h * 0.1)
          ..lineTo(w, h / 2)
          ..lineTo(w - h * 0.5, h * 0.9);
      case '—':
        path
          ..moveTo(0, h * 2)
          ..lineTo(w, h * 2);
      default: // overline / underline
        path
          ..moveTo(0, h / 2)
          ..lineTo(w, h / 2);
    }
    canvas.drawPath(path, p);
  }

  @override
  bool shouldRepaint(covariant _AccentPainter old) => old.accent != accent || old.color != color;
}

class _BracePainter extends CustomPainter {
  _BracePainter(this.color, this.over);

  final Color color;
  final bool over;

  @override
  void paint(Canvas canvas, Size size) {
    final p = Paint()
      ..color = color
      ..strokeWidth = 1.5
      ..style = PaintingStyle.stroke;
    final w = size.width, h = size.height;
    final top = over ? h * 0.85 : h * 0.15, tip = over ? h * 0.15 : h * 0.85, mid = h / 2;
    final path = Path()
      ..moveTo(0, top)
      ..quadraticBezierTo(0, mid, w * 0.1, mid)
      ..lineTo(w * 0.42, mid)
      ..quadraticBezierTo(w * 0.5, mid, w * 0.5, tip)
      ..quadraticBezierTo(w * 0.5, mid, w * 0.58, mid)
      ..lineTo(w * 0.9, mid)
      ..quadraticBezierTo(w, mid, w, top);
    canvas.drawPath(path, p);
  }

  @override
  bool shouldRepaint(covariant _BracePainter old) => old.color != color || old.over != over;
}
