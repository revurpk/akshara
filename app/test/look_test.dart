// Native look and feel: theme per platform, the persisted choice, dialogs
// that follow platform conventions, and the Appearance menu.

import 'dart:io';

import 'package:akshara/core/look.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/main.dart';
import 'package:cupertino_ui/cupertino_ui.dart' show CupertinoAlertDialog;
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'helpers.dart';

/// A checked item in an open popup menu.
Finder menuItem(String label) => find.widgetWithText(CheckedPopupMenuItem<String>, label);

double radiusOf(OutlinedBorder? shape) =>
    ((shape as RoundedRectangleBorder).borderRadius as BorderRadius).topLeft.x;

/// Opens a [showChoice] dialog with Material-ordered choices and returns
/// the button labels in the order they are shown.
Future<List<String>> openChoice(WidgetTester tester, List<String?> result) async {
  await tester.pumpWidget(MaterialApp(
    theme: buildTheme(LookSettings.instance.look, Brightness.light),
    home: Builder(
      builder: (context) => TextButton(
        onPressed: () async => result.add(await showChoice<String>(
          context,
          title: 'Save changes?',
          message: 'Your changes will be lost.',
          choices: const [
            DialogChoice("Don't save", 'discard', destructive: true),
            DialogChoice('Cancel', 'cancel'),
            DialogChoice('Save', 'save', primary: true),
          ],
        )),
        child: const Text('open'),
      ),
    ),
  ));
  await tester.tap(find.text('open'));
  await tester.pumpAndSettle();
  final labels = ["Don't save", 'Cancel', 'Save'];
  labels.sort((a, b) => tester.getCenter(find.text(a)).dx.compareTo(tester.getCenter(find.text(b)).dx));
  return labels;
}

void main() {
  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-look-').path;
  });

  tearDown(() {
    debugDefaultTargetPlatformOverride = null;
    LookSettings.instance.style = UiStyle.native;
  });

  test('native is the default and the choice is saved', () {
    final settings = Settings.load();
    expect(settings.uiStyle, 'native');
    LookSettings.instance.load(settings);
    expect(LookSettings.instance.style, UiStyle.native);
    LookSettings.instance.style = UiStyle.material;
    expect(Settings.load().uiStyle, 'material');
    expect(LookSettings.instance.look, Look.material);
    LookSettings.instance.style = UiStyle.native;
    expect(Settings.load().uiStyle, 'native');
    expect(UiStyle.fromName('bogus'), UiStyle.native);
  });

  test('native resolves to the host platform', () {
    const expected = {
      TargetPlatform.windows: Look.windows,
      TargetPlatform.macOS: Look.macos,
      TargetPlatform.linux: Look.linux,
      TargetPlatform.android: Look.android,
      TargetPlatform.iOS: Look.ios,
      TargetPlatform.fuchsia: Look.android,
    };
    for (final e in expected.entries) {
      debugDefaultTargetPlatformOverride = e.key;
      expect(Look.resolve(UiStyle.native), e.value);
      expect(Look.resolve(UiStyle.material), Look.material);
    }
  });

  test('each look has its platform fonts, accent and shapes', () {
    final win = buildTheme(Look.windows, Brightness.light);
    expect(win.textTheme.bodyMedium!.fontFamily, 'Segoe UI Variable Text');
    expect(win.colorScheme.primary, const Color(0xFF005FB8));
    expect(radiusOf(win.filledButtonTheme.style!.shape!.resolve({})), 4);
    expect(win.splashFactory, NoSplash.splashFactory);

    final mac = buildTheme(Look.macos, Brightness.dark);
    expect(mac.textTheme.bodyMedium!.fontFamily, 'CupertinoSystemText');
    expect(mac.colorScheme.primary, const Color(0xFF0A84FF));
    expect(mac.visualDensity, VisualDensity.compact);
    expect(buildTheme(Look.ios, Brightness.light).visualDensity, VisualDensity.standard);

    final linux = buildTheme(Look.linux, Brightness.light);
    expect(linux.colorScheme.primary, const Color(0xFF3584E4));
    expect(radiusOf(linux.dialogTheme.shape as OutlinedBorder), 12);

    final material = buildTheme(Look.material, Brightness.light);
    expect(material.textTheme.bodyMedium!.fontFamily, 'NotoSans');

    // Every look keeps the bundled Indic fonts as fallbacks, in both modes.
    for (final look in Look.values) {
      for (final b in Brightness.values) {
        final theme = buildTheme(look, b);
        expect(theme.colorScheme.brightness, b);
        expect(theme.textTheme.bodyMedium!.fontFamilyFallback, contains('NotoSansTelugu'), reason: '$look');
      }
    }
  });

  testWidgets('dialogs follow platform conventions', (tester) async {
    final result = <String?>[];

    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    await openChoice(tester, result);
    expect(find.byType(CupertinoAlertDialog), findsOneWidget);
    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();
    expect(result.last, 'save');

    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    expect(await openChoice(tester, result), ['Save', "Don't save", 'Cancel']);
    expect(find.byType(AlertDialog), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(result.last, 'cancel');

    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    expect(await openChoice(tester, result), ["Don't save", 'Cancel', 'Save']);
    await tester.tap(find.text("Don't save"));
    await tester.pumpAndSettle();
    expect(result.last, 'discard');

    LookSettings.instance.style = UiStyle.material;
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    await openChoice(tester, result);
    expect(find.byType(CupertinoAlertDialog), findsNothing);
    expect(find.byType(AlertDialog), findsOneWidget);
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('the Appearance menu switches between native and Material', (tester) async {
    setLanguage(AppLanguage.en);
    final view = tester.view;
    view.physicalSize = const Size(1600, 1200);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);

    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    await tester.pumpWidget(const AksharaApp());
    await tester.pumpAndSettle();
    BuildContext ctx() => tester.element(find.text('Writer'));
    expect(Theme.of(ctx()).colorScheme.primary, const Color(0xFF3584E4));

    await tester.tap(find.byTooltip('Appearance'));
    await tester.pumpAndSettle();
    expect(find.text('Native (Linux)'), findsOneWidget);
    await tester.tap(menuItem('Material Design'));
    await tester.pumpAndSettle();
    expect(LookSettings.instance.style, UiStyle.material);
    expect(Settings.load().uiStyle, 'material');
    expect(Theme.of(ctx()).textTheme.bodyMedium!.fontFamily, 'NotoSans');

    await tester.tap(find.byTooltip('Appearance'));
    await tester.pumpAndSettle();
    await tester.tap(menuItem('dark'));
    await tester.pumpAndSettle();
    expect(Theme.of(ctx()).brightness, Brightness.dark);
    expect(Settings.load().themeMode, 'dark');

    await tester.tap(find.byTooltip('Appearance'));
    await tester.pumpAndSettle();
    await tester.tap(menuItem('Native (Linux)'));
    await tester.pumpAndSettle();
    expect(Theme.of(ctx()).colorScheme.primary, const Color(0xFF62A0EA));
    debugDefaultTargetPlatformOverride = null;
  }, skip: useBuiltCore() == null);
}
