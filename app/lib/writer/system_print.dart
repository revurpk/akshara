// The operating system's own print dialog, through dart:ffi (no plugins).
//
// * Windows: the common Print dialog (PrintDlgW) with the printer's own
//   Preferences, then the pages are drawn with GDI at the printer's
//   resolution. The dialog starts with the document's paper size and
//   orientation selected.
// * Linux: GTK's print dialog (GtkPrintUnixDialog); the PDF goes to the
//   chosen printer as a GtkPrintJob, which applies copies, page ranges and
//   the other options. This needs the UI isolate on the GTK thread
//   (FL_UI_THREAD_POLICY_RUN_ON_PLATFORM_THREAD in linux/runner).
//
// Other platforms use the fallback in print_preview.dart.

import 'dart:async';
import 'dart:convert' show Utf8Encoder;
import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

/// Result of a system print request.
enum SystemPrintResult { printed, cancelled }

/// Thrown when the system print dialog or spooler fails.
class SystemPrintError implements Exception {
  SystemPrintError(this.message);
  final String message;
  @override
  String toString() => message;
}

/// One page rendered for the printer: RGBA bytes, top row first.
typedef PageRaster = ({Uint8List rgba, int width, int height});

/// Renders page [index] (0-based) at [pixelRatio] device pixels per point.
typedef PageRenderer = Future<PageRaster> Function(int index, double pixelRatio);

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

final class _PrintDlg extends Struct {
  @Uint32()
  external int lStructSize;
  @IntPtr()
  external int hwndOwner;
  @IntPtr()
  external int hDevMode;
  @IntPtr()
  external int hDevNames;
  @IntPtr()
  external int hDC;
  @Uint32()
  external int flags;
  @Uint16()
  external int nFromPage;
  @Uint16()
  external int nToPage;
  @Uint16()
  external int nMinPage;
  @Uint16()
  external int nMaxPage;
  @Uint16()
  external int nCopies;
  @IntPtr()
  external int hInstance;
  @IntPtr()
  external int lCustData;
  @IntPtr()
  external int lpfnPrintHook;
  @IntPtr()
  external int lpfnSetupHook;
  @IntPtr()
  external int lpPrintTemplateName;
  @IntPtr()
  external int lpSetupTemplateName;
  @IntPtr()
  external int hPrintTemplate;
  @IntPtr()
  external int hSetupTemplate;
}

final class _DocInfo extends Struct {
  @Int32()
  external int cbSize;
  external Pointer<Uint16> lpszDocName;
  external Pointer<Uint16> lpszOutput;
  external Pointer<Uint16> lpszDatatype;
  @Uint32()
  external int fwType;
}

final class _BitmapInfoHeader extends Struct {
  @Uint32()
  external int biSize;
  @Int32()
  external int biWidth;
  @Int32()
  external int biHeight;
  @Uint16()
  external int biPlanes;
  @Uint16()
  external int biBitCount;
  @Uint32()
  external int biCompression;
  @Uint32()
  external int biSizeImage;
  @Int32()
  external int biXPelsPerMeter;
  @Int32()
  external int biYPelsPerMeter;
  @Uint32()
  external int biClrUsed;
  @Uint32()
  external int biClrImportant;
}

class _Win32 {
  _Win32()
      : _comdlg = DynamicLibrary.open('comdlg32.dll'),
        _gdi = DynamicLibrary.open('gdi32.dll'),
        _kernel = DynamicLibrary.open('kernel32.dll'),
        _user = DynamicLibrary.open('user32.dll');

  final DynamicLibrary _comdlg, _gdi, _kernel, _user;

