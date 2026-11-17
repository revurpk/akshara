// One editable paragraph: a text field that paints attributed runs.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import '../core/translit.dart';
import 'writer_page.dart';

Color? _parseColor(Object? hex) {
  if (hex is! String || hex.length != 7) return null;
  final v = int.tryParse(hex.substring(1), radix: 16);
  return v == null ? null : Color(0xFF000000 | v);
}

/// Text controller that renders the paragraph's runs with their attributes.
class ParaController extends TextEditingController {
  List<Map<String, dynamic>> _runs = const [];

  /// The paragraph text as the core last reported it.
  String get coreText => _runs.map((r) => r['text'] as String).join();

  /// Replaces text and runs from core JSON (keeps the selection if possible).
  void setParagraph(Map<String, dynamic> para) {
    final runs = (para['runs'] as List<dynamic>).cast<Map<String, dynamic>>();
    final newText = runs.map((r) => r['text'] as String).join();
    _runs = runs;
    if (newText != text) {
      final sel = selection;
      value = TextEditingValue(
        text: newText,
        selection: sel.isValid && sel.end <= newText.length
            ? sel
            : TextSelection.collapsed(offset: newText.length),
      );
    } else {
      notifyListeners();
    }
  }

  /// Updates only the formatting (text already matches).
  void setRuns(Map<String, dynamic> para) {
    final runs = (para['runs'] as List<dynamic>).cast<Map<String, dynamic>>();
    if (runs.map((r) => r['text'] as String).join() == text) {
      _runs = runs;
      notifyListeners();
    } else {
      setParagraph(para);
    }
  }

  /// Attributes shared by all runs overlapping `start..end` (or of the
  /// character before `start` for a collapsed selection).
  Map<String, dynamic> attrsAt(int start, int end) {
    if (_runs.isEmpty) return const {};
    var pos = 0;
    Map<String, dynamic>? common;
    for (final run in _runs) {
      final len = (run['text'] as String).length;
      final runStart = pos, runEnd = pos + len;
      pos = runEnd;
      final overlaps = start == end
          ? (runStart < start && start <= runEnd) || (start == 0 && runStart == 0)
          : runStart < end && runEnd > start;
      if (!overlaps) continue;
      if (common == null) {
        common = Map.of(run)..remove('text');
      } else {
        common.removeWhere((k, v) => run[k] != v);
      }
      if (start == end) break;
    }
    return common ?? (Map.of(_runs.first)..remove('text'));
  }

  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    final joined = _runs.map((r) => r['text'] as String).join();
    if (joined != text || _runs.isEmpty || (withComposing && value.isComposingRangeValid)) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    final base = style ?? const TextStyle();
    final linkColor = Theme.of(context).colorScheme.primary;
    return TextSpan(
      style: base,
      children: [
        for (final run in _runs)
          if ((run['text'] as String).isNotEmpty) TextSpan(text: run['text'] as String, style: _runStyle(run, base, linkColor)),
      ],
    );
  }

  TextStyle _runStyle(Map<String, dynamic> run, TextStyle base, Color linkColor) {
    final decorations = <TextDecoration>[
      if (run['underline'] == true || run['link'] != null) TextDecoration.underline,
      if (run['strike'] == true) TextDecoration.lineThrough,
    ];
    final baseSize = base.fontSize ?? 16;
    final size = run['size'] is num ? (run['size'] as num).toDouble() * (baseSize / 16) * 1.3333 : baseSize;
    final script = run['superscript'] == true || run['subscript'] == true;
    return TextStyle(
      fontWeight: run['bold'] == true ? FontWeight.bold : null,
      fontStyle: run['italic'] == true ? FontStyle.italic : null,
      decoration: decorations.isEmpty ? null : TextDecoration.combine(decorations),
      fontFamily: run['code'] == true ? monoFont : null,
      color: _parseColor(run['color']) ?? (run['link'] != null ? linkColor : null),
      backgroundColor: _parseColor(run['highlight']),
      fontSize: script ? size * 0.65 : size,
      fontFeatures: run['superscript'] == true
          ? const [FontFeature.superscripts()]
          : run['subscript'] == true
              ? const [FontFeature.subscripts()]
              : null,
    );
  }
}

