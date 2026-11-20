// Painting of shapes, Mermaid figures and pictures from the core's JSON.
//
// Shapes (Impress slides, Draw pages, Writer drawings) and figures (Mermaid
// diagrams laid out by the core) are drawn here with the bundled fonts, so
// screen, slide show and print look the same. Pictures are fetched from the
// core once and kept decoded in [CoreImages].

import 'dart:convert';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';

/// Parses `#rrggbb` (null stays null).
Color? hexColor(Object? v) {
  if (v is! String || v.length != 7 || !v.startsWith('#')) return null;
  final n = int.tryParse(v.substring(1), radix: 16);
  return n == null ? null : Color(0xFF000000 | n);
}

/// `#rrggbb` of a colour.
String colorHex(Color c) => '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0')}';

double _d(Object? v, [double fallback = 0]) => (v as num?)?.toDouble() ?? fallback;

/// Decoded pictures of open documents, fetched from the core on demand.
class CoreImages extends ChangeNotifier {
  CoreImages._();

  static final CoreImages instance = CoreImages._();

  final Map<String, ui.Image> _images = {};
  final Set<String> _loading = {};
  final Set<String> _failed = {};

  /// The picture, or null while it is loading (listeners are notified when
  /// it arrives).
  ui.Image? get(int doc, String id) {
    final key = '$doc:$id';
    final image = _images[key];
    if (image == null && !_loading.contains(key) && !_failed.contains(key)) {
      _load(doc, id);
    }
    return image;
  }

  /// Whether a picture could not be decoded.
  bool failed(int doc, String id) => _failed.contains('$doc:$id');

  Future<void> _load(int doc, String id) async {
    final key = '$doc:$id';
    _loading.add(key);
    try {
      final r = core({'op': 'image.data', 'doc': doc, 'id': id}) as Map<String, dynamic>;
      final bytes = base64Decode(r['data'] as String);
      final codec = await ui.instantiateImageCodec(bytes);
      final frame = await codec.getNextFrame();
      _images[key] = frame.image;
    } catch (_) {
      _failed.add(key);
    } finally {
      _loading.remove(key);
      notifyListeners();
    }
  }

  /// Loads pictures before painting off screen (printing).
  Future<void> preload(int doc, Iterable<String> ids) async {
    for (final id in ids) {
      final key = '$doc:$id';
      if (!_images.containsKey(key) && !_failed.contains(key)) await _load(doc, id);
    }
  }

  /// Forgets the pictures of a closed document.
  void forget(int doc) {
    _images.removeWhere((k, v) {
      final drop = k.startsWith('$doc:');
      if (drop) v.dispose();
      return drop;
    });
    _failed.removeWhere((k) => k.startsWith('$doc:'));
  }
}

/// Picture ids used by shapes (for [CoreImages.preload]).
Iterable<String> shapeImageIds(List<dynamic> shapes) =>
    shapes.cast<Map<String, dynamic>>().where((s) => s['kind'] == 'image').map((s) => s['image'] as String);

TextStyle _textStyle(double size, Color color, {bool bold = false, bool italic = false, bool underline = false}) =>
    TextStyle(
      fontFamily: uiFont,
      fontFamilyFallback: indicFallback,
      fontSize: size,
      color: color,
      height: 1.25,
      fontWeight: bold ? FontWeight.bold : FontWeight.normal,
      fontStyle: italic ? FontStyle.italic : FontStyle.normal,
      decoration: underline ? TextDecoration.underline : null,
    );

Paint? _fill(Map<String, dynamic> m) {
  final c = hexColor(m['fill']);
  return c == null ? null : (Paint()..color = c);
}

Paint? _stroke(Map<String, dynamic> m, {String widthKey = 'width'}) {
  final c = hexColor(m['stroke']);
  final w = _d(m[widthKey], 1);
  if (c == null || w <= 0) return null;
  return Paint()
    ..color = c
    ..style = PaintingStyle.stroke
    ..strokeWidth = w
    ..strokeJoin = StrokeJoin.round;
}

