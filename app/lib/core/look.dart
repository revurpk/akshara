// Platform look and feel.
//
// "Native" (the default) styles the interface after the platform it runs on:
// Fluent on Windows, macOS and iOS (Cupertino), GNOME/Adwaita on Linux, and
// Material 3 on Android. "Material" uses one brand-coloured Material Design
// look everywhere. Only first-party Flutter libraries are used: Material
// components restyled per platform, plus Cupertino dialogs on Apple systems.
//
// Documents are not affected: pages, cells and formulas keep the bundled
// Noto fonts so they look the same on every device.

import 'package:cupertino_ui/cupertino_ui.dart'
    show
        CupertinoAlertDialog,
        CupertinoDialogAction,
        CupertinoPageTransitionsBuilder,
        CupertinoThemeData,
        showCupertinoDialog;
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

import 'fonts.dart';
import 'paths.dart';

/// Brand colour used by the Material look.
const brandSaffron = Color(0xFFD9640F);

/// The user's choice, persisted in the settings file.
enum UiStyle {
  native,
  material;

  static UiStyle fromName(String? name) =>
      UiStyle.values.firstWhere((s) => s.name == name, orElse: () => UiStyle.native);
}

/// A concrete look: one of the platform styles, or Material.
enum Look {
  material('Material'),
  windows('Windows'),
  macos('macOS'),
  linux('Linux'),
  android('Android'),
  ios('iOS');

  const Look(this.platformName);

  /// Name of the platform this look imitates (not translated).
  final String platformName;

  /// The native look of the platform the app is running on.
  static Look get host => switch (defaultTargetPlatform) {
    TargetPlatform.windows => Look.windows,
    TargetPlatform.macOS => Look.macos,
    TargetPlatform.linux => Look.linux,
    TargetPlatform.iOS => Look.ios,
    TargetPlatform.android || TargetPlatform.fuchsia => Look.android,
  };

  static Look resolve(UiStyle style) => style == UiStyle.native ? host : Look.material;

  bool get isApple => this == Look.macos || this == Look.ios;

  bool get isDesktop => this == Look.windows || this == Look.macos || this == Look.linux;
}

/// The current interface style. The app rebuilds its theme when it changes.
class LookSettings extends ChangeNotifier {
  LookSettings._();

  static final LookSettings instance = LookSettings._();

  Settings? _settings;
  UiStyle _style = UiStyle.native;

  UiStyle get style => _style;

  Look get look => Look.resolve(_style);

  void load(Settings settings) {
    _settings = settings;
    _style = UiStyle.fromName(settings.uiStyle);
    notifyListeners();
  }

  set style(UiStyle value) {
    if (value == _style) return;
    _style = value;
    _settings?.uiStyle = value.name;
    notifyListeners();
  }
}

/// Visual parameters of each look.
class _Palette {
  const _Palette({
    required this.accentLight,
    required this.accentDark,
    required this.fonts,
    required this.controlRadius,
    required this.windowRadius,
    required this.light,
    required this.dark,
  });

  final Color accentLight;
  final Color accentDark;

  /// System interface fonts, best first. The bundled Noto fonts follow, so
  /// Indic scripts always render even when a system font lacks them.
  final List<String> fonts;
  final double controlRadius; // buttons, fields, checkboxes
  final double windowRadius; // dialogs, menus
  final _Surfaces light;
  final _Surfaces dark;
}

class _Surfaces {
  const _Surfaces(this.window, this.view, this.chrome, this.raised, this.text, this.line);

  final Color window; // scaffold background
  final Color view; // content areas, dialogs, menus
  final Color chrome; // title and tool bars
  final Color raised; // hover/selected containers
  final Color text;
  final Color line; // dividers and outlines
}

const _windows = _Palette(
  // Windows 11 default accent; Segoe UI Variable (Windows 11), Segoe UI (10).
  accentLight: Color(0xFF005FB8),
  accentDark: Color(0xFF60CDFF),
  fonts: ['Segoe UI Variable Text', 'Segoe UI'],
  controlRadius: 4,
  windowRadius: 8,
  light: _Surfaces(Color(0xFFF3F3F3), Color(0xFFFBFBFB), Color(0xFFEEEEEE), Color(0xFFE9E9E9),
      Color(0xFF1B1B1B), Color(0xFFE0E0E0)),
  dark: _Surfaces(Color(0xFF202020), Color(0xFF2B2B2B), Color(0xFF1C1C1C), Color(0xFF383838),
      Color(0xFFFFFFFF), Color(0xFF3D3D3D)),
);

