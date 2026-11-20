// The full-screen slide show: one slide at a time on black, scaled to fit.
// Click, Space, Enter, the right and down arrows or Page Down go forward;
// Backspace, the left and up arrows or Page Up go back; Home and End jump;
// Escape ends the show.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import 'shape_paint.dart';

/// Shows the slides of `data` (the result of `draw.get`) from `start`.
Future<void> startSlideShow(BuildContext context, {required int doc, required Map<String, dynamic> data, int start = 0}) async {
  final pages = data['pages'] as List<dynamic>;
  for (final p in pages.cast<Map<String, dynamic>>()) {
    await CoreImages.instance.preload(doc, shapeImageIds(p['shapes'] as List<dynamic>));
  }
  if (!context.mounted) return;
  await Navigator.of(context).push(PageRouteBuilder<void>(
    opaque: true,
    pageBuilder: (_, _, _) => _SlideShow(doc: doc, data: data, start: start),
    transitionsBuilder: (_, animation, _, child) => FadeTransition(opacity: animation, child: child),
  ));
}

class _SlideShow extends StatefulWidget {
  const _SlideShow({required this.doc, required this.data, required this.start});

  final int doc;
  final Map<String, dynamic> data;
  final int start;

  @override
  State<_SlideShow> createState() => _SlideShowState();
}

class _SlideShowState extends State<_SlideShow> {
  late int _index = widget.start;
  final _focus = FocusNode();

  List<dynamic> get _pages => widget.data['pages'] as List<dynamic>;

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  void _go(int i) {
    // One step past the last slide shows "end of slide show"; one more ends.
    if (i > _pages.length) {
      Navigator.of(context).pop();
      return;
    }
    setState(() => _index = i.clamp(0, _pages.length));
  }

  KeyEventResult _key(FocusNode _, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
    switch (e.logicalKey) {
      case LogicalKeyboardKey.arrowRight || LogicalKeyboardKey.arrowDown || LogicalKeyboardKey.pageDown || LogicalKeyboardKey.space || LogicalKeyboardKey.enter || LogicalKeyboardKey.keyN:
        _go(_index + 1);
      case LogicalKeyboardKey.arrowLeft || LogicalKeyboardKey.arrowUp || LogicalKeyboardKey.pageUp || LogicalKeyboardKey.backspace || LogicalKeyboardKey.keyP:
        _go(_index - 1);
      case LogicalKeyboardKey.home:
        _go(0);
      case LogicalKeyboardKey.end:
        _go(_pages.length - 1);
      case LogicalKeyboardKey.escape:
        Navigator.of(context).pop();
      default:
        return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final w = (widget.data['width'] as num).toDouble(), h = (widget.data['height'] as num).toDouble();
    final end = _index >= _pages.length;
    final page = end ? null : _pages[_index] as Map<String, dynamic>;
    return Focus(
      focusNode: _focus,
      autofocus: true,
      onKeyEvent: _key,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => _go(_index + 1),
        onSecondaryTap: () => _go(_index - 1),
        onHorizontalDragEnd: (d) => _go(_index + ((d.primaryVelocity ?? 0) < 0 ? 1 : -1)),
        child: ColoredBox(
          color: Colors.black,
          child: Center(
            child: end
                ? Text(t('End of slide show. Click to exit.'), style: const TextStyle(color: Colors.white70, fontSize: 18))
                : SlideView(
                    doc: widget.doc,
                    shapes: page!['shapes'] as List<dynamic>,
                    width: w,
                    height: h,
                    background: hexColor(page['background']),
                  ),
          ),
        ),
      ),
    );
  }
}