  late final printDlg = _comdlg.lookupFunction<Int32 Function(Pointer<_PrintDlg>), int Function(Pointer<_PrintDlg>)>('PrintDlgW');
  late final extendedError = _comdlg.lookupFunction<Uint32 Function(), int Function()>('CommDlgExtendedError');
  late final localAlloc = _kernel.lookupFunction<Pointer<Void> Function(Uint32, IntPtr), Pointer<Void> Function(int, int)>('LocalAlloc');
  late final localFree = _kernel.lookupFunction<Pointer<Void> Function(Pointer<Void>), Pointer<Void> Function(Pointer<Void>)>('LocalFree');
  late final globalLock = _kernel.lookupFunction<Pointer<Uint8> Function(IntPtr), Pointer<Uint8> Function(int)>('GlobalLock');
  late final globalUnlock = _kernel.lookupFunction<Int32 Function(IntPtr), int Function(int)>('GlobalUnlock');
  late final globalFree = _kernel.lookupFunction<IntPtr Function(IntPtr), int Function(int)>('GlobalFree');
  late final foregroundWindow = _user.lookupFunction<IntPtr Function(), int Function()>('GetForegroundWindow');
  late final deviceCaps = _gdi.lookupFunction<Int32 Function(IntPtr, Int32), int Function(int, int)>('GetDeviceCaps');
  late final startDoc = _gdi.lookupFunction<Int32 Function(IntPtr, Pointer<_DocInfo>), int Function(int, Pointer<_DocInfo>)>('StartDocW');
  late final startPage = _gdi.lookupFunction<Int32 Function(IntPtr), int Function(int)>('StartPage');
  late final endPage = _gdi.lookupFunction<Int32 Function(IntPtr), int Function(int)>('EndPage');
  late final endDoc = _gdi.lookupFunction<Int32 Function(IntPtr), int Function(int)>('EndDoc');
  late final abortDoc = _gdi.lookupFunction<Int32 Function(IntPtr), int Function(int)>('AbortDoc');
  late final deleteDC = _gdi.lookupFunction<Int32 Function(IntPtr), int Function(int)>('DeleteDC');
  late final setStretchBltMode = _gdi.lookupFunction<Int32 Function(IntPtr, Int32), int Function(int, int)>('SetStretchBltMode');
  late final setBrushOrgEx =
      _gdi.lookupFunction<Int32 Function(IntPtr, Int32, Int32, Pointer<Void>), int Function(int, int, int, Pointer<Void>)>('SetBrushOrgEx');
  late final stretchDIBits = _gdi.lookupFunction<
      Int32 Function(IntPtr, Int32, Int32, Int32, Int32, Int32, Int32, Int32, Int32, Pointer<Uint8>, Pointer<_BitmapInfoHeader>, Uint32, Uint32),
      int Function(int, int, int, int, int, int, int, int, int, Pointer<Uint8>, Pointer<_BitmapInfoHeader>, int, int)>('StretchDIBits');

  /// Zero-initialised memory (LPTR); free with [free].
  Pointer<T> alloc<T extends NativeType>(int bytes) {
    final p = localAlloc(0x0040, bytes);
    if (p == nullptr) throw SystemPrintError('Out of memory');
    return p.cast<T>();
  }

  void free(Pointer p) => localFree(p.cast());

  Pointer<Uint16> wide(String s) {
    final p = alloc<Uint16>((s.length + 1) * 2);
    final units = p.asTypedList(s.length + 1);
    for (var i = 0; i < s.length; i++) {
      units[i] = s.codeUnitAt(i);
    }
    units[s.length] = 0;
    return p;
  }
}

// Win32 constants.
const _pdReturnDc = 0x100, _pdReturnDefault = 0x400, _pdNoSelection = 0x4, _pdPageNums = 0x2;
const _pdUseDevModeCopiesAndCollate = 0x40000, _pdNoCurrentPage = 0x800000;
const _logPixelsX = 88, _logPixelsY = 90, _physicalWidth = 110, _physicalHeight = 111;
const _physicalOffsetX = 112, _physicalOffsetY = 113;
const _dmOrientation = 0x1, _dmPaperSize = 0x2, _dmPaperLength = 0x4, _dmPaperWidth = 0x8;

/// Windows paper codes (DMPAPER_*) for the core's paper ids.
const _dmPaper = {
  'letter': 1, 'legal': 5, 'a3': 8, 'a4': 9, 'a5': 11,
  'env_10': 20, 'env_dl': 27, 'env_c5': 28, 'env_c4': 30, 'env_c6': 31,
};

