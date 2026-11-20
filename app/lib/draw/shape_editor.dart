// Interactive editing of shapes on a slide, a drawing page or a Writer
// drawing: select (click, Shift+click, rubber band), move, resize with
// handles, draw new shapes, edit text in place, keyboard nudging, clipboard
// and stacking order. Every change is sent to the core, which owns undo.

import 'dart:math' as math;

import 'package:flutter/gestures.dart' show kSecondaryMouseButton;
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/native.dart';
import '../l10n/l10n.dart';
import 'shape_paint.dart';

/// What a drag on the canvas does.
enum DrawTool {
  select(Icons.near_me_outlined, 'Select'),
  rect(Icons.crop_square, 'Rectangle'),
  roundRect(Icons.rounded_corner, 'Rounded rectangle'),
  ellipse(Icons.circle_outlined, 'Ellipse'),
  triangle(Icons.change_history, 'Triangle'),
  diamond(Icons.diamond_outlined, 'Diamond'),
  line(Icons.horizontal_rule, 'Line'),
  arrow(Icons.arrow_right_alt, 'Arrow'),
  text(Icons.text_fields, 'Text box');

  const DrawTool(this.icon, this.labelKey);

  final IconData icon;
  final String labelKey;

  String get label => t(labelKey);

  /// The core's shape kind for new shapes.
  String get kind => switch (this) {
        DrawTool.rect || DrawTool.select => 'rect',
        DrawTool.roundRect => 'round_rect',
        DrawTool.ellipse => 'ellipse',
        DrawTool.triangle => 'triangle',
        DrawTool.diamond => 'diamond',
        DrawTool.line || DrawTool.arrow => 'line',
        DrawTool.text => 'text_box',
      };
}

/// Selection, tool and commands of one shape editor. The owner sets
/// [target] (`{'page': n}` or `{'block': n}`) and reloads its shapes in
/// [onChanged].
class ShapeEditorController extends ChangeNotifier {
  ShapeEditorController({required this.doc, required this.onChanged, this.onError});

  final int doc;
  Map<String, Object?> target = const {'page': 0};
  final VoidCallback onChanged;
  final void Function(String message)? onError;

  /// Called to edit a diagram's source (double click on a diagram).
  Future<String?> Function(String source)? editDiagram;

  DrawTool _tool = DrawTool.select;
  Set<int> _selection = {};
  List<dynamic> shapes = const [];

  DrawTool get tool => _tool;
  set tool(DrawTool t) {
    _tool = t;
    notifyListeners();
  }

  Set<int> get selection => _selection;
  set selection(Set<int> ids) {
    _selection = ids;
    notifyListeners();
  }

  /// The selected shapes, bottom to top.
  List<Map<String, dynamic>> get selected =>
      shapes.cast<Map<String, dynamic>>().where((s) => _selection.contains(s['id'])).toList();

  dynamic call(String op, [Map<String, Object?> args = const {}]) {
    try {
      return core({'op': op, 'doc': doc, ...target, ...args});
    } on CoreError catch (e) {
      onError?.call(e.message);
      return null;
    }
  }

  void _changed() {
    onChanged();
    notifyListeners();
  }

  /// Updates shapes; `key` merges repeated edits into one undo step.
  void update(Map<int, Map<String, Object?>> patches, {String? key}) {
    if (patches.isEmpty) return;
    call('shapes.update', {
      'updates': [for (final e in patches.entries) {'id': e.key, 'patch': e.value}],
      'key': ?key,
    });
    // Keep the local copy current until the owner rebuilds (key repeats can
    // arrive before the next frame).
    shapes = [
      for (final s in shapes.cast<Map<String, dynamic>>())
        if (patches[s['id']] case final p?) {...s, for (final e in p.entries) if (_topLevel.contains(e.key)) e.key: e.value} else s,
    ];
    _changed();
  }

  /// Applies one patch to every selected shape.
  void apply(Map<String, Object?> patch) => update({for (final id in _selection) id: patch});

  /// Adds shapes and selects them.
  List<int> add(List<Map<String, Object?>> shapes) {
    final ids = (call('shapes.add', {'shapes': shapes}) as List<dynamic>?)?.cast<int>() ?? const [];
    _selection = ids.toSet();
    _changed();
    return ids;
  }

  void delete() {
    if (_selection.isEmpty) return;
    call('shapes.remove', {'ids': _selection.toList()});
    _selection = {};
    _changed();
  }

  void copy() {
    if (_selection.isNotEmpty) call('shapes.copy', {'ids': _selection.toList()});
  }

  void cut() {
    copy();
    delete();
  }

  void paste({double dx = 12, double dy = 12}) {
    final ids = (call('shapes.paste', {'dx': dx, 'dy': dy}) as List<dynamic>?)?.cast<int>();
    if (ids != null) _selection = ids.toSet();
    _changed();
  }

