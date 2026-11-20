// The AI chat: a conversation with a model running in Ollama on this
// computer. Answers come back as Markdown and can become a new Writer
// document or be inserted into the open one.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../core/fonts.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import 'assist_settings.dart';
import 'voice_button.dart';

class ChatMessage {
  ChatMessage(this.role, this.text, {this.error});

  final String role; // 'user' or 'assistant'
  String text;
  String? error;
}

/// The conversation, shared by every chat panel.
class ChatSession extends ChangeNotifier {
  ChatSession._();

  static final ChatSession instance = ChatSession._();

  final List<ChatMessage> messages = [];
  int? _job;
  Timer? _poll;

  bool get busy => _job != null;

  void clear() {
    stop();
    messages.clear();
    notifyListeners();
  }

  void send(String text) {
    final s = AssistSettings.instance;
    if (busy || text.trim().isEmpty) return;
    messages.add(ChatMessage('user', text.trim()));
    final history = [
      for (final m in messages)
        if (m.error == null && m.text.isNotEmpty) {'role': m.role, 'content': m.text},
    ];
    final reply = ChatMessage('assistant', '');
    messages.add(reply);
    final model = s.aiModel;
    if (model == null) {
      reply.error = t('Choose a model in Voice and AI settings first.');
      notifyListeners();
      return;
    }
    try {
      final r = core({'op': 'ai.chat', 'url': s.aiUrl, 'model': model, 'messages': history}) as Map<String, dynamic>;
      _job = r['job'] as int;
      _poll = Timer.periodic(const Duration(milliseconds: 120), (_) => _tick(reply));
    } on CoreError catch (e) {
      reply.error = e.message;
    }
    notifyListeners();
  }

  void _tick(ChatMessage reply) {
    final job = _job;
    if (job == null) return;
    try {
      final r = core({'op': 'ai.poll', 'job': job}) as Map<String, dynamic>;
      reply.text = r['text'] as String;
      if (r['done'] == true) {
        reply.error = r['error'] as String?;
        _finish();
      }
    } on CoreError catch (e) {
      reply.error = e.message;
      _finish();
    }
    notifyListeners();
  }

  void _finish() {
    _poll?.cancel();
    _poll = null;
    _job = null;
  }

  void stop() {
    final job = _job;
    if (job != null) {
      try {
        core({'op': 'ai.cancel', 'job': job});
      } catch (_) {}
    }
    _finish();
    notifyListeners();
  }
}

class ChatPanel extends StatefulWidget {
  const ChatPanel({super.key, required this.onNewDocument, this.onInsert, this.onClose});

  /// Opens a new Writer document (core handle).
  final void Function(int handle) onNewDocument;

  /// Inserts Markdown into the current document, when there is one.
  final void Function(String markdown)? onInsert;

  final VoidCallback? onClose;

  @override
  State<ChatPanel> createState() => _ChatPanelState();
}

