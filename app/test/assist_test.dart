// Widget tests of voice input and the AI chat, against the real core and a
// fake Ollama server on the loopback interface.

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:akshara/assist/assist_settings.dart';
import 'package:akshara/assist/chat_panel.dart';
import 'package:akshara/assist/system_speech.dart';
import 'package:akshara/assist/voice_button.dart';
import 'package:akshara/core/document.dart';
import 'package:akshara/core/native.dart';
import 'package:akshara/core/paths.dart';
import 'package:akshara/l10n/l10n.dart';
import 'package:akshara/writer/writer_page.dart';

import 'helpers.dart';

void main() {
  final skip = useBuiltCore() == null ? 'native library not built' : null;

  setUpAll(() {
    settingsDirectoryOverride = Directory.systemTemp.createTempSync('akshara-test-').path;
    AssistSettings.instance.load(Settings.load());
  });

  setUp(() {
    setLanguage(AppLanguage.en);
    final view = TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher.views.first;
    view.physicalSize = const Size(1800, 1200);
    view.devicePixelRatio = 1.0;
    AssistSettings.instance
      ..aiEnabled = false
      ..voiceModel = null
      ..voiceEngine = 'whisper'
      ..voiceLanguage = 'auto';
    ChatSession.instance.clear();
  });

  test('voice languages are English, Telugu, Hindi and Sanskrit', () {
    expect(voiceLanguages, ['en', 'te', 'hi', 'sa']);
    expect(voiceLanguageName('te'), 'Telugu');
    expect(voiceLanguageBadge('sa'), 'सं');
    AssistSettings.instance.voiceLanguage = 'xx';
    expect(AssistSettings.instance.voiceLanguage, 'auto');
    expect(AssistSettings.instance.aiUrl, defaultOllamaUrl);
  });

  testWidgets('the microphone asks for a speech model first', (tester) async {
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: Center(child: VoiceButton(onText: (_) {})))));
    expect(find.byIcon(Icons.mic_none), findsOneWidget);
    await tester.tap(find.byIcon(Icons.mic_none));
    await tester.pumpAndSettle();
    expect(find.text('Voice input needs a speech model'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.mic_none), findsOneWidget);
  }, skip: skip != null);

  group('system speech recognition', () {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    final calls = <String>[];

    /// Fakes the runner's recogniser: [languages] offline, and a status
    /// that says "recording" until stopped, then [text].
    void fake({List<String> languages = const ['en', 'hi'], String text = 'hello world'}) {
      var stopped = false;
      messenger.setMockMethodCallHandler(SystemSpeech.channel, (call) async {
        calls.add(call.method);
        switch (call.method) {
          case 'available':
            return {'available': true, 'languages': languages};
          case 'stop':
            stopped = true;
          case 'status':
            return stopped ? {'state': 'done', 'text': text} : {'state': 'recording', 'level': 0.1, 'seconds': 1.0};
        }
        return null;
      });
    }

    // Channel replies complete outside the fake clock: let real time pass,
    // then advance the fake one past the 100 ms polling interval.
    Future<void> settle(WidgetTester tester) async {
      for (var i = 0; i < 3; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 5)));
        await tester.pump(const Duration(milliseconds: 120));
      }
    }

    setUp(calls.clear);
    tearDown(() async {
      messenger.setMockMethodCallHandler(SystemSpeech.channel, null);
      await SystemSpeech.info(refresh: true);
    });

    test('is unavailable without a platform recogniser', () async {
      final info = await SystemSpeech.info(refresh: true);
      expect(info.available, isFalse);
      AssistSettings.instance.voiceEngine = 'bogus';
      expect(AssistSettings.instance.voiceEngine, 'whisper');
    });

    testWidgets('dictates with the system recogniser, without a Whisper model', (tester) async {
      fake();
      await tester.runAsync(() => SystemSpeech.info(refresh: true));
      AssistSettings.instance
        ..voiceEngine = 'system'
        ..voiceLanguage = 'en';
      final got = <String>[];
      await tester.pumpWidget(MaterialApp(home: Scaffold(body: Center(child: VoiceButton(onText: got.add)))));
      await tester.tap(find.byIcon(Icons.mic_none));
      await settle(tester);
      expect(find.text('Voice input needs a speech model'), findsNothing);
      expect(find.byIcon(Icons.mic), findsOneWidget);
      await tester.tap(find.byIcon(Icons.mic));
      await settle(tester);
      await settle(tester);
      expect(got, ['hello world']);
      expect(calls, containsAllInOrder(['start', 'status', 'stop', 'status']));
      expect(find.byIcon(Icons.mic_none), findsOneWidget);
    });

    testWidgets('refuses a language the system cannot recognise offline', (tester) async {
      fake(languages: ['en']);
      await tester.runAsync(() => SystemSpeech.info(refresh: true));
      AssistSettings.instance
        ..voiceEngine = 'system'
        ..voiceLanguage = 'te';
      await tester.pumpWidget(MaterialApp(home: Scaffold(body: Center(child: VoiceButton(onText: (_) {})))));
      await tester.tap(find.byIcon(Icons.mic_none));
      await settle(tester);
      expect(find.textContaining('cannot recognise Telugu offline'), findsOneWidget);
      expect(calls, isNot(contains('start')));
    });

    testWidgets('settings offer the engine only when the system has one', (tester) async {
      await tester.runAsync(() => SystemSpeech.info(refresh: true));
      await tester.pumpWidget(MaterialApp(home: Scaffold(body: Builder(builder: (c) => TextButton(onPressed: () => showAssistSettings(c), child: const Text('open'))))));
      await tester.tap(find.text('open'));
      await settle(tester);
      await tester.pumpAndSettle();
      expect(find.text('Speech engine'), findsNothing);
      await tester.tap(find.text('Close'));
      await tester.pumpAndSettle();

      fake();
      await tester.runAsync(() => SystemSpeech.info(refresh: true));
      await tester.tap(find.text('open'));
      await settle(tester);
      await tester.pumpAndSettle();
      expect(find.text('Speech engine'), findsOneWidget);
      await tester.tap(find.text('Whisper (built in)'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('System speech recognition').last);
      await tester.pumpAndSettle();
      expect(AssistSettings.instance.voiceEngine, 'system');
      expect(find.textContaining('Languages available offline: English, Hindi'), findsOneWidget);
      expect(find.text('No speech model chosen'), findsNothing);
    });
  });

  testWidgets('chat explains how to turn it on', (tester) async {
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: ChatPanel(onNewDocument: (_) {}))));
    expect(find.text('Set up AI chat…'), findsOneWidget);
    await tester.tap(find.text('Set up AI chat…'));
    await tester.pumpAndSettle();
    expect(find.text('Voice and AI settings'), findsOneWidget);
    expect(find.text('Use Ollama on this computer'), findsOneWidget);
  });

  testWidgets('a chat answer is inserted into a Writer document', (tester) async {
    // A fake Ollama that answers every chat with a small Markdown document.
    final server = (await tester.runAsync(() => HttpServer.bind(InternetAddress.loopbackIPv4, 0)))!;
    server.listen((req) async {
      await utf8.decoder.bind(req).join();
      req.response.headers.contentType = ContentType('application', 'x-ndjson');
      req.response.write('${jsonEncode({'message': {'content': '# Leave letter\n\nDear '}, 'done': false})}\n');
      req.response.write('${jsonEncode({'message': {'content': 'Principal,'}, 'done': true})}\n');
      await req.response.close();
    });
    addTearDown(() => server.close(force: true));
    AssistSettings.instance
      ..aiEnabled = true
      ..aiUrl = 'http://127.0.0.1:${server.port}'
      ..aiModel = 'test-model';

    final r = core({'op': 'new', 'kind': 'writer'}) as Map<String, dynamic>;
    final doc = OpenDoc(r['doc'] as int, DocKind.writer, null);
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: WriterPage(doc: doc))));
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.mic_none), findsOneWidget);
    await tester.tap(find.byTooltip('AI chat'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).last, 'Write a leave letter');
    await tester.tap(find.byTooltip('Send'));
    for (var i = 0; i < 100 && find.text('Insert into document').evaluate().isEmpty; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 30)));
      await tester.pump(const Duration(milliseconds: 150));
    }
    expect(find.textContaining('Dear Principal,'), findsOneWidget);
    await tester.tap(find.text('Insert into document'));
    await tester.pumpAndSettle();

    final blocks = (core({'op': 'writer.get', 'doc': doc.handle}) as Map<String, dynamic>)['blocks'] as List<dynamic>;
    final heading = blocks.cast<Map<String, dynamic>>().firstWhere((b) => b['kind'] == 'heading');
    expect((heading['runs'] as List<dynamic>).first['text'], 'Leave letter');
    core({'op': 'close', 'doc': doc.handle});
  }, skip: skip != null);
}
