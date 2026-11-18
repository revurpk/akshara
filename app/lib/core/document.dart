// An open document as seen by the UI shell.

import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import 'file_dialog.dart';
import 'native.dart';

/// The three applications of the suite.
enum DocKind {
  writer('Writer', 'Text Document', Icons.description_outlined, Color(0xFF2B6CB0)),
  calc('Calc', 'Spreadsheet', Icons.grid_on_outlined, Color(0xFF2F855A)),
  math('Math', 'Formula', Icons.functions, Color(0xFFB7410E));

  const DocKind(this.appKey, this.labelKey, this.icon, this.color);

  final String appKey;
  final String labelKey;

  /// Application name in the current language.
  String get app => t(appKey);

  /// Document type name in the current language.
  String get label => t(labelKey);
  final IconData icon;
  final Color color;

  static DocKind parse(String s) => DocKind.values.firstWhere((k) => k.name == s);

  /// Formats offered by "Save as", native format first.
  Map<String, String> get saveFormats => switch (this) {
        DocKind.writer => {
            'odt': t('OpenDocument Text'),
            'txt': t('Plain text'),
            'html': t('HTML page'),
            'md': t('Markdown'),
          },
        DocKind.calc => {
            'ods': t('OpenDocument Spreadsheet'),
            'csv': t('CSV (current sheet)'),
            'tsv': t('Tab separated (current sheet)'),
          },
        DocKind.math => {'odf': t('OpenDocument Formula'), 'mml': t('MathML')},
      };

  String get nativeExtension => saveFormats.keys.first;
}

/// A document handle plus UI state shared by the shell and the editor.
class OpenDoc extends ChangeNotifier {
  OpenDoc(this.handle, this.kind, this.path);

  final int handle;
  final DocKind kind;
  String? path;
  bool modified = false;
  bool canUndo = false;
  bool canRedo = false;

  /// Sheet shown by Calc (used when saving CSV).
  int currentSheet = 0;

  /// Untitled documents get a number for their tab title.
  int untitledNumber = 0;

  /// The file a document was imported from (Word, Excel, text...), which
  /// is not where it will be saved.
  String? importedFrom;

  String get title => path != null
      ? baseName(path!)
      : importedFrom != null
          ? baseName(importedFrom!)
          : t('Untitled {kind} {n}', {'kind': kind.label, 'n': untitledNumber});

  /// File name offered by "Save as": the document's name with the native
  /// extension (`report.docx` -> `report.odt`).
  String get suggestedSaveName {
    final source = path ?? importedFrom;
    if (source == null) return 'Untitled.${kind.nativeExtension}';
    final name = baseName(source);
    final dot = name.lastIndexOf('.');
    return '${dot > 0 ? name.substring(0, dot) : name}.${kind.nativeExtension}';
  }

  /// Whether saving to [path] would keep everything (native format).
  bool get hasNativePath {
    if (path == null) return false;
    final ext = extensionOf(path!);
    return ext == kind.nativeExtension || (kind == DocKind.math && ext == 'mml');
  }

  /// Re-reads modified/undo state from the core and notifies listeners.
  void refresh() {
    try {
      final info = core({'op': 'info', 'doc': handle}) as Map<String, dynamic>;
      final m = info['modified'] == true, u = info['can_undo'] == true, r = info['can_redo'] == true;
      if (m != modified || u != canUndo || r != canRedo) {
        modified = m;
        canUndo = u;
        canRedo = r;
      }
    } catch (_) {}
    notifyListeners();
  }

  /// Marks the formula as changed (Math has no core-side undo history).
  void markModified() {
    modified = true;
    notifyListeners();
  }

  Future<void> saveTo(String target) async {
    await coreAsync({'op': 'save', 'doc': handle, 'path': target, 'sheet': currentSheet});
    final ext = extensionOf(target);
    // Native formats (and MathML, which embeds the formula source) are
    // lossless: the document now lives there. Other formats are exports and
    // leave the document's own path and "modified" state alone.
    final lossless = ext == kind.nativeExtension || (kind == DocKind.math && ext == 'mml');
    if (lossless) {
      path = target;
      if (kind == DocKind.math) modified = false;
    }
    refreshQuiet();
    notifyListeners();
  }

  void refreshQuiet() {
    try {
      final info = core({'op': 'info', 'doc': handle}) as Map<String, dynamic>;
      if (kind != DocKind.math) modified = info['modified'] == true;
      canUndo = info['can_undo'] == true;
      canRedo = info['can_redo'] == true;
    } catch (_) {}
  }

  void close() {
    try {
      core({'op': 'close', 'doc': handle});
    } catch (_) {}
  }
}
