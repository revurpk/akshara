// Binding to the Rust core (libakshara) through dart:ffi.
//
// The native side exposes three C functions (see crates/ffi/src/lib.rs):
// akshara_alloc, akshara_free and akshara_call. Requests and responses are UTF-8 JSON, so
// this file is the only place in the app that touches raw memory.

import 'dart:convert';
import 'dart:ffi';
import 'dart:io';
import 'dart:isolate';

typedef _AllocC = Pointer<Uint8> Function(Size);
typedef _AllocD = Pointer<Uint8> Function(int);
typedef _FreeC = Void Function(Pointer<Uint8>, Size);
typedef _FreeD = void Function(Pointer<Uint8>, int);
typedef _CallC = Pointer<Uint8> Function(Pointer<Uint8>, Size, Pointer<Size>);
typedef _CallD = Pointer<Uint8> Function(Pointer<Uint8>, int, Pointer<Size>);

/// An error reported by the core (bad file, limit exceeded, ...).
class CoreError implements Exception {
  CoreError(this.kind, this.message);

  /// Stable category: io, zip, xml, limit, invalid, unsupported, internal.
  final String kind;

  /// Human readable explanation.
  final String message;

  @override
  String toString() => message;
}

/// Locates and opens the native library for the current platform.
///
/// The environment variable AKSHARA_LIB overrides the search (used by the
/// tests, which load the library straight from the Cargo target directory).
/// Explicit library path (set by tests); takes precedence over everything.
String? coreLibraryOverride;

DynamicLibrary _openLibrary() {
  if (coreLibraryOverride != null) return DynamicLibrary.open(coreLibraryOverride!);
  final override = Platform.environment['AKSHARA_LIB'];
  if (override != null && override.isNotEmpty) {
    return DynamicLibrary.open(override);
  }
  if (Platform.isIOS) {
    // Linked statically into the Runner executable.
    return DynamicLibrary.process();
  }
  if (Platform.isAndroid) {
    return DynamicLibrary.open('libakshara.so');
  }
  final exeDir = File(Platform.resolvedExecutable).parent.path;
  final sep = Platform.pathSeparator;
  final candidates = <String>[
    if (Platform.isLinux) ...[
      '$exeDir${sep}lib${sep}libakshara.so',
      'libakshara.so',
    ],
    if (Platform.isWindows) ...['$exeDir${sep}akshara.dll', 'akshara.dll'],
    if (Platform.isMacOS) ...[
      '$exeDir$sep..${sep}Frameworks${sep}libakshara.dylib',
      'libakshara.dylib',
    ],
  ];
  Object? lastError;
  for (final path in candidates) {
    try {
      return DynamicLibrary.open(path);
    } catch (e) {
      lastError = e;
    }
  }
  if (Platform.isMacOS) {
    // Also allow static linking into the app bundle.
    try {
      final lib = DynamicLibrary.process();
      lib.lookup<NativeFunction<_CallC>>('akshara_call');
      return lib;
    } catch (_) {}
  }
  throw StateError('Akshara core library not found ($lastError)');
}

class _Bindings {
  _Bindings(DynamicLibrary lib)
      : alloc = lib.lookupFunction<_AllocC, _AllocD>('akshara_alloc'),
        free = lib.lookupFunction<_FreeC, _FreeD>('akshara_free'),
        call = lib.lookupFunction<_CallC, _CallD>('akshara_call');

  final _AllocD alloc;
  final _FreeD free;
  final _CallD call;
}

_Bindings? _bindings;

_Bindings get _b => _bindings ??= _Bindings(_openLibrary());

/// Sends one request to the core and returns its `result`, or throws
/// [CoreError].
dynamic core(Map<String, Object?> request) {
  final b = _b;
  final bytes = utf8.encode(jsonEncode(request));
  final req = b.alloc(bytes.length);
  final outLen = b.alloc(sizeOf<Size>()).cast<Size>();
  if (req.address == 0 || outLen.address == 0) {
    throw CoreError('limit', 'Request is too large');
  }
  Pointer<Uint8> out = nullptr;
  int n = 0;
  try {
    req.asTypedList(bytes.length).setAll(0, bytes);
    out = b.call(req, bytes.length, outLen);
    n = outLen.value;
    final text = utf8.decode(out.asTypedList(n));
    final response = jsonDecode(text) as Map<String, dynamic>;
    if (response['ok'] == true) {
      return response['result'];
    }
    final error = response['error'] as Map<String, dynamic>;
    throw CoreError(error['kind'] as String, error['message'] as String);
  } finally {
    b.free(req, bytes.length);
    b.free(outLen.cast<Uint8>(), sizeOf<Size>());
    if (out.address != 0) {
      b.free(out, n);
    }
  }
}

/// Like [core], but runs in a background isolate so that loading or saving
/// a large file never freezes the UI. The document registry lives in the
/// native library, so it is shared by all isolates.
Future<dynamic> coreAsync(Map<String, Object?> request) async {
  final libraryOverride = coreLibraryOverride; // globals are per isolate
  final result = await Isolate.run(() {
    coreLibraryOverride = libraryOverride;
    try {
      return <String, Object?>{'ok': true, 'result': core(request)};
    } on CoreError catch (e) {
      return <String, Object?>{'ok': false, 'kind': e.kind, 'message': e.message};
    }
  });
  if (result['ok'] == true) {
    return result['result'];
  }
  throw CoreError(result['kind'] as String, result['message'] as String);
}

/// Whether the native library can be loaded (used to show a friendly error
/// screen instead of crashing when the app was packaged incorrectly).
String? probeCore() {
  try {
    core({'op': 'version'});
    return null;
  } catch (e) {
    return e.toString();
  }
}