class _ChatPanelState extends State<ChatPanel> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  final _focus = FocusNode();
  final session = ChatSession.instance;
  final settings = AssistSettings.instance;

  @override
  void initState() {
    super.initState();
    session.addListener(_changed);
    settings.addListener(_changed);
  }

  @override
  void dispose() {
    session.removeListener(_changed);
    settings.removeListener(_changed);
    _input.dispose();
    _scroll.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _changed() {
    if (!mounted) return;
    setState(() {});
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  void _send() {
    final text = _input.text;
    if (text.trim().isEmpty || session.busy) return;
    _input.clear();
    session.send(text);
    _focus.requestFocus();
  }

  void _newDocument(String markdown) {
    try {
      final r = core({'op': 'new_markdown', 'text': markdown}) as Map<String, dynamic>;
      widget.onNewDocument(r['doc'] as int);
    } on CoreError catch (e) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(e.message)));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Material(
      color: theme.colorScheme.surfaceContainerLow,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 4, 4, 4),
            child: Row(
              children: [
                Icon(Icons.auto_awesome_outlined, size: 20, color: theme.colorScheme.primary),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    settings.aiModel == null ? t('AI chat') : '${t('AI chat')} · ${settings.aiModel}',
                    style: theme.textTheme.titleSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                IconButton(tooltip: t('New chat'), icon: const Icon(Icons.add_comment_outlined), onPressed: session.messages.isEmpty ? null : session.clear),
                IconButton(tooltip: t('Voice and AI settings…'), icon: const Icon(Icons.tune), onPressed: () => showAssistSettings(context)),
                if (widget.onClose != null) IconButton(tooltip: t('Close'), icon: const Icon(Icons.close), onPressed: widget.onClose),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(child: settings.aiEnabled ? _conversation(theme) : _off(theme)),
          if (settings.aiEnabled) ...[const Divider(height: 1), _composer(theme)],
        ],
      ),
    );
  }

  Widget _off(ThemeData theme) => Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.auto_awesome_outlined, size: 40, color: theme.colorScheme.outline),
              const SizedBox(height: 12),
              Text(
                t('The AI chat uses a model running on this computer in Ollama (ollama.com). Akshara never sends your text to the internet.'),
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 12),
              FilledButton(onPressed: () => showAssistSettings(context), child: Text(t('Set up AI chat…'))),
            ],
          ),
        ),
      );

  Widget _conversation(ThemeData theme) {
    if (session.messages.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            t('Ask for a document, for example: "Write a letter to the school principal asking for two days of leave", or "Make a project plan with a flowchart".'),
            textAlign: TextAlign.center,
            style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        ),
      );
    }
    return ListView.builder(
      controller: _scroll,
      padding: const EdgeInsets.all(12),
      itemCount: session.messages.length,
      itemBuilder: (context, i) => _bubble(theme, session.messages[i], last: i == session.messages.length - 1),
    );
  }

  Widget _bubble(ThemeData theme, ChatMessage m, {required bool last}) {
    final user = m.role == 'user';
    final waiting = !user && last && session.busy;
    final body = m.text.isEmpty && waiting
        ? Row(mainAxisSize: MainAxisSize.min, children: [
            const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2)),
            const SizedBox(width: 8),
            Text(t('Thinking…')),
          ])
        : SelectableText(m.text, style: const TextStyle(fontFamilyFallback: indicFallback));
    return Align(
      alignment: user ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.all(10),
        constraints: const BoxConstraints(maxWidth: 560),
        decoration: BoxDecoration(
          color: user ? theme.colorScheme.primaryContainer : theme.colorScheme.surface,
          borderRadius: BorderRadius.circular(10),
          border: user ? null : Border.all(color: theme.colorScheme.outlineVariant),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (m.text.isNotEmpty || waiting) body,
            if (m.error != null)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(m.error!, style: TextStyle(color: theme.colorScheme.error)),
              ),
            if (!user && m.text.isNotEmpty && !waiting)
              Wrap(
                spacing: 4,
                children: [
                  TextButton.icon(
                    icon: const Icon(Icons.note_add_outlined, size: 18),
                    label: Text(t('New document')),
                    onPressed: () => _newDocument(m.text),
                  ),
                  if (widget.onInsert != null)
                    TextButton.icon(
                      icon: const Icon(Icons.input, size: 18),
                      label: Text(t('Insert into document')),
                      onPressed: () => widget.onInsert!(m.text),
                    ),
                  IconButton(
                    tooltip: t('Copy'),
                    icon: const Icon(Icons.copy, size: 18),
                    onPressed: () => Clipboard.setData(ClipboardData(text: m.text)),
                  ),
                ],
              ),
          ],
        ),
      ),
    );
  }

  Widget _composer(ThemeData theme) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          Expanded(
            child: CallbackShortcuts(
              bindings: {const SingleActivator(LogicalKeyboardKey.enter): _send},
              child: TextField(
                controller: _input,
                focusNode: _focus,
                minLines: 1,
                maxLines: 6,
                style: const TextStyle(fontFamilyFallback: indicFallback),
                decoration: InputDecoration(
                  hintText: t('Message (Enter to send, Shift+Enter for a new line)'),
                  border: const OutlineInputBorder(),
                  isDense: true,
                ),
              ),
            ),
          ),
          VoiceButton(
            showLanguage: false,
            onText: (text) {
              final v = _input.value;
              final at = v.selection.isValid ? v.selection.end : v.text.length;
              final before = v.text.substring(0, at);
              final insert = (before.isEmpty || before.endsWith(' ') || before.endsWith('\n')) ? text : ' $text';
              _input.value = TextEditingValue(
                text: before + insert + v.text.substring(at),
                selection: TextSelection.collapsed(offset: at + insert.length),
              );
              _focus.requestFocus();
            },
          ),
          session.busy
              ? IconButton(tooltip: t('Stop'), icon: const Icon(Icons.stop_circle_outlined), onPressed: session.stop)
              : IconButton(tooltip: t('Send'), icon: const Icon(Icons.send), onPressed: _send),
        ],
      ),
    );
  }
}