/// A dashed copy of a path.
Path dashed(Path source, double width) {
  final dash = math.max(2.0, width * 4), gap = math.max(2.0, width * 3);
  final out = Path();
  for (final metric in source.computeMetrics()) {
    var d = 0.0;
    while (d < metric.length) {
      out.addPath(metric.extractPath(d, math.min(d + dash, metric.length)), Offset.zero);
      d += dash + gap;
    }
  }
  return out;
}

void _drawPath(Canvas canvas, Path path, Paint? fill, Paint? stroke, bool dash) {
  if (fill != null) canvas.drawPath(path, fill);
  if (stroke != null) canvas.drawPath(dash ? dashed(path, stroke.strokeWidth) : path, stroke);
}

/// Arrowhead geometry, mirroring `office_draw::figure::arrow_points`.
(List<Offset>, bool, bool) _arrowPoints(String kind, Offset tip, Offset dir, double width) {
  final s = 3.0 + width * 3.0;
  final perp = Offset(-dir.dy, dir.dx);
  Offset at(double along, double side) => tip - dir * along + perp * side;
  switch (kind) {
    case 'triangle':
      return ([tip, at(s * 1.6, s * 0.7), at(s * 1.6, -s * 0.7)], true, true);
    case 'hollow':
      return ([tip, at(s * 2.0, s), at(s * 2.0, -s)], false, true);
    case 'open':
      return ([at(s * 1.6, s * 0.8), tip, at(s * 1.6, -s * 0.8)], false, false);
    case 'diamond':
      return ([tip, at(s * 1.3, s * 0.7), at(s * 2.6, 0), at(s * 1.3, -s * 0.7)], true, true);
    case 'hollow_diamond':
      return ([tip, at(s * 1.3, s * 0.7), at(s * 2.6, 0), at(s * 1.3, -s * 0.7)], false, true);
    case 'circle':
      final r = s * 0.6, c = at(r, 0);
      return ([for (var i = 0; i < 12; i++) c + Offset(math.cos(i * math.pi / 6), math.sin(i * math.pi / 6)) * r], true, true);
    default: // cross
      return ([at(s * 1.4, s * 0.7), at(0, -s * 0.7), at(s * 0.7, 0), at(0, s * 0.7), at(s * 1.4, -s * 0.7)], false, false);
  }
}

double _arrowInset(String kind, double width) {
  final s = 3.0 + width * 3.0;
  return switch (kind) {
    'triangle' => s * 1.4,
    'hollow' => s * 2.0,
    'diamond' || 'hollow_diamond' => s * 2.6,
    'circle' => s * 1.2,
    _ => 0,
  };
}

Offset _unit(Offset a, Offset b) {
  final d = b - a;
  final l = d.distance;
  return l < 1e-6 ? const Offset(1, 0) : d / l;
}

