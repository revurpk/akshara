// Built-in Indic transliteration, powered by teltools (Pradyumna Revur,
// Apache-2.0) in the Rust core (crates/indic, third_party/teltools).
//
// While transliteration is on, Roman letters typed in a text field turn into
// the chosen Indic script as you type, using the teltools RTS / ITRANS-style
// scheme (`namastE` -> నమస్తే / नमस्ते / நமஸ்தே ...).

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import 'native.dart';
import 'paths.dart';

/// A script transliteration can produce.
class IndicScript {
  const IndicScript(this.code, this.name, this.native, this.sample);

  /// ISO 15924 code used by the core (`deva`, `telu`, ...).
  final String code;
  final String name;
  final String native;

  /// One letter shown on the toolbar button.
  final String sample;
}

const indicScripts = <IndicScript>[
  IndicScript('deva', 'Devanagari', 'देवनागरी', 'अ'),
  IndicScript('telu', 'Telugu', 'తెలుగు', 'అ'),
  IndicScript('taml', 'Tamil', 'தமிழ்', 'அ'),
  IndicScript('knda', 'Kannada', 'ಕನ್ನಡ', 'ಅ'),
  IndicScript('mlym', 'Malayalam', 'മലയാളം', 'അ'),
  IndicScript('orya', 'Odia', 'ଓଡ଼ିଆ', 'ଅ'),
  IndicScript('beng', 'Bengali', 'বাংলা', 'অ'),
];

IndicScript scriptByCode(String code) =>
    indicScripts.firstWhere((s) => s.code == code, orElse: () => indicScripts.first);

/// The script that fits an interface language (Devanagari for English).
String defaultScriptFor(AppLanguage language) => switch (language) {
      AppLanguage.te => 'telu',
      AppLanguage.ta => 'taml',
      AppLanguage.kn => 'knda',
      AppLanguage.ml => 'mlym',
      AppLanguage.or => 'orya',
      AppLanguage.bn => 'beng',
      _ => 'deva',
    };

/// Global on/off state, chosen script and digit style (persisted).
class TranslitSettings extends ChangeNotifier {
  TranslitSettings._();

  static final TranslitSettings instance = TranslitSettings._();

  Settings? _store;
  bool _enabled = false;
  String _script = 'deva';
  bool _nativeDigits = false;

  bool get enabled => _enabled;

  /// The chosen script (also remembered while transliteration is off).
  String get script => _script;

  /// The active script, or null when transliteration is off.
  String? get activeScript => _enabled ? _script : null;

  bool get nativeDigits => _nativeDigits;

  void load(Settings store, AppLanguage language) {
    _store = store;
    _enabled = store.translitEnabled;
    _script = store.translitScript ?? defaultScriptFor(language);
    _nativeDigits = store.translitNativeDigits;
    notifyListeners();
  }

  void setEnabled(bool value) {
    _enabled = value;
    _store?.translitEnabled = value;
    notifyListeners();
  }

  void toggle() => setEnabled(!_enabled);

  /// Selects a script and turns transliteration on.
  void use(String script) {
    _script = script;
    _store?.translitScript = script;
    setEnabled(true);
  }

  void setNativeDigits(bool value) {
    _nativeDigits = value;
    _store?.translitNativeDigits = value;
    notifyListeners();
  }
}

/// Runs a transliteration in the core. [mode] is `roman` (teltools typing
/// scheme), `script` (any Indic script to [script]), `to_iast` or
/// `from_iast`.
String transliterate(String text, {required String script, String mode = 'roman', bool? nativeDigits}) {
  return core({
    'op': 'indic.translit',
    'text': text,
    'script': script,
    'mode': mode,
    'native_digits': nativeDigits ?? TranslitSettings.instance.nativeDigits,
  }) as String;
}