const _apple = _Palette(
  // System blue; the engine maps CupertinoSystemText to San Francisco.
  accentLight: Color(0xFF007AFF),
  accentDark: Color(0xFF0A84FF),
  fonts: ['CupertinoSystemText'],
  controlRadius: 6,
  windowRadius: 10,
  light: _Surfaces(Color(0xFFECECEC), Color(0xFFFFFFFF), Color(0xFFE3E3E3), Color(0xFFDCDCDC),
      Color(0xFF1D1D1F), Color(0xFFD4D4D4)),
  dark: _Surfaces(Color(0xFF1E1E1E), Color(0xFF2A2A2A), Color(0xFF323232), Color(0xFF3A3A3A),
      Color(0xFFF5F5F7), Color(0xFF424242)),
);

const _linux = _Palette(
  // GNOME/Adwaita blue and greys; system fonts are found through fontconfig.
  accentLight: Color(0xFF3584E4),
  accentDark: Color(0xFF62A0EA),
  fonts: ['Adwaita Sans', 'Cantarell', 'Ubuntu', 'DejaVu Sans'],
  controlRadius: 6,
  windowRadius: 12,
  light: _Surfaces(Color(0xFFFAFAFA), Color(0xFFFFFFFF), Color(0xFFEBEBEB), Color(0xFFE0E0E0),
      Color(0xFF2E3436), Color(0xFFDEDDDA)),
  dark: _Surfaces(Color(0xFF242424), Color(0xFF1E1E1E), Color(0xFF303030), Color(0xFF3A3A3A),
      Color(0xFFFFFFFF), Color(0xFF3D3D3D)),
);

const _tooltip = TooltipThemeData(waitDuration: Duration(milliseconds: 500));

/// Builds the theme for [look] in the given [brightness].
ThemeData buildTheme(Look look, Brightness brightness) {
  switch (look) {
    case Look.material:
      return ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: brandSaffron, brightness: brightness),
        useMaterial3: true,
        fontFamily: uiFont,
        fontFamilyFallback: indicFallback,
        visualDensity: VisualDensity.compact,
        tooltipTheme: _tooltip,
      );
    case Look.android:
      // Material 3 is Android's own design; Roboto is the system font.
      return ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: brandSaffron, brightness: brightness),
        useMaterial3: true,
        fontFamily: 'Roboto',
        fontFamilyFallback: const [uiFont, ...indicFallback],
        tooltipTheme: _tooltip,
      );
    case Look.windows:
      return _desktopTheme(look, _windows, brightness);
    case Look.macos:
    case Look.ios:
      return _desktopTheme(look, _apple, brightness);
    case Look.linux:
      return _desktopTheme(look, _linux, brightness);
  }
}

