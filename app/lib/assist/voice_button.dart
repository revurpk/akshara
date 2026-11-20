// The microphone button: click to dictate, click again to stop. The speech
// is recognised on this computer, by the core's Whisper engine or (when
// chosen in the settings) by the operating system's on-device recogniser,
// and the text is handed to [VoiceButton.onText].

import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart' show PlatformException;
import 'package:material_ui/material_ui.dart';

import '../core/file_dialog.dart';
import '../core/native.dart';
import '../core/paths.dart';
import '../l10n/l10n.dart';
import 'assist_settings.dart';
import 'system_speech.dart';

enum _State { idle, recording, transcribing }

class VoiceButton extends StatefulWidget {
  const VoiceButton({super.key, required this.onText, this.showLanguage = true});

  /// Receives the recognised text (never empty).
  final void Function(String text) onText;

  /// Whether to show the language picker next to the microphone.
  final bool showLanguage;

  @override
  State<VoiceButton> createState() => _VoiceButtonState();
}

class _VoiceButtonState extends State<VoiceButton> {
  _State _state = _State.idle;
  int? _session;
  // True while the operating system's recogniser runs the session.
  bool _system = false;
  bool _polling = false;
  Timer? _poll;
  double _level = 0;
  double _seconds = 0;

  @override
  void initState() {
    super.initState();
    AssistSettings.instance.addListener(_changed);
  }