/// Paints a polyline with optional arrowheads (`start` / `end`).
void paintPolyline(Canvas canvas, List<Offset> pts, Paint stroke, {String? start, String? end, bool dash = false, bool smooth = false}) {
  if (pts.length < 2) return;
  final line = [...pts];
  final n = line.length;
  if (start != null) line[0] = line[0] - _unit(pts[1], pts[0]) * _arrowInset(start, stroke.strokeWidth);
  if (end != null) line[n - 1] = line[n - 1] - _unit(pts[n - 2], pts[n - 1]) * _arrowInset(end, stroke.strokeWidth);
  final path = Path()..moveTo(line[0].dx, line[0].dy);
  if (smooth && n > 2) {
    for (var i = 1; i < n - 1; i++) {
      final a = line[i - 1], b = line[i], c = line[i + 1];
      final k1 = math.min(8 / math.max((b - a).distance, 1e-3), 0.5);
      final k2 = math.min(8 / math.max((c - b).distance, 1e-3), 0.5);
      final s = b + (a - b) * k1, e = b + (c - b) * k2;
      path
        ..lineTo(s.dx, s.dy)
        ..quadraticBezierTo(b.dx, b.dy, e.dx, e.dy);
    }
    path.lineTo(line[n - 1].dx, line[n - 1].dy);
  } else {
    for (final p in line.skip(1)) {
      path.lineTo(p.dx, p.dy);
    }
  }
  canvas.drawPath(dash ? dashed(path, stroke.strokeWidth) : path, stroke);
  for (final (kind, tip, from) in [(start, pts[0], pts[1]), (end, pts[n - 1], pts[n - 2])]) {
    if (kind == null) continue;
    final (poly, filled, closed) = _arrowPoints(kind, tip, _unit(from, tip), stroke.strokeWidth);
    final head = Path()..addPolygon(poly, closed);
    final p = Paint()
      ..color = stroke.color
      ..strokeWidth = math.max(1, stroke.strokeWidth)
      ..strokeJoin = StrokeJoin.round;
    if (filled) {
      canvas.drawPath(head, p..style = PaintingStyle.fill);
    } else {
      if (closed) canvas.drawPath(head, Paint()..color = const Color(0xFFFFFFFF));
      canvas.drawPath(head, p..style = PaintingStyle.stroke);
    }
  }
}

/// Paints text whose block is vertically centred on `y` (figure text).
void _paintFigureText(Canvas canvas, Map<String, dynamic> it) {
  final size = _d(it['size'], 12);
  final tp = TextPainter(
    text: TextSpan(
      text: it['text'] as String? ?? '',
      style: _textStyle(size, hexColor(it['color']) ?? const Color(0xFF333333),
          bold: it['bold'] == true, italic: it['italic'] == true),
    ),
    textAlign: switch (it['anchor']) { 'start' => TextAlign.left, 'end' => TextAlign.right, _ => TextAlign.center },
    textDirection: TextDirection.ltr,
  )..layout();
  final x = _d(it['x']), y = _d(it['y']);
  final dx = switch (it['anchor']) { 'start' => x, 'end' => x - tp.width, _ => x - tp.width / 2 };
  tp.paint(canvas, Offset(dx, y - tp.height / 2));
  tp.dispose();
}

/// Paints a Mermaid figure in its own coordinates (points).
void paintFigure(Canvas canvas, Map<String, dynamic> figure, {int? doc}) {
  for (final item in (figure['items'] as List<dynamic>? ?? const []).cast<Map<String, dynamic>>()) {
    _paintItem(canvas, item, doc);
  }
}