/// Shows the Windows Print dialog and prints [pageCount] pages.
Future<SystemPrintResult> printWindows({
  required String title,
  required int pageCount,
  required double widthPt,
  required double heightPt,
  required String? paper,
  required PageRenderer render,
}) async {
  final w = _Win32();
  final pd = w.alloc<_PrintDlg>(sizeOf<_PrintDlg>());
  var hdc = 0;
  try {
    // 1. The default printer's settings, without showing anything.
    pd.ref
      ..lStructSize = sizeOf<_PrintDlg>()
      ..flags = _pdReturnDefault;
    if (w.printDlg(pd) == 0) {
      final code = w.extendedError();
      throw SystemPrintError(code == 0x1008 ? 'No printer is installed.' : 'The print dialog failed (error $code).');
    }
    // 2. Preselect the document's paper size and orientation.
    if (pd.ref.hDevMode != 0) {
      final dm = w.globalLock(pd.ref.hDevMode);
      if (dm != nullptr) {
        final data = ByteData.sublistView(dm.asTypedList(96));
        var fields = data.getUint32(72, Endian.little) | _dmOrientation;
        data.setInt16(76, widthPt > heightPt ? 2 : 1, Endian.little);
        final code = paper == null ? null : _dmPaper[paper];
        if (code != null) {
          data.setInt16(78, code, Endian.little);
          fields |= _dmPaperSize;
        } else {
          // Custom size in tenths of a millimetre, portrait.
          final short = (widthPt < heightPt ? widthPt : heightPt) / 72 * 254;
          final long = (widthPt < heightPt ? heightPt : widthPt) / 72 * 254;
          data.setInt16(78, 0, Endian.little);
          data.setInt16(80, long.round(), Endian.little);
          data.setInt16(82, short.round(), Endian.little);
          fields |= _dmPaperSize | _dmPaperLength | _dmPaperWidth;
        }
        data.setUint32(72, fields, Endian.little);
        w.globalUnlock(pd.ref.hDevMode);
      }
    }
    // 3. The dialog itself.
    pd.ref
      ..hwndOwner = w.foregroundWindow()
      ..flags = _pdReturnDc | _pdUseDevModeCopiesAndCollate | _pdNoSelection | _pdNoCurrentPage
      ..nMinPage = 1
      ..nMaxPage = pageCount
      ..nFromPage = 1
      ..nToPage = pageCount
      ..nCopies = 1;
    if (w.printDlg(pd) == 0) {
      final code = w.extendedError();
      if (code == 0) return SystemPrintResult.cancelled;
      throw SystemPrintError('The print dialog failed (error $code).');
    }
    hdc = pd.ref.hDC;
    if (hdc == 0) throw SystemPrintError('The printer could not be opened.');
    var first = 0, last = pageCount - 1;
    if (pd.ref.flags & _pdPageNums != 0) {
      first = (pd.ref.nFromPage - 1).clamp(0, pageCount - 1);
      last = (pd.ref.nToPage - 1).clamp(first, pageCount - 1);
    }

    // 4. Draw the pages with GDI.
    final docInfo = w.alloc<_DocInfo>(sizeOf<_DocInfo>());
    final name = w.wide(title);
    docInfo.ref
      ..cbSize = sizeOf<_DocInfo>()
      ..lpszDocName = name;
    final job = w.startDoc(hdc, docInfo);
    w.free(name);
    w.free(docInfo);
    if (job <= 0) throw SystemPrintError('The printer refused the document.');
    try {
      final dpiX = w.deviceCaps(hdc, _logPixelsX), dpiY = w.deviceCaps(hdc, _logPixelsY);
      final paperW = w.deviceCaps(hdc, _physicalWidth), paperH = w.deviceCaps(hdc, _physicalHeight);
      final offX = w.deviceCaps(hdc, _physicalOffsetX), offY = w.deviceCaps(hdc, _physicalOffsetY);
      final pageW = widthPt / 72 * dpiX, pageH = heightPt / 72 * dpiY;
      // Actual size (labels must line up); shrink only if the paper is
      // smaller than the page.
      var scale = 1.0;
      if (paperW > 0 && paperH > 0 && (pageW > paperW * 1.02 || pageH > paperH * 1.02)) {
        scale = [paperW / pageW, paperH / pageH].reduce((a, b) => a < b ? a : b);
      }
      final destW = (pageW * scale).round(), destH = (pageH * scale).round();
      final x = scale < 1 ? ((paperW - destW) / 2).round() - offX : -offX;
      final y = scale < 1 ? ((paperH - destH) / 2).round() - offY : -offY;
      final ratio = (dpiX < 300 ? dpiX : 300) / 72;
      w.setStretchBltMode(hdc, 4); // HALFTONE
      w.setBrushOrgEx(hdc, 0, 0, nullptr);
      for (var i = first; i <= last; i++) {
        final raster = await render(i, ratio);
        final pixels = raster.width * raster.height;
        final bits = w.alloc<Uint8>(pixels * 4);
        final bmi = w.alloc<_BitmapInfoHeader>(sizeOf<_BitmapInfoHeader>() + 4);
        try {
          // RGBA -> BGRA.
          final src = raster.rgba, dst = bits.asTypedList(pixels * 4);
          for (var p = 0; p < pixels * 4; p += 4) {
            dst[p] = src[p + 2];
            dst[p + 1] = src[p + 1];
            dst[p + 2] = src[p];
            dst[p + 3] = 255;
          }
          bmi.ref
            ..biSize = sizeOf<_BitmapInfoHeader>()
            ..biWidth = raster.width
            ..biHeight = -raster.height // top-down
            ..biPlanes = 1
            ..biBitCount = 32;
          if (w.startPage(hdc) <= 0) throw SystemPrintError('The printer stopped.');
          final ok = w.stretchDIBits(hdc, x, y, destW, destH, 0, 0, raster.width, raster.height, bits, bmi, 0, 0x00CC0020);
          if (w.endPage(hdc) <= 0 || ok == 0) throw SystemPrintError('The printer stopped.');
        } finally {
          w.free(bits);
          w.free(bmi);
        }
      }
      w.endDoc(hdc);
    } catch (_) {
      w.abortDoc(hdc);
      rethrow;
    }
    return SystemPrintResult.printed;
  } finally {
    if (hdc != 0) w.deleteDC(hdc);
    if (pd.ref.hDevMode != 0) w.globalFree(pd.ref.hDevMode);
    if (pd.ref.hDevNames != 0) w.globalFree(pd.ref.hDevNames);
    w.free(pd);
  }
}