  @override
  void dispose() {
    AssistSettings.instance.removeListener(_changed);
    _poll?.cancel();
    if (_system) {
      SystemSpeech.cancel().catchError((_) {});
    }
    final s = _session;
    if (s != null) {
      try {
        core({'op': 'voice.cancel', 'session': s});
      } catch (_) {}
    }
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _message(String text) {
    if (mounted) ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
  }

  Future<void> _press() async {
    switch (_state) {
      case _State.idle:
        await _start();
      case _State.recording:
        _stop();
      case _State.transcribing:
        _cancel();
    }
  }

  Future<void> _start() async {
    if (AssistSettings.instance.voiceEngine == 'system') {
      final info = await SystemSpeech.info();
      if (!mounted) return;
      if (info.available) return _startSystem(info);
      _message(info.reason ?? t('System speech recognition is not available; using the built-in Whisper engine.'));
    }
    var model = AssistSettings.instance.voiceModel;
    if (model == null || !File(model).existsSync()) {
      model = await showVoiceModelSetup(context);
      if (model == null || !mounted) return;
    }
    final lang = AssistSettings.instance.voiceLanguage;
    try {
      final r = core({
        'op': 'voice.start',
        'model': model,
        'language': lang,
        'among': voiceLanguages,
      }) as Map<String, dynamic>;
      _session = r['session'] as int;
      setState(() {
        _state = _State.recording;
        _level = 0;
        _seconds = 0;
      });
      _poll = Timer.periodic(const Duration(milliseconds: 100), (_) => _tick());
    } on CoreError catch (e) {
      _message(e.message);
    }
  }

  Future<void> _startSystem(SystemSpeechInfo info) async {
    final lang = AssistSettings.instance.voiceLanguage;
    if (lang != 'auto' && !info.languages.contains(lang)) {
      _message(t('The system speech recogniser cannot recognise {language} offline on this device. Choose another language or the built-in Whisper engine.',
          {'language': voiceLanguageName(lang)}));
      return;
    }
    setState(() {
      _system = true;
      _state = _State.recording;
      _level = 0;
      _seconds = 0;
    });
    try {
      await SystemSpeech.start(lang);
    } on PlatformException catch (e) {
      _reset();
      _message(e.message ?? e.code);
      return;
    }
    if (!mounted || !_system) return;
    _poll = Timer.periodic(const Duration(milliseconds: 100), (_) => _tickSystem());
  }

  Future<void> _tickSystem() async {
    if (_polling || !_system) return;
    _polling = true;
    try {
      final r = await SystemSpeech.status();
      if (_system && mounted) _show(r);
    } on PlatformException catch (e) {
      _reset();
      _message(e.message ?? e.code);
    } finally {
      _polling = false;
    }
  }

  void _stop() {
    if (_system) {
      setState(() => _state = _State.transcribing);
      SystemSpeech.stop().catchError((Object e) {
        _reset();
        _message('$e');
      });
      return;
    }
    final s = _session;
    if (s == null) return;
    try {
      core({'op': 'voice.stop', 'session': s});
      setState(() => _state = _State.transcribing);
    } on CoreError catch (e) {
      _reset();
      _message(e.message);
    }
  }

  void _cancel() {
    if (_system) SystemSpeech.cancel().catchError((_) {});
    final s = _session;
    if (s != null) {
      try {
        core({'op': 'voice.cancel', 'session': s});
      } catch (_) {}
    }
    _reset();
  }

  void _reset() {
    _poll?.cancel();
    _poll = null;
    _session = null;
    _system = false;
    if (mounted) setState(() => _state = _State.idle);
  }

  void _tick() {
    final s = _session;
    if (s == null) return;
    Map<String, dynamic> r;
    try {
      r = core({'op': 'voice.status', 'session': s}) as Map<String, dynamic>;
    } on CoreError catch (e) {
      _reset();
      _message(e.message);
      return;
    }
    _show(r);
  }

  /// Shows a status report from either engine.
  void _show(Map<String, dynamic> r) {
    switch (r['state']) {
      case 'recording' when _system && _state == _State.transcribing:
        // Stopped while the system was still asking for permission.
        SystemSpeech.stop().catchError((_) {});
      case 'recording':
        setState(() {
          _level = (r['level'] as num).toDouble();
          _seconds = (r['seconds'] as num).toDouble();
        });
      case 'transcribing':
        if (_state != _State.transcribing) setState(() => _state = _State.transcribing);
      case 'done':
        _reset();
        final text = (r['text'] as String).trim();
        if (text.isEmpty) {
          _message(t('No speech was recognised.'));
        } else {
          widget.onText(text);
        }
      case 'error':
        _reset();
        _message('${r['error']}');
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final lang = AssistSettings.instance.voiceLanguage;
    final Widget icon = switch (_state) {
      _State.idle => const Icon(Icons.mic_none),
      _State.recording => Stack(
          alignment: Alignment.center,
          children: [
            SizedBox(
              width: 26,
              height: 26,
              child: CircularProgressIndicator(
                value: (_level * 3).clamp(0.05, 1.0),
                strokeWidth: 2.5,
                color: theme.colorScheme.error,
              ),
            ),
            Icon(Icons.mic, size: 18, color: theme.colorScheme.error),
          ],
        ),
      _State.transcribing => const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2)),
    };
    final tooltip = switch (_state) {
      _State.idle => t('Voice input ({language})', {'language': voiceLanguageName(lang)}),
      _State.recording => t('Listening… {s} s. Click to stop and insert the text.', {'s': _seconds.toStringAsFixed(0)}),
      _State.transcribing => t('Recognising speech… Click to cancel.'),
    };
    final button = IconButton(tooltip: tooltip, icon: icon, onPressed: _press);
    if (!widget.showLanguage) return button;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        button,
        PopupMenuButton<String>(
          tooltip: t('Voice input language'),
          enabled: _state == _State.idle,
          initialValue: lang,
          itemBuilder: (_) => [
            for (final code in ['auto', ...voiceLanguages])
              PopupMenuItem(value: code, child: Text(code == 'auto' ? t('Detect (English, Telugu, Hindi, Sanskrit)') : voiceLanguageName(code))),
            const PopupMenuDivider(),
            PopupMenuItem(value: 'settings', child: Text(t('Voice and AI settings…'))),
          ],
          onSelected: (v) {
            if (v == 'settings') {
              showAssistSettings(context);
            } else {
              AssistSettings.instance.voiceLanguage = v;
            }
          },
          child: Padding(
            padding: const EdgeInsets.only(right: 4),
            child: Text(voiceLanguageBadge(lang), style: theme.textTheme.labelSmall),
          ),
        ),
      ],
    );
  }
}

/// Explains where the speech model comes from and lets the user pick it.
/// Returns the chosen path (also saved in the settings) or null.
Future<String?> showVoiceModelSetup(BuildContext context) async {
  final choose = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(t('Voice input needs a speech model')),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText(t(
              'Akshara recognises speech on this computer; nothing is sent anywhere. It needs a Whisper model file in the whisper.cpp format (ggml-….bin), which you download once.',
            )),
            const SizedBox(height: 12),
            SelectableText(t(
              'For English, Telugu, Hindi and Sanskrit use a multilingual model, for example ggml-small-q5_1.bin (about 190 MB) or ggml-base.bin (about 150 MB, faster but less accurate), from huggingface.co/ggerganov/whisper.cpp.',
            )),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(t('Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(t('Choose model file…'))),
      ],
    ),
  );
  if (choose != true || !context.mounted) return null;
  return pickVoiceModel(context);
}

