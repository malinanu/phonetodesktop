import 'package:flutter/material.dart';

import '../app/connection.dart';
import '../app/files_url.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import 'files_screen.dart';
import 'widgets.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, required this.settings, required this.link, this.onAddPc});
  final AppSettings settings;
  final ConnectionController link;
  final VoidCallback? onAddPc;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late final TextEditingController _files = TextEditingController(text: widget.settings.filesUrl);
  String? _filesError;

  AppSettings get s => widget.settings;

  @override
  void dispose() {
    _files.dispose();
    super.dispose();
  }

  Widget _seg<T>(String title, String hint, T value, Map<T, String> options, String key) {
    return _row(
      title,
      hint,
      SegmentedButton<T>(
        showSelectedIcon: false,
        segments: [for (final e in options.entries) ButtonSegment(value: e.key, label: Text(e.value))],
        selected: {value},
        onSelectionChanged: (v) => s.set(key, v.first as Object),
      ),
      stacked: true,
    );
  }

  Widget _switch(String title, String hint, bool value, String key) =>
      _row(title, hint, Switch(value: value, onChanged: (v) => s.set(key, v)));

  Widget _row(String title, String hint, Widget trailing, {bool stacked = false}) {
    final c = context.pr;
    final text = Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(title, style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w700)),
      if (hint.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 2), child: Text(hint, style: TextStyle(color: c.dim, fontSize: 14))),
    ]);
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 12),
      decoration: BoxDecoration(border: Border(top: BorderSide(color: c.line))),
      child: stacked
          ? Column(crossAxisAlignment: CrossAxisAlignment.start, children: [text, const SizedBox(height: 10), SizedBox(width: double.infinity, child: trailing)])
          : Row(children: [Expanded(child: text), const SizedBox(width: 12), trailing]),
    );
  }

  Widget _h(String t) => Padding(padding: const EdgeInsets.only(top: 26, bottom: 4), child: Kicker(t));

  @override
  Widget build(BuildContext context) {
    final c = context.pr;
    return Scaffold(
      appBar: AppBar(title: const Text('Settings', style: TextStyle(fontWeight: FontWeight.w700))),
      body: ListenableBuilder(
        listenable: Listenable.merge([s, widget.link]),
        builder: (context, _) => ListView(padding: const EdgeInsets.fromLTRB(20, 0, 20, 32), children: [
          _h('Look'),
          _seg('Theme', 'Follow your phone, or pick one.', s.theme, const {'system': 'System', 'dark': 'Dark', 'light': 'Light'}, 'theme'),
          _h('Remote'),
          _seg('Skip step', 'How far the skip buttons jump.', s.skip, const {5: '5 s', 10: '10 s', 15: '15 s', 30: '30 s'}, 'skip'),
          _seg('Volume step', 'How much each volume tap changes.', s.volStep, const {1: '1%', 2: '2%', 5: '5%', 10: '10%'}, 'volStep'),
          _h('Touchpad'),
          _row('Speed', 'How fast the pointer moves.', SizedBox(width: 170, child: Slider(value: s.padSpeed, min: .5, max: 2, divisions: 15, label: '${s.padSpeed.toStringAsFixed(1)}×', onChanged: (v) => s.set('padSpeed', v)))),
          _seg('Scroll direction', 'Natural moves the page with your fingers.', s.scrollNatural, const {true: 'Natural', false: 'Classic'}, 'scrollNatural'),
          _switch('Tap to click', 'Off means use the Left and Right buttons only.', s.tapToClick, 'tapToClick'),
          _h('Comfort'),
          _switch('Vibration', 'A small buzz when you press a button.', s.vibrate, 'vibrate'),
          _switch('Keep screen on', 'Stops the phone sleeping while the app is open.', s.keepAwake, 'keepAwake'),
          _h('Send files'),
          _row(
            'File server',
            'The address of your Send files server. Leave empty to use the app’s built-in address${buildFilesUrl.isEmpty ? ' (none set in this build)' : ''}.',
            const SizedBox.shrink(),
          ),
          TextField(
            controller: _files,
            keyboardType: TextInputType.url,
            autocorrect: false,
            decoration: InputDecoration(hintText: 'https://files.example.com', errorText: _filesError),
            onChanged: (v) {
              final trimmed = v.trim();
              final cleaned = trimmed.isEmpty ? '' : FilesUrl.clean(trimmed);
              setState(() => _filesError = cleaned == null ? 'Use an https:// address, like files.example.com' : null);
              if (cleaned != null) s.set('filesUrl', cleaned);
            },
          ),
          _h('Your PCs'),
          if (widget.link.pcs.isEmpty) Text('No PC is paired yet.', style: TextStyle(color: c.dim)),
          for (final pc in widget.link.pcs)
            _row(pc.name, '${pc.host}:${pc.port}${pc.id == widget.link.active?.id ? ' · open now' : ''}', TextButton(onPressed: () => _confirmForget(pc.id, pc.name), child: const Text('Forget'))),
          const SizedBox(height: 10),
          OutlinedButton.icon(onPressed: widget.onAddPc, icon: const Icon(Icons.qr_code_scanner_rounded), label: const Text('Add a PC')),
          const SizedBox(height: 26),
          TextButton(onPressed: () => s.reset(), child: const Text('Reset settings to defaults')),
        ]),
      ),
    );
  }

  Future<void> _confirmForget(String id, String name) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (_) => AlertDialog(
        title: Text('Forget $name?'),
        content: const Text('You will have to scan its code again to use it.'),
        actions: [TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')), TextButton(onPressed: () => Navigator.pop(context, true), child: const Text('Forget'))],
      ),
    );
    if (ok == true) await widget.link.forget(id);
  }
}