// ---------------------------------------------------------------------------
// Linux (GTK 3)
// ---------------------------------------------------------------------------

typedef _ResponseC = Void Function(Pointer<Void> dialog, Int32 response, Pointer<Void> data);
typedef _CompleteC = Void Function(Pointer<Void> job, Pointer<Void> data, Pointer<Void> error);

class _Gtk {
  _Gtk(this._lib, this._libc);

  static _Gtk? open() {
    if (!Platform.isLinux) return null;
    try {
      final gtk = _Gtk(DynamicLibrary.open('libgtk-3.so.0'), DynamicLibrary.process());
      gtk.dialogNew; // probe: throws when the print dialog is missing
      return gtk;
    } catch (_) {
      return null;
    }
  }

  final DynamicLibrary _lib, _libc;

  late final malloc = _libc.lookupFunction<Pointer<Uint8> Function(IntPtr), Pointer<Uint8> Function(int)>('malloc');
  late final free = _libc.lookupFunction<Void Function(Pointer<Uint8>), void Function(Pointer<Uint8>)>('free');
  late final dialogNew = _lib.lookupFunction<Pointer<Void> Function(Pointer<Uint8>, Pointer<Void>), Pointer<Void> Function(Pointer<Uint8>, Pointer<Void>)>(
      'gtk_print_unix_dialog_new');
  late final setPageSetup = _lib.lookupFunction<Void Function(Pointer<Void>, Pointer<Void>), void Function(Pointer<Void>, Pointer<Void>)>(
      'gtk_print_unix_dialog_set_page_setup');
  late final setCapabilities = _lib.lookupFunction<Void Function(Pointer<Void>, Int32), void Function(Pointer<Void>, int)>(
      'gtk_print_unix_dialog_set_manual_capabilities');
  late final selectedPrinter = _lib.lookupFunction<Pointer<Void> Function(Pointer<Void>), Pointer<Void> Function(Pointer<Void>)>(
      'gtk_print_unix_dialog_get_selected_printer');
  late final getSettings = _lib.lookupFunction<Pointer<Void> Function(Pointer<Void>), Pointer<Void> Function(Pointer<Void>)>(
      'gtk_print_unix_dialog_get_settings');
  late final getPageSetup = _lib.lookupFunction<Pointer<Void> Function(Pointer<Void>), Pointer<Void> Function(Pointer<Void>)>(
      'gtk_print_unix_dialog_get_page_setup');
  late final pageSetupNew = _lib.lookupFunction<Pointer<Void> Function(), Pointer<Void> Function()>('gtk_page_setup_new');
  late final setPaperSize = _lib.lookupFunction<Void Function(Pointer<Void>, Pointer<Void>), void Function(Pointer<Void>, Pointer<Void>)>(
      'gtk_page_setup_set_paper_size');
  late final setOrientation = _lib.lookupFunction<Void Function(Pointer<Void>, Int32), void Function(Pointer<Void>, int)>(
      'gtk_page_setup_set_orientation');
  late final paperNew = _lib.lookupFunction<Pointer<Void> Function(Pointer<Uint8>), Pointer<Void> Function(Pointer<Uint8>)>('gtk_paper_size_new');
  late final paperCustom = _lib.lookupFunction<
      Pointer<Void> Function(Pointer<Uint8>, Pointer<Uint8>, Double, Double, Int32),
      Pointer<Void> Function(Pointer<Uint8>, Pointer<Uint8>, double, double, int)>('gtk_paper_size_new_custom');
  late final paperFree = _lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>('gtk_paper_size_free');
  late final jobNew = _lib.lookupFunction<
      Pointer<Void> Function(Pointer<Uint8>, Pointer<Void>, Pointer<Void>, Pointer<Void>),
      Pointer<Void> Function(Pointer<Uint8>, Pointer<Void>, Pointer<Void>, Pointer<Void>)>('gtk_print_job_new');
  late final jobSetSource = _lib.lookupFunction<
      Int32 Function(Pointer<Void>, Pointer<Uint8>, Pointer<Pointer<Void>>),
      int Function(Pointer<Void>, Pointer<Uint8>, Pointer<Pointer<Void>>)>('gtk_print_job_set_source_file');
  late final jobSend = _lib.lookupFunction<
      Void Function(Pointer<Void>, Pointer<NativeFunction<_CompleteC>>, Pointer<Void>, Pointer<Void>),
      void Function(Pointer<Void>, Pointer<NativeFunction<_CompleteC>>, Pointer<Void>, Pointer<Void>)>('gtk_print_job_send');
  late final signalConnect = _lib.lookupFunction<
      UnsignedLong Function(Pointer<Void>, Pointer<Uint8>, Pointer<NativeFunction<_ResponseC>>, Pointer<Void>, Pointer<Void>, Int32),
      int Function(Pointer<Void>, Pointer<Uint8>, Pointer<NativeFunction<_ResponseC>>, Pointer<Void>, Pointer<Void>, int)>('g_signal_connect_data');
  late final widgetShow = _lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>('gtk_widget_show');
  late final widgetDestroy = _lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>('gtk_widget_destroy');
  late final setModal = _lib.lookupFunction<Void Function(Pointer<Void>, Int32), void Function(Pointer<Void>, int)>('gtk_window_set_modal');
  late final listToplevels = _lib.lookupFunction<Pointer<Pointer<Void>> Function(), Pointer<Pointer<Void>> Function()>('gtk_window_list_toplevels');
  late final listFree = _lib.lookupFunction<Void Function(Pointer<Pointer<Void>>), void Function(Pointer<Pointer<Void>>)>('g_list_free');
  late final isActive = _lib.lookupFunction<Int32 Function(Pointer<Void>), int Function(Pointer<Void>)>('gtk_window_is_active');
  late final unref = _lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>('g_object_unref');

