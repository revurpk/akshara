// Shared test helpers: locate the Rust core built by Cargo.

import 'dart:io';

import 'package:akshara/core/native.dart';

/// Points the FFI binding at the Cargo build (call first in every test file).
/// Returns the library path, or null when it has not been built.
String? useBuiltCore() {
  final path = coreLibraryPath();
  if (path != null) coreLibraryOverride = path;
  return path;
}

/// Path of the debug build of libakshara, or null when it is missing.
String? coreLibraryPath() {
  final fromEnv = Platform.environment['AKSHARA_LIB'];
  if (fromEnv != null && File(fromEnv).existsSync()) return fromEnv;
  final name = Platform.isWindows
      ? 'akshara.dll'
      : Platform.isMacOS
          ? 'libakshara.dylib'
          : 'libakshara.so';
  for (final profile in ['debug', 'release']) {
    final f = File('../target/$profile/$name');
    if (f.existsSync()) return f.absolute.path;
  }
  return null;
}
