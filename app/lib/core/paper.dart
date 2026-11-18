// Paper sizes and label sheets known to the core (`paper_sizes`).

import 'native.dart';
import '../l10n/l10n.dart';

const _mm = 72 / 25.4;

/// A paper size or label sheet as reported by the core (`paper_sizes`).
class PaperInfo {
  PaperInfo(Map<String, dynamic> j)
      : id = j['id'] as String,
        name = j['name'] as String,
        kind = j['kind'] as String,
        width = (j['width'] as num).toDouble(),
        height = (j['height'] as num).toDouble();

  final String id;
  final String name;
  final String kind; // sheet, envelope, card
  final double width;
  final double height;

  /// Name in the current language with the size in millimetres.
  String get label {
    final what = switch (kind) {
      'envelope' => t('Envelope {name}', {'name': name}),
      'card' => t('Index card {name}', {'name': name}),
      _ => name,
    };
    return '$what (${(width / _mm).round()} × ${(height / _mm).round()} mm)';
  }
}

class LabelInfo {
  LabelInfo(Map<String, dynamic> j)
      : id = j['id'] as String,
        name = j['name'] as String,
        perSheet = (j['cols'] as int) * (j['rows'] as int),
        width = (j['width'] as num).toDouble(),
        height = (j['height'] as num).toDouble();

  final String id;
  final String name;
  final int perSheet;
  final double width;
  final double height;

  String get label =>
      '$name · ${t('{n} per sheet', {'n': perSheet})} (${(width / _mm).toStringAsFixed(1)} × ${(height / _mm).toStringAsFixed(1)} mm)';
}

(List<PaperInfo>, List<LabelInfo>) paperCatalog() {
  final r = core({'op': 'paper_sizes'}) as Map<String, dynamic>;
  return (
    [for (final p in r['papers'] as List<dynamic>) PaperInfo(p as Map<String, dynamic>)],
    [for (final l in r['labels'] as List<dynamic>) LabelInfo(l as Map<String, dynamic>)],
  );
}