  Pointer<Uint8> cString(String s) {
    final bytes = const Utf8Encoder().convert(s);
    final p = malloc(bytes.length + 1);
    p.asTypedList(bytes.length + 1)
      ..setAll(0, bytes)
      ..[bytes.length] = 0;
    return p;
  }

  /// The application window, to keep the dialog on top of it.
  Pointer<Void> appWindow() {
    final list = listToplevels();
    var found = nullptr.cast<Void>();
    // GList: {data, next, prev}.
    for (var node = list; node != nullptr; node = (node + 1).value.cast()) {
      final window = node.value;
      if (found == nullptr || isActive(window) != 0) found = window;
    }
    if (list != nullptr) listFree(list);
    return found;
  }
}

/// GTK paper names for the core's paper ids (others use a custom size).
const _gtkPaper = {
  'a3': 'iso_a3', 'a4': 'iso_a4', 'a5': 'iso_a5', 'letter': 'na_letter', 'legal': 'na_legal',
  'env_dl': 'iso_dl', 'env_c4': 'iso_c4', 'env_c5': 'iso_c5', 'env_c6': 'iso_c6', 'env_10': 'na_number-10',
};

/// Whether GTK's print dialog can be used.
bool get gtkPrintAvailable => _Gtk.open() != null;

// Callbacks stay alive while a dialog or job is running.
final _alive = <NativeCallable>{};

