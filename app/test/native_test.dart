import 'package:flutter_test/flutter_test.dart';
import 'package:akshara/core/native.dart';

import 'helpers.dart';

void main() {
  final lib = useBuiltCore();

  test('core library is available (run `cargo build -p akshara-ffi` first)', () {
    expect(lib, isNotNull);
    expect(probeCore(), isNull, reason: 'set AKSHARA_LIB=$lib');
  }, skip: lib == null ? 'native library not built' : false);

  group('protocol', () {
    test('version', () {
      final v = core({'op': 'version'}) as Map<String, dynamic>;
      expect(v['name'], 'Akshara');
    });

    test('errors become CoreError with a kind', () {
      expect(() => core({'op': 'no.such.op'}), throwsA(isA<CoreError>().having((e) => e.kind, 'kind', 'invalid')));
      expect(() => core({'op': 'open', 'path': '/definitely/missing.odt'}),
          throwsA(isA<CoreError>().having((e) => e.kind, 'kind', 'io')));
    });

    test('documents round-trip through the registry', () {
      final doc = (core({'op': 'new', 'kind': 'calc'}) as Map<String, dynamic>)['doc'];
      core({'op': 'calc.set_input', 'doc': doc, 'sheet': 0, 'cell': 'B2', 'text': '=6*7'});
      final cell = core({'op': 'calc.cell', 'doc': doc, 'sheet': 0, 'cell': 'B2'}) as Map<String, dynamic>;
      expect(cell['display'], '42');
      expect(core({'op': 'close', 'doc': doc}), true);
    });

    test('unicode survives the FFI boundary', () {
      final doc = (core({'op': 'new', 'kind': 'writer'}) as Map<String, dynamic>)['doc'];
      const text = 'Grüße 😀 مرحبا 你好';
      core({'op': 'writer.set_text', 'doc': doc, 'para': {'block': 0}, 'text': text});
      final para = core({'op': 'writer.para', 'doc': doc, 'para': {'block': 0}}) as Map<String, dynamic>;
      expect(para['runs'][0]['text'], text);
    });

    test('async calls run in a background isolate', () async {
      final v = await coreAsync({'op': 'version'}) as Map<String, dynamic>;
      expect(v['name'], 'Akshara');
      await expectLater(coreAsync({'op': 'bogus'}), throwsA(isA<CoreError>()));
    });
  }, skip: lib == null ? 'native library not built' : false);
}
