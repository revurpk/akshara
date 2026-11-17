// A built-in, plugin-free file browser for opening and saving documents.
// It works the same on every platform (on mobile it browses the app's own
// documents folder).

import 'dart:io';

import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';

import 'paths.dart';

/// File types the suite can open, by extension.
const openableExtensions = <String>{
  'odt', 'ott', 'txt', 'ods', 'ots', 'csv', 'tsv', 'odf', 'otf', 'mml',
};

String extensionOf(String path) {
  final name = path.split(RegExp(r'[\\/]')).last;
  final dot = name.lastIndexOf('.');
  return dot <= 0 ? '' : name.substring(dot + 1).toLowerCase();
}

String baseName(String path) => path.split(RegExp(r'[\\/]')).last;

IconData iconForExtension(String ext) {
  switch (ext) {
    case 'odt':
    case 'ott':
    case 'txt':
    case 'html':
    case 'md':
      return Icons.description_outlined;
    case 'ods':
    case 'ots':
    case 'csv':
    case 'tsv':
      return Icons.grid_on_outlined;
    case 'odf':
    case 'otf':
    case 'mml':
      return Icons.functions;
    default:
      return Icons.insert_drive_file_outlined;
  }
}

/// Shows the open dialog; returns the chosen path or null.
Future<String?> showOpenDialog(BuildContext context, {String? startDir}) {
  return showDialog<String>(
    context: context,
    builder: (_) => FileDialog(
      title: t('Open document'),
      startDir: startDir ?? documentsDirectory(),
      extensions: openableExtensions,
    ),
  );
}

/// Shows the save dialog. [formats] maps extensions to labels; the first is
/// the default. Returns the full path (with extension) or null.
Future<String?> showSaveDialog(
  BuildContext context, {
  required String suggestedName,
  required Map<String, String> formats,
  String? startDir,
}) {
  return showDialog<String>(
    context: context,
    builder: (_) => FileDialog(
      title: t('Save document'),
      startDir: startDir ?? documentsDirectory(),
      extensions: formats.keys.toSet(),
      saveFormats: formats,
      suggestedName: suggestedName,
    ),
  );
}

class FileDialog extends StatefulWidget {
  const FileDialog({
    super.key,
    required this.title,
    required this.startDir,
    required this.extensions,
    this.saveFormats,
    this.suggestedName,
  });

  final String title;
  final String startDir;
  final Set<String> extensions;
  final Map<String, String>? saveFormats;
  final String? suggestedName;

  bool get isSave => saveFormats != null;

  @override
  State<FileDialog> createState() => _FileDialogState();
}

class _FileDialogState extends State<FileDialog> {
  late Directory _dir;
  List<FileSystemEntity> _entries = [];
  String? _error;
  late final TextEditingController _name;
  late final TextEditingController _location;
  String? _format;
  bool _showHidden = false;

  @override
  void initState() {
    super.initState();
    _dir = Directory(widget.startDir);
    _format = widget.saveFormats?.keys.first;
    _name = TextEditingController(text: widget.suggestedName ?? '');
    _location = TextEditingController(text: _dir.path);
    _list();
  }

  @override
  void dispose() {
    _name.dispose();
    _location.dispose();
    super.dispose();
  }

  void _list() {
    try {
      final entries = _dir.listSync(followLinks: false).where((e) {
        final name = baseName(e.path);
        if (!_showHidden && name.startsWith('.')) return false;
        if (e is Directory) return true;
        return widget.extensions.contains(extensionOf(e.path));
      }).toList()
        ..sort((a, b) {
          final ad = a is Directory, bd = b is Directory;
          if (ad != bd) return ad ? -1 : 1;
          return baseName(a.path).toLowerCase().compareTo(baseName(b.path).toLowerCase());
        });
      setState(() {
        _entries = entries;
        _error = null;
        _location.text = _dir.path;
      });
    } catch (e) {
      setState(() {
        _entries = [];
        _error = t('Cannot read this folder: {error}', {'error': e});
        _location.text = _dir.path;
      });
    }
  }

  void _go(Directory dir) {
    _dir = dir;
    _list();
  }

  void _choose(String path) {
    if (widget.isSave) {
      _name.text = baseName(path);
      _save();
    } else {
      Navigator.pop(context, path);
    }
  }

