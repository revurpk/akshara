// Settings for voice input and the AI chat, kept in settings.json.

import 'package:material_ui/material_ui.dart';

import '../core/paths.dart';
import '../l10n/l10n.dart';

/// Voice input languages: Whisper codes, in menu order.
const voiceLanguages = <String>['en', 'te', 'hi', 'sa'];

/// The label for a voice language code ('auto' detects among them).
String voiceLanguageName(String code) => switch (code) {
      'en' => t('English'),
      'te' => t('Telugu'),
      'hi' => t('Hindi'),
      'sa' => t('Sanskrit'),
      _ => t('Detect language'),
    };

/// A two-letter badge for the toolbar.
String voiceLanguageBadge(String code) => switch (code) {
      'en' => 'EN',
      'te' => 'తె',
      'hi' => 'हि',
      'sa' => 'सं',
      _ => 'A',
    };

/// The local Ollama address used unless the user changes it.
const defaultOllamaUrl = 'http://127.0.0.1:11434';

class AssistSettings extends ChangeNotifier {
  AssistSettings._();

  static final AssistSettings instance = AssistSettings._();

  Settings? _store;

  void load(Settings store) {
    _store = store;
    notifyListeners();
  }

  String? _str(String key) {
    final v = _store?.value(key);
    return v is String && v.isNotEmpty ? v : null;
  }

  void _set(String key, Object? value) {
    _store?.setValue(key, value);
    notifyListeners();
  }

  /// Path of the whisper.cpp model file, or null if none was chosen.
  String? get voiceModel => _str('voice_model');
  set voiceModel(String? path) => _set('voice_model', path);

  /// Which recogniser the microphone uses: 'whisper' (built in, the
  /// default) or 'system' (the operating system's on-device recogniser,
  /// see system_speech.dart).
  String get voiceEngine => _str('voice_engine') == 'system' ? 'system' : 'whisper';
  set voiceEngine(String engine) => _set('voice_engine', engine == 'system' ? 'system' : null);

  /// 'auto' or one of [voiceLanguages].
  String get voiceLanguage {
    final v = _str('voice_language');
    return v != null && (v == 'auto' || voiceLanguages.contains(v)) ? v : 'auto';
  }

  set voiceLanguage(String code) => _set('voice_language', code);

  /// Whether the AI chat (which talks to Ollama on this computer) is on.
  bool get aiEnabled => _store?.value('ai_enabled') == true;
  set aiEnabled(bool on) => _set('ai_enabled', on);

  String get aiUrl => _str('ai_url') ?? defaultOllamaUrl;
  set aiUrl(String url) => _set('ai_url', url == defaultOllamaUrl ? null : url);

  String? get aiModel => _str('ai_model');
  set aiModel(String? name) => _set('ai_model', name);
}