  void duplicate() {
    if (_selection.isEmpty) return;
    copy();
    paste();
  }

  void order(String to) {
    for (final id in _selection) {
      call('shapes.order', {'id': id, 'to': to});
    }
    _changed();
  }

  void selectAll() => selection = shapes.cast<Map<String, dynamic>>().map((s) => s['id'] as int).toSet();

  /// Inserts a picture file at (x, y), at most `maxW` × `maxH` points.
  void insertPicture(String path, {double x = 36, double y = 36, double maxW = 400, double maxH = 300}) {
    final id = call('shapes.insert_image', {'path': path, 'x': x, 'y': y, 'max_w': maxW, 'max_h': maxH});
    if (id is int) _selection = {id};
    _changed();
  }

  /// Inserts a Mermaid diagram sized from its figure.
  void insertDiagram(String source, {double x = 36, double y = 36, double maxW = 500, double maxH = 400}) {
    final r = core({'op': 'mermaid.render', 'source': source}) as Map<String, dynamic>;
    final fig = r['figure'] as Map<String, dynamic>;
    final fw = (fig['width'] as num).toDouble(), fh = (fig['height'] as num).toDouble();
    final k = math.min(1.0, math.min(maxW / fw, maxH / fh));
    add([
      {'kind': 'diagram', 'x': x, 'y': y, 'w': fw * k, 'h': fh * k, 'source': source},
    ]);
  }

  void breakUndo() => call('draw.break_undo');
}

/// Patch fields that are stored at the top level of a shape's JSON.
const _topLevel = {'x', 'y', 'w', 'h', 'rotation', 'fill', 'stroke', 'stroke_width', 'dash', 'arrow_start', 'arrow_end', 'text', 'source'};

enum _Drag { none, move, resize, lineEnd, rubber, create }

double _d(Object? v) => (v as num?)?.toDouble() ?? 0;

Offset _rotate(Offset p, double deg) {
  final a = deg * math.pi / 180;
  final c = math.cos(a), s = math.sin(a);
  return Offset(p.dx * c - p.dy * s, p.dx * s + p.dy * c);
}

/// The editable canvas.
class ShapeEditor extends StatefulWidget {
  const ShapeEditor({
    super.key,
    required this.controller,
    required this.shapes,
    required this.width,
    required this.height,
    required this.scale,
    this.background,
    this.hints = false,
    this.autofocus = false,
  });

  final ShapeEditorController controller;
  final List<dynamic> shapes;

  /// Page size in points and pixels per point.
  final double width, height, scale;
  final Color? background;

  /// Show hints in empty placeholders (slides).
  final bool hints;
  final bool autofocus;

  @override
  State<ShapeEditor> createState() => ShapeEditorState();
}

class ShapeEditorState extends State<ShapeEditor> {
  final _focus = FocusNode(debugLabel: 'shape editor');
  _Drag _drag = _Drag.none;
  Offset _start = Offset.zero, _current = Offset.zero;
  int _handle = 0;
  Map<String, dynamic>? _dragShape;
  int? _editing;
  TextEditingController? _text;
  final _textFocus = FocusNode(debugLabel: 'shape text');

  ShapeEditorController get c => widget.controller;
  double get k => widget.scale;

  @override
  void initState() {
    super.initState();
    c.shapes = widget.shapes;
    c.addListener(_onController);
    _textFocus.addListener(() {
      if (!_textFocus.hasFocus) _endEditing();
    });
  }