  Future<void> _save() async {
    var name = _name.text.trim();
    if (name.isEmpty || name.contains('/') || name.contains('\\')) {
      setState(() => _error = t('Please enter a valid file name.'));
      return;
    }
    final ext = extensionOf(name);
    if (!widget.saveFormats!.containsKey(ext)) {
      name = '$name.$_format';
    }
    final path = '${_dir.path}${Platform.pathSeparator}$name';
    if (File(path).existsSync()) {
      final replace = await showDialog<bool>(
        context: context,
        builder: (c) => AlertDialog(
          title: Text(t('Replace file?')),
          content: Text(t('"{name}" already exists. Do you want to replace it?', {'name': name})),
          actions: [
            TextButton(onPressed: () => Navigator.pop(c, false), child: Text(t('Cancel'))),
            FilledButton(onPressed: () => Navigator.pop(c, true), child: Text(t('Replace'))),
          ],
        ),
      );
      if (replace != true) return;
    }
    if (mounted) Navigator.pop(context, path);
  }

  Future<void> _newFolder() async {
    final controller = TextEditingController();
    final name = await showDialog<String>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(t('New folder')),
        content: TextField(controller: controller, autofocus: true, onSubmitted: (v) => Navigator.pop(c, v)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c), child: Text(t('Cancel'))),
          FilledButton(onPressed: () => Navigator.pop(c, controller.text), child: Text(t('Create'))),
        ],
      ),
    );
    controller.dispose();
    if (name == null || name.trim().isEmpty || name.contains(RegExp(r'[\\/]'))) return;
    try {
      Directory('${_dir.path}${Platform.pathSeparator}${name.trim()}').createSync();
      _list();
    } catch (e) {
      setState(() => _error = t('Cannot create folder: {error}', {'error': e}));
    }
  }

  List<(String, String)> _places() {
    final home = homeDirectory();
    final docs = documentsDirectory();
    return [
      (t('Documents'), docs),
      if (home != docs) (t('Home'), home),
      if (!Platform.isAndroid && !Platform.isIOS) ...[
        (t('Desktop'), '$home${Platform.pathSeparator}Desktop'),
        (t('Downloads'), '$home${Platform.pathSeparator}Downloads'),
      ],
      // Android: the app's external files folder is reachable from a PC over
      // USB and from file manager apps, so documents can be exchanged there.
      if (Platform.isAndroid) (t('Shared (Android/data)'), '/storage/emulated/0/Android/data/org.akshara.office/files'),
      if (Platform.isWindows)
        for (final letter in 'CDEFGH'.split(''))
          if (Directory('$letter:\\').existsSync()) (t('Drive {name}', {'name': '$letter:'}), '$letter:\\'),
      if (Platform.isLinux || Platform.isMacOS) (t('Computer'), '/'),
    ].where((p) => Directory(p.$2).existsSync()).toList();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final wide = MediaQuery.sizeOf(context).width > 640;
    final list = _error != null && _entries.isEmpty
        ? Center(child: Text(_error!, style: TextStyle(color: theme.colorScheme.error)))
        : ListView.builder(
            itemCount: _entries.length,
            itemBuilder: (context, i) {
              final e = _entries[i];
              final isDir = e is Directory;
              final name = baseName(e.path);
              String subtitle = '';
              if (!isDir) {
                try {
                  final stat = e.statSync();
                  final kb = (stat.size / 1024).ceil();
                  final m = stat.modified;
                  subtitle = '$kb KB · ${m.year}-${_two(m.month)}-${_two(m.day)} ${_two(m.hour)}:${_two(m.minute)}';
                } catch (_) {}
              }
              return ListTile(
                dense: true,
                leading: Icon(isDir ? Icons.folder_outlined : iconForExtension(extensionOf(name))),
                title: Text(name, overflow: TextOverflow.ellipsis),
                subtitle: subtitle.isEmpty ? null : Text(subtitle),
                onTap: () => isDir ? _go(Directory(e.path)) : _choose(e.path),
              );
            },
          );
    final places = ListView(
      children: [
        for (final p in _places())
          ListTile(
            dense: true,
            leading: const Icon(Icons.folder_special_outlined),
            title: Text(p.$1),
            selected: _dir.path == p.$2,
            onTap: () => _go(Directory(p.$2)),
          ),
      ],
    );
    return Dialog(
      insetPadding: const EdgeInsets.all(16),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 820, maxHeight: 620),
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.title, style: theme.textTheme.titleLarge),
              const SizedBox(height: 12),
              Row(
                children: [
                  IconButton(
                    tooltip: t('Parent folder'),
                    icon: const Icon(Icons.arrow_upward),
                    onPressed: _dir.parent.path == _dir.path ? null : () => _go(_dir.parent),
                  ),
                  Expanded(
                    child: TextField(
                      controller: _location,
                      decoration: const InputDecoration(isDense: true, border: OutlineInputBorder()),
                      onSubmitted: (v) {
                        if (Directory(v).existsSync()) {
                          _go(Directory(v));
                        } else if (!widget.isSave && File(v).existsSync()) {
                          Navigator.pop(context, v);
                        } else {
                          setState(() => _error = t('No such folder.'));
                        }
                      },
                    ),
                  ),
                  IconButton(tooltip: t('New folder'), icon: const Icon(Icons.create_new_folder_outlined), onPressed: _newFolder),
                  IconButton(
                    tooltip: _showHidden ? t('Hide hidden files') : t('Show hidden files'),
                    icon: Icon(_showHidden ? Icons.visibility_off_outlined : Icons.visibility_outlined),
                    onPressed: () {
                      _showHidden = !_showHidden;
                      _list();
                    },
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Expanded(
                child: DecoratedBox(
                  decoration: BoxDecoration(border: Border.all(color: theme.dividerColor), borderRadius: BorderRadius.circular(8)),
                  child: wide
                      ? Row(children: [
                          SizedBox(width: 180, child: places),
                          VerticalDivider(width: 1, color: theme.dividerColor),
                          Expanded(child: list),
                        ])
                      : list,
                ),
              ),
              if (_error != null && _entries.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_error!, style: TextStyle(color: theme.colorScheme.error)),
                ),
              const SizedBox(height: 12),
              if (widget.isSave)
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        controller: _name,
                        autofocus: true,
                        decoration: InputDecoration(labelText: t('File name'), isDense: true, border: OutlineInputBorder()),
                        onSubmitted: (_) => _save(),
                      ),
                    ),
                    const SizedBox(width: 12),
                    DropdownButton<String>(
                      value: _format,
                      items: [
                        for (final f in widget.saveFormats!.entries)
                          DropdownMenuItem(value: f.key, child: Text('${f.value} (.${f.key})')),
                      ],
                      onChanged: (v) => setState(() {
                        _format = v;
                        final name = _name.text;
                        final dot = name.lastIndexOf('.');
                        final stem = dot > 0 ? name.substring(0, dot) : name;
                        if (stem.isNotEmpty) _name.text = '$stem.$v';
                      }),
                    ),
                  ],
                ),
              const SizedBox(height: 12),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(onPressed: () => Navigator.pop(context), child: Text(t('Cancel'))),
                  if (widget.isSave) ...[
                    const SizedBox(width: 8),
                    FilledButton(onPressed: _save, child: Text(t('Save'))),
                  ],
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

String _two(int v) => v.toString().padLeft(2, '0');

/// Shows an error message.
Future<void> showError(BuildContext context, String title, Object error) {
  return showDialog<void>(
    context: context,
    builder: (c) => AlertDialog(
      icon: const Icon(Icons.error_outline),
      title: Text(title),
      content: SelectableText(error.toString()),
      actions: [FilledButton(onPressed: () => Navigator.pop(c), child: Text(t('OK')))],
    ),
  );
}

/// Asks for a line of text.
Future<String?> askText(BuildContext context, String title, {String initial = '', String? label}) async {
  final controller = TextEditingController(text: initial);
  final result = await showDialog<String>(
    context: context,
    builder: (c) => AlertDialog(
      title: Text(title),
      content: TextField(
        controller: controller,
        autofocus: true,
        decoration: InputDecoration(labelText: label),
        onSubmitted: (v) => Navigator.pop(c, v),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(c), child: Text(t('Cancel'))),
        FilledButton(onPressed: () => Navigator.pop(c, controller.text), child: Text(t('OK'))),
      ],
    ),
  );
  controller.dispose();
  return result;
}
