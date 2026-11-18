// Akshara Office: application entry point and shell (home screen, tabs,
// file commands).

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:flutter/rendering.dart' show RenderRepaintBoundary;

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'l10n/l10n.dart';

import 'calc/calc_page.dart';
import 'core/document.dart';
import 'core/file_dialog.dart';
import 'core/look.dart';
import 'core/native.dart';
import 'core/paths.dart';
import 'core/translit.dart';
import 'math/math_page.dart';
import 'writer/writer_page.dart';

/// Brand colours: saffron (see core/look.dart) and palm-leaf cream.
const brandLeaf = Color(0xFFF6E7C1);

void main(List<String> args) {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(
    AksharaApp(initialFiles: args.where((a) => !a.startsWith('-')).toList()),
  );
}

class AksharaApp extends StatefulWidget {
  const AksharaApp({super.key, this.initialFiles = const []});

  final List<String> initialFiles;

  @override
  State<AksharaApp> createState() => _AksharaAppState();
}

class _AksharaAppState extends State<AksharaApp> {
  final Settings _settings = Settings.load();

  @override
  void initState() {
    super.initState();
    final saved = _settings.language;
    setLanguage(
      saved == null ? AppLanguage.system() : AppLanguage.fromCode(saved),
    );
    _loadTranslit();
    LookSettings.instance.load(_settings);
    LookSettings.instance.addListener(_onLook);
  }

  @override
  void dispose() {
    LookSettings.instance.removeListener(_onLook);
    super.dispose();
  }

  void _onLook() => setState(() {});

  void _loadTranslit() => TranslitSettings.instance.load(_settings, currentLanguage);

  void _setLanguage(AppLanguage language) {
    setState(() {
      setLanguage(language);
      _settings.language = language.code;
      // Until a script is chosen explicitly, it follows the interface language.
      if (_settings.translitScript == null) _loadTranslit();
    });
  }

  ThemeMode get _mode => switch (_settings.themeMode) {
    'light' => ThemeMode.light,
    'dark' => ThemeMode.dark,
    _ => ThemeMode.system,
  };

  void _setTheme(String mode) => setState(() => _settings.themeMode = mode);

  @override
  Widget build(BuildContext context) {
    final look = LookSettings.instance.look;
    final language = currentLanguage;
    return MaterialApp(
      // No key here: a language change rebuilds the tree in place, so open
      // documents and their editing state survive it.
      title: t('Akshara Office'),
      debugShowCheckedModeBanner: false,
      locale: Locale(language.materialCode),
      supportedLocales: {
        for (final l in AppLanguage.values) Locale(l.materialCode),
      }.toList(),
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      theme: buildTheme(look, Brightness.light),
      darkTheme: buildTheme(look, Brightness.dark),
      themeMode: _mode,
      home: Shell(
        settings: _settings,
        initialFiles: widget.initialFiles,
        themeMode: _settings.themeMode,
        onTheme: _setTheme,
        onLanguage: _setLanguage,
      ),
    );
  }
}

class Shell extends StatefulWidget {
  const Shell({
    super.key,
    required this.settings,
    required this.initialFiles,
    required this.themeMode,
    required this.onTheme,
    required this.onLanguage,
  });

  final Settings settings;
  final List<String> initialFiles;
  final String themeMode;
  final ValueChanged<String> onTheme;
  final ValueChanged<AppLanguage> onLanguage;

  @override
  State<Shell> createState() => ShellState();
}

class ShellState extends State<Shell> {
  final List<OpenDoc> _docs = [];
  int _active = -1; // -1 = home screen
  int _untitled = 0;
  String? _coreProblem;