ThemeData _desktopTheme(Look look, _Palette p, Brightness brightness) {
  final dark = brightness == Brightness.dark;
  final s = dark ? p.dark : p.light;
  final accent = dark ? p.accentDark : p.accentLight;
  final onAccent = dark && look == Look.windows ? Colors.black : Colors.white;
  final scheme = ColorScheme.fromSeed(seedColor: accent, brightness: brightness).copyWith(
    primary: accent,
    onPrimary: onAccent,
    surface: s.view,
    onSurface: s.text,
    surfaceContainerLowest: s.view,
    surfaceContainerLow: s.window,
    surfaceContainer: s.window,
    surfaceContainerHigh: s.chrome,
    surfaceContainerHighest: s.raised,
    outlineVariant: s.line,
    surfaceTint: Colors.transparent,
  );
  final control = RoundedRectangleBorder(borderRadius: BorderRadius.circular(p.controlRadius));
  final window = RoundedRectangleBorder(borderRadius: BorderRadius.circular(p.windowRadius));
  final touch = look == Look.ios;
  final buttonSize = WidgetStatePropertyAll(Size(0, touch ? 44 : 32));
  final buttonPadding = const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 14));
  ButtonStyle button() => ButtonStyle(
    shape: WidgetStatePropertyAll(control),
    minimumSize: buttonSize,
    padding: buttonPadding,
    elevation: const WidgetStatePropertyAll(0),
  );
  final transitions = PageTransitionsTheme(
    builders: {
      for (final platform in TargetPlatform.values)
        platform: look.isApple
            ? const CupertinoPageTransitionsBuilder()
            : const FadeForwardsPageTransitionsBuilder(),
    },
  );
  final fonts = [...p.fonts.skip(1), uiFont, ...indicFallback];
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    fontFamily: p.fonts.first,
    fontFamilyFallback: fonts,
    scaffoldBackgroundColor: s.window,
    canvasColor: s.window,
    dividerColor: s.line,
    visualDensity: touch ? VisualDensity.standard : VisualDensity.compact,
    materialTapTargetSize: touch ? MaterialTapTargetSize.padded : MaterialTapTargetSize.shrinkWrap,
    // Desktop systems and iOS do not draw Material ink ripples.
    splashFactory: NoSplash.splashFactory,
    pageTransitionsTheme: transitions,
    filledButtonTheme: FilledButtonThemeData(style: button()),
    elevatedButtonTheme: ElevatedButtonThemeData(style: button()),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: button().copyWith(side: WidgetStatePropertyAll(BorderSide(color: s.line))),
    ),
    textButtonTheme: TextButtonThemeData(style: button()),
    iconButtonTheme: IconButtonThemeData(
      style: ButtonStyle(shape: WidgetStatePropertyAll(control)),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: s.view,
      surfaceTintColor: Colors.transparent,
      shape: window,
      elevation: 8,
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: s.view,
      surfaceTintColor: Colors.transparent,
      shape: window.copyWith(side: BorderSide(color: s.line)),
      elevation: 4,
    ),
    menuTheme: MenuThemeData(
      style: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(s.view),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        shape: WidgetStatePropertyAll(window),
      ),
    ),
    inputDecorationTheme: InputDecorationThemeData(
      isDense: true,
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(p.controlRadius)),
    ),
    checkboxTheme: CheckboxThemeData(
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(p.controlRadius - 1)),
    ),
    dividerTheme: DividerThemeData(color: s.line, space: 1),
    scrollbarTheme: ScrollbarThemeData(
      thickness: const WidgetStatePropertyAll(6),
      radius: const Radius.circular(3),
    ),
    tooltipTheme: _tooltip.copyWith(
      decoration: BoxDecoration(
        color: s.view,
        border: Border.all(color: s.line),
        borderRadius: BorderRadius.circular(p.controlRadius),
      ),
      textStyle: TextStyle(color: s.text, fontSize: 12),
    ),
    cupertinoOverrideTheme: look.isApple
        ? CupertinoThemeData(primaryColor: accent, brightness: brightness)
        : null,
  );
}

/// One button of a [showChoice] dialog.
class DialogChoice<T> {
  const DialogChoice(this.label, this.value, {this.primary = false, this.destructive = false});

  final String label;
  final T value;

  /// The default action (filled button; Return activates it).
  final bool primary;

  /// Loses data (red on Apple systems).
  final bool destructive;
}

/// Shows a short message with buttons, following the platform's conventions:
/// a Cupertino alert on macOS and iOS; the primary button first on Windows;
/// and last elsewhere. [choices] are given in the Material order, with the
/// primary action last.
Future<T?> showChoice<T>(
  BuildContext context, {
  required String title,
  String? message,
  Widget? icon,
  required List<DialogChoice<T>> choices,
}) {
  final look = LookSettings.instance.look;
  if (look.isApple) {
    return showCupertinoDialog<T>(
      context: context,
      barrierDismissible: true,
      builder: (c) => CupertinoAlertDialog(
        title: Text(title),
        content: message == null ? null : Text(message),
        actions: [
          for (final choice in choices)
            CupertinoDialogAction(
              isDefaultAction: choice.primary,
              isDestructiveAction: choice.destructive,
              onPressed: () => Navigator.pop(c, choice.value),
              child: Text(choice.label),
            ),
        ],
      ),
    );
  }
  // Windows puts the default button first (Save, Don't save, Cancel);
  // Material and GNOME put it last.
  final ordered = look == Look.windows
      ? [...choices.where((c) => c.primary), ...choices.where((c) => !c.primary)]
      : choices;
  return showDialog<T>(
    context: context,
    builder: (c) => AlertDialog(
      icon: icon,
      title: Text(title),
      content: message == null ? null : SelectableText(message),
      actions: [
        for (final choice in ordered)
          choice.primary
              ? FilledButton(
                  autofocus: true,
                  onPressed: () => Navigator.pop(c, choice.value),
                  child: Text(choice.label),
                )
              : TextButton(
                  onPressed: () => Navigator.pop(c, choice.value),
                  child: Text(choice.label),
                ),
      ],
    ),
  );
}