void _paintItem(Canvas canvas, Map<String, dynamic> it, int? doc) {
  final dash = it['dash'] == true;
  switch (it['t']) {
    case 'rect':
      final r = Rect.fromLTWH(_d(it['x']), _d(it['y']), _d(it['w']), _d(it['h']));
      final rr = _d(it['r']);
      _drawPath(canvas, Path()..addRRect(RRect.fromRectAndRadius(r, Radius.circular(rr))), _fill(it), _stroke(it), dash);
    case 'ellipse':
      final c = Offset(_d(it['cx']), _d(it['cy']));
      final r = Rect.fromCenter(center: c, width: 2 * _d(it['rx']), height: 2 * _d(it['ry']));
      _drawPath(canvas, Path()..addOval(r), _fill(it), _stroke(it), dash);
    case 'poly':
      final pts = (it['points'] as List<dynamic>).map((p) => Offset(_d((p as List)[0]), _d(p[1]))).toList();
      if (pts.length < 2) return;
      if (it['closed'] == true) {
        _drawPath(canvas, Path()..addPolygon(pts, true), _fill(it), _stroke(it), dash);
      } else {
        final s = _stroke(it);
        if (s != null) {
          paintPolyline(canvas, pts, s, start: it['start'] as String?, end: it['end'] as String?, dash: dash, smooth: it['smooth'] == true);
        }
      }
    case 'wedge':
      final c = Offset(_d(it['cx']), _d(it['cy']));
      final r = _d(it['r']);
      final start = (_d(it['start']) - 90) * math.pi / 180, sweep = _d(it['sweep']) * math.pi / 180;
      final rect = Rect.fromCircle(center: c, radius: r);
      final path = sweep >= 2 * math.pi - 1e-4
          ? (Path()..addOval(rect))
          : (Path()
            ..moveTo(c.dx, c.dy)
            ..arcTo(rect, start, sweep, false)
            ..close());
      _drawPath(canvas, path, _fill(it), _stroke(it), dash);
    case 'text':
      _paintFigureText(canvas, it);
    case 'image':
      final rect = Rect.fromLTWH(_d(it['x']), _d(it['y']), _d(it['w']), _d(it['h']));
      final img = doc == null ? null : CoreImages.instance.get(doc, it['id'] as String);
      if (img != null) {
        paintImage(canvas: canvas, rect: rect, image: img, fit: BoxFit.fill, filterQuality: FilterQuality.medium);
      }
    case 'rotate':
      canvas.save();
      final cx = _d(it['cx']), cy = _d(it['cy']);
      canvas
        ..translate(cx, cy)
        ..rotate(_d(it['angle']) * math.pi / 180)
        ..translate(-cx, -cy);
      for (final child in (it['items'] as List<dynamic>).cast<Map<String, dynamic>>()) {
        _paintItem(canvas, child, doc);
      }
      canvas.restore();
  }
}

/// Paints a figure scaled and centred into `rect`, keeping its aspect.
void paintFigureInto(Canvas canvas, Map<String, dynamic> figure, Rect rect, {int? doc}) {
  final fw = math.max(_d(figure['width'], 1), 1.0), fh = math.max(_d(figure['height'], 1), 1.0);
  final k = math.min(rect.width / fw, rect.height / fh);
  canvas.save();
  canvas.translate(rect.left + (rect.width - fw * k) / 2, rect.top + (rect.height - fh * k) / 2);
  canvas.scale(k);
  paintFigure(canvas, figure, doc: doc);
  canvas.restore();
}

/// The outline of a closed shape, in page coordinates (unrotated).
Path? shapeOutline(Map<String, dynamic> s) {
  final x = _d(s['x']), y = _d(s['y']), w = _d(s['w']), h = _d(s['h']);
  final r = Rect.fromLTWH(x, y, w, h);
  switch (s['kind']) {
    case 'rect':
    case 'text_box':
    case 'image':
    case 'diagram':
      return Path()..addRect(r);
    case 'round_rect':
      return Path()..addRRect(RRect.fromRectAndRadius(r, Radius.circular(math.min(w, h) * 0.15)));
    case 'ellipse':
      return Path()..addOval(r);
    case 'triangle':
      return Path()..addPolygon([Offset(x + w / 2, y), Offset(x + w, y + h), Offset(x, y + h)], true);
    case 'diamond':
      return Path()..addPolygon([Offset(x + w / 2, y), Offset(x + w, y + h / 2), Offset(x + w / 2, y + h), Offset(x, y + h / 2)], true);
    case 'polygon':
      final pts = (s['points'] as List<dynamic>).map((p) => Offset(x + _d((p as List)[0]) * w, y + _d(p[1]) * h)).toList();
      return Path()..addPolygon(pts, s['closed'] != false);
  }
  return null;
}

/// Hint shown in empty presentation placeholders (screen only).
String placeholderHint(String? role) => switch (role) {
      'title' => t('Click to add title'),
      'subtitle' => t('Click to add subtitle'),
      'outline' => t('Click to add text'),
      _ => '',
    };