/// Shows GTK's print dialog and sends [pdfPath] to the chosen printer.
Future<SystemPrintResult> printGtk({
  required String title,
  required String pdfPath,
  required double widthPt,
  required double heightPt,
  required String? paper,
}) {
  final gtk = _Gtk.open();
  if (gtk == null) throw SystemPrintError('The GTK print dialog is not available.');
  final done = Completer<SystemPrintResult>();
  final titleC = gtk.cString(title);
  final dialog = gtk.dialogNew(titleC, gtk.appWindow());
  gtk.free(titleC);
  if (dialog == nullptr) throw SystemPrintError('The print dialog could not be opened.');

  // Preselect the document's paper size and orientation.
  final setup = gtk.pageSetupNew();
  final gtkName = paper == null ? null : _gtkPaper[paper];
  Pointer<Void> size;
  if (gtkName != null) {
    final n = gtk.cString(gtkName);
    size = gtk.paperNew(n);
    gtk.free(n);
  } else {
    final n = gtk.cString('custom_akshara'), d = gtk.cString(title);
    final short = widthPt < heightPt ? widthPt : heightPt, long = widthPt < heightPt ? heightPt : widthPt;
    size = gtk.paperCustom(n, d, short, long, 1); // GTK_UNIT_POINTS
    gtk.free(n);
    gtk.free(d);
  }
  gtk.setPaperSize(setup, size);
  gtk.paperFree(size);
  gtk.setOrientation(setup, widthPt > heightPt ? 1 : 0);
  gtk.setPageSetup(dialog, setup);
  gtk.unref(setup);
  gtk.setCapabilities(dialog, 1 << 5); // GENERATE_PDF: "Print to File" offers PDF
  gtk.setModal(dialog, 1);

  late final NativeCallable<_ResponseC> onResponse;
  onResponse = NativeCallable<_ResponseC>.listener((Pointer<Void> d, int response, Pointer<Void> _) {
    _alive.remove(onResponse);
    onResponse.close();
    if (response != -5) {
      // GTK_RESPONSE_OK is -5; anything else cancels.
      gtk.widgetDestroy(dialog);
      done.complete(SystemPrintResult.cancelled);
      return;
    }
    final printer = gtk.selectedPrinter(dialog);
    if (printer == nullptr) {
      gtk.widgetDestroy(dialog);
      done.completeError(SystemPrintError('No printer was selected.'));
      return;
    }
    final settings = gtk.getSettings(dialog);
    final pageSetup = gtk.getPageSetup(dialog);
    final t = gtk.cString(title);
    final job = gtk.jobNew(t, printer, settings, pageSetup);
    gtk.free(t);
    gtk.unref(settings);
    gtk.widgetDestroy(dialog);
    final path = gtk.cString(pdfPath);
    final error = gtk.malloc(8).cast<Pointer<Void>>()..value = nullptr;
    final ok = gtk.jobSetSource(job, path, error);
    gtk.free(path);
    gtk.free(error.cast());
    if (ok == 0) {
      gtk.unref(job);
      done.completeError(SystemPrintError('The printer does not accept the document.'));
      return;
    }
    late final NativeCallable<_CompleteC> onComplete;
    onComplete = NativeCallable<_CompleteC>.listener((Pointer<Void> j, Pointer<Void> _, Pointer<Void> err) {
      _alive.remove(onComplete);
      onComplete.close();
      gtk.unref(job);
      if (err != nullptr) {
        done.completeError(SystemPrintError('Printing failed.'));
      } else {
        done.complete(SystemPrintResult.printed);
      }
    });
    _alive.add(onComplete);
    gtk.jobSend(job, onComplete.nativeFunction, nullptr, nullptr);
  });
  _alive.add(onResponse);
  final signal = gtk.cString('response');
  gtk.signalConnect(dialog, signal, onResponse.nativeFunction, nullptr, nullptr, 0);
  gtk.free(signal);
  gtk.widgetShow(dialog);
  return done.future;
}
