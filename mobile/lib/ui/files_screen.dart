import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../app/files_url.dart';
import '../core/session.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import 'pair_screen.dart';
import 'widgets.dart';

/// Send files runs in the phone's browser: receiving big files streams to storage there, which an in-app web view
/// cannot do. The page comes from the computer itself (built into Phone Remote, nothing to set up); an own server
/// from the build or Settings is only for sending between different networks.
const buildFilesUrl = String.fromEnvironment('FILES_URL');

/// Opens the scanner for the code the computer's Send files page shows. Returns the link, or null if cancelled.
typedef LinkScanner = Future<String?> Function(BuildContext context, bool Function(String? raw) accept);

Future<String?> scanLink(BuildContext context, bool Function(String? raw) accept) => Navigator.of(context).push<String>(
      MaterialPageRoute(
        builder: (_) => ScanPage<String>(
          accept: (raw) => accept(raw) ? raw!.trim() : null,
          title: 'Scan the code on the computer',
          notOurs: 'That is not the Send files code. On the computer, click Send files.',
        ),
      ),
    );

class FilesScreen extends StatelessWidget {
  const FilesScreen({super.key, required this.settings, this.pc, this.onOpenSettings, this.onGoToRemote, this.open, this.scan = scanLink});
  final AppSettings settings;

  /// The computer this phone is connected to (null before pairing).
  final PcRecord? pc;
  final VoidCallback? onOpenSettings;
  final VoidCallback? onGoToRemote;

  /// Opens a URL (replaced in tests).
  final Future<bool> Function(Uri url)? open;
  final LinkScanner scan;

  Future<void> _open(BuildContext context, String url) async {
    final ok = await (open ?? (u) => launchUrl(u, mode: LaunchMode.externalApplication))(Uri.parse(url));
    if (!ok && context.mounted) ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('No browser found')));
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) {
        final c = context.pr;
        final own = FilesUrl.resolve(settings.filesUrl, buildFilesUrl);
        final local = pc == null ? null : FilesUrl.local(pc!.host, pc!.port);
        final ready = own != null || local != null;
        return ListView(padding: const EdgeInsets.fromLTRB(24, 16, 24, 24), children: [
          const Kicker('Files'),
          const SizedBox(height: 8),
          const Text('Send files.', style: TextStyle(fontSize: 38, fontWeight: FontWeight.w700, height: .95)),
          const SizedBox(height: 14),
          Text(
            own != null
                ? 'You are using your own Send files server. Open it on both devices, then share the room link or QR code.'
                : local != null
                    ? 'Send files of any size between this phone and your computer. Nothing is uploaded to the internet.'
                    : 'First connect this phone to your computer on the Remote tab. Then you can send files between them.',
            style: TextStyle(color: c.dim, fontSize: 17, height: 1.35),
          ),
          if (own == null && local != null) ...[
            const SizedBox(height: 18),
            for (final (i, t) in ['On your computer, click Send files.', 'Here, tap the big button and point the camera at the code on the computer screen.', 'Pick files. They go straight between the two devices.'].indexed)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 10),
                child: Row(children: [
                  SizedBox(width: 38, child: Text('${i + 1}', style: TextStyle(color: c.accentText, fontSize: 26, fontWeight: FontWeight.w700))),
                  Expanded(child: Text(t, style: const TextStyle(fontSize: 16))),
                ]),
              ),
          ],
          const SizedBox(height: 18),
          if (ready)
            FilledButton(
              style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(60), textStyle: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700)),
              onPressed: () async {
                bool accept(String? raw) => (pc != null && FilesUrl.isLocalLink(raw, pc!.host, pc!.port)) || (own != null && FilesUrl.clean(raw) != null && raw!.trim().startsWith(own.replaceFirst(RegExp(r'/$'), '')));
                final link = await scan(context, accept);
                if (link != null && context.mounted) await _open(context, link);
              },
              child: const Text('Scan the code on the computer'),
            ),
          if (ready) const SizedBox(height: 8),
          OutlinedButton(
            onPressed: () async {
              final url = own ?? local;
              if (url == null) {
                onGoToRemote?.call();
                return;
              }
              await _open(context, url);
            },
            child: Text(ready ? 'Open Send files on this phone' : 'Go to Remote'),
          ),
          const SizedBox(height: 6),
          TextButton(onPressed: onOpenSettings, child: Text(own != null ? 'Change or remove my own server' : 'Use my own Send files server instead')),
        ]);
      },
    );
  }
}