/// Paints one shape (page coordinates, points). With [hints], empty
/// placeholders show their hint and a dashed frame.
void paintShape(Canvas canvas, Map<String, dynamic> s, {required int doc, bool hints = false, bool hideText = false}) {
  final x = _d(s['x']), y = _d(s['y']), w = _d(s['w']), h = _d(s['h']);
  final rotation = _d(s['rotation']);
  final kind = s['kind'] as String;
  canvas.save();
  if (rotation != 0 && kind != 'line') {
    canvas
      ..translate(x + w / 2, y + h / 2)
      ..rotate(rotation * math.pi / 180)
      ..translate(-(x + w / 2), -(y + h / 2));
  }
  final dash = s['dash'] == true;
  final stroke = _stroke(s, widthKey: 'stroke_width');
  final fill = _fill(s);
  final rect = Rect.fromLTWH(x, y, w, h);
  switch (kind) {
    case 'line':
      if (stroke != null) {
        paintPolyline(canvas, [Offset(x, y), Offset(x + w, y + h)], stroke,
            start: s['arrow_start'] == true ? 'triangle' : null, end: s['arrow_end'] == true ? 'triangle' : null, dash: dash);
      }
    case 'image':
      final img = CoreImages.instance.get(doc, s['image'] as String);
      if (img != null) {
        paintImage(canvas: canvas, rect: rect, image: img, fit: BoxFit.fill, filterQuality: FilterQuality.medium);
      } else {
        canvas.drawRect(rect, Paint()..color = const Color(0x22808080));
      }
      if (stroke != null) _drawPath(canvas, Path()..addRect(rect), null, stroke, dash);
    case 'diagram':
      if (fill != null) canvas.drawRect(rect, fill);
      final figure = s['figure'] as Map<String, dynamic>?;
      if (figure != null) paintFigureInto(canvas, figure, rect, doc: doc);
      if (stroke != null) _drawPath(canvas, Path()..addRect(rect), null, stroke, dash);
    case 'polygon':
      final path = shapeOutline(s)!;
      _drawPath(canvas, path, s['closed'] == false ? null : fill, stroke, dash);
    default:
      final path = shapeOutline(s);
      if (path != null) _drawPath(canvas, path, fill, stroke, dash);
  }
  final text = s['text'] as String? ?? '';
  final style = s['style'] as Map<String, dynamic>? ?? const {};
  if (kind != 'line' && !hideText) {
    if (text.isNotEmpty) {
      paintShapeText(canvas, rect, text, style);
    } else if (hints && s['placeholder'] != null) {
      canvas.drawPath(dashed(Path()..addRect(rect), 1), Paint()
        ..color = const Color(0xFF9E9E9E)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1);
      paintShapeText(canvas, rect, placeholderHint(s['placeholder'] as String?),
          {...style, 'color': '#9e9e9e', 'bullets': false});
    }
  }
  canvas.restore();
}

/// Text padding inside shapes, in points (as in the core).
const shapePadding = 6.0;

/// The text style of a shape's text at 1 logical pixel per point.
TextStyle shapeTextStyle(Map<String, dynamic> style) => _textStyle(
      _d(style['size'], 18),
      hexColor(style['color']) ?? const Color(0xFF000000),
      bold: style['bold'] == true,
      italic: style['italic'] == true,
      underline: style['underline'] == true,
    );

TextAlign shapeTextAlign(Map<String, dynamic> style) =>
    switch (style['align']) { 'left' => TextAlign.left, 'right' => TextAlign.right, _ => TextAlign.center };

/// Paints a shape's text inside its box.
void paintShapeText(Canvas canvas, Rect rect, String text, Map<String, dynamic> style) {
  final bullets = style['bullets'] == true;
  final shown = bullets ? text.split('\n').map((l) => '•  $l').join('\n') : text;
  final tp = TextPainter(
    text: TextSpan(text: shown, style: shapeTextStyle(style)),
    textAlign: shapeTextAlign(style),
    textDirection: TextDirection.ltr,
  );
  // Lay out at the full box width so the alignment applies.
  final width = math.max(rect.width - 2 * shapePadding, 1.0);
  tp.layout(minWidth: width, maxWidth: width);
  final top = switch (style['valign']) {
    'top' => rect.top + shapePadding,
    'bottom' => rect.bottom - shapePadding - tp.height,
    _ => rect.top + (rect.height - tp.height) / 2,
  };
  canvas.save();
  canvas.clipRect(rect.inflate(2));
  tp.paint(canvas, Offset(rect.left + shapePadding, top));
  canvas.restore();
  tp.dispose();
}