/// Keys that start or extend a word while typing. Quotes are left out:
/// in the teltools scheme `'` and `"` are Vedic accents, which would
/// otherwise replace ordinary quotation marks.
bool isRomanKey(String ch) {
  if (ch.length != 1) return false;
  final c = ch.codeUnitAt(0);
  final alnum = (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
  return alnum || '~^.|\\_/'.contains(ch);
}

/// Whether [offset] lies inside a "double-quoted" part of [text]: formulas
/// and Math sources only transliterate quoted text.
bool insideQuotes(String text, int offset) {
  var n = 0;
  for (var i = 0; i < offset && i < text.length; i++) {
    if (text.codeUnitAt(i) == 0x22) n++;
  }
  return n.isOdd;
}

/// Turns Roman keystrokes into the active script as they are typed.
///
/// The formatter remembers the Roman letters of the word being typed and
/// re-transliterates the whole word on every key, so multi-letter sequences
/// (`k` + `h` = ఖ, `R^i` = ఋ) resolve correctly. Backspace removes the last
/// Roman letter; any other key, a caret move or a paste ends the word.
class TransliterationFormatter extends TextInputFormatter {
  TransliterationFormatter({this.allowAt});

  /// Optional filter: transliterate only where this returns true for the
  /// text before the edit and the caret position (e.g. inside quotes).
  final bool Function(String text, int offset)? allowAt;

  String _roman = '';
  int _start = 0;
  int _length = 0;

  void reset() {
    _roman = '';
    _length = 0;
  }

  @override
  TextEditingValue formatEditUpdate(TextEditingValue oldValue, TextEditingValue newValue) {
    final script = TranslitSettings.instance.activeScript;
    if (script == null) {
      reset();
      return newValue;
    }
    final oldText = oldValue.text, newText = newValue.text;
    // An empty, never-focused field reports no caret (offset -1): treat it
    // as being at the end, or the first keystroke would be missed.
    final o = oldValue.selection.isValid ? oldValue.selection : TextSelection.collapsed(offset: oldText.length);
    final n = newValue.selection;
    if (o.isValid && n.isValid && o.isCollapsed && n.isCollapsed) {
      final p = o.baseOffset;
      // One character typed at the caret.
      if (newText.length == oldText.length + 1 &&
          n.baseOffset == p + 1 &&
          newText.startsWith(oldText.substring(0, p)) &&
          newText.endsWith(oldText.substring(p))) {
        final ch = newText[p];
        if (!isRomanKey(ch) || (allowAt != null && !allowAt!(oldText, p))) {
          reset();
          return newValue;
        }
        if (_roman.isEmpty || p != _start + _length) {
          _roman = '';
          _start = p;
          _length = 0;
        }
        _roman += ch;
        return _compose(oldText, script);
      }
      // Backspace at the end of the word being typed.
      if (_roman.isNotEmpty &&
          p == _start + _length &&
          newText.length == oldText.length - 1 &&
          n.baseOffset == p - 1 &&
          p <= oldText.length) {
        _roman = _roman.substring(0, _roman.length - 1);
        if (_roman.isEmpty) {
          final text = oldText.replaceRange(_start, _start + _length, '');
          final caret = _start;
          reset();
          return TextEditingValue(text: text, selection: TextSelection.collapsed(offset: caret));
        }
        return _compose(oldText, script);
      }
    }
    reset();
    return newValue;
  }

  TextEditingValue _compose(String oldText, String script) {
    String out;
    try {
      out = transliterate(_roman, script: script);
    } catch (_) {
      out = _roman; // never lose what was typed
    }
    final end = (_start + _length).clamp(0, oldText.length);
    final text = oldText.replaceRange(_start.clamp(0, end), end, out);
    _length = out.length;
    return TextEditingValue(text: text, selection: TextSelection.collapsed(offset: _start + _length));
  }
}

/// Title-bar control: on/off, script choice, digits, typing guide.
class TranslitButton extends StatelessWidget {
  const TranslitButton({super.key});

  @override
  Widget build(BuildContext context) {
    final settings = TranslitSettings.instance;
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) {
        final theme = Theme.of(context);
        final on = settings.enabled;
        final current = scriptByCode(settings.script);
        return PopupMenuButton<String>(
          tooltip: '${t('Transliteration')} (Ctrl+Shift+K)',
          itemBuilder: (_) => [
            CheckedPopupMenuItem(value: 'off', checked: !on, child: Text(t('Off'))),
            const PopupMenuDivider(),
            for (final s in indicScripts)
              CheckedPopupMenuItem(
                value: s.code,
                checked: on && s.code == settings.script,
                child: Text('${s.native}  ·  ${s.name}'),
              ),
            const PopupMenuDivider(),
            CheckedPopupMenuItem(value: 'digits', checked: settings.nativeDigits, child: Text(t('Native digits'))),
            PopupMenuItem(value: 'guide', child: ListTile(leading: const Icon(Icons.help_outline), title: Text(t('Typing guide')))),
          ],
          onSelected: (v) {
            switch (v) {
              case 'off':
                settings.setEnabled(false);
              case 'digits':
                settings.setNativeDigits(!settings.nativeDigits);
              case 'guide':
                showTranslitGuide(context);
              default:
                settings.use(v);
            }
          },
          child: Container(
            margin: const EdgeInsets.symmetric(horizontal: 4, vertical: 8),
            padding: const EdgeInsets.symmetric(horizontal: 8),
            decoration: BoxDecoration(
              color: on ? theme.colorScheme.primaryContainer : null,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(color: on ? theme.colorScheme.primary : theme.dividerColor),
            ),
            alignment: Alignment.center,
            child: Row(mainAxisSize: MainAxisSize.min, children: [
              Icon(Icons.keyboard_outlined, size: 18, color: on ? theme.colorScheme.primary : null),
              const SizedBox(width: 4),
              Text(on ? 'a→${current.sample}' : 'abc', style: theme.textTheme.labelLarge),
            ]),
          ),
        );
      },
    );
  }
}

