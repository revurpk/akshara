// The operating system's own speech recogniser, as an alternative to the
// built-in Whisper engine. The platform code lives in each runner (no
// plugins) and answers on the 'akshara/speech' method channel:
//
// * macOS and iOS: SFSpeechRecognizer with requiresOnDeviceRecognition, so
//   audio never leaves the device (MainFlutterWindow.swift, AppDelegate.swift).
// * Android 12 and later: SpeechRecognizer.createOnDeviceSpeechRecognizer
//   (MainActivity.kt).
// * Windows: the SAPI desktop recogniser, which is always offline
//   (windows/runner/system_speech.cpp).
// * Linux has no system recogniser; the channel is missing and the option
//   is not offered.
//
// Recognisers that might send audio to a server are never used: every
// platform asks for on-device recognition only and reports itself
// unavailable otherwise.
//
// Methods: `available` → {available, languages, reason?}; `start`
// {language} → null; `stop`; `cancel`; `status` → the same shapes as the
// core's voice.status ({state: recording, level, seconds} | {state:
// transcribing} | {state: done, text} | {state: error, error}).

import 'package:flutter/services.dart';

/// What the system recogniser can do on this device.
class SystemSpeechInfo {
  const SystemSpeechInfo({required this.available, this.languages = const [], this.reason});

  static const none = SystemSpeechInfo(available: false);

  final bool available;

  /// The voice languages ('en', 'te', 'hi', 'sa') it recognises offline.
  final List<String> languages;

  /// Why it is unavailable (for example, permission denied), if known.
  final String? reason;
}

class SystemSpeech {
  SystemSpeech._();

  static const channel = MethodChannel('akshara/speech');

  static Future<SystemSpeechInfo>? _info;

  /// Probes the platform once; [refresh] probes again (after a permission
  /// change, for instance).
  static Future<SystemSpeechInfo> info({bool refresh = false}) {
    if (refresh) _info = null;
    return _info ??= _probe();
  }

  static Future<SystemSpeechInfo> _probe() async {
    try {
      final r = await channel.invokeMapMethod<String, dynamic>('available');
      if (r == null) return SystemSpeechInfo.none;
      return SystemSpeechInfo(
        available: r['available'] == true,
        languages: [for (final l in (r['languages'] as List<dynamic>? ?? const [])) '$l'],
        reason: r['reason'] as String?,
      );
    } on MissingPluginException {
      return SystemSpeechInfo.none;
    } on PlatformException catch (e) {
      return SystemSpeechInfo(available: false, reason: e.message);
    }
  }

  /// Starts listening in [language] ('auto' uses the system's language).
  /// Asks for the microphone and speech permissions the first time.
  static Future<void> start(String language) => channel.invokeMethod<void>('start', {'language': language});

  /// Stops listening; the text arrives through [status].
  static Future<void> stop() => channel.invokeMethod<void>('stop');

  static Future<void> cancel() => channel.invokeMethod<void>('cancel');

  static Future<Map<String, dynamic>> status() async =>
      await channel.invokeMapMethod<String, dynamic>('status') ?? const {'state': 'error', 'error': 'no status'};
}