/// Paints a page: background, then its shapes.
class PagePainter extends CustomPainter {
  PagePainter({
    required this.doc,
    required this.shapes,
    required this.width,
    required this.height,
    this.background,
    this.hints = false,
    this.hidden = const {},
  }) : super(repaint: CoreImages.instance);

  final int doc;
  final List<dynamic> shapes;
  final double width, height;
  final Color? background;
  final bool hints;

  /// Shapes whose text is being edited (their text is drawn by the editor).
  final Set<int> hidden;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.save();
    canvas.scale(size.width / width, size.height / height);
    canvas.clipRect(Rect.fromLTWH(0, 0, width, height));
    if (background != null) canvas.drawRect(Rect.fromLTWH(0, 0, width, height), Paint()..color = background!);
    for (final s in shapes.cast<Map<String, dynamic>>()) {
      paintShape(canvas, s, doc: doc, hints: hints, hideText: hidden.contains(s['id']));
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(PagePainter old) =>
      old.shapes != shapes || old.width != width || old.height != height || old.background != background || old.hints != hints || old.hidden != hidden;
}

/// A read-only page (thumbnails, slide show, printing).
class SlideView extends StatelessWidget {
  const SlideView({
    super.key,
    required this.doc,
    required this.shapes,
    required this.width,
    required this.height,
    this.background,
    this.hints = false,
  });

  final int doc;
  final List<dynamic> shapes;
  final double width, height;
  final Color? background;
  final bool hints;

  @override
  Widget build(BuildContext context) => AspectRatio(
        aspectRatio: width / height,
        child: CustomPaint(
          painter: PagePainter(doc: doc, shapes: shapes, width: width, height: height, background: background ?? Colors.white, hints: hints),
        ),
      );
}

/// A Mermaid figure scaled to the available width (or a fixed size).
class FigureView extends StatelessWidget {
  const FigureView({super.key, required this.figure, this.width, this.height});

  final Map<String, dynamic> figure;
  final double? width, height;

  @override
  Widget build(BuildContext context) {
    final fw = math.max(_d(figure['width'], 1), 1.0), fh = math.max(_d(figure['height'], 1), 1.0);
    final w = width ?? fw, h = height ?? (w * fh / fw);
    return SizedBox(
      width: w,
      height: h,
      child: CustomPaint(painter: _FigurePainter(figure)),
    );
  }
}

class _FigurePainter extends CustomPainter {
  _FigurePainter(this.figure);

  final Map<String, dynamic> figure;

  @override
  void paint(Canvas canvas, Size size) => paintFigureInto(canvas, figure, Offset.zero & size);

  @override
  bool shouldRepaint(_FigurePainter old) => old.figure != figure;
}

/// A picture from a document, at a given size.
class CoreImage extends StatelessWidget {
  const CoreImage({super.key, required this.doc, required this.id, required this.width, required this.height});

  final int doc;
  final String id;
  final double width, height;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: CoreImages.instance,
        builder: (context, _) {
          final img = CoreImages.instance.get(doc, id);
          if (img == null) {
            final broken = CoreImages.instance.failed(doc, id);
            return Container(
              width: width,
              height: height,
              color: const Color(0x14808080),
              child: Icon(broken ? Icons.broken_image_outlined : Icons.image_outlined, color: Colors.grey),
            );
          }
          return RawImage(image: img, width: width, height: height, fit: BoxFit.fill, filterQuality: FilterQuality.medium);
        },
      );
}