  @override
  void initState() {
    super.initState();
    // Rebuild editors when transliteration is switched (autocorrect flags).
    TranslitSettings.instance.addListener(_onDocChanged);
    _coreProblem = probeCore();
    if (_coreProblem == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) async {
        for (final f in widget.initialFiles) {
          await openPath(f);
        }
        final shotDir = Platform.environment['AKSHARA_SCREENSHOT_DIR'];
        if (kDebugMode && shotDir != null) await _captureScreens(shotDir);
      });
    }
  }

  @override
  void dispose() {
    TranslitSettings.instance.removeListener(_onDocChanged);
    for (final d in _docs) {
      d.close();
    }
    super.dispose();
  }

  final _screenKey = GlobalKey();

  /// Debug-only helper for documentation and visual checks: renders the home
  /// screen and every open tab to PNG files, then exits. Compiled out of
  /// release builds (kDebugMode) and only active when AKSHARA_SCREENSHOT_DIR
  /// is set.
  Future<void> _captureScreens(String dir) async {
    for (var i = -1; i < _docs.length; i++) {
      setState(() => _active = i);
      await Future<void>.delayed(const Duration(milliseconds: 1500));
      final boundary =
          _screenKey.currentContext?.findRenderObject()
              as RenderRepaintBoundary?;
      if (boundary == null) continue;
      final image = await boundary.toImage(pixelRatio: 1.0);
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      if (bytes != null) {
        File('$dir${Platform.pathSeparator}screen-${i + 1}.png')
            .writeAsBytesSync(bytes.buffer.asUint8List());
      }
    }
    exit(0);
  }

  OpenDoc? get _current =>
      _active >= 0 && _active < _docs.length ? _docs[_active] : null;

  void _add(OpenDoc doc) {
    doc.addListener(_onDocChanged);
    setState(() {
      _docs.add(doc);
      _active = _docs.length - 1;
    });
  }

  void _onDocChanged() {
    if (!mounted) return;
    // Editors may report changes while the tree is building; defer then.
    if (SchedulerBinding.instance.schedulerPhase ==
        SchedulerPhase.persistentCallbacks) {
      SchedulerBinding.instance.addPostFrameCallback((_) {
        if (mounted) setState(() {});
      });
    } else {
      setState(() {});
    }
  }

  void newDocument(DocKind kind) {
    try {
      final r = core({'op': 'new', 'kind': kind.name}) as Map<String, dynamic>;
      final doc = OpenDoc(r['doc'] as int, kind, null)
        ..untitledNumber = ++_untitled;
      _add(doc);
    } on CoreError catch (e) {
      showError(context, t('Cannot create document'), e);
    }
  }

  /// Opens a document the core already created (envelope, labels...) in a
  /// new tab.
  void adoptDocument(int handle, DocKind kind) {
    _add(OpenDoc(handle, kind, null)..untitledNumber = ++_untitled);
  }

  Future<void> openPath(String path) async {
    final existing = _docs.indexWhere((d) => d.path == path);
    if (existing >= 0) {
      setState(() => _active = existing);
      return;
    }
    try {
      final r =
          await coreAsync({'op': 'open', 'path': path}) as Map<String, dynamic>;
      final doc = OpenDoc(
        r['doc'] as int,
        DocKind.parse(r['kind'] as String),
        path,
      );
      final ext = extensionOf(path);
      if (ext != doc.kind.nativeExtension &&
          !(doc.kind == DocKind.math && ext == 'mml')) {
        // Imported from a foreign format: saving goes to "Save as".
        doc.path = null;
        doc.untitledNumber = ++_untitled;
      }
      widget.settings.addRecent(path);
      _add(doc);
    } on CoreError catch (e) {
      if (mounted) {
        await showError(
          context,
          t('Cannot open {name}', {'name': baseName(path)}),
          e,
        );
      }
      if (e.kind == 'io') widget.settings.removeRecent(path);
      setState(() {});
    }
  }

  Future<void> open() async {
    final path = await showOpenDialog(context);
    if (path != null) await openPath(path);
  }

  Future<bool> save({bool saveAs = false}) async {
    final doc = _current;
    if (doc == null) return false;
    String? target = saveAs || !doc.hasNativePath ? null : doc.path;
    if (target == null) {
      final suggested = doc.path != null
          ? baseName(doc.path!)
          : 'Untitled.${doc.kind.nativeExtension}';
      target = await showSaveDialog(
        context,
        suggestedName: suggested,
        formats: doc.kind.saveFormats,
        startDir: doc.path != null ? File(doc.path!).parent.path : null,
      );
      if (target == null) return false;
    }
    try {
      await doc.saveTo(target);
      widget.settings.addRecent(target);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(t('Saved {name}', {'name': baseName(target)})),
            duration: const Duration(seconds: 2),
          ),
        );
      }
      setState(() {});
      return true;
    } on CoreError catch (e) {
      if (mounted) await showError(context, t('Cannot save'), e);
      return false;
    }
  }

  Future<void> closeDoc(int index) async {
    final doc = _docs[index];
    if (doc.modified) {
      setState(() => _active = index);
      final choice = await showChoice<String>(
        context,
        title: t('Save changes to "{name}"?', {'name': doc.title}),
        message: t('Your changes will be lost if you don\'t save them.'),
        choices: [
          DialogChoice(t('Don\'t save'), 'discard', destructive: true),
          DialogChoice(t('Cancel'), 'cancel'),
          DialogChoice(t('Save'), 'save', primary: true),
        ],
      );
      if (choice == null || choice == 'cancel') return;
      if (choice == 'save' && !await save()) return;
    }
    doc.removeListener(_onDocChanged);
    doc.close();
    setState(() {
      _docs.removeAt(index);
      if (_active >= _docs.length) _active = _docs.length - 1;
    });
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    if (_coreProblem != null) {
      return Scaffold(
        body: Center(
          child: Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.error_outline,
                  size: 64,
                  color: theme.colorScheme.error,
                ),
                const SizedBox(height: 16),
                Text(
                  t('The Akshara core library could not be loaded.'),
                  style: theme.textTheme.titleLarge,
                ),
                const SizedBox(height: 8),
                SelectableText(_coreProblem!),
                const SizedBox(height: 8),
                Text(
                  t(
                    'Build it with ./run.sh or scripts/build-native.sh, then restart the app.',
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }
    final ctrl = Platform.isMacOS
        ? const SingleActivator(LogicalKeyboardKey.keyS, meta: true)
        : const SingleActivator(LogicalKeyboardKey.keyS, control: true);
    final meta = Platform.isMacOS;
    return CallbackShortcuts(
      bindings: {
        ctrl: () => save(),
        SingleActivator(
          LogicalKeyboardKey.keyS,
          control: !meta,
          meta: meta,
          shift: true,
        ): () =>
            save(saveAs: true),
        SingleActivator(LogicalKeyboardKey.keyO, control: !meta, meta: meta):
            open,
        SingleActivator(
          LogicalKeyboardKey.keyN,
          control: !meta,
          meta: meta,
        ): () =>
            newDocument(_current?.kind ?? DocKind.writer),
        SingleActivator(
          LogicalKeyboardKey.keyW,
          control: !meta,
          meta: meta,
        ): () {
          if (_active >= 0) closeDoc(_active);
        },
        meta ? translitShortcutMac : translitShortcut: TranslitSettings.instance.toggle,
      },
      child: RepaintBoundary(
        key: _screenKey,
        child: Scaffold(
          body: SafeArea(
            child: Column(
              children: [
                _tabBar(theme),
                const Divider(height: 1),
                Expanded(
                  child: IndexedStack(
                    index: _active + 1,
                    children: [
                      HomeScreen(
                        shell: this,
                        recent: widget.settings.recentFiles,
                      ),
                      for (final d in _docs)
                        KeyedSubtree(
                          key: ObjectKey(d),
                          child: switch (d.kind) {
                            DocKind.writer => WriterPage(doc: d, onNewDocument: (h) => adoptDocument(h, DocKind.writer)),
                            DocKind.calc => CalcPage(doc: d),
                            DocKind.math => MathPage(doc: d),
                          },
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _tabBar(ThemeData theme) {
    final doc = _current;
    return Material(
      color: theme.colorScheme.surfaceContainerHigh,
      child: SizedBox(
        height: 44,
        child: Row(
          children: [
            const SizedBox(width: 6),
            _TabChip(
              icon: Image.asset(
                'assets/branding/logo.png',
                width: 20,
                height: 20,
                errorBuilder: (_, _, _) =>
                    const Icon(Icons.home_outlined, size: 20),
              ),
              label: t('Home'),
              selected: _active == -1,
              onTap: () => setState(() => _active = -1),
            ),
            Expanded(
              child: ListView(
                scrollDirection: Axis.horizontal,
                children: [
                  for (var i = 0; i < _docs.length; i++)
                    _TabChip(
                      icon: Icon(
                        _docs[i].kind.icon,
                        size: 18,
                        color: _docs[i].kind.color,
                      ),
                      label:
                          '${_docs[i].modified ? '• ' : ''}${_docs[i].title}',
                      selected: _active == i,
                      onTap: () => setState(() => _active = i),
                      onClose: () => closeDoc(i),
                    ),
                ],
              ),
            ),
            PopupMenuButton<DocKind>(
              tooltip: t('New document'),
              icon: const Icon(Icons.add),
              itemBuilder: (_) => [
                for (final k in DocKind.values)
                  PopupMenuItem(
                    value: k,
                    child: ListTile(
                      leading: Icon(k.icon, color: k.color),
                      title: Text('${k.label} (${k.app})'),
                    ),
                  ),
              ],
              onSelected: newDocument,
            ),
            IconButton(
              tooltip: t('Open… (Ctrl+O)'),
              icon: const Icon(Icons.folder_open_outlined),
              onPressed: open,
            ),
            IconButton(
              tooltip: t('Save (Ctrl+S)'),
              icon: const Icon(Icons.save_outlined),
              onPressed: doc == null ? null : () => save(),
            ),
            IconButton(
              tooltip: t('Save as / export… (Ctrl+Shift+S)'),
              icon: const Icon(Icons.save_as_outlined),
              onPressed: doc == null ? null : () => save(saveAs: true),
            ),
            const TranslitButton(),
            PopupMenuButton<AppLanguage>(
              tooltip: t('Language'),
              icon: const Icon(Icons.translate),
              initialValue: currentLanguage,
              itemBuilder: (_) => [
                for (final l in AppLanguage.values)
                  PopupMenuItem(
                    value: l,
                    child: Text(
                      l == AppLanguage.en
                          ? l.nativeName
                          : '${l.nativeName}  ·  ${l.englishName}',
                    ),
                  ),
              ],
              onSelected: widget.onLanguage,
            ),
            _AppearanceButton(themeMode: widget.themeMode, onTheme: widget.onTheme),
            IconButton(
              tooltip: t('About'),
              icon: const Icon(Icons.info_outline),
              onPressed: () => _about(context),
            ),
            const SizedBox(width: 4),
          ],
        ),
      ),
    );
  }

  void _about(BuildContext context) {
    final version =
        (core({'op': 'version'}) as Map<String, dynamic>)['version'];
    showAboutDialog(
      context: context,
      applicationName: 'Akshara Office',
      applicationVersion: '$version',
      applicationIcon: Image.asset(
        'assets/branding/logo.png',
        width: 48,
        height: 48,
      ),
      applicationLegalese: t(
        'Licensed under the Apache License 2.0. An independent implementation of the OpenDocument formats used by Apache OpenOffice; not affiliated with the Apache Software Foundation. Fonts: Google Noto (SIL Open Font License 1.1).',
      ),
    );
  }
}

/// Title-bar menu for the colour theme and the look and feel.
class _AppearanceButton extends StatelessWidget {
  const _AppearanceButton({required this.themeMode, required this.onTheme});

  final String themeMode;
  final ValueChanged<String> onTheme;

  @override
  Widget build(BuildContext context) {
    final looks = LookSettings.instance;
    final header = Theme.of(context).textTheme.labelMedium;
    return PopupMenuButton<String>(
      tooltip: t('Appearance'),
      icon: Icon(switch (themeMode) {
        'light' => Icons.light_mode_outlined,
        'dark' => Icons.dark_mode_outlined,
        _ => Icons.brightness_auto_outlined,
      }),
      itemBuilder: (_) => [
        PopupMenuItem(enabled: false, height: 28, child: Text(t('Theme'), style: header)),
        for (final mode in ['system', 'light', 'dark'])
          CheckedPopupMenuItem(value: 'theme:$mode', checked: themeMode == mode, child: Text(t(mode))),
        const PopupMenuDivider(),
        PopupMenuItem(enabled: false, height: 28, child: Text(t('Look and feel'), style: header)),
        CheckedPopupMenuItem(
          value: 'style:native',
          checked: looks.style == UiStyle.native,
          child: Text(t('Native ({platform})', {'platform': Look.host.platformName})),
        ),
        CheckedPopupMenuItem(
          value: 'style:material',
          checked: looks.style == UiStyle.material,
          child: Text(t('Material Design')),
        ),
      ],
      onSelected: (v) {
        final value = v.substring(v.indexOf(':') + 1);
        if (v.startsWith('theme:')) {
          onTheme(value);
        } else {
          looks.style = UiStyle.fromName(value);
        }
      },
    );
  }
}

class _TabChip extends StatelessWidget {
  const _TabChip({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
    this.onClose,
  });

  final Widget icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 5),
      child: Material(
        color: selected ? theme.colorScheme.surface : Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onTap: onTap,
          onTertiaryTapUp: onClose == null ? null : (_) => onClose!(),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                icon,
                const SizedBox(width: 6),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 200),
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodyMedium,
                  ),
                ),
                if (onClose != null)
                  IconButton(
                    visualDensity: VisualDensity.compact,
                    iconSize: 16,
                    tooltip: t('Close (Ctrl+W)'),
                    icon: const Icon(Icons.close),
                    onPressed: onClose,
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key, required this.shell, required this.recent});

  final ShellState shell;
  final List<String> recent;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 880),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Image.asset(
                    'assets/branding/logo.png',
                    width: 72,
                    height: 72,
                    errorBuilder: (_, _, _) => const SizedBox(width: 72),
                  ),
                  const SizedBox(width: 16),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          t('Akshara Office'),
                          style: theme.textTheme.headlineMedium?.copyWith(
                            fontWeight: FontWeight.bold,
                          ),
                        ),
                        Text(
                          t(
                            'Documents, spreadsheets and formulas. Private, offline, open formats.',
                          ),
                          style: theme.textTheme.bodyLarge,
                        ),
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 20),
              // Language chips in their own scripts: the first thing a new
              // user can change, no menus to find.
              Wrap(
                spacing: 8,
                runSpacing: 8,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Icon(
                    Icons.translate,
                    size: 20,
                    color: theme.colorScheme.primary,
                  ),
                  for (final l in AppLanguage.values)
                    ChoiceChip(
                      label: Text(l.nativeName),
                      tooltip: l.englishName,
                      selected: l == currentLanguage,
                      onSelected: (_) => shell.widget.onLanguage(l),
                    ),
                ],
              ),
              const SizedBox(height: 28),
              Text(t('Create'), style: theme.textTheme.titleMedium),
              const SizedBox(height: 12),
              Wrap(
                spacing: 16,
                runSpacing: 16,
                children: [
                  for (final k in DocKind.values)
                    _CreateCard(kind: k, onTap: () => shell.newDocument(k)),
                  _CreateCard(kind: null, onTap: shell.open),
                ],
              ),
              const SizedBox(height: 32),
              if (recent.isNotEmpty) ...[
                Text(t('Recent documents'), style: theme.textTheme.titleMedium),
                const SizedBox(height: 8),
                Card(
                  child: Column(
                    children: [
                      for (final path in recent)
                        ListTile(
                          leading: Icon(iconForExtension(extensionOf(path))),
                          title: Text(baseName(path)),
                          subtitle: Text(
                            File(path).parent.path,
                            overflow: TextOverflow.ellipsis,
                          ),
                          onTap: () => shell.openPath(path),
                        ),
                    ],
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class _CreateCard extends StatelessWidget {
  const _CreateCard({required this.kind, required this.onTap});

  final DocKind? kind;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final color = kind?.color ?? theme.colorScheme.secondary;
    return SizedBox(
      width: 196,
      height: 148,
      child: Card(
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(
                  kind?.icon ?? Icons.folder_open_outlined,
                  size: 36,
                  color: color,
                ),
                const Spacer(),
                Text(
                  kind == null ? t('Open…') : kind!.app,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.titleMedium?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                Text(
                  kind == null
                      ? 'ODT · ODS · ODF · CSV · TXT · MathML'
                      : t('New {kind}', {'kind': kind!.label}),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