/// Key groups shown in the typing guide (teltools scheme).
const _guide = <String, List<String>>{
  'Vowels': ['a', 'A', 'i', 'I', 'u', 'U', 'R^i', 'R^I', 'e', 'E', 'ai', 'o', 'O', 'au', 'aM', 'aH', 'a.N'],
  'Consonants': [
    'ka', 'kha', 'ga', 'gha', '~Na', 'cha', 'chha', 'ja', 'jha', '~na', 'Ta', 'Tha', 'Da', 'Dha', 'Na', //
    'ta', 'tha', 'da', 'dha', 'na', 'pa', 'pha', 'ba', 'bha', 'ma', 'ya', 'ra', 'la', 'va', 'sha', 'Sha', //
    'sa', 'ha', 'La', 'Ra', '.La', 'N.a', 'kSha', 'j~na',
  ],
  'Signs and punctuation': ['k', 'ki', 'kya', 'kra', 'k.h', 'shrI', '\\om', '|', '||', '.a', '0123'],
  'Examples': ['namastE', 'satyamEva jayatE', 'dharmakShEtrE kurukShEtrE', 'vaNakkam', 'kR^iShNa', 'saMskR^itam', 'bhArata'],
};

Future<void> showTranslitGuide(BuildContext context) {
  return showDialog<void>(context: context, builder: (_) => const _GuideDialog());
}

class _GuideDialog extends StatefulWidget {
  const _GuideDialog();

  @override
  State<_GuideDialog> createState() => _GuideDialogState();
}

class _GuideDialogState extends State<_GuideDialog> {
  late String _script = TranslitSettings.instance.script;
  final _tryIt = TextEditingController(text: 'namastE');

  @override
  void dispose() {
    _tryIt.dispose();
    super.dispose();
  }

  String _out(String roman) {
    try {
      return transliterate(roman, script: _script);
    } catch (_) {
      return '';
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final mono = theme.textTheme.bodyMedium?.copyWith(fontFamily: 'NotoSansMono');
    return Dialog(
      insetPadding: const EdgeInsets.all(16),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760, maxHeight: 720),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Row(children: [
              Expanded(child: Text(t('Transliteration typing guide'), style: theme.textTheme.titleLarge)),
              DropdownButton<String>(
                value: _script,
                items: [for (final s in indicScripts) DropdownMenuItem(value: s.code, child: Text(s.native))],
                onChanged: (v) => setState(() => _script = v ?? _script),
              ),
            ]),
            const SizedBox(height: 8),
            Text(t('Type Roman letters and they turn into the chosen script as you type. Case matters (t త, T ట). Space or punctuation ends a word; Backspace removes the last letter typed. Turn it on or off with Ctrl+Shift+K.')),
            const SizedBox(height: 12),
            TextField(
              controller: _tryIt,
              style: mono,
              decoration: InputDecoration(labelText: t('Try it'), isDense: true, border: const OutlineInputBorder()),
              onChanged: (_) => setState(() {}),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: SelectableText(_out(_tryIt.text), style: theme.textTheme.headlineSmall),
            ),
            const Divider(),
            Expanded(
              child: ListView(children: [
                for (final group in _guide.entries) ...[
                  Padding(
                    padding: const EdgeInsets.only(top: 12, bottom: 6),
                    child: Text(t(group.key), style: theme.textTheme.titleSmall),
                  ),
                  Wrap(spacing: 6, runSpacing: 6, children: [
                    for (final key in group.value)
                      Container(
                        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                        decoration: BoxDecoration(
                          border: Border.all(color: theme.dividerColor),
                          borderRadius: BorderRadius.circular(6),
                        ),
                        child: Row(mainAxisSize: MainAxisSize.min, children: [
                          Text(key, style: mono),
                          const Text('  →  '),
                          Text(_out(key), style: theme.textTheme.titleMedium),
                        ]),
                      ),
                  ]),
                ],
              ]),
            ),
            const SizedBox(height: 8),
            Text(
              t('Based on teltools by Pradyumna Revur (Apache License 2.0).'),
              style: theme.textTheme.bodySmall,
            ),
            Align(
              alignment: Alignment.centerRight,
              child: FilledButton(onPressed: () => Navigator.pop(context), child: Text(t('Close'))),
            ),
          ]),
        ),
      ),
    );
  }
}

/// Builds the "Convert text" menu entries shared by Writer and Calc.
/// Each value is `mode:script` for [transliterate].
List<PopupMenuEntry<String>> convertMenuItems() {
  final current = scriptByCode(TranslitSettings.instance.script);
  return [
    PopupMenuItem(value: 'roman:${current.code}', child: Text(t('Roman letters → {script}', {'script': current.native}))),
    PopupMenuItem(value: 'from_iast:${current.code}', child: Text(t('IAST → {script}', {'script': current.native}))),
    PopupMenuItem(value: 'to_iast:deva', child: Text(t('Romanize (IAST)'))),
    const PopupMenuDivider(),
    for (final s in indicScripts)
      PopupMenuItem(value: 'script:${s.code}', child: Text(t('Convert script to {script}', {'script': s.native}))),
  ];
}

/// Keyboard shortcut for toggling transliteration.
const translitShortcut = SingleActivator(LogicalKeyboardKey.keyK, control: true, shift: true);
const translitShortcutMac = SingleActivator(LogicalKeyboardKey.keyK, meta: true, shift: true);
