// Platform directories without plugins.

import 'dart:convert';
import 'dart:io';

String _join(String a, String b) => '$a${Platform.pathSeparator}$b';

/// The user's home directory (desktop) or the app's sandbox (mobile).
String homeDirectory() {
  if (Platform.isAndroid || Platform.isIOS) {
    // The system temp directory lives inside the app sandbox:
    //   Android: /data/user/0/<package>/cache   iOS: <sandbox>/tmp
    return Directory.systemTemp.parent.path;
  }
  final env = Platform.environment;
  return env['HOME'] ?? env['USERPROFILE'] ?? Directory.current.path;
}

/// Where documents are opened from and saved to by default.
String documentsDirectory() {
  final home = homeDirectory();
  final String dir;
  if (Platform.isAndroid) {
    dir = _join(_join(home, 'files'), 'Documents');
  } else {
    dir = _join(home, 'Documents');
  }
  final d = Directory(dir);
  if (!d.existsSync()) {
    try {
      d.createSync(recursive: true);
    } catch (_) {
      return home;
    }
  }
  return dir;
}

/// Overrides [settingsDirectory] (used by tests so they never touch the
/// user's real settings).
String? settingsDirectoryOverride;

/// Per-user application settings directory.
String settingsDirectory() {
  if (settingsDirectoryOverride != null) return settingsDirectoryOverride!;
  final env = Platform.environment;
  final home = homeDirectory();
  String dir;
  if (Platform.isWindows) {
    dir = _join(env['APPDATA'] ?? home, 'Akshara');
  } else if (Platform.isMacOS) {
    dir = _join(_join(_join(home, 'Library'), 'Application Support'), 'Akshara');
  } else if (Platform.isLinux) {
    dir = _join(env['XDG_CONFIG_HOME'] ?? _join(home, '.config'), 'akshara');
  } else {
    dir = _join(home, Platform.isAndroid ? 'files' : 'Library');
  }
  try {
    Directory(dir).createSync(recursive: true);
  } catch (_) {}
  return dir;
}

/// Small persistent settings store (a JSON file).
class Settings {
  Settings._(this._file, this._data);

  final File _file;
  final Map<String, dynamic> _data;

  static Settings load() {
    final file = File(_join(settingsDirectory(), 'settings.json'));
    Map<String, dynamic> data = {};
    try {
      if (file.existsSync() && file.lengthSync() < 1 << 20) {
        final decoded = jsonDecode(file.readAsStringSync());
        if (decoded is Map<String, dynamic>) data = decoded;
      }
    } catch (_) {
      // A corrupt settings file is simply ignored.
    }
    return Settings._(file, data);
  }

  List<String> get recentFiles =>
      (_data['recent'] as List?)?.whereType<String>().toList() ?? <String>[];

  void addRecent(String path) {
    final list = recentFiles..remove(path);
    list.insert(0, path);
    _data['recent'] = list.take(12).toList();
    _save();
  }

  void removeRecent(String path) {
    _data['recent'] = recentFiles..remove(path);
    _save();
  }

  /// Interface language code, or null to follow the system.
  String? get language => _data['language'] as String?;

  set language(String? value) {
    _data['language'] = value;
    _save();
  }

  bool get translitEnabled => _data['translit'] == true;

  set translitEnabled(bool value) {
    _data['translit'] = value;
    _save();
  }

  String? get translitScript => _data['translit_script'] as String?;

  set translitScript(String? value) {
    _data['translit_script'] = value;
    _save();
  }

  bool get translitNativeDigits => _data['translit_native_digits'] == true;

  set translitNativeDigits(bool value) {
    _data['translit_native_digits'] = value;
    _save();
  }

  String get themeMode => (_data['theme'] as String?) ?? 'system';

  set themeMode(String value) {
    _data['theme'] = value;
    _save();
  }

  void _save() {
    try {
      final tmp = File('${_file.path}.tmp');
      tmp.writeAsStringSync(jsonEncode(_data));
      tmp.renameSync(_file.path);
    } catch (_) {}
  }
}