  @override
  void didUpdateWidget(ShapeEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onController);
      widget.controller.addListener(_onController);
    }
    c.shapes = widget.shapes;
    // Drop ids of shapes that no longer exist.
    final ids = widget.shapes.cast<Map<String, dynamic>>().map((s) => s['id']).toSet();
    if (c.selection.any((id) => !ids.contains(id))) {
      c.selection = c.selection.where(ids.contains).toSet();
    }
  }

  @override
  void dispose() {
    c.removeListener(_onController);
    _focus.dispose();
    _textFocus.dispose();
    _text?.dispose();
    super.dispose();
  }

  void _onController() {
    if (mounted) setState(() {});
  }

  Map<String, dynamic>? _shape(int id) {
    for (final s in widget.shapes.cast<Map<String, dynamic>>()) {
      if (s['id'] == id) return s;
    }
    return null;
  }

  /// A point in the shape's unrotated frame.
  Offset _local(Map<String, dynamic> s, Offset p) {
    final r = _d(s['rotation']);
    if (r == 0 || s['kind'] == 'line') return p;
    final center = Offset(_d(s['x']) + _d(s['w']) / 2, _d(s['y']) + _d(s['h']) / 2);
    return center + _rotate(p - center, -r);
  }

  bool _hits(Map<String, dynamic> s, Offset p) {
    final tol = 6 / k;
    if (s['kind'] == 'line') {
      final a = Offset(_d(s['x']), _d(s['y']));
      final b = a + Offset(_d(s['w']), _d(s['h']));
      final ab = b - a;
      final len2 = ab.dx * ab.dx + ab.dy * ab.dy;
      final t = len2 == 0 ? 0.0 : (((p - a).dx * ab.dx + (p - a).dy * ab.dy) / len2).clamp(0.0, 1.0);
      return (p - (a + ab * t)).distance <= tol;
    }
    final q = _local(s, p);
    return Rect.fromLTWH(_d(s['x']), _d(s['y']), _d(s['w']), _d(s['h'])).inflate(tol).contains(q);
  }

  Map<String, dynamic>? _hitShape(Offset p) {
    for (final s in widget.shapes.reversed.cast<Map<String, dynamic>>()) {
      if (_hits(s, p)) return s;
    }
    return null;
  }

  /// Handle positions in page coordinates (rotated with the shape).
  List<Offset> _handles(Map<String, dynamic> s) {
    final x = _d(s['x']), y = _d(s['y']), w = _d(s['w']), h = _d(s['h']);
    if (s['kind'] == 'line') return [Offset(x, y), Offset(x + w, y + h)];
    final pts = [
      Offset(x, y), Offset(x + w / 2, y), Offset(x + w, y), Offset(x + w, y + h / 2),
      Offset(x + w, y + h), Offset(x + w / 2, y + h), Offset(x, y + h), Offset(x, y + h / 2),
    ];
    final r = _d(s['rotation']);
    if (r == 0) return pts;
    final center = Offset(x + w / 2, y + h / 2);
    return [for (final p in pts) center + _rotate(p - center, r)];
  }

  (Map<String, dynamic>, int)? _hitHandle(Offset p) {
    if (c.selection.length != 1) return null;
    final s = _shape(c.selection.first);
    if (s == null) return null;
    final hs = _handles(s);
    for (var i = 0; i < hs.length; i++) {
      if ((hs[i] - p).distance <= 7 / k) return (s, i);
    }
    return null;
  }

  // ---- gestures ---------------------------------------------------------------

  Offset _pt(Offset local) => local / k;

  bool get _shift => HardwareKeyboard.instance.isShiftPressed;

  void _pointerDown(PointerDownEvent e) {
    if (_editing != null) return;
    _focus.requestFocus();
    if (e.buttons == kSecondaryMouseButton) return;
    final p = _pt(e.localPosition);
    _start = p;
    _current = p;
    if (c.tool != DrawTool.select) return;
    if (_hitHandle(p) != null) return;
    final hit = _hitShape(p);
    if (hit == null) {
      if (!_shift) c.selection = {};
      return;
    }
    final id = hit['id'] as int;
    if (_shift) {
      c.selection = c.selection.contains(id) ? (c.selection.toSet()..remove(id)) : {...c.selection, id};
    } else if (!c.selection.contains(id)) {
      c.selection = {id};
    }
  }

  void _panStart(DragStartDetails d) {
    if (_editing != null) return;
    final p = _start;
    _current = _pt(d.localPosition);
    if (c.tool != DrawTool.select) {
      _drag = _Drag.create;
    } else if (_hitHandle(p) case (final s, final i)) {
      _dragShape = s;
      _handle = i;
      _drag = s['kind'] == 'line' ? _Drag.lineEnd : _Drag.resize;
    } else if (_hitShape(p) != null && c.selection.isNotEmpty) {
      _drag = _Drag.move;
    } else {
      _drag = _Drag.rubber;
    }
    setState(() {});
  }

  void _panUpdate(DragUpdateDetails d) {
    if (_drag == _Drag.none) return;
    setState(() => _current = _pt(d.localPosition));
  }

  void _panEnd(DragEndDetails _) {
    final drag = _drag;
    _drag = _Drag.none;
    switch (drag) {
      case _Drag.move:
        final delta = _current - _start;
        if (delta.distance > 0.01) {
          c.update({
            for (final s in c.selected) s['id'] as int: {'x': _d(s['x']) + delta.dx, 'y': _d(s['y']) + delta.dy},
          });
        }
      case _Drag.resize || _Drag.lineEnd:
        final s = _dragShape;
        if (s != null) c.update({s['id'] as int: _resized(s)});
      case _Drag.rubber:
        final r = Rect.fromPoints(_start, _current);
        final ids = widget.shapes.cast<Map<String, dynamic>>().where((s) {
          final b = Rect.fromPoints(Offset(_d(s['x']), _d(s['y'])), Offset(_d(s['x']) + _d(s['w']), _d(s['y']) + _d(s['h'])));
          return r.contains(b.topLeft) && r.contains(b.bottomRight);
        }).map((s) => s['id'] as int);
        c.selection = _shift ? {...c.selection, ...ids} : ids.toSet();
      case _Drag.create:
        _create(_start, _current);
      case _Drag.none:
        break;
    }
    _dragShape = null;
    setState(() {});
  }

  void _tapUp(TapUpDetails d) {
    if (c.tool != DrawTool.select && _editing == null) _create(_pt(d.localPosition), _pt(d.localPosition));
  }

  void _create(Offset a, Offset b) {
    final tool = c.tool;
    final line = tool == DrawTool.line || tool == DrawTool.arrow;
    Map<String, Object?> spec;
    if (line) {
      var end = b;
      if ((b - a).distance < 3) end = a + const Offset(144, 0);
      if (_shift) {
        final v = end - a;
        final angle = (math.atan2(v.dy, v.dx) / (math.pi / 4)).round() * (math.pi / 4);
        end = a + Offset(math.cos(angle), math.sin(angle)) * v.distance;
      }
      spec = {'kind': 'line', 'x': a.dx, 'y': a.dy, 'w': end.dx - a.dx, 'h': end.dy - a.dy, if (tool == DrawTool.arrow) 'arrow_end': true};
    } else {
      var r = Rect.fromPoints(a, b);
      if (r.width < 4 || r.height < 4) {
        final size = tool == DrawTool.text ? const Size(200, 40) : const Size(120, 80);
        r = Rect.fromLTWH(a.dx, a.dy, size.width, size.height);
      }
      if (_shift && tool != DrawTool.text) r = Rect.fromLTWH(r.left, r.top, r.width, r.width);
      spec = {'kind': tool.kind, 'x': r.left, 'y': r.top, 'w': r.width, 'h': r.height};
    }
    final ids = c.add([spec]);
    c.tool = DrawTool.select;
    if (tool == DrawTool.text && ids.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _beginEditing(ids.first, removeIfEmpty: true));
    }
  }

  /// The patch for the shape being resized with the current drag.
  Map<String, Object?> _resized(Map<String, dynamic> s) {
    final x = _d(s['x']), y = _d(s['y']), w = _d(s['w']), h = _d(s['h']);
    if (s['kind'] == 'line') {
      return _handle == 0
          ? {'x': _current.dx, 'y': _current.dy, 'w': x + w - _current.dx, 'h': y + h - _current.dy}
          : {'w': _current.dx - x, 'h': _current.dy - y};
    }
    final rot = _d(s['rotation']);
    final d = _rotate(_current - _start, -rot);
    var l = x, t = y, r = x + w, b = y + h;
    switch (_handle) {
      case 0:
        l += d.dx;
        t += d.dy;
      case 1:
        t += d.dy;
      case 2:
        r += d.dx;
        t += d.dy;
      case 3:
        r += d.dx;
      case 4:
        r += d.dx;
        b += d.dy;
      case 5:
        b += d.dy;
      case 6:
        l += d.dx;
        b += d.dy;
      case 7:
        l += d.dx;
    }
    const minSide = 4.0;
    if (r - l < minSide) {
      if ([0, 6, 7].contains(_handle)) {
        l = r - minSide;
      } else {
        r = l + minSide;
      }
    }
    if (b - t < minSide) {
      if ([0, 1, 2].contains(_handle)) {
        t = b - minSide;
      } else {
        b = t + minSide;
      }
    }
    // Corners keep the aspect ratio of pictures (and of anything with Shift).
    final corner = _handle.isEven;
    if (corner && (s['kind'] == 'image' || s['kind'] == 'diagram' || _shift) && w > 0) {
      final nh = (r - l) * h / w;
      if (_handle <= 2) {
        t = b - nh;
      } else {
        b = t + nh;
      }
    }
    var rect = Rect.fromLTRB(l, t, r, b);
    if (rot != 0) {
      // Keep the opposite handle where it was on the page.
      final anchorIndex = (_handle + 4) % 8;
      final orig = Rect.fromLTWH(x, y, w, h);
      Offset local(Rect q, int i) => [q.topLeft, q.topCenter, q.topRight, q.centerRight, q.bottomRight, q.bottomCenter, q.bottomLeft, q.centerLeft][i];
      Offset world(Rect q, Offset p) => q.center + _rotate(p - q.center, rot);
      final want = world(orig, local(orig, anchorIndex));
      final got = world(rect, local(rect, anchorIndex));
      rect = rect.shift(want - got);
    }
    return {'x': rect.left, 'y': rect.top, 'w': rect.width, 'h': rect.height};
  }

  /// Shapes as painted during a drag.
  List<dynamic> _preview() {
    if (_drag == _Drag.move) {
      final delta = _current - _start;
      return [
        for (final s in widget.shapes.cast<Map<String, dynamic>>())
          c.selection.contains(s['id']) ? {...s, 'x': _d(s['x']) + delta.dx, 'y': _d(s['y']) + delta.dy} : s,
      ];
    }
    if ((_drag == _Drag.resize || _drag == _Drag.lineEnd) && _dragShape != null) {
      final patch = _resized(_dragShape!);
      return [
        for (final s in widget.shapes.cast<Map<String, dynamic>>()) s['id'] == _dragShape!['id'] ? {...s, ...patch} : s,
      ];
    }
    return widget.shapes;
  }

  // ---- text editing -------------------------------------------------------------

  bool _removeIfEmpty = false;

  Future<void> _doubleTap(Offset local) async {
    final hit = _hitShape(_pt(local));
    if (hit == null) return;
    final id = hit['id'] as int;
    if (hit['kind'] == 'diagram') {
      final edit = c.editDiagram;
      if (edit == null) return;
      final source = await edit(hit['source'] as String? ?? '');
      if (source != null) c.update({id: {'source': source}});
      return;
    }
    if (hit['kind'] == 'line' || hit['kind'] == 'image') return;
    _beginEditing(id);
  }

  void _beginEditing(int id, {bool removeIfEmpty = false}) {
    final s = _shape(id);
    if (s == null) return;
    _text?.dispose();
    _text = TextEditingController(text: s['text'] as String? ?? '');
    _removeIfEmpty = removeIfEmpty;
    c.breakUndo();
    setState(() {
      _editing = id;
      c.selection = {id};
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => _textFocus.requestFocus());
  }

  void _endEditing() {
    final id = _editing;
    if (id == null) return;
    final text = _text?.text ?? '';
    setState(() => _editing = null);
    c.breakUndo();
    if (_removeIfEmpty && text.trim().isEmpty) {
      c.selection = {id};
      c.delete();
    }
    _focus.requestFocus();
  }

  Widget _editor(Map<String, dynamic> s) {
    final style = s['style'] as Map<String, dynamic>? ?? const {};
    final base = shapeTextStyle(style);
    final x = _d(s['x']) * k, y = _d(s['y']) * k, w = _d(s['w']) * k, h = _d(s['h']) * k;
    final valign = switch (style['valign']) { 'top' => TextAlignVertical.top, 'bottom' => TextAlignVertical.bottom, _ => TextAlignVertical.center };
    return Positioned(
      left: x,
      top: y,
      width: w,
      height: h,
      child: Transform.rotate(
        angle: _d(s['rotation']) * math.pi / 180,
        child: Container(
          decoration: BoxDecoration(border: Border.all(color: Theme.of(context).colorScheme.primary, width: 1.5)),
          padding: EdgeInsets.all(shapePadding * k - 1.5),
          child: CallbackShortcuts(
            bindings: {const SingleActivator(LogicalKeyboardKey.escape): () => _textFocus.unfocus()},
            child: TextField(
              controller: _text,
              focusNode: _textFocus,
              maxLines: null,
              expands: true,
              textAlign: shapeTextAlign(style),
              textAlignVertical: valign,
              style: base.copyWith(fontSize: (base.fontSize ?? 18) * k),
              cursorColor: base.color,
              decoration: const InputDecoration(border: InputBorder.none, isCollapsed: true, filled: false),
              onChanged: (v) => c.update({s['id'] as int: {'text': v}}, key: 'text'),
            ),
          ),
        ),
      ),
    );
  }

  // ---- keyboard and context menu ---------------------------------------------------

  KeyEventResult _key(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
    if (_editing != null) return KeyEventResult.ignored;
    final ctrl = HardwareKeyboard.instance.isControlPressed || HardwareKeyboard.instance.isMetaPressed;
    final key = e.logicalKey;
    final step = _shift ? 10.0 : 1.0;
    Offset? nudge = switch (key) {
      LogicalKeyboardKey.arrowLeft => Offset(-step, 0),
      LogicalKeyboardKey.arrowRight => Offset(step, 0),
      LogicalKeyboardKey.arrowUp => Offset(0, -step),
      LogicalKeyboardKey.arrowDown => Offset(0, step),
      _ => null,
    };
    if (nudge != null && c.selection.isNotEmpty) {
      c.update({for (final s in c.selected) s['id'] as int: {'x': _d(s['x']) + nudge.dx, 'y': _d(s['y']) + nudge.dy}}, key: 'nudge');
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.delete || key == LogicalKeyboardKey.backspace) {
      if (c.selection.isEmpty) return KeyEventResult.ignored;
      c.delete();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.escape) {
      c.tool = DrawTool.select;
      c.selection = {};
      return KeyEventResult.handled;
    }
    if ((key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.f2) && c.selection.length == 1) {
      final s = _shape(c.selection.first);
      if (s != null && s['kind'] != 'line' && s['kind'] != 'image' && s['kind'] != 'diagram') {
        _beginEditing(s['id'] as int);
        return KeyEventResult.handled;
      }
    }
    if (ctrl) {
      switch (key) {
        case LogicalKeyboardKey.keyC:
          c.copy();
        case LogicalKeyboardKey.keyX:
          c.cut();
        case LogicalKeyboardKey.keyV:
          c.paste();
        case LogicalKeyboardKey.keyD:
          c.duplicate();
        case LogicalKeyboardKey.keyA:
          c.selectAll();
        default:
          return KeyEventResult.ignored;
      }
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  Future<void> _contextMenu(Offset global, Offset local) async {
    final hit = _hitShape(_pt(local));
    if (hit != null && !c.selection.contains(hit['id'])) c.selection = {hit['id'] as int};
    final has = c.selection.isNotEmpty;
    final diagram = has && c.selected.length == 1 && c.selected.first['kind'] == 'diagram';
    PopupMenuItem<String> item(String v, String label, {bool enabled = true}) => PopupMenuItem(value: v, enabled: enabled, child: Text(label));
    final action = await showMenu<String>(
      context: context,
      position: RelativeRect.fromLTRB(global.dx, global.dy, global.dx, global.dy),
      items: [
        if (diagram) item('diagram', t('Edit diagram…')),
        if (has && !diagram && c.selected.first['kind'] != 'line' && c.selected.first['kind'] != 'image') item('text', t('Edit text')),
        item('cut', t('Cut'), enabled: has),
        item('copy', t('Copy'), enabled: has),
        item('paste', t('Paste')),
        item('duplicate', t('Duplicate'), enabled: has),
        const PopupMenuDivider(),
        item('front', t('Bring to front'), enabled: has),
        item('forward', t('Bring forward'), enabled: has),
        item('backward', t('Send backward'), enabled: has),
        item('back', t('Send to back'), enabled: has),
        const PopupMenuDivider(),
        item('delete', t('Delete'), enabled: has),
      ],
    );
    switch (action) {
      case 'diagram':
        await _doubleTap(local);
      case 'text':
        _beginEditing(c.selection.first);
      case 'cut':
        c.cut();
      case 'copy':
        c.copy();
      case 'paste':
        final p = _pt(local);
        final first = c.selection.isEmpty ? null : c.selected.first;
        c.paste(dx: first == null ? 12 : p.dx - _d(first['x']), dy: first == null ? 12 : p.dy - _d(first['y']));
      case 'duplicate':
        c.duplicate();
      case 'front' || 'forward' || 'backward' || 'back':
        c.order(action!);
      case 'delete':
        c.delete();
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final shapes = _preview();
    final editing = _editing == null ? null : _shape(_editing!);
    return Focus(
      focusNode: _focus,
      autofocus: widget.autofocus,
      onKeyEvent: _key,
      child: MouseRegion(
        cursor: c.tool == DrawTool.select ? SystemMouseCursors.basic : SystemMouseCursors.precise,
        child: Listener(
          onPointerDown: _pointerDown,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onPanStart: _panStart,
            onPanUpdate: _panUpdate,
            onPanEnd: _panEnd,
            onTapUp: _tapUp,
            onDoubleTapDown: (d) => _doubleTap(d.localPosition),
            onSecondaryTapUp: (d) => _contextMenu(d.globalPosition, d.localPosition),
            onLongPressStart: (d) => _contextMenu(d.globalPosition, d.localPosition),
            child: SizedBox(
              width: widget.width * k,
              height: widget.height * k,
              child: Stack(
                clipBehavior: Clip.none,
                children: [
                  Positioned.fill(
                    child: CustomPaint(
                      painter: PagePainter(
                        doc: c.doc,
                        shapes: shapes,
                        width: widget.width,
                        height: widget.height,
                        background: widget.background,
                        hints: widget.hints,
                        hidden: {?_editing},
                      ),
                    ),
                  ),
                  Positioned.fill(
                    child: IgnorePointer(
                      child: CustomPaint(
                        painter: _OverlayPainter(
                          state: this,
                          shapes: shapes,
                          color: theme.colorScheme.primary,
                        ),
                      ),
                    ),
                  ),
                  if (editing != null) _editor(editing),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Selection frames, handles, rubber band and the shape being drawn.
class _OverlayPainter extends CustomPainter {
  _OverlayPainter({required this.state, required this.shapes, required this.color});

  final ShapeEditorState state;
  final List<dynamic> shapes;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final k = state.k;
    final line = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1;
    final handleFill = Paint()..color = Colors.white;
    for (final s in shapes.cast<Map<String, dynamic>>()) {
      if (!state.c.selection.contains(s['id'])) continue;
      final x = _d(s['x']), y = _d(s['y']), w = _d(s['w']), h = _d(s['h']);
      canvas.save();
      canvas.scale(k);
      if (s['kind'] != 'line') {
        final r = _d(s['rotation']);
        canvas.save();
        if (r != 0) {
          canvas
            ..translate(x + w / 2, y + h / 2)
            ..rotate(r * math.pi / 180)
            ..translate(-(x + w / 2), -(y + h / 2));
        }
        canvas.drawPath(dashed(Path()..addRect(Rect.fromLTWH(x, y, w, h)), 1 / k), line..strokeWidth = 1 / k);
        canvas.restore();
      }
      canvas.restore();
      if (state.c.selection.length == 1) {
        for (final p in state._handles(s)) {
          final r = Rect.fromCenter(center: p * k, width: 8, height: 8);
          canvas.drawRect(r, handleFill);
          canvas.drawRect(r, line..strokeWidth = 1);
        }
      }
    }
    final a = state._start * k, b = state._current * k;
    switch (state._drag) {
      case _Drag.rubber:
        canvas.drawRect(Rect.fromPoints(a, b), Paint()..color = color.withValues(alpha: 0.12));
        canvas.drawRect(Rect.fromPoints(a, b), line..strokeWidth = 1);
      case _Drag.create:
        final tool = state.c.tool;
        if (tool == DrawTool.line || tool == DrawTool.arrow) {
          paintPolyline(canvas, [a, b], line..strokeWidth = 1.5, end: tool == DrawTool.arrow ? 'triangle' : null);
        } else if (tool == DrawTool.ellipse) {
          canvas.drawOval(Rect.fromPoints(a, b), line..strokeWidth = 1.5);
        } else {
          canvas.drawRect(Rect.fromPoints(a, b), line..strokeWidth = 1.5);
        }
      default:
        break;
    }
  }

  @override
  bool shouldRepaint(_OverlayPainter old) => true;
}

/// Colours offered for fills, lines and text.
const shapeColors = <Color>[
  Color(0xFF000000), Color(0xFF595959), Color(0xFFA6A6A6), Color(0xFFFFFFFF),
  Color(0xFFC00000), Color(0xFFE36C09), Color(0xFFFFC000), Color(0xFF00B050),
  Color(0xFF00843D), Color(0xFF729FCF), Color(0xFF3465A4), Color(0xFF1F3864),
  Color(0xFF7030A0), Color(0xFFFF66CC), Color(0xFFFFF2CC), Color(0xFFDDEBF7),
];

/// A colour menu button; `null` in the result means "none".
Widget colorMenuButton({required String tooltip, required IconData icon, required ValueChanged<Color?> onSelected, bool allowNone = true, Color? current}) {
  return PopupMenuButton<int>(
    tooltip: tooltip,
    icon: Icon(icon, color: current),
    itemBuilder: (_) => [
      if (allowNone)
        PopupMenuItem(value: -1, child: Row(children: [const Icon(Icons.block, size: 20), const SizedBox(width: 12), Text(t('None'))])),
      PopupMenuItem(
        enabled: false,
        child: Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (var i = 0; i < shapeColors.length; i++)
              Builder(
                builder: (context) => InkWell(
                  onTap: () => Navigator.pop(context, i),
                  child: Container(
                    width: 24,
                    height: 24,
                    decoration: BoxDecoration(color: shapeColors[i], border: Border.all(color: Colors.grey)),
                  ),
                ),
              ),
          ],
        ),
      ),
    ],
    onSelected: (i) => onSelected(i < 0 ? null : shapeColors[i]),
  );
}

/// Drawing tools and formatting of the selected shapes.
class ShapeToolbar extends StatelessWidget {
  const ShapeToolbar({super.key, required this.controller, this.onInsertPicture, this.onInsertDiagram, this.leading = const [], this.trailing = const []});

  final ShapeEditorController controller;
  final VoidCallback? onInsertPicture;
  final VoidCallback? onInsertDiagram;
  final List<Widget> leading, trailing;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final c = controller;
        final sel = c.selected;
        final first = sel.isEmpty ? null : sel.first;
        final style = first?['style'] as Map<String, dynamic>? ?? const {};
        final has = first != null;
        final isLine = first?['kind'] == 'line';
        Widget toggle(String key, IconData icon, String tip) => IconButton(
              tooltip: t(tip),
              isSelected: style[key] == true || (first != null && first[key] == true),
              icon: Icon(icon),
              onPressed: has ? () => c.apply({key: !(style[key] == true || first[key] == true)}) : null,
            );
        return Material(
          color: Theme.of(context).colorScheme.surfaceContainer,
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
            child: Row(
              children: [
                ...leading,
                for (final tool in DrawTool.values)
                  IconButton(
                    tooltip: tool.label,
                    isSelected: c.tool == tool,
                    icon: Icon(tool.icon),
                    onPressed: () => c.tool = tool,
                  ),
                if (onInsertPicture != null)
                  IconButton(tooltip: t('Insert picture…'), icon: const Icon(Icons.image_outlined), onPressed: onInsertPicture),
                if (onInsertDiagram != null)
                  IconButton(tooltip: t('Insert diagram (Mermaid)…'), icon: const Icon(Icons.account_tree_outlined), onPressed: onInsertDiagram),
                const _Sep(),
                colorMenuButton(
                  tooltip: t('Fill colour'),
                  icon: Icons.format_color_fill,
                  current: hexColor(first?['fill']),
                  onSelected: has ? (col) => c.apply({'fill': col == null ? null : colorHex(col)}) : (_) {},
                ),
                colorMenuButton(
                  tooltip: t('Line colour'),
                  icon: Icons.border_color_outlined,
                  current: hexColor(first?['stroke']),
                  onSelected: has ? (col) => c.apply({'stroke': col == null ? null : colorHex(col)}) : (_) {},
                ),
                PopupMenuButton<double>(
                  tooltip: t('Line width'),
                  enabled: has,
                  icon: const Icon(Icons.line_weight),
                  itemBuilder: (_) => [
                    for (final w in const [0.5, 1.0, 2.0, 3.0, 4.5, 6.0])
                      CheckedPopupMenuItem(value: w, checked: first?['stroke_width'] == w, child: Text('$w pt')),
                  ],
                  onSelected: (w) => c.apply({'stroke_width': w}),
                ),
                toggle('dash', Icons.more_horiz, 'Dashed line'),
                if (isLine) ...[
                  toggle('arrow_start', Icons.west, 'Arrow at start'),
                  toggle('arrow_end', Icons.east, 'Arrow at end'),
                ],
                const _Sep(),
                PopupMenuButton<double>(
                  tooltip: t('Font size'),
                  enabled: has && !isLine,
                  icon: const Icon(Icons.format_size),
                  itemBuilder: (_) => [
                    for (final s in const [10.0, 12.0, 14.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 44.0, 54.0, 72.0])
                      CheckedPopupMenuItem(value: s, checked: style['size'] == s, child: Text('${s.toInt()} pt')),
                  ],
                  onSelected: (s) => c.apply({'size': s}),
                ),
                toggle('bold', Icons.format_bold, 'Bold (Ctrl+B)'),
                toggle('italic', Icons.format_italic, 'Italic (Ctrl+I)'),
                toggle('underline', Icons.format_underlined, 'Underline (Ctrl+U)'),
                colorMenuButton(
                  tooltip: t('Text colour'),
                  icon: Icons.format_color_text,
                  allowNone: false,
                  current: hexColor(style['color']),
                  onSelected: has ? (col) => c.apply({'color': colorHex(col ?? Colors.black)}) : (_) {},
                ),
                for (final (a, icon, tip) in [
                  ('left', Icons.format_align_left, 'Align left'),
                  ('center', Icons.format_align_center, 'Centre'),
                  ('right', Icons.format_align_right, 'Align right'),
                ])
                  IconButton(
                    tooltip: t(tip),
                    isSelected: has && style['align'] == a,
                    icon: Icon(icon),
                    onPressed: has ? () => c.apply({'align': a}) : null,
                  ),
                PopupMenuButton<String>(
                  tooltip: t('Vertical alignment'),
                  enabled: has && !isLine,
                  icon: const Icon(Icons.vertical_align_center),
                  itemBuilder: (_) => [
                    CheckedPopupMenuItem(value: 'top', checked: style['valign'] == 'top', child: Text(t('Top'))),
                    CheckedPopupMenuItem(value: 'middle', checked: style['valign'] == 'middle', child: Text(t('Middle'))),
                    CheckedPopupMenuItem(value: 'bottom', checked: style['valign'] == 'bottom', child: Text(t('Bottom'))),
                  ],
                  onSelected: (v) => c.apply({'valign': v}),
                ),
                toggle('bullets', Icons.format_list_bulleted, 'Bulleted list'),
                const _Sep(),
                PopupMenuButton<String>(
                  tooltip: t('Arrange'),
                  enabled: has,
                  icon: const Icon(Icons.flip_to_front),
                  itemBuilder: (_) => [
                    PopupMenuItem(value: 'front', child: Text(t('Bring to front'))),
                    PopupMenuItem(value: 'forward', child: Text(t('Bring forward'))),
                    PopupMenuItem(value: 'backward', child: Text(t('Send backward'))),
                    PopupMenuItem(value: 'back', child: Text(t('Send to back'))),
                    const PopupMenuDivider(),
                    PopupMenuItem(value: 'rotate', child: Text(t('Rotate 90°'))),
                  ],
                  onSelected: (v) {
                    if (v == 'rotate') {
                      c.update({for (final s in c.selected) s['id'] as int: {'rotation': ((s['rotation'] as num?) ?? 0) + 90}});
                    } else {
                      c.order(v);
                    }
                  },
                ),
                IconButton(tooltip: t('Duplicate (Ctrl+D)'), icon: const Icon(Icons.copy_all_outlined), onPressed: has ? c.duplicate : null),
                IconButton(tooltip: t('Delete'), icon: const Icon(Icons.delete_outline), onPressed: has ? c.delete : null),
                ...trailing,
              ],
            ),
          ),
        );
      },
    );
  }
}

class _Sep extends StatelessWidget {
  const _Sep();

  @override
  Widget build(BuildContext context) => const SizedBox(height: 28, child: VerticalDivider(width: 12));
}