/// Picks and checks a model file; saves it in the settings when valid.
Future<String?> pickVoiceModel(BuildContext context) async {
  final sep = Platform.pathSeparator;
  final downloads = Directory('${homeDirectory()}${sep}Downloads');
  final path = await showDialog<String>(
    context: context,
    builder: (_) => FileDialog(
      title: t('Choose speech model'),
      startDir: downloads.existsSync() ? downloads.path : documentsDirectory(),
      extensions: const {'bin'},
    ),
  );
  if (path == null || !context.mounted) return null;
  try {
    final info = core({'op': 'voice.model_info', 'path': path}) as Map<String, dynamic>;
    if (info['multilingual'] != true && context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        content: Text(t('This model only understands English. Choose a model without ".en" in its name for Telugu, Hindi and Sanskrit.')),
      ));
    }
    AssistSettings.instance.voiceModel = path;
    return path;
  } on CoreError catch (e) {
    if (context.mounted) await showError(context, t('Choose speech model'), e.message);
    return null;
  }
}

/// Settings for voice input and the AI chat.
Future<void> showAssistSettings(BuildContext context) {
  return showDialog<void>(context: context, builder: (_) => const _AssistSettingsDialog());
}

class _AssistSettingsDialog extends StatefulWidget {
  const _AssistSettingsDialog();

  @override
  State<_AssistSettingsDialog> createState() => _AssistSettingsDialogState();
}

class _AssistSettingsDialogState extends State<_AssistSettingsDialog> {
  final s = AssistSettings.instance;
  late final _url = TextEditingController(text: s.aiUrl);
  List<String> _models = const [];
  String? _status;
  bool _checking = false;
  Map<String, dynamic>? _modelInfo;
  SystemSpeechInfo _system = SystemSpeechInfo.none;

  @override
  void initState() {
    super.initState();
    s.addListener(_changed);
    _probe();
    SystemSpeech.info().then((info) {
      if (mounted) setState(() => _system = info);
    });
    if (s.aiEnabled) _refresh();
  }