class ParaField extends StatefulWidget {
  const ParaField({
    super.key,
    required this.paraRef,
    required this.controller,
    required this.focusNode,
    required this.kind,
    required this.level,
    required this.align,
    required this.label,
    required this.scale,
    required this.page,
  });

  final ParaRef paraRef;
  final ParaController controller;
  final FocusNode focusNode;
  final String kind;
  final int level;
  final String align;
  final String? label;
  final double scale;
  final WriterPageState page;

  @override
  State<ParaField> createState() => _ParaFieldState();
}

class _ParaFieldState extends State<ParaField> {
  final _translit = TransliterationFormatter();

  @override
  void initState() {
    super.initState();
    widget.focusNode.addListener(_onFocus);
  }

  @override
  void didUpdateWidget(ParaField old) {
    super.didUpdateWidget(old);
    if (old.focusNode != widget.focusNode) {
      old.focusNode.removeListener(_onFocus);
      widget.focusNode.addListener(_onFocus);
    }
  }

  @override
  void dispose() {
    widget.focusNode.removeListener(_onFocus);
    super.dispose();
  }

  void _onFocus() {
    if (widget.focusNode.hasFocus) widget.page.onFocused(widget.paraRef);
  }

  void _onChanged(String text) {
    final old = widget.controller.coreText;
    final sel = widget.controller.selection;
    // A newline typed without Shift (soft keyboards send no key events)
    // splits the paragraph; Shift+Enter keeps it as a line break.
    if (text.length == old.length + 1 &&
        sel.isCollapsed &&
        sel.baseOffset > 0 &&
        text[sel.baseOffset - 1] == '\n' &&
        !HardwareKeyboard.instance.isShiftPressed &&
        text.substring(0, sel.baseOffset - 1) + text.substring(sel.baseOffset) == old) {
      widget.controller.value = TextEditingValue(
        text: old,
        selection: TextSelection.collapsed(offset: sel.baseOffset - 1),
      );
      widget.page.onSplit(widget.paraRef, sel.baseOffset - 1, sel.baseOffset - 1);
      return;
    }
    widget.page.onTextChanged(widget.paraRef, text);
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) return KeyEventResult.ignored;
    final keys = HardwareKeyboard.instance;
    final ctrl = keys.isControlPressed || keys.isMetaPressed;
    final shift = keys.isShiftPressed;
    final c = widget.controller;
    final sel = c.selection;
    final key = event.logicalKey;
    if (ctrl) {
      if (key == LogicalKeyboardKey.keyB) {
        widget.page.toggleAttr('bold');
      } else if (key == LogicalKeyboardKey.keyI) {
        widget.page.toggleAttr('italic');
      } else if (key == LogicalKeyboardKey.keyU) {
        widget.page.toggleAttr('underline');
      } else if (key == LogicalKeyboardKey.keyZ && !shift) {
        widget.page.undo();
      } else if (key == LogicalKeyboardKey.keyY || (key == LogicalKeyboardKey.keyZ && shift)) {
        widget.page.redo();
      } else {
        return KeyEventResult.ignored;
      }
      return KeyEventResult.handled;
    }
    if ((key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.numpadEnter) && !shift) {
      final start = sel.isValid ? sel.start : c.text.length;
      final end = sel.isValid ? sel.end : c.text.length;
      widget.page.onSplit(widget.paraRef, start, end);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.backspace && sel.isCollapsed && sel.baseOffset == 0) {
      widget.page.onBackspaceAtStart(widget.paraRef);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.tab) {
      widget.page.onIndent(widget.paraRef, shift ? -1 : 1);
      return KeyEventResult.handled;
    }
    final atStart = sel.isCollapsed && sel.baseOffset == 0;
    final atEnd = sel.isCollapsed && sel.baseOffset == c.text.length;
    if (!shift && atStart && (key == LogicalKeyboardKey.arrowLeft || key == LogicalKeyboardKey.arrowUp)) {
      widget.page.onNavigate(widget.paraRef, false);
      return KeyEventResult.handled;
    }
    if (!shift && atEnd && (key == LogicalKeyboardKey.arrowRight || key == LogicalKeyboardKey.arrowDown)) {
      widget.page.onNavigate(widget.paraRef, true);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  TextStyle _style(ThemeData theme) {
    final s = widget.scale;
    final body = 12.0 * s;
    switch (widget.kind) {
      case 'title':
        return TextStyle(fontSize: 28 * s, fontWeight: FontWeight.bold, height: 1.3);
      case 'heading':
        const sizes = [20.0, 17.0, 14.5, 13.0, 12.0, 12.0];
        return TextStyle(
          fontSize: sizes[(widget.level - 1).clamp(0, 5)] * s,
          fontWeight: FontWeight.bold,
          fontStyle: widget.level >= 5 ? FontStyle.italic : null,
          height: 1.3,
        );
      case 'quote':
        return TextStyle(fontSize: body, fontStyle: FontStyle.italic, height: 1.4);
      case 'code':
        return TextStyle(
          fontSize: 10 * s,
          fontFamily: monoFont,
          fontFamilyFallback: docFallback,
          height: 1.3,
        );
      default:
        return TextStyle(fontSize: body, height: 1.4);
    }
  }

  TextAlign get _textAlign => switch (widget.align) {
        'center' => TextAlign.center,
        'right' => TextAlign.right,
        'justify' => TextAlign.justify,
        _ => TextAlign.left,
      };

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final base = _style(theme);
    final style = base.copyWith(
      color: theme.colorScheme.onSurface,
      fontFamily: base.fontFamily ?? docFont,
      fontFamilyFallback: docFallback,
    );
    final isHeading = widget.kind == 'heading' || widget.kind == 'title';
    Widget field = Focus(
      canRequestFocus: false,
      skipTraversal: true,
      onKeyEvent: _onKey,
      child: TextField(
        controller: widget.controller,
        focusNode: widget.focusNode,
        maxLines: null,
        keyboardType: TextInputType.multiline,
        inputFormatters: [_translit],
        // Phonetic typing needs the raw letters, not autocorrected words.
        autocorrect: !TranslitSettings.instance.enabled,
        enableSuggestions: !TranslitSettings.instance.enabled,
        textAlign: _textAlign,
        style: style,
        decoration: const InputDecoration.collapsed(hintText: null),
        onChanged: _onChanged,
        contextMenuBuilder: (context, state) => AdaptiveTextSelectionToolbar.editableText(editableTextState: state),
      ),
    );
    final indent = 18.0 * widget.scale * (widget.level + 1);
    if (widget.label != null) {
      field = Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: indent,
            child: Padding(
              padding: EdgeInsets.only(right: 6 * widget.scale),
              child: Text(widget.label!, textAlign: TextAlign.right, style: style),
            ),
          ),
          Expanded(child: field),
        ],
      );
    } else if (widget.kind == 'quote') {
      field = Container(
        margin: EdgeInsets.symmetric(horizontal: 18 * widget.scale),
        padding: EdgeInsets.only(left: 10 * widget.scale),
        decoration: BoxDecoration(border: Border(left: BorderSide(color: theme.colorScheme.outline, width: 3))),
        child: field,
      );
    } else if (widget.kind == 'code') {
      field = Container(
        color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.6),
        padding: EdgeInsets.symmetric(horizontal: 6 * widget.scale, vertical: 2),
        child: field,
      );
    }
    return Padding(
      padding: EdgeInsets.only(
        top: isHeading ? 10 * widget.scale : 0,
        bottom: (widget.kind == 'code' ? 0 : (isHeading ? 4 : 5)) * widget.scale,
      ),
      child: field,
    );
  }
}
