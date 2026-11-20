// Widget tests of voice input and the AI chat, against the real core and a
// fake Ollama server on the loopback interface.

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:akshara/assist/assist_settings.dart';
import 'package:akshara/assist/chat_panel.dart';
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