  @override
  void dispose() {
    s.removeListener(_changed);
    _url.dispose();
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _probe() {
    final m = s.voiceModel;
    if (m == null) {
      _modelInfo = null;
      return;
    }
    try {
      _modelInfo = core({'op': 'voice.model_info', 'path': m}) as Map<String, dynamic>;
    } on CoreError {
      _modelInfo = null;
    }
  }

  Future<void> _refresh() async {
    setState(() {
      _checking = true;
      _status = null;
    });
    try {
      final r = await coreAsync({'op': 'ai.models', 'url': _url.text.trim()}) as Map<String, dynamic>;
      final models = [for (final m in r['models'] as List<dynamic>) (m as Map<String, dynamic>)['name'] as String];
      s.aiUrl = r['url'] as String;
      if (!mounted) return;
      setState(() {
        _models = models;
        _status = models.isEmpty
            ? t('Ollama is running but has no models. Download one, for example: ollama pull llama3.2')
            : t('Connected: {n} models installed.', {'n': models.length});
      });
      if (models.isNotEmpty && (s.aiModel == null || !models.contains(s.aiModel))) s.aiModel = models.first;
    } on CoreError catch (e) {
      if (mounted) setState(() => _status = e.message);
    } finally {
      if (mounted) setState(() => _checking = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final info = _modelInfo;
    final model = s.voiceModel;
    final useSystem = _system.available && s.voiceEngine == 'system';
    return AlertDialog(
      title: Text(t('Voice and AI settings')),
      content: SizedBox(
        width: 520,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(t('Voice input'), style: theme.textTheme.titleMedium),
              const SizedBox(height: 4),
              if (_system.available) ...[
                Row(
                  children: [
                    Text(t('Speech engine')),
                    const SizedBox(width: 12),
                    Expanded(
                      child: DropdownButton<String>(
                        isExpanded: true,
                        value: s.voiceEngine,
                        items: [
                          DropdownMenuItem(value: 'whisper', child: Text(t('Whisper (built in)'))),
                          DropdownMenuItem(value: 'system', child: Text(t('System speech recognition'))),
                        ],
                        onChanged: (v) => s.voiceEngine = v ?? 'whisper',
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 4),
              ],
              if (useSystem)
                Text(
                  t('Speech is recognised on this device by the operating system, in on-device mode only; nothing is sent anywhere. Languages available offline: {languages}.', {
                    'languages': _system.languages.isEmpty ? t('the system language') : _system.languages.map(voiceLanguageName).join(', '),
                  }),
                  style: theme.textTheme.bodySmall,
                )
              else
                Text(t('Speech is recognised on this computer with a Whisper model file; nothing is sent anywhere.'), style: theme.textTheme.bodySmall),
              const SizedBox(height: 8),
              if (!useSystem) ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const Icon(Icons.graphic_eq),
                title: Text(model == null ? t('No speech model chosen') : baseName(model)),
                subtitle: info == null
                    ? (model == null ? null : Text(t('The model file cannot be read.'), style: TextStyle(color: theme.colorScheme.error)))
                    : Text([
                        '${info['size']}',
                        info['multilingual'] == true ? t('multilingual') : t('English only'),
                        if (info['quantized'] == true) t('compressed'),
                      ].join(' · ')),
                trailing: Wrap(spacing: 4, children: [
                  TextButton(
                    onPressed: () async {
                      await pickVoiceModel(context);
                      setState(_probe);
                    },
                    child: Text(t('Choose…')),
                  ),
                  if (model != null)
                    IconButton(
                      tooltip: t('Forget the model'),
                      icon: const Icon(Icons.close),
                      onPressed: () {
                        s.voiceModel = null;
                        setState(_probe);
                      },
                    ),
                ]),
              ),
              Row(
                children: [
                  Text(t('Language')),
                  const SizedBox(width: 12),
                  Expanded(
                    child: DropdownButton<String>(
                      isExpanded: true,
                      value: s.voiceLanguage,
                      items: [
                        for (final code in ['auto', ...voiceLanguages])
                          DropdownMenuItem(value: code, child: Text(code == 'auto' ? t('Detect (English, Telugu, Hindi, Sanskrit)') : voiceLanguageName(code))),
                      ],
                      onChanged: (v) => s.voiceLanguage = v ?? 'auto',
                    ),
                  ),
                ],
              ),
              const Divider(height: 32),
              Text(t('AI chat'), style: theme.textTheme.titleMedium),
              const SizedBox(height: 4),
              Text(
                t('Chat with an AI model running on this computer in Ollama (ollama.com) and turn the answers into documents. Akshara only connects to Ollama on this computer, never to the internet.'),
                style: theme.textTheme.bodySmall,
              ),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(t('Use Ollama on this computer')),
                value: s.aiEnabled,
                onChanged: (on) {
                  s.aiEnabled = on;
                  if (on) _refresh();
                },
              ),
              if (s.aiEnabled) ...[
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _url,
                        decoration: InputDecoration(labelText: t('Address'), helperText: t('Only addresses on this computer are allowed.')),
                        onSubmitted: (_) => _refresh(),
                      ),
                    ),
                    const SizedBox(width: 8),
                    _checking
                        ? const SizedBox(width: 24, height: 24, child: CircularProgressIndicator(strokeWidth: 2))
                        : IconButton(tooltip: t('Connect'), icon: const Icon(Icons.refresh), onPressed: _refresh),
                  ],
                ),
                if (_models.isNotEmpty)
                  Row(
                    children: [
                      Text(t('Model')),
                      const SizedBox(width: 12),
                      Expanded(
                        child: DropdownButton<String>(
                          isExpanded: true,
                          value: _models.contains(s.aiModel) ? s.aiModel : null,
                          items: [for (final m in _models) DropdownMenuItem(value: m, child: Text(m))],
                          onChanged: (v) => s.aiModel = v,
                        ),
                      ),
                    ],
                  ),
                if (_status != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_status!, style: theme.textTheme.bodySmall)),
              ],
            ],
          ),
        ),
      ),
      actions: [FilledButton(onPressed: () => Navigator.pop(context), child: Text(t('Close')))],
    );
  }
}
